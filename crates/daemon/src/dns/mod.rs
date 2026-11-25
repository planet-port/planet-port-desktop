//! 系统 DNS 接线(平台分叉):网络本地配置 `dnsMode != none` 的网络在
//! netconf 应用时把 DNS 写进系统,离开网络/停实例时还原。
//!
//! - macos / linux(`posix`):networksetup 写主网络服务的 resolver;
//! - windows(`win`):NRPT(DnsClientNrptRule)按托管域分流解析请求。
//!
//! # 约束与简化(两平台同构)
//!
//! - 全部走 `std::process::Command`,调用方(netconf sink / REST handler /
//!   停止路径)已在 `spawn_blocking` 内执行,**本模块不做任何异步操作**;
//! - 所有失败只记事件(`dnsFailed`),绝不向上返回错误:网络配置数据面
//!   不因 DNS 失败而失败;
//! - 「最后应用者拥有」语义与标记文件:posix 是单一 resolver 资源,
//!   `<config>/dns-applied` 只存一条记录;windows 规则按网络独立存在,
//!   `<config>/dns-applied-nrpt` 存每网络一条(两文件互不兼容,分名以免
//!   跨平台混读);
//! - 「不再托管」(netconf 计算出的 servers 为空:dnsMode=none / network 未
//!   下发 / custom 无配置)同样走 `restore`:该网络不再托管应立即还原,
//!   owner 校验保证只动自己名下的记录;
//! - 所有调用方须先持 `AppState::dns_gate` 再进 `spawn_blocking`:本模块是
//!   「读标记 → 写系统/写标记」的 check-then-act,并发执行会让标记与系统
//!   真实状态脱节(错误还原/漏还原)。
//!
//! 各平台细节(快照还原/规则命名约定)见子模块头注释。

#[cfg(not(windows))]
mod posix;

#[cfg(windows)]
mod win;

use std::path::Path;

use crate::events::Ring;

/// 应用系统 DNS(netconf 应用路径,须持 `dns_gate` 并在 `spawn_blocking` 内
/// 调用)。servers 为空不动系统(调用方对空 servers 走 `restore`)。
/// `domain` 是网络下发的 DNS 搜索域,仅 Windows NRPT 需要(按域分流的
/// 命名空间);posix 忽略该参数,mac/linux 行为零改动。
pub fn apply(
    config_dir: &Path,
    events: &Ring,
    planet_id: &str,
    nwid: u64,
    domain: Option<&str>,
    servers: &[String],
) {
    #[cfg(windows)]
    {
        win::apply(config_dir, events, planet_id, nwid, domain, servers);
    }
    #[cfg(not(windows))]
    {
        let _ = domain;
        posix::apply(config_dir, events, planet_id, nwid, servers);
    }
}

/// 还原(net_leave / planet 停止 / 删除路径,须持 `dns_gate` 并在
/// `spawn_blocking` 内调用)。
/// `nwid = None` 表示整个 planet(停止/删除,任意 nwid 的记录都归它)。
/// owner 不匹配(已被后应用者覆盖)→ 不动系统。
pub fn restore(config_dir: &Path, events: &Ring, planet_id: &str, nwid: Option<u64>) {
    #[cfg(windows)]
    {
        win::restore(config_dir, events, planet_id, nwid);
    }
    #[cfg(not(windows))]
    {
        posix::restore(config_dir, events, planet_id, nwid);
    }
}

/// 启动早期残留清理(kill -9 未走 stop 路径):无条件还原。
/// 在恢复 planet 之前调用,避免恢复后的实例又被旧记录干扰。
/// 调用方须持 `dns_gate` 并在 `spawn_blocking` 内执行。
pub fn restore_leftover(config_dir: &Path, events: &Ring) {
    #[cfg(windows)]
    {
        win::restore_leftover(config_dir, events);
    }
    #[cfg(not(windows))]
    {
        posix::restore_leftover(config_dir, events);
    }
}
