//! Windows 版 ZeroTier node 驱动:zt-core/node.rs 架构的平移,POC 精简。
//!
//! 线程模型:单一驱动线程串行调用全部 process*(UDP 收包由独立读线程阻塞
//! recv_from 经通道投递;唤醒用 mpsc 替代 Unix pipe),回调出口通道
//! (frame/event/netcfg)也在此线程消费并转发给 sink,保证"回调数据 → sink"
//! 与 process* 同线程,无需对 Node 加锁。
//!
//! # 与 zt-core/node.rs 的 Windows 改造点(仅此四处,其余语义逐行保留)
//!
//! 1. UDP 拆双线程:`Inner.udp` 只留给 wire_send;独立读线程阻塞 recv_from,
//!    把 `(payload, from)` 投进 `udp_rx` 后唤醒驱动线程;驱动线程消费并调
//!    `ZT_Node_processWirePacket`(local_socket 传 winsock SOCKET)。
//! 2. 唤醒:`wake_tx/wake_rx` mpsc + `recv_timeout(clamp(1, 500ms))` 等价原
//!    poll 语义(物理包可读/命令/停机即时唤醒,超时进入下一圈)。
//! 3. imports:`std::os::windows::io::AsRawSocket`,无 libc。
//! 4. `multicastSubscribe` 的 ADI 形参在 Win32 绑定下是 `c_ulong`(32 位)。
//!
//! # 架构红线(平移自原型,不改)
//!
//! - 回调 glue 只做「拷贝 + channel send」,绝不直接调 sink、不回调 Node 方法;
//! - 全部 sink 调用(on_event/on_net_config/on_frame/local_ipv4s)只发生在
//!   驱动线程,且以 catch_unwind 包裹:sink panic 不许穿越驱动线程/FFI;
//! - FrameOut(核心→设备,sink 写设备)与 FrameIn(设备→核心,
//!   processVirtualNetworkFrame)两条通道方向不可混用;
//! - 关停硬顺序:running=false → 唤醒 → Cmd::Stop → join 驱动线程 →
//!   ZT_Node_delete → drop Inner(join 必须先于 delete,否则悬垂);
//! - FFI 边界禁 panic/unwrap;通道对端断开一律转错误返回,不炸进程。
//!
//! # POC 精简(相对原型裁掉,正式实现再补)
//!
//! - Cmd 只留 Join/Stop(leave/orbit/deorbit 不在 Gate 2a 路径上);
//! - `frame_in_tx()` 直接交出 `Sender<FrameIn>`:驱动线程每圈固定消费,
//!   帧入核心延迟上限为一个 tick(≤500ms),数据面 POC 足够。

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::os::windows::io::AsRawSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::netcfg::NetConfig;
use crate::sa;
use crate::store::{zt_obj, StateStore};
use zt_sys as zt;

/// 物理 recv 缓冲:ZT_MAX_PHYSPAYLOAD + ZT_MAX_HEADROOM
const PHY_BUF: usize = (zt::ZT_MAX_PHYSPAYLOAD + zt::ZT_MAX_HEADROOM) as usize;
/// 组播订阅扫描周期(ms)
const MCAST_SCAN_MS: i64 = 5000;
/// 唤醒等待上限(ms):等价原 poll 单次阻塞上限 POLL_MAX_MS
const WAKE_MAX_MS: i64 = 500;
/// UDP 读线程 socket 读超时(读线程借此周期性检查 running,保证可被关停)
const UDP_READ_TIMEOUT: Duration = Duration::from_millis(500);

/// 发给驱动线程的串行命令(与 process* 同线程执行,天然互斥)。
/// Join 携带应答端:驱动线程执行完核心 C 调用后回报真实 rc。
pub(crate) enum Cmd {
    Join(u64, Sender<Result<(), String>>),
    Stop,
}

