//! macOS 系统 resolver 覆写(networksetup):网络本地配置 `dnsMode != none` 的
//! 网络在 netconf 应用时把 DNS 写进主网络服务,离开网络/停实例时还原。
//!
//! # 约束与简化
//!
//! - 全部走 `std::process::Command`,调用方(netconf sink / REST handler /
//!   停止路径)已在 `spawn_blocking` 内执行,**本模块不做任何异步操作**;
//! - 所有失败只记事件(`dnsFailed`),绝不向上返回错误:网络配置数据面
//!   不因 DNS 失败而失败;
//! - 多网络/多 planet「最后应用者拥有」:`<config>/dns-applied` 只存一条
//!   记录(服务名 + owner + servers),新的应用直接覆盖,不叠加;
//! - owner 不匹配(已被后应用者覆盖)时还原动作不动系统,只删不了——
//!   即谁最后写的谁负责还原;
//! - 「不再托管」(netconf 计算出的 servers 为空:dnsMode=none / network 未
//!   下发 / custom 无配置)同样走 `restore`:该网络不再托管应立即还原,
//!   owner 校验保证只动自己名下的记录;
//! - 所有调用方须先持 `AppState::dns_gate` 再进 `spawn_blocking`:本模块是
//!   「读标记 → 写系统/写标记」的 check-then-act,并发执行会让标记与系统
//!   真实状态脱节(错误还原/漏还原);
//! - 还原恢复 apply 时快照的「原手动 DNS」(`prior`,空 = 原本自动获取),
//!   而非一律 `Empty`:用户手动配过的 DNS 不因托管一轮而丢失。

use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::events::Ring;

/// `dns-applied` 标记文件:当前系统 DNS 归谁所有
const MARKER: &str = "dns-applied";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DnsRecord {
    /// 被覆写的网络服务名(networksetup 语义,非设备名)
    service: String,
    /// owner:planet id(decimal worldId)
    planet_id: String,
    nwid: u64,
    /// 已下发的 DNS 服务器
    servers: Vec<String>,
    /// apply 前该服务的手动 DNS 快照(空 = 原本自动获取);还原时恢复它。
    /// `#[serde(default)]` 兼容旧格式标记(缺字段按空处理)
    #[serde(default)]
    prior: Vec<String>,
}

fn marker_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(MARKER)
}

