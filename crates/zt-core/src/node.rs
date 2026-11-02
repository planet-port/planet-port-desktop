//! NodeHandle:uptr 还原、6 必填回调 glue、驱动线程、命令通道、new/delete。
//!
//! 线程模型:单一驱动线程串行调用全部 process*(poll UDP fd + 唤醒管道),
//! 回调出口通道(frame/event/netcfg)也在此线程消费并转发给 sink,
//! 从而保证"回调数据 → sink"与 process* 同线程,无需对 Node 加锁。
//!
//! # 架构红线
//!
//! - 回调 glue 只做「拷贝 + channel send」,绝不直接调 sink、不回调 Node 方法;
//! - 全部 sink 调用(on_event/on_net_config/on_frame/local_ipv4s)只发生在
//!   驱动线程,且以 catch_unwind 包裹:sink panic 不许穿越驱动线程/FFI;
//! - FrameOut(核心→设备,sink 写设备)与 FrameIn(设备→核心,
//!   processVirtualNetworkFrame)两条通道方向不可混用;
//! - 关停硬顺序:running=false → 唤醒 poll → Cmd::Stop → join 驱动线程
//!   → ZT_Node_delete → drop Inner(join 必须先于 delete,否则悬垂);
//! - `networks()/peers()` 等查询返回的核心缓冲必须用
//!   `ZT_Node_freeQueryResult` 释放(本模块内完成,不外借指针);
//! - FFI 边界禁 panic/unwrap;对端应答通道断开一律转 `NodeError`,不炸进程。
//!
//! FFI 约束:回调内禁 panic(不 unwrap、不索引越界)、不跨 FFI 保存 Rust 引用;
//! 同一 Node 的 process* 串行由驱动线程单线程保证;
//! ZT_Node_delete 前必须先 join 驱动线程(见 shutdown)。
//!
//! # 平台分叉(仅驱动机制,语义逐行保留;Unix 路径零行为改动)
//!
//! 1. 唤醒:Unix 用 CLOEXEC 管道(wake_wr/wake_rd + poll);Windows 用 mpsc
//!    `Sender<u8>`/`Receiver<u8>` + `recv_timeout`(等价 poll 阻塞语义)。
//! 2. UDP:Unix 是「非阻塞 socket + poll 排空」;Windows 是「阻塞 socket +
//!    读超时 + 独立读线程」——`recv_from` → `udp_rx` 通道 → 唤醒驱动线程,
//!    驱动线程每圈 `try_recv` 排空;读线程忽略 WSAECONNRESET(此前 send 到
//!    某地址收到 ICMP port-unreachable 后下一次 recv 必报,ZeroTier 官方同款)。
//! 3. `udp_fd`:Unix `as_raw_fd()`;Windows `as_raw_socket()`(winsock SOCKET,
//!    收发同一底层 socket,读线程侧句柄值为 `try_clone` 复制件)。
//! 4. `multicastSubscribe` 的 ADI 形参在 Win32 绑定下是 `c_ulong`(32 位)。

#[cfg(not(windows))]
use std::fs::File;
use std::io;
#[cfg(windows)]
use std::net::SocketAddr;
use std::net::UdpSocket;
#[cfg(not(windows))]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{channel, Receiver, Sender};
#[cfg(windows)]
use std::sync::Arc;
use std::thread::JoinHandle;
#[cfg(windows)]
use std::time::Duration;

use crate::config::{NetConfig, NetConfigOp, NodeEvent, NodeParams, NodeSink};
use crate::device::{FrameIn, FrameInTx};
use crate::sa;
use crate::state::{zt_obj, StateStore};
use zt_sys as zt;

/// 物理 recv 缓冲:ZT_MAX_PHYSPAYLOAD + ZT_MAX_HEADROOM
const PHY_BUF: usize = (zt::ZT_MAX_PHYSPAYLOAD + zt::ZT_MAX_HEADROOM) as usize;
/// 组播订阅扫描周期(ms)
const MCAST_SCAN_MS: i64 = 5000;
/// 设备健康巡检周期(ms):驱动线程以此节拍回调 sink.on_tick
const HEALTH_TICK_MS: i64 = 10_000;
/// poll 单次阻塞上限(ms)
#[cfg(not(windows))]
const POLL_MAX_MS: i32 = 500;
/// 唤醒等待上限(ms):Windows 无 poll,recv_timeout 等价原 poll 阻塞上限
#[cfg(windows)]
const WAKE_MAX_MS: i64 = 500;
/// UDP 读线程 socket 读超时(ms):读线程借此周期性检查 running,保证可被关停
/// (Windows 阻塞 recv_from 无 poll,必须以短超时换取停机响应)
#[cfg(windows)]
const UDP_READ_TIMEOUT_MS: u64 = 50;

