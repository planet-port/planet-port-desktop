//! daemon 共享状态:AppState、PlanetRuntime、冲突扫描用的网段快照缓存。
//!
//! # 锁纪律(全模块适用)
//!
//! - AppState 的锁(尤其 registry / planets)不跨 await、不在持锁期间做
//!   node 阻塞调用(join/leave 应答等待、shutdown);
//! - sink(驱动线程)只碰 plane / net_local / net_cache / conflicts / Ring
//!   这类短临界区 Mutex,绝不取 registry / planets 锁(stop 路径持后者时
//!   sink 仍在跑,取了即互等);
//! - 嵌套只有单向:registry → net_local(net_local 从不反向包 registry)。

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum::Json;
use planet_port_api::types::ApiError;
use zt_core::{DevicePlane, IpNet, NodeHandle, TransportInfo};

use crate::events::Ring;
use crate::registry::{NetLocal, Registry};

/// 单 planet 实例的运行时(handle + 设备面 + sink 共享的缓存)。
/// 存放在 `AppState::planets`,以 `Arc` 供 handlers 短锁取出后无锁使用。
pub struct PlanetRuntime {
    pub planet_id: String,
    /// Some = 实例存活;`shutdown(self)` 消费所有权,故停在短锁 Mutex 里,
    /// stop 时 take 走 shutdown,期间查询一律按「已停止」处理
    pub node: Mutex<Option<NodeHandle>>,
    pub plane: Arc<Mutex<DevicePlane>>,
    /// stop 流程置位;sink 见 true 即不再动设备(stop 正在拆)
    pub stopping: Arc<AtomicBool>,
    /// nwid → "subnet" | "address"(冲突扫描结果,networks 合成用)
    pub conflicts: Arc<Mutex<HashMap<u64, String>>>,
    /// nwid → NetLocal:registry.net_local 的运行时镜像,sink 在驱动线程
    /// 读它取 route_via_zero_tier,set/join handler 写它(registry 为持久层)。
    /// 选它而非"sink 直接锁 registry":stop 路径持 registry 锁时 sink 若也
    /// 等同一把锁,驱动线程会被 REST 拖住(死等风险),镜像代价最小。
    pub net_local: Arc<Mutex<HashMap<u64, NetLocal>>>,
}

impl PlanetRuntime {
    fn with_node<T>(&self, f: impl FnOnce(&NodeHandle) -> T) -> Option<T> {
        let g = self.node.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.as_ref().map(f)
    }

    /// 核心是否 online(实例已停 → false)
    pub fn online(&self) -> bool {
        self.with_node(|n| n.online()).unwrap_or(false)
    }

    /// 网络配置快照(实例已停 → 空)
    pub fn networks(&self) -> Vec<zt_core::NetConfig> {
        self.with_node(|n| n.networks()).unwrap_or_default()
    }

    /// 指定网络当前的 feth 设备名(dev 端;未建设备/已停 → None)。
    /// /networks 展示用,plane 短锁
    pub fn if_name(&self, nwid: u64) -> Option<String> {
        self.plane
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .if_name_of(nwid)
            .map(|s| s.to_string())
    }

    /// 对端快照(实例已停 → 空)
    pub fn peers(&self) -> Vec<zt_core::PeerInfo> {
        self.with_node(|n| n.peers()).unwrap_or_default()
    }

    /// 节点地址十字节 hex(实例已停 → None)
    pub fn node_id(&self) -> Option<String> {
        self.with_node(|n| format!("{:010x}", n.address()))
    }

    /// 通道状态快照(实例已停 → None)
    pub fn transport(&self) -> Option<TransportInfo> {
        self.with_node(|n| n.transport())
    }

    /// join(命令转发驱动线程,阻塞等应答;调用方须放 spawn_blocking)
    pub fn node_join(&self, nwid: u64) -> Result<(), zt_core::NodeError> {
        match self.node.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            Some(n) => n.join(nwid),
            None => Err(zt_core::NodeError::DataStoreFailed("实例已停止".into())),
        }
    }

    /// leave(同上)
    pub fn node_leave(&self, nwid: u64) -> Result<(), zt_core::NodeError> {
        match self.node.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            Some(n) => n.leave(nwid),
            None => Err(zt_core::NodeError::DataStoreFailed("实例已停止".into())),
        }
    }

    /// orbit(moon 命令在驱动线程执行并阻塞等应答;调用方须放 spawn_blocking)
    pub fn node_orbit(&self, moon_world_id: u64, seed: u64) -> Result<(), zt_core::NodeError> {
        match self.node.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            Some(n) => n.orbit(moon_world_id, seed),
            None => Err(zt_core::NodeError::DataStoreFailed("实例已停止".into())),
        }
    }

    /// deorbit(同上)
    pub fn node_deorbit(&self, moon_world_id: u64) -> Result<(), zt_core::NodeError> {
        match self.node.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            Some(n) => n.deorbit(moon_world_id),
            None => Err(zt_core::NodeError::DataStoreFailed("实例已停止".into())),
        }
    }
}

