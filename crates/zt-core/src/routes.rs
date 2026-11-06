//! managed 路由差分应用:route_manager(unix 走 BSD 路由 socket / netlink,
//! RTM_ADD/RTM_DELETE;Windows 走 IP Helper 的 CreateIpForwardEntry2 /
//! DeleteIpForwardEntry2,poc4 POC Gate 1 实机验证)。
//!
//! # 架构红线
//!
//! - 默认路由(0.0.0.0/0、::/0)一律跳过,设备绝不劫持默认路由;
//! - managed 路由一律安装:本模块不做「目标被其它接口占用则跳过」的跨实例
//!   判断(该检测上移到 daemon 层做全局扫描),失败只告警不中断其余条目;
//! - **不得压过本机直连路由**(2026-11-07 casaos 事故:G-Home 下发的
//!   `192.168.8.0/24 via ...` 与主机物理网段同前缀,metric 0 安装后把
//!   本机局域网流量全部引进隧道,主机从局域网失联约 3 分钟)。防护分平台:
//!   Linux/Windows 与官方客户端对齐,统一 metric 5000(直连路由 metric
//!   0/100 永远优先);macOS BSD 协议栈无 metric 概念,改为「目标网段覆盖
//!   其它接口上的本机地址则跳过安装」;
//! - teardown 尽力清理;内核在设备销毁后也会回收绑定其上的路由。
//!
//! # Windows 后端(自 poc4 routes_win.rs 平移)
//!
//! - 构造与 unix 同形:`RouteManager::new()` + `Route::new(addr, prefix)`
//!   `.with_if_index(..)` / `.with_gateway(..)`,由 route_manager 落到 IP Helper;
//! - 已知观察(POC Gate 1 实测):/24 探针路由与地址配置后系统自动生成的
//!   on-link 路由同前缀时,CreateIpForwardEntry2 报 error 87,属告警级——
//!   on-link 路由已覆盖该前缀,本条失败不影响连通,符合「单条失败告警继续」;
//! - **不安装任何 v6 托管路由**:wintun L3 设备面无 NDP 垫层(device_win.rs
//!   reader/send 侧对 v6 帧双向丢弃计 v6drop),此时装 v6 路由只会把双栈流量
//!   吸进黑洞适配器。Windows 侧对 v6 路由条目跳过并打告警,这是与 mac/linux
//!   的如实平台差异;待 v6 垫层(NDP)落地后再放开。

use std::io;
use std::net::IpAddr;

use crate::config::{IpNet, ManagedRoute};
use route_manager::{Route, RouteManager};

/// 路由识别键(忽略 metric:核心每次都带,不参与增删判定)。
fn key(r: &ManagedRoute) -> (IpNet, Option<IpAddr>) {
    (r.target.clone(), r.via)
}

/// 默认路由目标:前缀 0。
fn is_default(target: &IpNet) -> bool {
    target.prefix == 0
}

/// 托管路由安装 metric:与官方客户端(zerotier-one)对齐的 5000。内核
/// 直连路由 metric 为 0/100,本机物理网段的流量永远优先走直连,托管
/// 路由只在直连不存在时兜底——官方客户端在 Linux 上即此行为。
#[cfg(any(target_os = "linux", target_os = "windows"))]
const MANAGED_ROUTE_METRIC: u32 = 5000;

/// macOS 防护:目标网段是否覆盖「其它接口上的本机地址」。BSD 协议栈
/// 无路由 metric,同前缀的网关路由会压过直连路由(实机复现:G-Home 的
/// 192.168.8.0/24 via 抢走 en0 同网段流量),唯一安全做法是跳过安装。
/// v6 目标不判定(托管 v6 路由本就少,待有实网案例再补)。
#[cfg(target_os = "macos")]
fn shadows_local_subnet(target: &IpNet, own_if: &str) -> bool {
    let IpAddr::V4(t) = target.addr else { return false };
    let hit = crate::ifaddr::local_v4_by_if().into_iter().any(|(name, ip)| {
        name != own_if && ipv4_in_net(IpAddr::V4(ip), IpAddr::V4(t), target.prefix)
    });
    if hit {
        eprintln!(
            "[routes] 跳过 {}(覆盖其它接口的本机地址,装了会遮蔽其直连网段)",
            fmt_target(target)
        );
    }
    hit
}

#[cfg(target_os = "macos")]
fn ipv4_in_net(ip: IpAddr, net: IpAddr, prefix: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(ip), IpAddr::V4(net)) => {
            let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
            u32::from(ip) & mask == u32::from(net) & mask
        }
        _ => false,
    }
}