/// 设备读出的待回喂核心的帧(方向:设备→核心)。
#[derive(Debug, Clone)]
pub struct FrameIn {
    pub nwid: u64,
    pub src: u64,
    pub dst: u64,
    pub eth: u16,
    pub payload: Vec<u8>,
}

/// 核心生成的虚拟网络帧(vnet_frame 回调)→ 驱动线程 → sink 写设备。
pub(crate) struct FrameOut {
    pub nwid: u64,
    pub src: u64,
    pub dst: u64,
    pub eth: u16,
    pub payload: Vec<u8>,
}

/// 网络配置回调的操作类型(vnet_config 回调携带;驱动线程维护已 join 集合用)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetConfigOp {
    Up,
    ConfigUpdate,
    Down,
    Destroy,
}

/// 上层接收 Node 事件/网络配置/虚拟网帧的出口(Task 7 数据面直接挂上去)。
pub trait PocSink: Send + 'static {
    fn on_event(&mut self, ev: &str);
    fn on_net_config(&mut self, cfg: NetConfig);
    fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]);
    fn local_ipv4s(&self) -> Vec<Ipv4Addr>;
}

/// 对端信任层级角色(ZT_PeerRole)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    Leaf,
    Moon,
    Planet,
}

/// 到某对端的一条物理路径快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerPath {
    pub addr: SocketAddr,
    /// 未过期且有收包记录(`expired == 0 && lastReceive != 0`)
    pub active: bool,
    /// 核心标记的 trusted path(trustedPathId 非零)
    pub trusted: bool,
}

/// 单个对端快照(`ZT_Node_peers` 查询结果的 Rust 拷贝,不持有核心内存)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub address: u64,
    pub role: PeerRole,
    /// 远端版本三元组;任一分量 <0(未知)即 None
    pub version: Option<(i32, i32, i32)>,
    /// 最近测得的延迟毫秒;核心报告 <0(未知)即 None
    pub latency_ms: Option<i64>,
    pub paths: Vec<PeerPath>,
}

impl PeerInfo {
    /// 从 ZT_Peer 拷贝(查询结果指针随时可能被 free,必须立即深拷)。
    ///
    /// # Safety
    /// `p` 必须指向核心刚返回、尚未释放的有效 `ZT_Peer`。
    pub(crate) unsafe fn from_zt(p: &zt::ZT_Peer) -> Self {
        let version = (p.versionMajor >= 0 && p.versionMinor >= 0 && p.versionRev >= 0)
            .then_some((p.versionMajor, p.versionMinor, p.versionRev));
        let latency_ms = (p.latency >= 0).then_some(i64::from(p.latency));
        // 切片上限做 min 防御:核心计数异常时按数组容量截断
        let n_paths = (p.pathCount as usize).min(p.paths.len());
        let paths = p.paths[..n_paths]
            .iter()
            // 空槽(核心未填充)特征:lastSend 与 lastReceive 均为 0,跳过
            .filter(|pp| !(pp.lastSend == 0 && pp.lastReceive == 0))
            .filter_map(|pp| {
                Some(PeerPath {
                    addr: sa::from_sockaddr_storage(&pp.address)?,
                    active: pp.expired == 0 && pp.lastReceive != 0,
                    trusted: pp.trustedPathId != 0,
                })
            })
            .collect();
        let role = match p.role {
            zt::ZT_PeerRole_ZT_PEER_ROLE_MOON => PeerRole::Moon,
            zt::ZT_PeerRole_ZT_PEER_ROLE_PLANET => PeerRole::Planet,
            _ => PeerRole::Leaf,
        };
        Self { address: p.address, role, version, latency_ms, paths }
    }
}

