//! ZT_VirtualNetworkConfig 的完整拷贝(回调内指针不保证存活,必须立即拷)。
//! 字段语义与 zt-core/config.rs 一致,这里只裁掉 POC 不用的 DNS 字段。
//!
//! Windows 绑定差异:netconfRevision 在 Win32 下是 `c_ulong`(32 位),
//! 拷贝时经 `u64::from` 收敛到固定宽度字段。

use std::ffi::CStr;
use std::net::IpAddr;

use crate::sa;
use zt_sys as zt;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

/// 前缀长度编码在 sin_port(核心 InetAddress 以 port 字段存 netmaskBits)。
///
/// # Safety
/// 调用方须保证 `ss` 指向核心已初始化的字段。
unsafe fn ip_net_of(ss: &zt::sockaddr_storage) -> Option<IpNet> {
    let sa = sa::from_sockaddr_storage(ss)?;
    Some(IpNet { addr: sa.ip(), prefix: sa.port() as u8 })
}

impl NetConfig {
    /// 从 ZT_VirtualNetworkConfig 拷贝(回调内指针不保证存活,必须立即拷)。
    ///
    /// # Safety
    /// `c` 必须指向核心已初始化的完整 `ZT_VirtualNetworkConfig`。
    pub unsafe fn from_zt(c: &zt::ZT_VirtualNetworkConfig) -> Self {
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_ACCESS_DENIED as ACCESS_DENIED;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_AUTHENTICATION_REQUIRED as AUTH_REQUIRED;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_CLIENT_TOO_OLD as CLIENT_TOO_OLD;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_NOT_FOUND as NOT_FOUND;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_OK as OK;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_PORT_ERROR as PORT_ERROR;
        use zt::ZT_VirtualNetworkStatus_ZT_NETWORK_STATUS_REQUESTING_CONFIGURATION as REQUESTING;
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
                    via: sa::from_sockaddr_storage(&r.via).map(|a| a.ip()),
                    metric: r.metric,
                })
            })
            .collect();
        Self {
            nwid: c.nwid,
            name,
            mac: c.mac,
            status,
            // Core/include/ZeroTierOne.h: ZT_NETWORK_TYPE_PRIVATE=0 / PUBLIC=1
            is_private: c.type_ == zt::ZT_VirtualNetworkType_ZT_NETWORK_TYPE_PRIVATE,
            mtu: c.mtu as u16,
            netconf_revision: u64::from(c.netconfRevision),
            assigned,
            routes,
        }
    }
}
