//! NodeHandle:uptr 还原、6 必填回调 glue、驱动线程、命令通道、new/delete。
//!
//! 线程模型:单一驱动线程串行调用全部 process*(poll UDP fd + 唤醒管道),
//! 回调出口通道(frame/event/netcfg)也在此线程消费并转发给 sink,
//! 从而保证"回调数据 → sink"与 process* 同线程,无需对 Node 加锁。
//!
//! FFI 约束:回调内禁 panic(不 unwrap、不索引越界)、不跨 FFI 保存 Rust 引用;
//! 同一 Node 的 process* 串行由驱动线程单线程保证;
//! ZT_Node_delete 前必须先 join 驱动线程(见 shutdown)。

use std::fs::File;
use std::io;
use std::net::UdpSocket;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use crate::config::{NetConfig, NetConfigOp, NodeEvent, NodeParams, NodeSink};
use crate::device::{FrameIn, FrameInTx};
use crate::sa;
use crate::state::{zt_obj, StateStore};
use zt_sys as zt;

/// 物理 recv 缓冲:ZT_MAX_PHYSPAYLOAD + ZT_MAX_HEADROOM
const PHY_BUF: usize = (zt::ZT_MAX_PHYSPAYLOAD + zt::ZT_MAX_HEADROOM) as usize;
/// 组播订阅扫描周期(ms)
const MCAST_SCAN_MS: i64 = 5000;
/// poll 单次阻塞上限(ms)
const POLL_MAX_MS: i32 = 500;

/// 发给驱动线程的串行命令(与 process* 同线程执行,天然互斥)。
pub(crate) enum Cmd {
    Join(u64),
    Leave(u64),
    Stop,
}

#[derive(Debug)]
pub enum NodeError {
    DataStoreFailed(String),
    InvalidArg(String),
    Io(std::io::Error),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::DataStoreFailed(m) => write!(f, "dataStore 失败: {m}"),
            NodeError::InvalidArg(m) => write!(f, "参数无效: {m}"),
            NodeError::Io(e) => write!(f, "IO 错误: {e}"),
        }
    }
}

impl std::error::Error for NodeError {}

// 核心生成的虚拟网络帧(vnet_frame 回调)→ 驱动线程 → sink 写设备。
// 反方向(设备收帧 → processVirtualNetworkFrame)走 FrameIn 通道,与 FrameOut 完全独立。
#[allow(dead_code)]
pub(crate) enum FrameOut {
    Vnet { nwid: u64, src: u64, dst: u64, eth: u16, payload: Vec<u8> },
}

struct Inner {
    store: StateStore,
    udp: UdpSocket,
    // 回调出口通道(回调线程 = 驱动线程,send 只在 shutdown 后才可能失败)
    frame_out: Sender<FrameOut>,
    event_out: Sender<NodeEvent>,
    netcfg_out: Sender<(u64, NetConfigOp, NetConfig)>,
    // 设备→核心帧通道发送端(设备线程经 NodeHandle::frame_in_tx 取得)
    frame_in: Sender<FrameIn>,
    // 命令通道发送端;NodeHandle 借此下发 join/leave/stop
    pub(crate) cmd_tx: Sender<Cmd>,
    // 唤醒管道写端(shutdown 时写一字节让 poll 立即返回)
    pub(crate) wake_wr: File,
    pub(crate) running: AtomicBool,
}

impl Inner {
    /// 向唤醒管道写一字节,让驱动线程的 poll 立即返回(fd 写,免 &mut)。
    fn wake(&self) {
        let mut b = [1u8; 1];
        let _ = unsafe { libc::write(self.wake_wr.as_raw_fd(), b.as_mut_ptr() as *const _, 1) };
    }
}

/// 驱动线程的入参集合(整体 move 进线程;裸指针跨线程由 unsafe impl Send 承担)。
struct Driver {
    node: *mut zt::ZT_Node,
    inner: *mut Inner,
    sink: Box<dyn NodeSink>,
    cmd_rx: Receiver<Cmd>,
    frame_rx: Receiver<FrameOut>,
    frame_in_rx: Receiver<FrameIn>,
    event_rx: Receiver<NodeEvent>,
    netcfg_rx: Receiver<(u64, NetConfigOp, NetConfig)>,
    wake_rd: File,
    deadline: i64,
}

unsafe impl Send for Driver {}

pub struct NodeHandle {
    node: *mut zt::ZT_Node,
    inner: *mut Inner,
    // Some = 驱动线程存活;shutdown 时 join 后置 None
    driver: Option<JoinHandle<()>>,
}

