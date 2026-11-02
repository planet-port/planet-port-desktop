use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PlanetKind {
    Official,
    Custom,
}

/// GET /status 的每一项;/planets 列表项复用同一结构
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanetStatus {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub color: String,
    #[serde(rename = "type")]
    pub kind: PlanetKind,
    pub status: String, // running | stopped
    pub online: bool,
    pub node_id: Option<String>,
    pub core_version: Option<String>,
    pub latency_ms: Option<u64>,
    pub network_count: usize,
    pub port: u16,
}

/// GET /planets/{id} 返回体
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanetDetail {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub color: String,
    #[serde(rename = "type")]
    pub kind: PlanetKind,
    pub world_id: u64,
    pub roots: Vec<String>,
    pub file_size: u64,
    pub revision_time_ms: u64,
    pub last_node_id: Option<String>,
    pub source: String,
    pub network_count: usize,
    pub port: u16,
    pub status: String,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteLine {
    pub target: String,
    pub via: Option<String>,
}

/// GET /networks 的每一项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEntry {
    pub planet: String,
    pub nwid: String,
    pub name: Option<String>,
    pub status: String,
    pub conflict: Option<String>, // address | subnet | timeout
    #[serde(rename = "type")]
    pub kind: String, // PUBLIC | PRIVATE
    pub mac: String,
    /// feth 设备名(dev 端);设备未建(REQUESTING 等未获配置阶段)为 None。
    /// serde default:旧版 daemon 响应无此键时反序列化仍成功
    #[serde(default)]
    pub interface: Option<String>,
    pub mtu: u32,
    pub assigned_addrs: Vec<String>,
    pub managed_routes: Vec<RouteLine>,
    pub dns_servers: Vec<String>,
    pub route_via_zero_tier: bool,
    pub route_via_locked: bool,
    pub dns_mode: String, // none | network | custom
    pub custom_dns: Vec<String>,
}

/// GET /peers 的每一项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerEntry {
    pub planet: String,
    pub address: String,
    pub role: String, // PLANET | MOON | LEAF
    pub version: Option<String>,
    pub latency_ms: Option<u64>,
    pub path: Option<String>, // None = relay
}

/// GET /moons 的每一项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoonEntry {
    pub planet: String,
    pub moon_id: String,
    pub seed: Option<String>,
    pub from_file: bool,
    pub cached: bool,
}

/// GET /events 的每一项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEntry {
    pub planet: String,
    pub ts_ms: u64,
    pub kind: String,
    pub message: String,
}

/// 统一错误体(候选列表仅 planet 歧义时出现)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<String>>,
}

/// GET /identity 的全局身份段
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalIdentity {
    pub exists: bool,
    /// 十位 hex 节点地址;全局身份缺失或不可解析时 None
    pub node_id: Option<String>,
}

/// GET /identity 的单 planet 段
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanetIdentity {
    pub id: String,
    pub name: String,
    /// "own" | "global"
    pub identity: String,
    pub node_id: Option<String>,
    pub running: bool,
}

/// POST /planets/{id}/share 返回体
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLink {
    pub url: String,
    pub expires_at: u64, // unix 秒
}