struct Inner {
    store: StateStore,
    // 发送端(wire_send 用);接收阻塞在独立读线程的克隆 socket 上
    udp: UdpSocket,
    // 回调出口通道(回调线程 = 驱动线程,send 只在 shutdown 后才可能失败)
    frame_out: Sender<FrameOut>,
    event_out: Sender<String>,
    netcfg_out: Sender<(NetConfigOp, NetConfig)>,
    // 设备→核心帧通道发送端(设备线程经 PocNode::frame_in_tx 取得)
    #[allow(dead_code)] // Task 7 数据面使用
    frame_in: Sender<FrameIn>,
    // 命令通道发送端;PocNode 借此下发 join/stop
    cmd_tx: Sender<Cmd>,
    // 唤醒通道发送端(shutdown/命令/帧入时发一字节让 recv_timeout 立即返回)
    wake_tx: Sender<u8>,
    // Arc:UDP 读线程要跨线程检查同一开关
    running: Arc<AtomicBool>,
}

impl Inner {
    /// 向唤醒通道发一字节,让驱动线程的 recv_timeout 立即返回(免 &mut)。
    fn wake(&self) {
        let _ = self.wake_tx.send(1);
    }
}

/// 驱动线程的入参集合(整体 move 进线程;裸指针跨线程由 unsafe impl Send 承担)。
struct Driver {
    node: *mut zt::ZT_Node,
    inner: *mut Inner,
    sink: Box<dyn PocSink>,
    cmd_rx: Receiver<Cmd>,
    frame_rx: Receiver<FrameOut>,
    frame_in_rx: Receiver<FrameIn>,
    event_rx: Receiver<String>,
    netcfg_rx: Receiver<(NetConfigOp, NetConfig)>,
    udp_rx: Receiver<(Vec<u8>, SocketAddr)>,
    wake_rx: Receiver<u8>,
    /// winsock SOCKET(收发共用同一 socket,句柄值一致)
    local_socket: i64,
    deadline: i64,
}

unsafe impl Send for Driver {}

pub struct PocNode {
    node: *mut zt::ZT_Node,
    inner: *mut Inner,
    // Some = 驱动线程存活;shutdown 时 join 后置 None
    driver: Option<JoinHandle<()>>,
    // UDP 读线程(start 期 spawn,shutdown 时 join;晚于驱动线程退出无害)
    udp_reader: Option<JoinHandle<()>>,
    // Inner 是否已回收(shutdown 已执行);Drop 据此避免二次释放
    shut_down: bool,
}

// SAFETY:句柄仅由裸指针 + JoinHandle + bool 组成,可跨线程转移/共享,前提:
// - process*(含全部回调出口消费)只在唯一驱动线程执行(本模块保证);
// - join 经命令通道转发到驱动线程串行执行;
// - 查询类(networks/peers/online/address)由调用线程直接调核心查询接口,
//   良性数据竞争论证同 zt-core/node.rs(最坏读到旧值,不涉及指针有效性);
// - 结构安全:wake() 只做 channel send(线程安全);shutdown 的 join 先于
//   ZT_Node_delete 的硬顺序不受调用线程影响。
unsafe impl Send for PocNode {}
unsafe impl Sync for PocNode {}