fn read_record(config_dir: &Path) -> Option<DnsRecord> {
    let bytes = fs::read(marker_path(config_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 写标记文件。失败向上返回:此时系统 DNS 已被 setdnsservers 改写但没有
/// marker 兜底,apply 据此改记 `dnsFailed`(不发 dnsApplied),让事件与
/// 系统事实一致
fn write_record(config_dir: &Path, rec: &DnsRecord) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(rec).map_err(|e| e.to_string())?;
    fs::write(marker_path(config_dir), bytes).map_err(|e| e.to_string())
}

/// `route -n get default` 取默认接口名。
/// 注意:route 此命令即使成功也常以非零码退出,只解析 stdout、不判退出码。
fn default_interface() -> Result<String, String> {
    let out = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .map_err(|e| format!("无法执行 route: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == "interface" {
                let iface = v.trim();
                if !iface.is_empty() {
                    return Ok(iface.to_string());
                }
            }
        }
    }
    Err("route 输出中没有 interface 行(无默认路由?)".into())
}

/// `networksetup -listnetworkserviceorder` 按设备名找网络服务名。
/// 真实输出形如(无 `-listnetworkservicehardwareport` 这个子命令):
///
/// ```text
/// An asterisk (*) denotes that a network service is disabled.
/// (1) PPPoE
/// (Hardware Port: AX88179A, Device: en10)
///
/// (5) Wi-Fi
/// (Hardware Port: Wi-Fi, Device: en0)
/// ```
///
/// 服务名可能含空格与 `/`(如 `USB 10/100/1G/2.5G LAN`),以 `(N)` 行整行截取。
fn service_of_device(device: &str) -> Result<String, String> {
    let out = Command::new("networksetup")
        .args(["-listnetworkserviceorder"])
        .output()
        .map_err(|e| format!("无法执行 networksetup: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        let Some(inner) = line.strip_prefix('(').and_then(|l| l.split_once(')')) else {
            continue;
        };
        let tag = inner.0.trim();
        let rest = inner.1.trim();
        if rest.is_empty() {
            // "(Hardware Port: Wi-Fi, Device: en0)" 行:此行无服务名,内容在 tag
            if let Some(dev) = tag.split(',').map(str::trim).find_map(|p| {
                p.strip_prefix("Device:").map(|d| d.trim().to_string())
            }) {
                if dev == device {
                    if let Some(name) = current {
                        return Ok(name);
                    }
                }
            }
        } else if tag == "*" || tag.chars().all(|c| c.is_ascii_digit()) {
            // "(1) 服务名" / "(*) 服务名"(禁用服务)
            current = Some(rest.to_string());
        }
    }
    Err(format!(
        "networksetup 服务列表中找不到设备 {device} 对应的网络服务"
    ))
}

/// `networksetup -setdnsservers`:servers 为空 → `Empty`(回到 DHCP/自动)
fn set_dnsservers(service: &str, servers: &[String]) -> Result<(), String> {
    let mut args = vec!["-setdnsservers".to_string(), service.to_string()];
    if servers.is_empty() {
        args.push("Empty".to_string());
    } else {
        args.extend(servers.iter().cloned());
    }
    let out = Command::new("networksetup")
        .args(&args)
        .output()
        .map_err(|e| format!("无法执行 networksetup: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "networksetup -setdnsservers {service} 失败: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// `networksetup -getdnsservers`:读某服务当前的手动 DNS。
/// 无手动配置时输出 "There aren't any DNS Servers set for <svc>...",返回空
fn get_dnsservers(service: &str) -> Result<Vec<String>, String> {
    let out = Command::new("networksetup")
        .args(["-getdnsservers", service])
        .output()
        .map_err(|e| format!("无法执行 networksetup: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.contains("aren't any DNS Servers") {
        return Ok(Vec::new());
    }
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// 应用系统 DNS(netconf 应用路径,须持 `dns_gate` 并在 `spawn_blocking` 内
/// 调用)。servers 为空不动系统(调用方对空 servers 走 `restore`);目标状态
/// 与当前一致时跳过(netconf 反复 ConfigUpdate 不应反复改写系统)。失败只记
/// `dnsFailed`。
pub fn apply(
    config_dir: &Path,
    events: &Ring,
    planet_id: &str,
    nwid: u64,
    servers: &[String],
) {
    if servers.is_empty() {
        return;
    }
    // 已是本 owner 下发的同一组服务器:无需重写系统
    if let Some(cur) = read_record(config_dir) {
        if cur.planet_id == planet_id && cur.nwid == nwid && cur.servers == servers {
            return;
        }
    }
    // prior 快照:已有任何 owner 的记录时沿用其 prior(被覆盖场景不重置快照,
    // 保住最早一次接管前的用户手动配置);否则读当前手动 DNS
    let prior = read_record(config_dir)
        .map(|cur| cur.prior)
        .unwrap_or_default();
    let r = default_interface()
        .and_then(|iface| service_of_device(&iface))
        .and_then(|service| match get_dnsservers(&service) {
            Ok(cur) if prior.is_empty() && cur != servers => Ok((service, cur)),
            Ok(_) => Ok((service, prior)),
            Err(e) => Err(e),
        })
        .and_then(|(service, prior)| match set_dnsservers(&service, servers) {
            Ok(()) => Ok((service, prior)),
            Err(e) => Err(e),
        });
    match r {
        Ok((service, prior)) => {
            let rec = DnsRecord {
                service,
                planet_id: planet_id.to_string(),
                nwid,
                servers: servers.to_vec(),
                prior,
            };
            match write_record(config_dir, &rec) {
                Ok(()) => events.push(
                    planet_id,
                    "dnsApplied",
                    &format!(
                        "网络 {nwid:016x} 系统 DNS 已切换为 {}",
                        servers.join(", ")
                    ),
                ),
                // setdnsservers 已生效但标记写不进去:重启后无残留可清理,
                // 也不该宣称应用成功,如实记 dnsFailed
                Err(m) => events.push(
                    planet_id,
                    "dnsFailed",
                    &format!(
                        "网络 {nwid:016x} 系统 DNS 已切换但标记文件写入失败\
                         (重启后无法自动还原):{m}"
                    ),
                ),
            }
        }
        Err(m) => events.push(
            planet_id,
            "dnsFailed",
            &format!("网络 {nwid:016x} 系统 DNS 应用失败:{m}"),
        ),
    }
}

/// 还原(net_leave / planet 停止 / 删除路径,须持 `dns_gate` 并在
/// `spawn_blocking` 内调用)。
/// `nwid = None` 表示整个 planet(停止/删除,任意 nwid 的记录都归它)。
/// owner 不匹配(已被后应用者覆盖)→ 不动系统。
pub fn restore(
    config_dir: &Path,
    events: &Ring,
    planet_id: &str,
    nwid: Option<u64>,
) {
    let Some(rec) = read_record(config_dir) else {
        return;
    };
    if rec.planet_id != planet_id {
        return;
    }
    if let Some(n) = nwid {
        if rec.nwid != n {
            return;
        }
    }
    match set_dnsservers(&rec.service, &rec.prior) {
        Ok(()) => {
            let _ = fs::remove_file(marker_path(config_dir));
            let what = if rec.prior.is_empty() {
                "自动获取".to_string()
            } else {
                rec.prior.join(", ")
            };
            events.push(
                planet_id,
                "dnsRestored",
                &format!("网络 {:016x} 系统 DNS 已还原为 {what}(服务 {})", rec.nwid, rec.service),
            );
        }
        Err(m) => events.push(
            planet_id,
            "dnsFailed",
            &format!("网络 {:016x} 系统 DNS 还原失败:{m}", rec.nwid),
        ),
    }
}

/// 启动早期残留清理(kill -9 未走 stop 路径):无条件还原并删标记文件。
/// 在恢复 planet 之前调用,避免恢复后的实例又被旧记录干扰。
/// 调用方须持 `dns_gate` 并在 `spawn_blocking` 内执行。
pub fn restore_leftover(config_dir: &Path, events: &Ring) {
    let Some(rec) = read_record(config_dir) else {
        return;
    };
    let r = set_dnsservers(&rec.service, &rec.prior);
    match r {
        Ok(()) => {
            let _ = fs::remove_file(marker_path(config_dir));
            let what = if rec.prior.is_empty() {
                "自动获取".to_string()
            } else {
                rec.prior.join(", ")
            };
            events.push(
                &rec.planet_id,
                "dnsRestored",
                &format!(
                    "启动检测到 dns-applied 残留,系统 DNS 已还原为 {what}(服务 {})",
                    rec.service
                ),
            );
        }
        Err(m) => {
            // 还原失败仍删标记:残留记录只会让后续 owner 判定出错,系统侧
            // 留下的旧 DNS 由用户手动恢复(事件里可见失败原因)
            let _ = fs::remove_file(marker_path(config_dir));
            eprintln!("[dns] 残留 DNS 还原失败: {m}");
            events.push(
                &rec.planet_id,
                "dnsFailed",
                &format!("启动检测到 dns-applied 残留,还原失败:{m}"),
            );
        }
    }
}
