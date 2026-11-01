//! managed 路由差分应用:route_manager(unix 走 BSD 路由 socket / netlink,
//! RTM_ADD/RTM_DELETE;Windows 走 IP Helper 的 CreateIpForwardEntry2 /
//! DeleteIpForwardEntry2,poc4 POC Gate 1 实机验证)。
//!
//! # 架构红线
//!
//! - 默认路由(0.0.0.0/0、::/0)一律跳过,设备绝不劫持默认路由;
//! - managed 路由一律安装:本模块不做「目标被其它接口占用则跳过」的跨实例
//!   判断(该检测上移到 daemon 层做全局扫描),失败只告警不中断其余条目;
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

/// unix 侧无平台级过滤,行为与既有实现完全一致。
#[cfg(not(windows))]
fn platform_skip(_target: &IpNet) -> bool {
    false
}

pub struct RouteApplier {
    rm: RouteManager,
    if_index: u32,
    applied: Vec<ManagedRoute>,
}

impl RouteApplier {
    pub fn new(if_index: u32) -> io::Result<Self> {
        Ok(Self { rm: RouteManager::new()?, if_index, applied: Vec::new() })
    }

    pub fn applied(&self) -> &[ManagedRoute] {
        &self.applied
    }

    /// ManagedRoute -> route_manager::Route(on-link 用 if_index 绑定,有 via 用网关)。
    fn to_route(&self, r: &ManagedRoute) -> Option<Route> {
        if is_default(&r.target) {
            return None;
        }
        let mut route = Route::new(r.target.addr, r.target.prefix).with_if_index(self.if_index);
        if let Some(via) = r.via {
            route = route.with_gateway(via);
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
