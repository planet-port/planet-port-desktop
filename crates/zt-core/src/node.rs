//! NodeHandle:uptr 还原、6 必填回调 glue、new/delete。
//! 本任务只做"构造 → identity 生成 → 析构";驱动线程与 UDP 收包循环在 Task 3。
//!
//! FFI 约束:回调内禁 panic(不 unwrap、不索引越界)、不跨 FFI 保存 Rust 引用;
//! 同一 Node 的 process* 串行由调用方保证(本任务仅 start 内驱动一次)。

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::config::{NetConfig, NetConfigOp, NodeEvent, NodeParams, NodeSink};
use crate::sa;
use crate::state::{zt_obj, StateStore};
use zt_sys as zt;

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

// Task 3 驱动线程起才消费,字段暂未读取
#[allow(dead_code)]
pub(crate) enum FrameOut {
    Vnet { nwid: u64, src: u64, dst: u64, eth: u16, payload: Vec<u8> },
}

struct Inner {
    store: StateStore,
    udp: UdpSocket,
    // 以下通道由 Task 3 的驱动线程消费;本任务 receiver 暂存于 NodeHandle,shutdown 关闭
    frame_out: Sender<FrameOut>,
    event_out: Sender<NodeEvent>,
    netcfg_out: Sender<(u64, NetConfigOp, NetConfig)>,
    // Task 3 起下沉到驱动线程,由其转发给 sink;本任务仅保活签名
    _sink: Box<dyn NodeSink>,
    pub(crate) running: AtomicBool,
}

/// 回调出口通道的接收端;Task 3 交驱动线程消费,本任务保活至 shutdown。
#[allow(dead_code)]
struct CallbackRx {
    frame: Receiver<FrameOut>,
    event: Receiver<NodeEvent>,
    netcfg: Receiver<(u64, NetConfigOp, NetConfig)>,
}

pub struct NodeHandle {
    node: *mut zt::ZT_Node,
    inner: *mut Inner,
    // 保活到 shutdown,保证回调里的 send 失败只发生在 shutdown 之后
    _rx: CallbackRx,
}

unsafe impl Send for NodeHandle {}

impl NodeHandle {
    pub fn start(params: NodeParams, sink: impl NodeSink) -> Result<Self, NodeError> {
        std::fs::create_dir_all(&params.data_dir).map_err(NodeError::Io)?;
        let planet_path = params
            .planet_file
            .as_ref()
            .ok_or_else(|| NodeError::InvalidArg("planet_file 必填".into()))?;
        let planet = std::fs::read(planet_path).map_err(NodeError::Io)?;
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
        let (event_tx, event_rx) = channel();
        let (netcfg_tx, netcfg_rx) = channel();
        let inner = Box::into_raw(Box::new(Inner {
            store: StateStore::new(params.data_dir.clone(), world_id, params.planet_file.clone()),
            udp,
            frame_out: frame_tx,
            event_out: event_tx,
            netcfg_out: netcfg_tx,
            _sink: Box::new(sink),
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
        if rc != zt::ZT_ResultCode_ZT_RESULT_OK {
            unsafe {
                zt::ZT_Node_delete(node);
                drop(Box::from_raw(inner));
            }
            return Err(NodeError::DataStoreFailed(format!("首次后台任务失败 rc={rc}")));
        }
        Ok(Self {
            node,
            inner,
            _rx: CallbackRx { frame: frame_rx, event: event_rx, netcfg: netcfg_rx },
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

    pub fn shutdown(self) {
        unsafe {
            self.inner().running.store(false, Ordering::SeqCst);
            // Task 3 起:先 join 驱动/设备线程,再 delete
            zt::ZT_Node_delete(self.node);
            drop(Box::from_raw(self.inner));
        }
    }

    fn inner(&self) -> &Inner {
        unsafe { &*self.inner }
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