/// (planet_id, nwid) → 该网络的网段/IP 快照。
/// sink 在 netconf apply 后写入,冲突扫描读它(据此避免在驱动线程调用
/// 其它 planet 的核心查询接口);stop/leave 时移除。
pub type NetCache = Arc<Mutex<HashMap<(String, u64), NetSnapshot>>>;

/// 一条网络的网段快照(冲突扫描数据源)
#[derive(Debug, Clone)]
pub struct NetSnapshot {
    pub planet_name: String,
    /// assigned 网段 + managed 路由 target
    pub subnets: Vec<IpNet>,
    /// assigned 地址(判断 address 级冲突)
    pub addrs: Vec<IpAddr>,
}

/// 单 planet 的实时状态(GET /status、/planets、/planets/{id} 合成用)。
#[derive(Debug, Clone)]
pub struct LiveInfo {
    pub online: bool,
    pub node_id: String,
    pub core_version: String,
    pub latency_ms: Option<u64>,
    pub network_count: usize,
    /// 通道状态快照(实例已停 / 节点槽位已空 → None)
    pub transport: Option<TransportInfo>,
}

/// 一条分享链接记录:token → (planet id, 过期时间 unix 秒)
pub struct ShareRecord {
    pub planet_id: String,
    pub expires_unix: u64,
}

#[derive(Clone)]
pub struct AppState {
    /// 运行时配置目录(--config-dir / env / 三级解析结果)
    pub config_dir: PathBuf,
    /// daemon 实际监听的 API 端口(share 链接 url 需要它)
    pub api_port: u16,
    pub registry: Arc<Mutex<Registry>>,
    pub events: Arc<Ring>,
    /// planet id → 运行时(Arc 化:handlers 取出后即释放 map 锁再做 node 调用)
    pub planets: Arc<Mutex<HashMap<String, Arc<PlanetRuntime>>>>,
    /// start/stop 串行门闩(异步锁,允许跨 await):并发双 start 会让第二个
    /// NodeHandle 覆盖第一个造成线程/端口泄漏,故启停全程持门闩
    pub runtime_gate: Arc<tokio::sync::Mutex<()>>,
    /// DNS 托管串行门闩(异步锁):dns.rs 的 apply/restore/restore_leftover
    /// 都是「读标记 → 写系统/写标记」的 check-then-act,并发执行会让
    /// dns-applied 标记与系统真实状态脱节(错误还原/漏还原)。所有 DNS 调用
    /// 路径(netconf spawn 出的 apply/restore、net_leave、teardown_runtime、
    /// 启动 restore_leftover)先持本锁再进 spawn_blocking;锁只包 DNS 操作,
    /// 绝不包 netconf/网络面逻辑
    pub dns_gate: Arc<tokio::sync::Mutex<()>>,
    /// 全部 planet 的网络网段快照(冲突扫描数据源,见 NetCache)
    pub net_cache: NetCache,
    /// token → (planet id, 过期 unix 秒);仅内存,重启即失效
    pub shares: Arc<Mutex<HashMap<String, ShareRecord>>>,
    pub token: Arc<String>,
}

/// 统一错误响应体 `{"error","message","candidates"}`(REST 契约定义的形态)
pub fn err(
    code: StatusCode,
    error: &str,
    message: &str,
    candidates: Option<Vec<String>>,
) -> (StatusCode, Json<ApiError>) {
    (
        code,
        Json(ApiError {
            error: error.to_string(),
            message: message.to_string(),
            candidates,
        }),
    )
}

/// 取运行时快照(短锁 clone Arc,不在持锁期间做 node 调用)
pub fn runtime_of(state: &AppState, id: &str) -> Option<Arc<PlanetRuntime>> {
    state.planets.lock().unwrap().get(id).cloned()
}