impl PocNode {
    pub fn start(
        data_dir: PathBuf,
        planet_file: PathBuf,
        bind_port: u16,
        sink: impl PocSink,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
        // planet 注入为必填:本项目语义即"以自定义 planet 接入",不走核心内置默认 world
        let planet =
            std::fs::read(&planet_file).map_err(|e| format!("读取 planet 失败: {e}"))?;
        if planet.len() < 9 {
            return Err(format!("planet 文件过短({} 字节),无法解析 worldId", planet.len()));
        }
        // World 二进制:第 0 字节 type,随后 8 字节 worldId(大端)
        let world_id = u64::from_be_bytes(planet[1..9].try_into().expect("长度已校验"));
        let udp = UdpSocket::bind(("0.0.0.0", bind_port))
            .map_err(|e| format!("绑定 UDP {bind_port} 失败: {e}"))?;
        let local_socket = udp.as_raw_socket() as i64;
        // 接收端克隆给读线程;发送端留在 Inner 供 wire_send(同一 SOCKET)
        let udp_rd = udp.try_clone().map_err(|e| format!("克隆 UDP socket 失败: {e}"))?;
        let (frame_tx, frame_rx) = channel();
        let (frame_in_tx, frame_in_rx) = channel();
        let (event_tx, event_rx) = channel();
        let (netcfg_tx, netcfg_rx) = channel();
        let (cmd_tx, cmd_rx) = channel();
        let (wake_tx, wake_rx) = channel();
        let (udp_tx, udp_rx) = channel();
        let running = Arc::new(AtomicBool::new(true));
        // 读线程的唤醒克隆:物理包投进 udp_rx 后即唤醒驱动线程(对齐 poll 语义)
        let wake_tx_rd = wake_tx.clone();
        let inner = Box::into_raw(Box::new(Inner {
            store: StateStore::new(data_dir.clone(), world_id, planet_file.clone()),
            udp,
            frame_out: frame_tx,
            frame_in: frame_in_tx,
            event_out: event_tx,
            netcfg_out: netcfg_tx,
            cmd_tx,
            wake_tx,
            running: Arc::clone(&running),
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
            // 读线程尚未启动,Inner 无他人引用,直接回收
            unsafe { drop(Box::from_raw(inner)); }
            return Err(match rc {
                zt::ZT_ResultCode_ZT_RESULT_FATAL_ERROR_DATA_STORE_FAILED => {
                    "dataStore 失败:核心写入 dataStore 失败(identity/planet 落盘)".into()
                }
                zt::ZT_ResultCode_ZT_RESULT_FATAL_ERROR_OUT_OF_MEMORY => {
                    "核心内存分配失败".into()
                }
                zt::ZT_ResultCode_ZT_RESULT_FATAL_ERROR_INTERNAL => "核心内部错误".into(),
                other => format!("ZT_Node_new 返回错误码 {other}"),
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
            return Err(format!("首次后台任务失败 rc={rc}"));
        }
        // UDP 读线程:阻塞 recv_from → 通道 → 唤醒 → 驱动线程 processWirePacket
        let reader = std::thread::Builder::new()
            .name("zt-udp-reader".into())
            .spawn(move || run_udp_reader(udp_rd, running, udp_tx, wake_tx_rd))
            .map_err(|e| {
                unsafe {
                    zt::ZT_Node_delete(node);
                    drop(Box::from_raw(inner));
                }
                format!("UDP 读线程启动失败: {e}")
            })?;
        // 回调出口通道与 sink 一并交给驱动线程消费
        let driver = Driver {
            node,
            inner,
            sink: Box::new(sink),
            cmd_rx,
            frame_rx,
            frame_in_rx,
            event_rx,
            netcfg_rx,
            udp_rx,
            wake_rx,
            local_socket,
            deadline: if dl > 0 { dl } else { now_ms() + 1 },
        };
        let handle = std::thread::Builder::new()
            .name("zt-driver".into())
            .spawn(move || unsafe { run_driver(driver) })
            .map_err(|e| {
                // 线程起不来:节点尚未对外发包,直接清理;先置停 running 让
                // 读线程(已启动)退出,再 drop Inner
                unsafe {
                    (*inner).running.store(false, Ordering::SeqCst);
                    zt::ZT_Node_delete(node);
                    drop(Box::from_raw(inner));
                }
                format!("驱动线程启动失败: {e}")
            })?;
        Ok(Self { node, inner, driver: Some(handle), udp_reader: Some(reader), shut_down: false })
    }

    pub fn address(&self) -> u64 {
        unsafe { zt::ZT_Node_address(self.node) }
    }

    pub fn core_version() -> (u32, u32, u32) {
        let (mut a, mut b, mut c) = (0, 0, 0);
        unsafe { zt::ZT_version(&mut a, &mut b, &mut c) };
        (a as _, b as _, c as _)
    }

    /// 加入网络。实际 ZT_Node_join 在驱动线程执行(与 process* 串行),
    /// 核心 rc 非 OK 时返回真实错误。
    pub fn join(&self, nwid: u64) -> Result<(), String> {
        self.send_cmd(|tx| Cmd::Join(nwid, tx))
    }

    /// 设备→核心帧通道发送端(数据面读线程用)。
    /// 驱动线程每圈固定消费该通道,帧入核心延迟上限一个 tick(≤500ms)。
    #[allow(dead_code)] // Task 7 数据面使用
    pub fn frame_in_tx(&self) -> Sender<FrameIn> {
        self.inner().frame_in.clone()
    }

    /// 当前网络配置快照(直接读核心查询接口;网络列表受核心
    /// `_networks_m` 保护,无数据竞争)。
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

    /// 已知对端列表快照(直接读核心查询接口)。
    ///
    /// 红线:`ZT_Node_peers` 返回的缓冲必须用 `ZT_Node_freeQueryResult`
    /// 释放 —— 拷贝在本函数内完成,指针不外借。
    pub fn peers(&self) -> Vec<PeerInfo> {
        unsafe {
            let l = zt::ZT_Node_peers(self.node);
            let mut out = Vec::new();
            if !l.is_null() {
                let cnt = (*l).peerCount as usize;
                let peers = std::slice::from_raw_parts((*l).peers, cnt);
                for p in peers {
                    out.push(PeerInfo::from_zt(p));
                }
                zt::ZT_Node_freeQueryResult(self.node, l as *mut _);
            }
            out
        }
    }

    /// 核心是否 online(有可用物理路径到达根服务器)。
    /// `_online` 为普通 bool,读侧与驱动线程写存在良性数据竞争(脏读布尔)。
    pub fn online(&self) -> bool {
        unsafe {
            let mut s: zt::ZT_NodeStatus = std::mem::zeroed();
            zt::ZT_Node_status(self.node, &mut s);
            s.online != 0
        }
    }

    /// 下发命令并等待驱动线程的 rc 应答(命令与 process* 同线程串行执行)。
    ///
    /// 红线:阻塞等应答,故绝不可在 sink 回调(驱动线程)内调用本路径,
    /// 否则自等自死锁。
    fn send_cmd(&self, make: impl FnOnce(Sender<Result<(), String>>) -> Cmd) -> Result<(), String> {
        let (tx, rx) = channel();
        self.inner()
            .cmd_tx
            .send(make(tx))
            .map_err(|_| "驱动线程已退出".to_string())?;
        // 唤醒驱动线程,让命令立即被处理
        self.inner().wake();
        rx.recv().map_err(|_| "驱动线程已退出(命令无应答)".to_string())?
    }

    /// 关停顺序(强制):running=false → 唤醒 → Cmd::Stop → join 驱动线程
    /// → join UDP 读线程 → ZT_Node_delete → drop Inner。join 必须先于
    /// delete,否则驱动线程仍在使用已释放的 Node(悬垂)。
    pub fn shutdown(mut self) {
        self.shutdown_inner();
    }

    fn shutdown_inner(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        self.inner().running.store(false, Ordering::SeqCst);
        self.inner().wake();
        let _ = self.inner().cmd_tx.send(Cmd::Stop);
        if let Some(h) = self.driver.take() {
            let _ = h.join();
        }
        if let Some(h) = self.udp_reader.take() {
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

impl Drop for PocNode {
    fn drop(&mut self) {
        if self.shut_down {
            return;
        }
        // 兜底:调用方漏 shutdown。debug_assert 在测试/调试构建立即暴露,
        // 发布构建仍按硬顺序尽力回收,绝不泄漏 Node/线程。
        debug_assert!(
            false,
            "PocNode 未显式 shutdown 即 drop,已兜底执行关停(顺序:线程 join → ZT_Node_delete)"
        );
        self.shutdown_inner();
    }
}

/// UDP 读线程:阻塞 recv_from → `(payload, from)` 投通道 → 唤醒驱动线程。
///
/// 唤醒对齐原型 poll 语义:fd 可读即唤醒驱动线程的 recv_timeout,包不等待
/// 500ms tick;每包只唤醒一次,驱动线程每圈把 udp_rx 排空后再睡,不忙转。
///
/// 错误策略:
/// - 读超时/WouldBlock:正常轮询出口,回到循环头检查 running;
/// - ConnectionReset/ConnectionRefused:Windows 特有 —— 此前 send 到某地址
///   收到 ICMP port-unreachable 后,下一次 recv 会报 WSAECONNRESET。必须
///   忽略,否则一次不可达就把读线程打死(POC 关键坑,ZeroTier 官方同款处理);
/// - 其他错误退避 200ms 继续,不因一次 IO 抖动永久退出。
fn run_udp_reader(
    sock: UdpSocket,
    running: Arc<AtomicBool>,
    tx: Sender<(Vec<u8>, SocketAddr)>,
    wake: Sender<u8>,
) {
    let _ = sock.set_read_timeout(Some(UDP_READ_TIMEOUT));
    let mut buf = vec![0u8; PHY_BUF];
    while running.load(Ordering::SeqCst) {
        match sock.recv_from(&mut buf) {
            Ok((len, from)) if len > 0 => {
                if tx.send((buf[..len].to_vec(), from)).is_err() {
                    // 驱动线程已退出,通道对端不在了
                    return;
                }
                // 先投包再唤醒:驱动线程醒来即从 udp_rx 排空消费
                let _ = wake.send(1);
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionRefused
                ) => {}
            Err(e) => {
                eprintln!("[udp-reader] recv 错误(退避 200ms 继续): {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// sink 调用统一包裹:panic 不穿越驱动线程(架构红线)。
///
/// panic → 打日志 + 尽力推 Fatal 事件 + running=false,返回 None;
/// 调用方见 None 须立即退出驱动线程(sink 已不可信,不杀进程)。
fn call_sink<R>(
    sink: &mut Box<dyn PocSink>,
    running: &AtomicBool,
    what: &str,
    f: impl FnOnce(&mut dyn PocSink) -> R,
) -> Option<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut **sink))) {
        Ok(r) => Some(r),
        Err(p) => {
            let msg = format!("FATAL sink panic({what}): {}", panic_msg(p));
            eprintln!("[driver] {msg}");
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sink.on_event(&msg);
            }));
            running.store(false, Ordering::SeqCst);
            None
        }
    }
}

/// panic 载荷转可读文本(不 unwrap、不假定类型)。
fn panic_msg(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

/// 驱动线程主循环:串行执行全部 process* + 命令 + 回调出口转发 + 组播订阅
/// + UDP 收包消费;循环尾部以 recv_timeout(clamp(1,500ms)) 等待下一次唤醒。
///
/// # Safety
/// `d.node`/`d.inner` 须为 start() 中有效分配且在 join 本线程前不被释放
/// (shutdown 的顺序保证)。
unsafe fn run_driver(mut d: Driver) {
    let inner = unsafe { &*d.inner };
    let mut last_mcast_ms: i64 = i64::MIN / 4; // 首圈立即做一次订阅
    // 已 join 的网络(来自 vnet_config 回调的 UP/UPDATE/DOWN/DESTROY)
    let mut current_networks: Vec<u64> = Vec::new();
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
                call_sink(&mut d.sink, &inner.running, "on_event", |s| {
                    s.on_event(&format!("FATAL 核心致命错误 rc={rc}"))
                });
                inner.running.store(false, Ordering::SeqCst);
                return;
            }
        }
        // 命令(本线程执行,保证 process* 串行;执行后回报真实 rc)
        loop {
            match d.cmd_rx.try_recv() {
                Ok(Cmd::Join(nwid, reply)) => {
                    let rc = unsafe {
                        zt::ZT_Node_join(d.node, nwid, std::ptr::null_mut(), std::ptr::null_mut())
                    };
                    let _ = reply.send(rc_to_err("ZT_Node_join", rc));
                }
                Ok(Cmd::Stop) => return,
                Err(_) => break,
            }
        }
        // 网络配置回调 → 维护已 join 集合 + 送达 sink
        while let Ok((op, cfg)) = d.netcfg_rx.try_recv() {
            match op {
                NetConfigOp::Up | NetConfigOp::ConfigUpdate => {
                    if !current_networks.contains(&cfg.nwid) {
                        current_networks.push(cfg.nwid);
                    }
                }
                NetConfigOp::Down | NetConfigOp::Destroy => {
                    current_networks.retain(|&x| x != cfg.nwid);
                }
            }
            if call_sink(&mut d.sink, &inner.running, "on_net_config", |s| s.on_net_config(cfg))
                .is_none()
            {
                return;
            }
        }
        // 事件 → sink
        while let Ok(ev) = d.event_rx.try_recv() {
            if call_sink(&mut d.sink, &inner.running, "on_event", |s| s.on_event(&ev)).is_none() {
                return;
            }
        }
        // 核心产出帧 → sink(由 sink 写设备;不得在此回喂 processVirtualNetworkFrame)
        while let Ok(f) = d.frame_rx.try_recv() {
            if call_sink(&mut d.sink, &inner.running, "on_frame", |s| {
                s.on_frame(f.nwid, f.src, f.dst, f.eth, &f.payload)
            })
            .is_none()
            {
                return;
            }
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
            if rc as i32 >= 100
                && call_sink(&mut d.sink, &inner.running, "on_event", |s| {
                    s.on_event(&format!("FATAL processVirtualNetworkFrame rc={rc}"))
                })
                .is_none()
            {
                return;
            }
        }
        // 组播订阅:每 5000ms 对本机每个 IPv4 订阅 (ffffffffffff, ADI=IPv4 主机序)。
        // Task 6 控制面无设备,local_ipv4s 返回空即跳过;数据面接入后自然生效。
        if now - last_mcast_ms >= MCAST_SCAN_MS {
            last_mcast_ms = now;
            let Some(ips) = call_sink(&mut d.sink, &inner.running, "local_ipv4s", |s| {
                s.local_ipv4s()
            }) else {
                return;
            };
            for nwid in &current_networks {
                for ip in &ips {
                    let _ = unsafe {
                        zt::ZT_Node_multicastSubscribe(
                            d.node,
                            std::ptr::null_mut(),
                            *nwid,
                            0xffff_ffff_ffff,
                            u32::from(*ip) as std::os::raw::c_ulong,
                        )
                    };
                }
            }
        }
        // UDP 读线程投递的物理包 → processWirePacket;每包都可能推进 deadline
        while let Ok((pkt, from)) = d.udp_rx.try_recv() {
            if pkt.is_empty() {
                continue;
            }
            let sa = sa::to_sockaddr_storage(&from, None);
            let mut dl: i64 = 0;
            let _ = unsafe {
                zt::ZT_Node_processWirePacket(
                    d.node,
                    std::ptr::null_mut(),
                    now_ms() as _,
                    d.local_socket,
                    &sa,
                    pkt.as_ptr() as *const _,
                    pkt.len() as _,
                    &mut dl,
                )
            };
            if dl > 0 && dl < d.deadline {
                d.deadline = dl;
            }
        }
        // 等待:下一次后台任务 deadline,或任一通道唤醒(等价原 poll 语义)。
        // 帧入/命令/wake 各通道都会先于下一圈被消费,这里只负责"睡到点"。
        let tick = ((d.deadline - now_ms()).clamp(1, i64::from(WAKE_MAX_MS))) as u64;
        match d.wake_rx.recv_timeout(Duration::from_millis(tick)) {
            Ok(_) => {
                // 排空积压的唤醒字节(合并为一次提前醒)
                while d.wake_rx.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // 发送端随 Inner 消失:视为停机信号
                inner.running.store(false, Ordering::SeqCst);
                return;
            }
        }
        if !inner.running.load(Ordering::SeqCst) {
            return;
        }
    }
}

/// 核心 rc -> Result;非 OK 时把真实错误码报给调用方(不再吞成 Ok)。
fn rc_to_err(what: &str, rc: zt::ZT_ResultCode) -> Result<(), String> {
    if rc == zt::ZT_ResultCode_ZT_RESULT_OK {
        return Ok(());
    }
    Err(format!("{what} 返回错误码 {rc}"))
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
    let inner = unsafe { &*(uptr as *const Inner) };
    let Some(o) = map_state_obj(obj, false) else { return -2 };
    let ids = unsafe { &*(id as *const [u64; 2]) };
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
    let inner = unsafe { &*(uptr as *const Inner) };
    let Some(o) = map_state_obj(obj, true) else { return };
    let ids = unsafe { &*(id as *const [u64; 2]) };
    // secure 标记(identity.secret/planet)在 POC 不收敛权限,store 侧仅留签名
    let secure = matches!(
        obj,
        zt::ZT_StateObjectType_ZT_STATE_OBJECT_IDENTITY_SECRET
            | zt::ZT_StateObjectType_ZT_STATE_OBJECT_PLANET
    );
    let _ = unsafe { inner.store.put(o, ids, data as *const u8, len as isize, secure) };
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
    let inner = unsafe { &*(uptr as *const Inner) };
    let Some(addr) = (unsafe { sa::from_sockaddr_storage(remote) }) else { return -1 };
    let bytes = if data.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data as *const u8, len as usize) }
    };
    match inner.udp.send_to(bytes, addr) {
        Ok(_) => 0,
        // 回调内禁 panic:发送失败以 -1 回报核心,由核心路径管理重试
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
    let inner = unsafe { &*(uptr as *const Inner) };
    let payload = if data.is_null() || len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(data as *const u8, len as usize) }.to_vec()
    };
    let _ = inner.frame_out.send(FrameOut { nwid, src, dst, eth: eth as u16, payload });
}

