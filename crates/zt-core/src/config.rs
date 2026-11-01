//! 对外类型:Node 参数、回调交付的数据模型、ZT 结构体拷贝。
//!
//! # 架构红线
//!
//! - 本模块类型是回调(glue)与驱动线程之间传递的数据快照:`from_zt` 必须
//!   完整拷贝,绝不持有指向核心内存的指针/引用(回调返回后核心缓冲即失效);
//! - 回调内(调用 `from_zt` 处)只做拷贝 + channel send,不调用任何 sink。

use std::ffi::CStr;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use crate::sa;

/// Node 启动参数。
pub struct NodeParams {
    pub data_dir: PathBuf,
    /// 注入的 planet 文件(必填:本项目语义即以自定义 planet 接入,不用核心内置 world)
    pub planet_file: PathBuf,
    pub bind_port: u16,
}

/// 上层接收 Node 事件/网络配置/虚拟网帧的出口(Task 3 起由驱动线程转发)。
pub trait NodeSink: Send + 'static {
    fn on_event(&mut self, ev: NodeEvent);
    fn on_net_config(&mut self, nwid: u64, op: NetConfigOp, cfg: NetConfig);
    fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]);
    fn local_ipv4s(&self) -> Vec<std::net::Ipv4Addr>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeEvent {
    Up,
    Online,
    Offline,
    Down,
    IdentityCollision,
    Fatal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetConfigOp {
    Up,
    ConfigUpdate,
    Down,
    Destroy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpNet {
    pub addr: IpAddr,
    pub prefix: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRoute {
    pub target: IpNet,
    pub via: Option<IpAddr>,
    pub metric: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetStatus {
    RequestingConfiguration,
    Ok,
    AccessDenied,
    NotFound,
    PortError,
    ClientTooOld,
    AuthenticationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetConfig {
    pub nwid: u64,
    pub name: String,
    pub mac: u64,
    pub status: NetStatus,
    /// 是否私有网络(核心 ZT_VirtualNetworkType:PRIVATE=true / PUBLIC=false)
    pub is_private: bool,
    pub mtu: u16,
    pub netconf_revision: u64,
    pub assigned: Vec<IpNet>,
    pub routes: Vec<ManagedRoute>,
    pub dns_domain: Option<String>,
    pub dns_servers: Vec<IpAddr>,
}

/// 前缀长度编码在 sin_port(核心 InetAddress 以 port 字段存 netmaskBits)。
///
/// # Safety
/// 调用方须保证 `ss` 指向核心已初始化的字段。
unsafe fn ip_net_of(ss: &zt_sys::sockaddr_storage) -> Option<IpNet> {
    let sa = sa::from_sockaddr_storage(ss)?;
    Some(IpNet { addr: sa.ip(), prefix: sa.port() as u8 })
}

impl NetConfig {
    /// 从 ZT_VirtualNetworkConfig 拷贝(回调内指针不保证存活,必须立即拷)。
    ///
    /// # Safety
    /// `c` 必须指向核心已初始化的完整 `ZT_VirtualNetworkConfig`。
    pub unsafe fn from_zt(c: &zt_sys::ZT_VirtualNetworkConfig) -> Self {
        use zt_sys::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_ACCESS_DENIED as ACCESS_DENIED;
        use zt_sys::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_AUTHENTICATION_REQUIRED as AUTH_REQUIRED;
        use zt_sys::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_CLIENT_TOO_OLD as CLIENT_TOO_OLD;
        use zt_sys::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_NOT_FOUND as NOT_FOUND;
        use zt_sys::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_OK as OK;
        use zt_sys::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_PORT_ERROR as PORT_ERROR;
        use zt_sys::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_REQUESTING_CONFIGURATION as REQUESTING;
        let status = match c.status {
            REQUESTING => NetStatus::RequestingConfiguration,
            OK => NetStatus::Ok,
            ACCESS_DENIED => NetStatus::AccessDenied,
            NOT_FOUND => NetStatus::NotFound,
            PORT_ERROR => NetStatus::PortError,
            CLIENT_TOO_OLD => NetStatus::ClientTooOld,
            AUTH_REQUIRED => NetStatus::AuthenticationRequired,
            _ => NetStatus::RequestingConfiguration,
        };
        let name = CStr::from_ptr(c.name.as_ptr()).to_string_lossy().into_owned();
        // 切片上限做 min 防御:回调内禁 panic,核心计数异常时按数组容量截断
        let n_assigned = (c.assignedAddressCount as usize).min(c.assignedAddresses.len());
        let assigned = c.assignedAddresses[..n_assigned]
            .iter()
            .filter_map(|ss| ip_net_of(ss))
            .collect();
        let n_routes = (c.routeCount as usize).min(c.routes.len());
        let routes = c.routes[..n_routes]
            .iter()
            .filter_map(|r| {
                Some(ManagedRoute {
                    target: ip_net_of(&r.target)?,
                    via: sa::from_sockaddr_storage(&r.via).map(|a: SocketAddr| a.ip()),
                    metric: r.metric,
                })
            })
            .collect();
        let dns_domain_raw = CStr::from_ptr(c.dns.domain.as_ptr()).to_string_lossy().into_owned();
        let dns_domain = if dns_domain_raw.is_empty() { None } else { Some(dns_domain_raw) };
        let dns_servers = c
            .dns
            .server_addr
            .iter()
            .filter_map(|ss| sa::from_sockaddr_storage(ss).map(|a| a.ip()))
            .collect();
        Self {
            nwid: c.nwid,
            name,
            mac: c.mac,
            status,
            // Core/include/ZeroTierOne.h: ZT_NETWORK_TYPE_PRIVATE=0 / PUBLIC=1
            is_private: c.type_ == zt_sys::ZT_VirtualNetworkType_ZT_NETWORK_TYPE_PRIVATE,
            mtu: c.mtu as u16,
            // netconfRevision 为 c_ulong:mac/linux 64 位,Win32 下 32 位,
            // u64::from 收敛到固定宽度(mac/linux 上为恒等,零行为改动)
            netconf_revision: u64::from(c.netconfRevision),
            assigned,
            routes,
            dns_domain,
            dns_servers,
        }
    }
}