fn fmt_target(target: &IpNet) -> String {
    format!("{}/{}", target.addr, target.prefix)
}

/// Windows 侧平台级过滤:v6 目标一律不装(理由见模块注释「不安装 v6 托管
/// 路由」),与默认路由跳过同级的告警日志。
#[cfg(windows)]
fn platform_skip(target: &IpNet) -> bool {
    let v6 = matches!(target.addr, IpAddr::V6(_));
    if v6 {
        eprintln!(
            "[routes] 跳过 v6 路由 {}/{}(Windows 设备面无 v6 垫层,装了只会把流量吸进黑洞)",
            target.addr, target.prefix
        );
    }
    v6
}

/// macOS 走 shadows_local_subnet 防护(见上);Linux 的 metric 防护在
/// to_route 里,均不在本过滤钩子。非 macOS 的 unix 无平台级过滤。
#[cfg(not(windows))]
fn platform_skip(_target: &IpNet) -> bool {
    false
}

pub struct RouteApplier {
    rm: RouteManager,
    if_index: u32,
    if_name: String,
    applied: Vec<ManagedRoute>,
}

impl RouteApplier {
    pub fn new(if_index: u32, if_name: &str) -> io::Result<Self> {
        Ok(Self { rm: RouteManager::new()?, if_index, if_name: if_name.to_string(), applied: Vec::new() })
    }

    pub fn applied(&self) -> &[ManagedRoute] {
        &self.applied
    }

    /// ManagedRoute -> route_manager::Route(on-link 用 if_index 绑定,有 via 用网关;
    /// Linux/Windows 带 metric 5000,确保不压过本机直连路由,见模块红线)。
    fn to_route(&self, r: &ManagedRoute) -> Option<Route> {
        if is_default(&r.target) {
            return None;
        }
        let mut route = Route::new(r.target.addr, r.target.prefix).with_if_index(self.if_index);
        if let Some(via) = r.via {
            route = route.with_gateway(via);
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            route = route.with_metric(MANAGED_ROUTE_METRIC);
        }
        Some(route)
    }

    /// 差分应用:目标里新增的一律补上,已不在目标里的删除。
    /// 返回本次新安装的路由(供 daemon 记事件;与他网段共存的检测由 daemon
    /// 层做全局扫描,本模块不做跨实例判断)。
    /// 单条失败打告警继续(最常见:内核随地址自建的 on-link 路由已存在 EEXIST)。
    pub fn apply(&mut self, want: &[ManagedRoute]) -> Vec<ManagedRoute> {
        let mut installed: Vec<ManagedRoute> = Vec::new();
        let want_kept: Vec<&ManagedRoute> = want
            .iter()
            .filter(|r| !is_default(&r.target))
            .filter(|r| !platform_skip(&r.target))
            .filter(|r| {
                #[cfg(target_os = "macos")]
                { !shadows_local_subnet(&r.target, &self.if_name) }
                #[cfg(not(target_os = "macos"))]
                { true }
            })
            .collect();
        for r in &want_kept {
            if self.applied.iter().any(|a| key(a) == key(r)) {
                continue;
            }
            let Some(route) = self.to_route(r) else { continue };
            if let Err(e) = self.rm.add(&route) {
                eprintln!("[routes] add {}/{} 失败(忽略): {}", r.target.addr, r.target.prefix, e);
                continue;
            }
            self.applied.push((*r).clone());
            installed.push((*r).clone());
        }
        let stale: Vec<ManagedRoute> = self
            .applied
            .iter()
            .filter(|a| !want_kept.iter().any(|w| key(w) == key(a)))
            .cloned()
            .collect();
        for r in &stale {
            self.remove(r);
        }
        installed
    }

    fn remove(&mut self, r: &ManagedRoute) {
        let Some(route) = self.to_route(r) else { return };
        if let Err(e) = self.rm.delete(&route) {
            eprintln!("[routes] delete {}/{} 失败(忽略): {}", r.target.addr, r.target.prefix, e);
        }
        self.applied.retain(|a| key(a) != key(r));
    }

    /// 拆除全部已应用路由(尽力而为;设备销毁后内核也会回收绑定其上的路由)。
    pub fn teardown(&mut self) {
        let all: Vec<ManagedRoute> = self.applied.clone();
        for r in &all {
            self.remove(r);
        }
    }
}