/// 发给驱动线程的串行命令(与 process* 同线程执行,天然互斥)。
///
/// 除 Stop 外均携带 `Sender<Result<(), NodeError>>` 应答端:驱动线程执行完
/// 核心 C 调用后回报真实 rc,调用方才能把错误返回给上层(而非一律 Ok)。
pub(crate) enum Cmd {
    Join(u64, Sender<Result<(), NodeError>>),
    Leave(u64, Sender<Result<(), NodeError>>),
    Orbit(u64, u64, Sender<Result<(), NodeError>>),
    Deorbit(u64, Sender<Result<(), NodeError>>),
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

/// 核心 rc -> Result;非 OK 时把真实错误码报给调用方(不再吞成 Ok)。
fn rc_to_err(what: &str, rc: zt::ZT_ResultCode) -> Result<(), NodeError> {
    if rc == zt::ZT_ResultCode_ZT_RESULT_OK {
        return Ok(());
    }
    Err(NodeError::InvalidArg(format!("{what} 返回错误码 {rc}")))
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
    pub addr: std::net::SocketAddr,
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
    #[cfg(not(windows))]
    pub(crate) wake_wr: File,
    // 唤醒通道发送端(shutdown/命令/帧入/UDP 收包时发一字节让 recv_timeout 立即返回)
    #[cfg(windows)]
    pub(crate) wake_tx: Sender<u8>,
    #[cfg(not(windows))]
    pub(crate) running: AtomicBool,
    // Arc:Windows 的 UDP 读线程要跨线程检查同一开关
    #[cfg(windows)]
    pub(crate) running: Arc<AtomicBool>,
}

impl Inner {
    /// 唤醒驱动线程:Unix 向唤醒管道写一字节让 poll 立即返回;
    /// Windows 向唤醒通道发一字节让 recv_timeout 立即返回(均免 &mut)。
    fn wake(&self) {
        #[cfg(not(windows))]
        {
            let mut b = [1u8; 1];
            let _ = unsafe { libc::write(self.wake_wr.as_raw_fd(), b.as_mut_ptr() as *const _, 1) };
        }
        #[cfg(windows)]
        {
            let _ = self.wake_tx.send(1);
        }
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
    #[cfg(not(windows))]
    wake_rd: File,
    // Windows:UDP 读线程投递的物理包队列 + 唤醒接收端 + winsock SOCKET
    #[cfg(windows)]
    udp_rx: Receiver<(Vec<u8>, SocketAddr)>,
    #[cfg(windows)]
    wake_rx: Receiver<u8>,
    #[cfg(windows)]
    local_socket: i64,
    deadline: i64,
}

unsafe impl Send for Driver {}

pub struct NodeHandle {
    node: *mut zt::ZT_Node,
    inner: *mut Inner,
    // Some = 驱动线程存活;shutdown 时 join 后置 None
    driver: Option<JoinHandle<()>>,
    // Windows 独有:UDP 读线程(start 期 spawn,shutdown 时 join;晚于驱动线程退出无害)
    #[cfg(windows)]
    udp_reader: Option<JoinHandle<()>>,
    // Inner 是否已回收(shutdown 已执行);Drop 据此避免二次释放
    shut_down: bool,
}

// SAFETY:句柄仅由裸指针 + JoinHandle + bool 组成,可跨线程转移/共享,前提:
// - process*(含全部回调出口消费)只在唯一驱动线程执行(本模块保证);
// - join/leave/orbit/deorbit 经命令通道转发到驱动线程串行执行;
// - 查询类(networks/peers/status)由调用线程直接调核心查询接口:核心对
//   查询缓冲的分配/释放自洽(freeQueryResult),但 peers()/online 对 Peer
//   的 latency/version/paths 及 _online 的逐字段读不持核心互斥量,与驱动
//   线程的写构成**良性数据竞争** —— 最坏是读到撕裂前的旧值(时延/地址
//   脏读),不涉及指针有效性,属可接受的快照不精确;本期不改实现,
//   查询迁移到驱动线程命令通道留二期;
// - 结构安全(裸指针/channel/pipe)论证不变:dataStore/UDP 仅由驱动线程
//   触达;wake() 只对管道 fd 做 write(线程安全);shutdown 的 join 先于
//   ZT_Node_delete 的硬顺序不受调用线程影响。
// 上层(daemon)据此可在 REST 工作线程持有句柄,并把 start/shutdown 放进
// spawn_blocking。node/inner 指针不外借,调用方无其它形式触及。
unsafe impl Send for NodeHandle {}
unsafe impl Sync for NodeHandle {}

/// CLOEXEC 管道两端(用 File 持有,drop 即关闭)。Windows 无管道,走 mpsc。
#[cfg(not(windows))]
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
        // Unix:非阻塞 socket,驱动线程 poll 排空;Windows:阻塞 socket + 读超时,
        // 收包由独立读线程负责(见 run_udp_reader)
        #[cfg(not(windows))]
        udp.set_nonblocking(true).map_err(NodeError::Io)?;
        #[cfg(windows)]
        udp.set_read_timeout(Some(Duration::from_millis(UDP_READ_TIMEOUT_MS)))
            .map_err(NodeError::Io)?;
        // Windows:接收端克隆给读线程;发送端留在 Inner 供 wire_send(同一 SOCKET)
        #[cfg(windows)]
        let udp_rd = udp.try_clone().map_err(NodeError::Io)?;
        // winsock SOCKET(收发同一底层 socket,读线程侧句柄值为复制件);须在读线程 move 走克隆前取值
        #[cfg(windows)]
        let local_socket = udp_rd.as_raw_socket() as i64;
        let (frame_tx, frame_rx) = channel();
        let (frame_in_tx, frame_in_rx) = channel();
        let (event_tx, event_rx) = channel();
        let (netcfg_tx, netcfg_rx) = channel();
        let (cmd_tx, cmd_rx) = channel();
        #[cfg(not(windows))]
        let (wake_rd, wake_wr) = make_pipe().map_err(NodeError::Io)?;
        #[cfg(windows)]
        let (wake_tx, wake_rx) = channel::<u8>();
        #[cfg(windows)]
        let (udp_tx, udp_rx) = channel::<(Vec<u8>, SocketAddr)>();
        #[cfg(not(windows))]
        let running = AtomicBool::new(true);
        #[cfg(windows)]
        let running = Arc::new(AtomicBool::new(true));
        // 读线程的唤醒克隆:物理包投进 udp_rx 后即唤醒驱动线程(对齐 poll 语义)
        #[cfg(windows)]
        let wake_tx_rd = wake_tx.clone();
        let inner = Box::into_raw(Box::new(Inner {
            store: StateStore::new(
                params.data_dir.clone(),
                world_id,
                params.planet_file.clone(),
                params.identity_dir.clone(),
            ),
            udp,
            frame_out: frame_tx,
            frame_in: frame_in_tx,
            event_out: event_tx,
            netcfg_out: netcfg_tx,
            cmd_tx,
            #[cfg(not(windows))]
            wake_wr,
            #[cfg(windows)]
            wake_tx,
            #[cfg(not(windows))]
            running,
            #[cfg(windows)]
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
        // Windows:UDP 读线程先于驱动线程启动(阻塞 recv_from → 通道 → 唤醒 → 驱动
        // 线程 processWirePacket);起不来即回收节点(此刻尚未对外发包)
        #[cfg(windows)]
        let reader = std::thread::Builder::new()
            .name("zt-udp-reader".into())
            .spawn(move || run_udp_reader(udp_rd, running, udp_tx, wake_tx_rd))
            .map_err(|e| {
                unsafe {
                    zt::ZT_Node_delete(node);
                    drop(Box::from_raw(inner));
                }
                NodeError::Io(io::Error::other(format!("UDP 读线程启动失败: {e}")))
            })?;
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
            #[cfg(not(windows))]
            wake_rd,
            #[cfg(windows)]
            udp_rx,
            #[cfg(windows)]
            wake_rx,
            #[cfg(windows)]
            local_socket,
            deadline: if dl > 0 { dl } else { now_ms() + 1 },
        };
        let handle = std::thread::Builder::new()
            .name("zt-driver".into())
            .spawn(move || unsafe { run_driver(driver) })
            .map_err(|e| {
                // 线程起不来:节点尚未对外发包,直接清理;Windows 先置停 running
                // 让已启动的读线程(在 Inner drop 前仍持有发送端克隆)退出
                unsafe {
                    #[cfg(windows)]
                    (*inner).running.store(false, Ordering::SeqCst);
                    zt::ZT_Node_delete(node);
                    drop(Box::from_raw(inner));
                }
                NodeError::Io(io::Error::other(format!("驱动线程启动失败: {e}")))
            })?;
        Ok(Self {
            node,
            inner,
            driver: Some(handle),
            #[cfg(windows)]
            udp_reader: Some(reader),
            shut_down: false,
        })
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
    pub fn join(&self, nwid: u64) -> Result<(), NodeError> {
        self.send_cmd(|tx| Cmd::Join(nwid, tx))
    }

    /// 离开网络。实际 ZT_Node_leave 在驱动线程执行,rc 非 OK 时返回真实错误。
    pub fn leave(&self, nwid: u64) -> Result<(), NodeError> {
        self.send_cmd(|tx| Cmd::Leave(nwid, tx))
    }

    /// 加入 moon(实际 ZT_Node_orbit 在驱动线程执行,rc 非 OK 时返回真实错误)。
    pub fn orbit(&self, moon_world_id: u64, seed: u64) -> Result<(), NodeError> {
        self.send_cmd(|tx| Cmd::Orbit(moon_world_id, seed, tx))
    }

    /// 移除 moon(实际 ZT_Node_deorbit 在驱动线程执行,不存在时核心返回 OK)。
    pub fn deorbit(&self, moon_world_id: u64) -> Result<(), NodeError> {
        self.send_cmd(|tx| Cmd::Deorbit(moon_world_id, tx))
    }

    /// 设备→核心帧通道发送端(设备读线程用;发出后自动唤醒驱动线程)。
    pub fn frame_in_tx(&self) -> FrameInTx {
        #[cfg(not(windows))]
        {
            let wake = self.inner().wake_wr.try_clone().expect("复制唤醒管道写端失败");
            FrameInTx::new(self.inner().frame_in.clone(), wake)
        }
        #[cfg(windows)]
        {
            let wake = self.inner().wake_tx.clone();
            FrameInTx::new(self.inner().frame_in.clone(), wake)
        }
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
    ///
    /// 已知不精确:对每个 Peer 的 latency/version/paths 逐字段读不持核心
    /// 互斥量,与驱动线程的写存在良性数据竞争,可能读到旧值(见
    /// `unsafe impl Send/Sync` 的 SAFETY 注释);内存安全不受影响。
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
    /// 红线:阻塞等应答,故绝不可在 sink 回调(驱动线程)内调用本路径
    /// (join/leave/orbit/deorbit),否则自等自死锁。
    fn send_cmd(&self, make: impl FnOnce(Sender<Result<(), NodeError>>) -> Cmd) -> Result<(), NodeError> {
        let (tx, rx) = channel();
        self.inner().cmd_tx.send(make(tx)).map_err(|_| {
            NodeError::DataStoreFailed("驱动线程已退出".into())
        })?;
        // 唤醒 poll,让命令立即被处理
        self.inner().wake();
        rx.recv().map_err(|_| NodeError::DataStoreFailed("驱动线程已退出(命令无应答)".into()))?
    }

    /// 关停顺序(强制):running=false → 唤醒 poll → Cmd::Stop → join 驱动线程
    /// → [Windows: join UDP 读线程] → ZT_Node_delete → drop Inner。join 必须
    /// 先于 delete,否则驱动线程仍在使用已释放的 Node(悬垂)。
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
        // 读线程晚于驱动线程 join:其退出只依赖 running 开关与读超时,无悬垂
        #[cfg(windows)]
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

impl Drop for NodeHandle {
    fn drop(&mut self) {
        if self.shut_down {
            return;
        }
        // 兜底:调用方漏 shutdown。debug_assert 在测试/调试构建立即暴露,
        // 发布构建仍按硬顺序尽力回收,绝不泄漏 Node/驱动线程。
        debug_assert!(
            false,
            "NodeHandle 未显式 shutdown 即 drop,已兜底执行关停(顺序:驱动线程 join → ZT_Node_delete)"
        );
        self.shutdown_inner();
    }
}

/// sink 调用统一包裹:panic 不穿越驱动线程(架构红线)。
///
/// panic → 打日志 + 尽力推 `NodeEvent::Fatal` + running=false,返回 None;
/// 调用方见 None 须立即退出驱动线程(sink 已不可信,不杀进程)。
fn call_sink<R>(
    sink: &mut Box<dyn NodeSink>,
    running: &AtomicBool,
    what: &str,
    f: impl FnOnce(&mut dyn NodeSink) -> R,
) -> Option<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut **sink))) {
        Ok(r) => Some(r),
        Err(p) => {
            let msg = format!("sink panic({what}): {}", panic_msg(p));
            eprintln!("[driver] {msg}");
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sink.on_event(NodeEvent::Fatal(msg));
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
/// + UDP 收包消费(Windows);循环尾部 Unix poll / Windows recv_timeout 等待。
///
/// # Safety
/// `d.node`/`d.inner` 须为 start() 中有效分配且在 join 本线程前不被释放
/// (shutdown 的顺序保证)。
unsafe fn run_driver(mut d: Driver) {
    let inner = unsafe { &*d.inner };
    #[cfg(not(windows))]
    let udp_fd = inner.udp.as_raw_fd();
    let mut last_mcast_ms: i64 = i64::MIN / 4; // 首圈立即做一次订阅
    let mut last_health_ms: i64 = i64::MIN / 4; // 首圈立即做一次巡检
    // 已 join 的网络(来自 vnet_config 回调的 UP/UPDATE/DOWN/DESTROY)
    let mut current_networks: Vec<u64> = Vec::new();
    #[cfg(not(windows))]
    let mut buf = [0u8; PHY_BUF];
    #[cfg(not(windows))]
    let mut polls = [
        libc::pollfd { fd: udp_fd, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: d.wake_rd.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    // POLLERR/POLLNVAL 日志去重:状态保持时只报一次,避免忙转刷屏
    #[cfg(not(windows))]
    let mut poll_reported = [false; 2];
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
                    s.on_event(NodeEvent::Fatal(format!("核心致命错误 rc={rc}")))
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
                Ok(Cmd::Leave(nwid, reply)) => {
                    let mut nuptr: *mut core::ffi::c_void = std::ptr::null_mut();
                    let rc = unsafe {
                        zt::ZT_Node_leave(d.node, nwid, &mut nuptr, std::ptr::null_mut())
                    };
                    current_networks.retain(|&x| x != nwid);
                    let _ = reply.send(rc_to_err("ZT_Node_leave", rc));
                }
                Ok(Cmd::Orbit(world_id, seed, reply)) => {
                    let rc = unsafe {
                        zt::ZT_Node_orbit(d.node, std::ptr::null_mut(), world_id, seed)
                    };
                    let _ = reply.send(rc_to_err("ZT_Node_orbit", rc));
                }
                Ok(Cmd::Deorbit(world_id, reply)) => {
                    let rc = unsafe {
                        zt::ZT_Node_deorbit(d.node, std::ptr::null_mut(), world_id)
                    };
                    let _ = reply.send(rc_to_err("ZT_Node_deorbit", rc));
                }
                Ok(Cmd::Stop) => return,
                Err(_) => break,
            }
        }
        // 设备健康巡检节拍:Windows 侧检测适配器被外部重启等设备失效并自愈
        if now - last_health_ms >= HEALTH_TICK_MS {
            last_health_ms = now;
            if call_sink(&mut d.sink, &inner.running, "on_tick", |s| s.on_tick()).is_none() {
                return;
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
            if call_sink(&mut d.sink, &inner.running, "on_net_config", |s| {
                s.on_net_config(nwid, op, cfg)
            })
            .is_none()
            {
                return;
            }
        }
        // 事件 → sink
        while let Ok(ev) = d.event_rx.try_recv() {
            if call_sink(&mut d.sink, &inner.running, "on_event", |s| s.on_event(ev)).is_none() {
                return;
            }
        }
        // 核心产出帧 → sink(由 sink 写设备;不得在此回喂 processVirtualNetworkFrame)
        while let Ok(f) = d.frame_rx.try_recv() {
            let FrameOut::Vnet { nwid, src, dst, eth, payload } = f;
            if call_sink(&mut d.sink, &inner.running, "on_frame", |s| {
                s.on_frame(nwid, src, dst, eth, &payload)
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
                    s.on_event(NodeEvent::Fatal(format!("processVirtualNetworkFrame rc={rc}")))
                })
                .is_none()
            {
                return;
            }
        }
        // 组播订阅:每 5000ms 对本机每个 IPv4 订阅 (ffffffffffff, ADI=IPv4 主机序)
        if now - last_mcast_ms >= MCAST_SCAN_MS {
            last_mcast_ms = now;
            let Some(ips) = call_sink(&mut d.sink, &inner.running, "local_ipv4s", |s| {
                s.local_ipv4s()
            }) else {
                return;
            };
            for nwid in &current_networks {
                for ip in &ips {
                    // ADI=IPv4 主机序;Win32 绑定下形参 c_ulong 为 32 位,类型天然成立
                    #[cfg(not(windows))]
                    let adi = u32::from(*ip) as libc::c_ulong;
                    #[cfg(windows)]
                    let adi = u32::from(*ip) as std::os::raw::c_ulong;
                    let _ = unsafe {
                        zt::ZT_Node_multicastSubscribe(
                            d.node,
                            std::ptr::null_mut(),
                            *nwid,
                            0xffff_ffff_ffff,
                            adi,
                        )
                    };
                }
            }
        }
        #[cfg(not(windows))]
        {
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
            // POLLERR/POLLNVAL:fd 级异常,两者后果天差地别。NVAL = fd 无效
            // (节点已不可用,置 running=false 并退出驱动线程防忙转);ERR 多为
            // 暂态(如 ICMP 差错),**只打 stderr 日志,不发 Fatal** —— 上层把
            // Fatal 视为停机,一次暂态 ERR 不应冻结设备面。日志按状态保持去重。
            for (i, p) in polls.iter().enumerate() {
                let err = p.revents & libc::POLLERR != 0;
                let nval = p.revents & libc::POLLNVAL != 0;
                if !err && !nval {
                    poll_reported[i] = false;
                    continue;
                }
                let what = if i == 0 { "UDP socket" } else { "唤醒管道" };
                let kind = if nval { "POLLNVAL(fd 无效)" } else { "POLLERR(多为暂态,已忽略)" };
                if !poll_reported[i] {
                    poll_reported[i] = true;
                    eprintln!("[driver] {what} poll 异常: {kind}");
                }
                if nval {
                    inner.running.store(false, Ordering::SeqCst);
                    return;
                }
            }
        }
        #[cfg(windows)]
        {
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
        }
        if !inner.running.load(Ordering::SeqCst) {
            return;
        }
    }
}

/// UDP 读线程(Windows):阻塞 recv_from → `(payload, from)` 投通道 → 唤醒
/// 驱动线程。读超时(start 期已设)使循环可周期性检查 running,保证可被关停。
///
/// 唤醒对齐 Unix poll 语义:fd 可读即唤醒驱动线程,包不等待下一 tick;
/// 每包只唤醒一次,驱动线程每圈把 udp_rx 排空后再睡,不忙转。
///
/// 错误策略:
/// - 读超时/WouldBlock:正常轮询出口,回到循环头检查 running;
/// - ConnectionReset/ConnectionRefused:Windows 特有 —— 此前 send 到某地址
///   收到 ICMP port-unreachable 后,下一次 recv 会报 WSAECONNRESET。必须
///   忽略,否则一次不可达就把读线程打死(ZeroTier 官方同款处理);
/// - 其他错误退避 200ms 继续,不因一次 IO 抖动永久退出。
#[cfg(windows)]
fn run_udp_reader(
    sock: UdpSocket,
    running: Arc<AtomicBool>,
    tx: Sender<(Vec<u8>, SocketAddr)>,
    wake: Sender<u8>,
) {
    let mut buf = vec![0u8; PHY_BUF];
    // 兜底:克隆端不继承读超时的平台差异下,这里再设一次,保证关停可被响应
    let _ = sock.set_read_timeout(Some(Duration::from_millis(UDP_READ_TIMEOUT_MS)));
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