unsafe extern "C" fn vnet_config(
    _n: *mut zt::ZT_Node,
    uptr: *mut core::ffi::c_void,
    _t: *mut core::ffi::c_void,
    // nwid 已含于 NetConfig.nwid,通道不重复携带
    _nwid: u64,
    _nuptr: *mut *mut core::ffi::c_void,
    op: zt::ZT_VirtualNetworkConfigOperation,
    cfg: *const zt::ZT_VirtualNetworkConfig,
) -> core::ffi::c_int {
    if uptr.is_null() || cfg.is_null() {
        return 0;
    }
    let inner = unsafe { &*(uptr as *const Inner) };
    // 拷贝后经通道交驱动线程转 sink;回调内不得调用 node 方法(死锁)
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
    let copy = unsafe { NetConfig::from_zt(&*cfg) };
    let _ = inner.netcfg_out.send((op, copy));
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
    let inner = unsafe { &*(uptr as *const Inner) };
    // 映射:UP/DOWN/ONLINE/OFFLINE/IDENTITY_COLLISION;TRACE/REMOTE_TRACE 忽略
    let mapped = match ev {
        zt::ZT_Event_ZT_EVENT_UP => Some("UP"),
        zt::ZT_Event_ZT_EVENT_ONLINE => Some("ONLINE"),
        zt::ZT_Event_ZT_EVENT_OFFLINE => Some("OFFLINE"),
        zt::ZT_Event_ZT_EVENT_DOWN => Some("DOWN"),
        zt::ZT_Event_ZT_EVENT_FATAL_ERROR_IDENTITY_COLLISION => Some("FATAL IDENTITY_COLLISION"),
        _ => None,
    };
    if let Some(e) = mapped {
        let _ = inner.event_out.send(e.to_string());
    }
}
