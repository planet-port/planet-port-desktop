//! managed 路由差分应用(route_manager 的 Windows 后端,CreateIpForwardEntry2)。
//! 语义平移 zt-core/routes.rs:默认路由一律跳过,绝不劫持默认路由;
//! 单条失败告警继续;teardown 尽力清理。
use std::io;
use std::net::IpAddr;
use route_manager::{Route, RouteManager};

/// 与 zt-core config::ManagedRoute 对应的精简条目(metric 不参与增删判定)。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RouteSpec {
    pub target: IpAddr,
    pub prefix: u8,
    pub via: Option<IpAddr>,
}

fn is_default(_target: &IpAddr, prefix: u8) -> bool {
    prefix == 0
}

pub struct RouteApplier {
    rm: RouteManager,
    if_index: u32,
    applied: Vec<RouteSpec>,
}

impl RouteApplier {
    pub fn new(if_index: u32) -> io::Result<Self> {
        Ok(Self {
            rm: RouteManager::new()?,
            if_index,
            applied: Vec::new(),
        })
    }

    fn to_route(&self, r: &RouteSpec) -> Option<Route> {
        if is_default(&r.target, r.prefix) {
            return None;
        }
        let mut route = Route::new(r.target, r.prefix).with_if_index(self.if_index);
        if let Some(via) = r.via {
            route = route.with_gateway(via);
        }
        Some(route)
    }

    /// 差分应用;返回本次新安装条数。
    pub fn apply(&mut self, want: &[RouteSpec]) -> usize {
        let mut installed = 0;
        for r in want {
            if is_default(&r.target, r.prefix) || self.applied.contains(r) {
                continue;
            }
            let Some(route) = self.to_route(r) else { continue };
            if let Err(e) = self.rm.add(&route) {
                eprintln!("[routes] add {}/{} 失败(忽略): {}", r.target, r.prefix, e);
                continue;
            }
            self.applied.push(r.clone());
            installed += 1;
        }
        let stale: Vec<RouteSpec> =
            self.applied.iter().filter(|a| !want.contains(a)).cloned().collect();
        for r in &stale {
            self.remove(r);
        }
        installed
    }

    fn remove(&mut self, r: &RouteSpec) {
        if let Some(route) = self.to_route(r) {
            if let Err(e) = self.rm.delete(&route) {
                eprintln!("[routes] delete {}/{} 失败(忽略): {}", r.target, r.prefix, e);
            }
        }
        self.applied.retain(|a| a != r);
    }

    pub fn teardown(&mut self) {
        let all: Vec<RouteSpec> = self.applied.clone();
        for r in &all {
            self.remove(r);
        }
    }
}