/// CLOEXEC 管道两端(用 File 持有,drop 即关闭)。
fn make_pipe() -> io::Result<(File, File)> {
    let mut fds = [0 as RawFd; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    Ok((unsafe { File::from_raw_fd(fds[0]) }, unsafe { File::from_raw_fd(fds[1]) }))
}

impl NodeHandle {
    pub fn start(params: NodeParams, sink: impl NodeSink) -> Result<Self, NodeError> {
        std::fs::create_dir_all(&params.data_dir).map_err(NodeError::Io)?;
        // planet 注入为必填:本项目语义即"以自定义 planet 接入",不走核心内置默认 world
        let planet = std::fs::read(&params.planet_file).map_err(NodeError::Io)?;
        if planet.len() < 9 {
            return Err(NodeError::InvalidArg(format!(
                "planet 文件过短({} 字节),无法解析 worldId",
                planet.len()
            )));
        }
        // World 二进制:第 0 字节 type,随后 8 字节 worldId(大端)
        let world_id = u64::from_be_bytes(planet[1..9].try_into().expect("长度已校验"));
        let udp = UdpSocket::bind(("0.0.0.0", params.bind_port)).map_err(NodeError::Io)?;
        udp.set_nonblocking(true).map_err(NodeError::Io)?;
        let (frame_tx, frame_rx) = channel();
        let (frame_in_tx, frame_in_rx) = channel();
        let (event_tx, event_rx) = channel();
        let (netcfg_tx, netcfg_rx) = channel();
        let (cmd_tx, cmd_rx) = channel();
        let (wake_rd, wake_wr) = make_pipe().map_err(NodeError::Io)?;
        let inner = Box::into_raw(Box::new(Inner {
            store: StateStore::new(params.data_dir.clone(), world_id, params.planet_file.clone()),
            udp,
            frame_out: frame_tx,
            frame_in: frame_in_tx,
            event_out: event_tx,
            netcfg_out: netcfg_tx,
            cmd_tx,
            wake_wr,
            running: AtomicBool::new(true),
        }));
        let cbs = zt::ZT_Node_Callbacks {
            version: 0,
            statePutFunction: Some(state_put),
            stateGetFunction: Some(state_get),
            wirePacketSendFunction: Some(wire_send),
            virtualNetworkFrameFunction: Some(vnet_frame),
            virtualNetworkConfigFunction: Some(vnet_config),
            eventCallback: Some(on_event),
            pathCheckFunction: None,
            pathLookupFunction: None,
        };
        // ZT_Node_Config 指针必须非 NULL(全零即可)
        let cfg = zt::ZT_Node_Config { enableEncryptedHello: 0, lowBandwidthMode: 0 };
        let now = now_ms();
        let mut node: *mut zt::ZT_Node = std::ptr::null_mut();
        let rc =
            unsafe { zt::ZT_Node_new(&mut node, &cfg, inner as *mut _, std::ptr::null_mut(), &cbs, now) };
        if rc != zt::ZT_ResultCode_ZT_RESULT_OK {
            unsafe { drop(Box::from_raw(inner)); }
            return Err(match rc {
                zt::ZT_ResultCode_ZT_RESULT_FATAL_ERROR_DATA_STORE_FAILED => {
                    NodeError::DataStoreFailed("核心写入 dataStore 失败(identity/planet 落盘)".into())
                }
                zt::ZT_ResultCode_ZT_RESULT_FATAL_ERROR_OUT_OF_MEMORY => {
                    NodeError::DataStoreFailed("核心内存分配失败".into())
                }
                zt::ZT_ResultCode_ZT_RESULT_FATAL_ERROR_INTERNAL => {
                    NodeError::DataStoreFailed("核心内部错误".into())
                }
                other => NodeError::InvalidArg(format!("ZT_Node_new 返回错误码 {other}")),
            });
        }
        // 启动即驱动一次:identity 生成/装载、planet 读入在此发生(对齐 Android)
        let mut dl: i64 = 0;
        let rc = unsafe { zt::ZT_Node_processBackgroundTasks(node, std::ptr::null_mut(), now_ms(), &mut dl) };
        if rc as i32 >= 100 {
            unsafe {
                zt::ZT_Node_delete(node);
                drop(Box::from_raw(inner));
            }
            return Err(NodeError::DataStoreFailed(format!("首次后台任务失败 rc={rc}")));
        }
        // 回调出口通道与 sink 一并交给驱动线程消费(事件送达 = on_event/on_net_config)
        let driver = Driver {
            node,
            inner,
            sink: Box::new(sink),
            cmd_rx,
            frame_rx,
            frame_in_rx,
            event_rx,
            netcfg_rx,
            wake_rd,
            deadline: if dl > 0 { dl } else { now_ms() + 1 },
        };
        let handle = std::thread::Builder::new()
            .name("zt-driver".into())
            .spawn(move || unsafe { run_driver(driver) })
            .map_err(|e| {
                // 线程起不来:节点尚未对外发包,直接清理
                unsafe {
                    zt::ZT_Node_delete(node);
                    drop(Box::from_raw(inner));
                }
                NodeError::Io(io::Error::other(format!("驱动线程启动失败: {e}")))
            })?;
        Ok(Self { node, inner, driver: Some(handle) })
    }

    pub fn address(&self) -> u64 {
        unsafe { zt::ZT_Node_address(self.node) }
    }

    pub fn core_version() -> (u32, u32, u32) {
        let (mut a, mut b, mut c) = (0, 0, 0);
        unsafe { zt::ZT_version(&mut a, &mut b, &mut c) };
        (a as _, b as _, c as _)
    }

    /// 加入网络。实际 ZT_Node_join 在驱动线程执行(与 process* 串行)。
    pub fn join(&self, nwid: u64) -> Result<(), NodeError> {
        self.send_cmd(Cmd::Join(nwid))
    }

    /// 离开网络。实际 ZT_Node_leave 在驱动线程执行。
    pub fn leave(&self, nwid: u64) -> Result<(), NodeError> {
        self.send_cmd(Cmd::Leave(nwid))
    }

    /// 设备→核心帧通道发送端(设备读线程用;发出后自动唤醒驱动线程 poll)。
    pub fn frame_in_tx(&self) -> FrameInTx {
        let wake = self.inner().wake_wr.try_clone().expect("复制唤醒管道写端失败");
        FrameInTx::new(self.inner().frame_in.clone(), wake)
    }

    /// 当前网络配置快照(直接读核心查询接口;只读,与驱动线程无共享可变状态)。
    pub fn networks(&self) -> Vec<NetConfig> {
        unsafe {
            let l = zt::ZT_Node_networks(self.node);
            let mut out = Vec::new();
            if !l.is_null() {
                let cnt = (*l).networkCount as usize;
                let cfgs = std::slice::from_raw_parts((*l).networks, cnt);
                for c in cfgs {
                    out.push(NetConfig::from_zt(c));
                }
                zt::ZT_Node_freeQueryResult(self.node, l as *mut _);
            }
            out
        }
    }

    /// 核心是否 online(有可用物理路径到达根服务器)。
    pub fn online(&self) -> bool {
        unsafe {
            let mut s: zt::ZT_NodeStatus = std::mem::zeroed();
            zt::ZT_Node_status(self.node, &mut s);
            s.online != 0
        }
    }

    fn send_cmd(&self, cmd: Cmd) -> Result<(), NodeError> {
        self.inner().cmd_tx.send(cmd).map_err(|_| {
            NodeError::DataStoreFailed("驱动线程已退出".into())
        })?;
        // 唤醒 poll,让命令立即被处理
        self.inner().wake();
        Ok(())
    }

    /// 关停顺序(强制):running=false → 唤醒 poll → Cmd::Stop → join 驱动线程
    /// → ZT_Node_delete → drop Inner。join 必须先于 delete,否则驱动线程仍在
    /// 使用已释放的 Node(悬垂)。
    pub fn shutdown(mut self) {
        self.inner().running.store(false, Ordering::SeqCst);
        self.inner().wake();
        let _ = self.inner().cmd_tx.send(Cmd::Stop);
        if let Some(h) = self.driver.take() {
            let _ = h.join();
        }
        unsafe {
            zt::ZT_Node_delete(self.node);
            drop(Box::from_raw(self.inner));
        }
    }

    fn inner(&self) -> &Inner {
        unsafe { &*self.inner }
    }
}

/// 驱动线程主循环:串行执行全部 process* + 命令 + 回调出口转发 + 组播订阅。
///
/// # Safety
/// `d.node`/`d.inner` 须为 start() 中有效分配且在 join 本线程前不被释放
/// (shutdown 的顺序保证)。
unsafe fn run_driver(mut d: Driver) {
    let inner = unsafe { &*d.inner };
    let udp_fd = inner.udp.as_raw_fd();
    let mut last_mcast_ms: i64 = i64::MIN / 4; // 首圈立即做一次订阅
    // 已 join 的网络(来自 vnet_config 回调的 UP/UPDATE/DOWN/DESTROY)
    let mut current_networks: Vec<u64> = Vec::new();
    let mut buf = [0u8; PHY_BUF];
    let mut polls = [
        libc::pollfd { fd: udp_fd, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: d.wake_rd.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    loop {
        let now = now_ms();
        if d.deadline <= now {
            let mut dl: i64 = 0;
            let rc = unsafe {
                zt::ZT_Node_processBackgroundTasks(d.node, std::ptr::null_mut(), now as _, &mut dl)
            };
            // 核心 deadline 恒为将来时刻(下限 ZT_CORE_TIMER_TASK_GRANULARITY),防御性钳制
            d.deadline = if dl > now { dl } else { now + 1 };
            if rc as i32 >= 100 {
                d.sink.on_event(NodeEvent::Fatal(format!("核心致命错误 rc={rc}")));
                inner.running.store(false, Ordering::SeqCst);
                return;
            }
        }
        // 命令(本线程执行,保证 process* 串行)
        loop {
            match d.cmd_rx.try_recv() {
                Ok(Cmd::Join(nwid)) => {
                    let _ = unsafe {
                        zt::ZT_Node_join(d.node, nwid, std::ptr::null_mut(), std::ptr::null_mut())
                    };
                }
                Ok(Cmd::Leave(nwid)) => {
                    let mut nuptr: *mut core::ffi::c_void = std::ptr::null_mut();
                    let _ = unsafe {
                        zt::ZT_Node_leave(d.node, nwid, &mut nuptr, std::ptr::null_mut())
                    };
                    current_networks.retain(|&x| x != nwid);
                }
                Ok(Cmd::Stop) => return,
                Err(_) => break,
            }
        }
        // 网络配置回调 → 维护已 join 集合 + 送达 sink
        while let Ok((nwid, op, cfg)) = d.netcfg_rx.try_recv() {
            match op {
                NetConfigOp::Up | NetConfigOp::ConfigUpdate => {
                    if !current_networks.contains(&nwid) {
                        current_networks.push(nwid);
                    }
                }
                NetConfigOp::Down | NetConfigOp::Destroy => {
                    current_networks.retain(|&x| x != nwid);
                }
            }
            d.sink.on_net_config(nwid, op, cfg);
        }
        // 事件 → sink
        while let Ok(ev) = d.event_rx.try_recv() {
            d.sink.on_event(ev);
        }
        // 核心产出帧 → sink(由 sink 写设备;不得在此回喂 processVirtualNetworkFrame)
        while let Ok(f) = d.frame_rx.try_recv() {
            let FrameOut::Vnet { nwid, src, dst, eth, payload } = f;
            d.sink.on_frame(nwid, src, dst, eth, &payload);
        }
        // 设备读出的帧 → processVirtualNetworkFrame(方向:设备→核心,与上面互斥)
        while let Ok(f) = d.frame_in_rx.try_recv() {
            if f.payload.is_empty() || f.nwid == 0 {
                continue;
            }
            let mut dl: i64 = 0;
            let rc = unsafe {
                zt::ZT_Node_processVirtualNetworkFrame(
                    d.node,
                    std::ptr::null_mut(),
                    now_ms(),
                    f.nwid,
                    f.src,
                    f.dst,
                    u32::from(f.eth),
                    0,
                    f.payload.as_ptr() as *const _,
                    f.payload.len() as _,
                    &mut dl,
                )
            };
            if dl > 0 && dl < d.deadline {
                d.deadline = dl;
            }
            if rc as i32 >= 100 {
                d.sink.on_event(NodeEvent::Fatal(format!("processVirtualNetworkFrame rc={rc}")));
            }
        }
        // 组播订阅:每 5000ms 对本机每个 IPv4 订阅 (ffffffffffff, ADI=IPv4 主机序)
        if now - last_mcast_ms >= MCAST_SCAN_MS {
            last_mcast_ms = now;
            let ips = d.sink.local_ipv4s();
            for nwid in &current_networks {
                for ip in &ips {
                    let _ = unsafe {
                        zt::ZT_Node_multicastSubscribe(
                            d.node,
                            std::ptr::null_mut(),
                            *nwid,
                            0xffff_ffff_ffff,
                            u32::from(*ip) as libc::c_ulong,
                        )
                    };
                }
            }
        }
        // poll:UDP 可读 / 唤醒;最长等到位下一次后台任务
        let timeout = ((d.deadline - now_ms()).clamp(1, i64::from(POLL_MAX_MS))) as i32;
        let n = unsafe { libc::poll(polls.as_mut_ptr(), 2, timeout) };
        if n > 0 && polls[0].revents & libc::POLLIN != 0 {
            // 排空当前可读包;每包都可能推进 deadline
            loop {
                match inner.udp.recv_from(&mut buf) {
                    Ok((len, from)) if len > 0 => {
                        let sa = sa::to_sockaddr_storage(&from, None);
                        let mut dl: i64 = 0;
                        let _ = unsafe {
                            zt::ZT_Node_processWirePacket(
                                d.node,
                                std::ptr::null_mut(),
                                now_ms() as _,
                                i64::from(udp_fd),
                                &sa,
                                buf.as_ptr() as *const _,
                                len as _,
                                &mut dl,
                            )
                        };
                        if dl > 0 && dl < d.deadline {
                            d.deadline = dl;
                        }
                    }
                    _ => break,
                }
            }
        }
        if n > 0 && polls[1].revents & libc::POLLIN != 0 {
            let mut b = [0u8; 64];
            let _ = unsafe { libc::read(polls[1].fd, b.as_mut_ptr() as *mut _, b.len()) };
        }
        if !inner.running.load(Ordering::SeqCst) {
            return;
        }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时间早于 UNIX_EPOCH")
        .as_millis() as i64
}

// ===== 6 个回调 glue:uptr 还原 Inner,内部禁 panic/禁 unwrap =====

/// ZT_StateObjectType -> store 对象种类;None = 未知对象。
fn map_state_obj(o: zt::ZT_StateObjectType, put: bool) -> Option<zt_obj> {
    use zt::ZT_StateObjectType_ZT_STATE_OBJECT_IDENTITY_PUBLIC as PUB;
    use zt::ZT_StateObjectType_ZT_STATE_OBJECT_IDENTITY_SECRET as SECRET;
    use zt::ZT_StateObjectType_ZT_STATE_OBJECT_MOON as MOON;
    use zt::ZT_StateObjectType_ZT_STATE_OBJECT_NETWORK_CONFIG as NETCONF;
    use zt::ZT_StateObjectType_ZT_STATE_OBJECT_PEER as PEER;
    use zt::ZT_StateObjectType_ZT_STATE_OBJECT_PLANET as PLANET;
    use zt_obj::*;
    match (o, put) {
        (PUB, _) => Some(IdentityPublic),
        (SECRET, _) => Some(IdentitySecret),
        (PLANET, false) => Some(PlanetGet),
        (PLANET, true) => Some(PlanetPut),
        (MOON, _) => Some(Moon),
        (PEER, _) => Some(Peer),
        (NETCONF, _) => Some(NetworkConfig),
        _ => None,
    }
}

unsafe extern "C" fn state_get(
    _n: *mut zt::ZT_Node,
    uptr: *mut core::ffi::c_void,
    _t: *mut core::ffi::c_void,
    obj: zt::ZT_StateObjectType,
    id: *const u64,
    buf: *mut core::ffi::c_void,
    len: core::ffi::c_uint,
) -> core::ffi::c_int {
    if uptr.is_null() || id.is_null() {
        return -2;
    }
    let inner = &*(uptr as *const Inner);
    let Some(o) = map_state_obj(obj, false) else { return -2 };
    let ids = &*(id as *const [u64; 2]);
    inner.store.get(o, ids, buf as *mut u8, len as usize) as core::ffi::c_int
}

/// 核心 typedef 为 void 返回(删除以 len == -1 表达),无错误回报通道。
unsafe extern "C" fn state_put(
    _n: *mut zt::ZT_Node,
    uptr: *mut core::ffi::c_void,
    _t: *mut core::ffi::c_void,
    obj: zt::ZT_StateObjectType,
    id: *const u64,
    data: *const core::ffi::c_void,
    len: core::ffi::c_int,
) {
    if uptr.is_null() || id.is_null() {
        return;
    }
    let inner = &*(uptr as *const Inner);
    let Some(o) = map_state_obj(obj, true) else { return };
    let ids = &*(id as *const [u64; 2]);
    // planet 回写用 PlanetPut(根下 planet 文件);identity.secret/planet 强制 0600
    let secure = matches!(
        obj,
        zt::ZT_StateObjectType_ZT_STATE_OBJECT_IDENTITY_SECRET
            | zt::ZT_StateObjectType_ZT_STATE_OBJECT_PLANET
    );
    let _ = inner.store.put(o, ids, data as *const u8, len as isize, secure);
}

unsafe extern "C" fn wire_send(
    _n: *mut zt::ZT_Node,
    uptr: *mut core::ffi::c_void,
    _t: *mut core::ffi::c_void,
    _local_socket: i64,
    remote: *const zt::sockaddr_storage,
    data: *const core::ffi::c_void,
    len: core::ffi::c_uint,
    _ttl: core::ffi::c_uint,
) -> core::ffi::c_int {
    if uptr.is_null() {
        return -1;
    }
    let inner = &*(uptr as *const Inner);
    let Some(addr) = sa::from_sockaddr_storage(remote) else { return -1 };
    let bytes = if data.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(data as *const u8, len as usize)
    };
    match inner.udp.send_to(bytes, addr) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

unsafe extern "C" fn vnet_frame(
    _n: *mut zt::ZT_Node,
    uptr: *mut core::ffi::c_void,
    _t: *mut core::ffi::c_void,
    nwid: u64,
    _nuptr: *mut *mut core::ffi::c_void,
    src: u64,
    dst: u64,
    eth: core::ffi::c_uint,
    _vlan: core::ffi::c_uint,
    data: *const core::ffi::c_void,
    len: core::ffi::c_uint,
) {
    if uptr.is_null() {
        return;
    }
    let inner = &*(uptr as *const Inner);
    let payload = if data.is_null() || len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(data as *const u8, len as usize).to_vec()
    };
    let _ = inner.frame_out.send(FrameOut::Vnet { nwid, src, dst, eth: eth as u16, payload });
}

unsafe extern "C" fn vnet_config(
    _n: *mut zt::ZT_Node,
    uptr: *mut core::ffi::c_void,
    _t: *mut core::ffi::c_void,
    nwid: u64,
    _nuptr: *mut *mut core::ffi::c_void,
    op: zt::ZT_VirtualNetworkConfigOperation,
    cfg: *const zt::ZT_VirtualNetworkConfig,
) -> core::ffi::c_int {
    if uptr.is_null() || cfg.is_null() {
        return 0;
    }
    let inner = &*(uptr as *const Inner);
    // 拷贝后经通道交驱动线程转 sink(Task 3);回调内不得调用 node 方法(死锁)
    let op = match op {
        zt::ZT_VirtualNetworkConfigOperation_ZT_VIRTUAL_NETWORK_CONFIG_OPERATION_UP => NetConfigOp::Up,
        zt::ZT_VirtualNetworkConfigOperation_ZT_VIRTUAL_NETWORK_CONFIG_OPERATION_CONFIG_UPDATE => {
            NetConfigOp::ConfigUpdate
        }
        zt::ZT_VirtualNetworkConfigOperation_ZT_VIRTUAL_NETWORK_CONFIG_OPERATION_DOWN => NetConfigOp::Down,
        zt::ZT_VirtualNetworkConfigOperation_ZT_VIRTUAL_NETWORK_CONFIG_OPERATION_DESTROY => {
            NetConfigOp::Destroy
        }
        _ => return 0,
    };
    let copy = NetConfig::from_zt(&*cfg);
    let _ = inner.netcfg_out.send((nwid, op, copy));
    0
}

unsafe extern "C" fn on_event(
    _n: *mut zt::ZT_Node,
    uptr: *mut core::ffi::c_void,
    _t: *mut core::ffi::c_void,
    ev: zt::ZT_Event,
    _meta: *const core::ffi::c_void,
) {
    if uptr.is_null() {
        return;
    }
    let inner = &*(uptr as *const Inner);
    // 映射:UP/DOWN/ONLINE/OFFLINE/IDENTITY_COLLISION;TRACE/REMOTE_TRACE 忽略
    let mapped = match ev {
        zt::ZT_Event_ZT_EVENT_UP => Some(NodeEvent::Up),
        zt::ZT_Event_ZT_EVENT_ONLINE => Some(NodeEvent::Online),
        zt::ZT_Event_ZT_EVENT_OFFLINE => Some(NodeEvent::Offline),
        zt::ZT_Event_ZT_EVENT_DOWN => Some(NodeEvent::Down),
        zt::ZT_Event_ZT_EVENT_FATAL_ERROR_IDENTITY_COLLISION => Some(NodeEvent::IdentityCollision),
        _ => None,
    };
    if let Some(e) = mapped {
        let _ = inner.event_out.send(e);
    }
}
