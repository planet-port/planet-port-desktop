//! Windows NRPT 接线(DnsClientNrptRule,经 PowerShell 子进程):托管域的
//! 解析请求按 Namespace 分流到网络下发的 DNS 服务器,不动全局 resolver。
//! daemon 服务以 LocalSystem 运行,cmdlet 天然有权限;失败只记事件。
//!
//! # 规则归属约定(Task 7 的 cli uninstall 清理依赖,两处必须一致)
//!
//! NRPT 规则不可命名,以 `-Comment "planet-port:<nwid:016x>"` 标记归属:
//! - 本模块 leave/停止:Comment 精确等值匹配,只删本网络规则;
//! - daemon 启动残留清理 / cli uninstall 全清:Comment 前缀
//!   `planet-port:` 匹配,绝不触碰他产品的 NRPT 规则。
//!
//! # 与 posix 的差异
//!
//! - NRPT 是「按域分流」的增量规则,不改写任何既有 resolver 配置,没有
//!   「原 DNS 快照」要还原——移除规则即还原;
//! - 多网络规则相互独立,可同时存在,无「最后应用者拥有」的单资源约束;
//!   标记文件 `<config>/dns-applied-nrpt`(每网络一条)只用于去重(反复
//!   ConfigUpdate 不重写系统)与 owner 判定,标记丢了仍可按 Comment 清理
//!   (restore_leftover 兜底)。

use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::events::Ring;

/// 规则归属标记:Comment 前缀(cli uninstall 清理同款,勿单方改动)
const COMMENT_PREFIX: &str = "planet-port:";

/// NRPT 标记文件:与 posix 的 dns-applied 分名,存每网络一条的记录
const MARKER: &str = "dns-applied-nrpt";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NrptRecord {
    /// owner:planet id(decimal worldId)
    planet_id: String,
    nwid: u64,
    /// NRPT 分流的命名空间(网络下发的 DNS 搜索域,已去首部点)
    domain: String,
    /// 规则指向的 DNS 服务器
    servers: Vec<String>,
}

fn marker_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(MARKER)
}

fn read_records(config_dir: &Path) -> Vec<NrptRecord> {
    fs::read(marker_path(config_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// 写标记文件。失败只影响去重(下次同配置会重复重写),不影响清理——
/// 清理按 Comment 匹配规则,不依赖标记
fn write_records(config_dir: &Path, recs: &[NrptRecord]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(recs).map_err(|e| e.to_string())?;
    fs::write(marker_path(config_dir), bytes).map_err(|e| e.to_string())
}

/// 规则的 Comment 值(十六进制 16 位,与事件文案的 nwid 表述一致)
fn comment_of(nwid: u64) -> String {
    format!("{COMMENT_PREFIX}{nwid:016x}")
}

/// PowerShell 单引号字面量(内部 `''` 表一个单引号):domain 来自控制器
/// 下发的 netconf,注入只能到「规则内容」层面,进不了命令文本
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 执行一段 PowerShell;成功返回 stdout(已 trim),失败返回 stderr。
/// 调用方在 spawn_blocking 内,阻塞无害
fn run_ps(script: &str) -> Result<String, String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("无法执行 powershell: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            err
        })
    }
}

/// 命名空间归一化:去首部 `.`(NRPT 规则以「域及其子域」匹配,点前缀
/// 无意义);空串视为未下发,返回 None
fn norm_domain(domain: &str) -> Option<String> {
    let d = domain.trim().trim_start_matches('.').trim();
    if d.is_empty() { None } else { Some(d.to_string()) }
}

/// 删除匹配 filter(Where-Object 脚本块内容)的 NRPT 规则;无匹配时管道为
/// 空、cmdlet 不执行,天然幂等
fn remove_rules(filter: &str) -> Result<(), String> {
    run_ps(&format!(
        "Get-DnsClientNrptRule | Where-Object {{ {filter} }} | Remove-DnsClientNrptRule -Force"
    ))
    .map(|_| ())
}

fn remove_rules_of_nwid(nwid: u64) -> Result<(), String> {
    remove_rules(&format!("$_.Comment -eq {}", ps_quote(&comment_of(nwid))))
}

fn add_rule(nwid: u64, domain: &str, servers: &[String]) -> Result<(), String> {
    let ns = servers.iter().map(|s| ps_quote(s)).collect::<Vec<_>>().join(",");
    run_ps(&format!(
        "Add-DnsClientNrptRule -Namespace {} -NameServers {} -Comment {}",
        ps_quote(domain),
        ns,
        ps_quote(&comment_of(nwid))
    ))
    .map(|_| ())
}

/// 应用 NRPT 规则(须持 `dns_gate` 并在 `spawn_blocking` 内调用)。
/// 目标状态与当前一致时跳过;变更时先删本网络旧规则再建(整体替换)。
/// 网络未下发搜索域(NRPT 无命名空间可挂)如实记 `dnsFailed`。
pub(super) fn apply(
    config_dir: &Path,
    events: &Ring,
    planet_id: &str,
    nwid: u64,
    domain: Option<&str>,
    servers: &[String],
) {
    if servers.is_empty() {
        return;
    }
    let Some(domain) = domain.and_then(norm_domain) else {
        // 域撤销时清残留,避免旧规则持续分流:控制器先下发 domain+servers
        // 建过规则、后续撤掉 domain 只留 servers 时,若不清理,旧规则仍把旧
        // 域分流到旧服务器。标记里无本网络记录则系统本就无规则,维持原状。
        let recs = read_records(config_dir);
        let stale = recs
            .iter()
            .find(|r| r.planet_id == planet_id && r.nwid == nwid);
        if let Some(stale) = stale {
            match remove_rules_of_nwid(nwid) {
                Ok(()) => {
                    let recs: Vec<NrptRecord> = recs
                        .iter()
                        .filter(|r| !(r.planet_id == planet_id && r.nwid == nwid))
                        .cloned()
                        .collect();
                    if recs.is_empty() {
                        let _ = fs::remove_file(marker_path(config_dir));
                    } else {
                        let _ = write_records(config_dir, &recs);
                    }
                    events.push(
                        planet_id,
                        "dnsFailed",
                        &format!(
                            "网络 {nwid:016x} 域 {} 的 NRPT 残留规则已移除:网络未再下发 DNS 搜索域,Windows 无法按域分流",
                            stale.domain
                        ),
                    );
                }
                Err(m) => events.push(
                    planet_id,
                    "dnsFailed",
                    &format!(
                        "网络 {nwid:016x} NRPT 残留规则清理失败(网络未下发 DNS 搜索域):{m}"
                    ),
                ),
            }
        } else {
            events.push(
                planet_id,
                "dnsFailed",
                &format!(
                    "网络 {nwid:016x} NRPT 规则未建立:网络未下发 DNS 搜索域,Windows 无法按域分流"
                ),
            );
        }
        return;
    };
    let recs = read_records(config_dir);
    if recs
        .iter()
        .any(|r| r.planet_id == planet_id && r.nwid == nwid && r.domain == domain && r.servers == servers)
    {
        return; // 已是同一规则:netconf 反复 ConfigUpdate 不重写系统
    }
    match remove_rules_of_nwid(nwid).and_then(|()| add_rule(nwid, &domain, servers)) {
        Ok(()) => {
            let mut recs = recs;
            recs.retain(|r| !(r.planet_id == planet_id && r.nwid == nwid));
            recs.push(NrptRecord {
                planet_id: planet_id.to_string(),
                nwid,
                domain: domain.clone(),
                servers: servers.to_vec(),
            });
            match write_records(config_dir, &recs) {
                Ok(()) => events.push(
                    planet_id,
                    "dnsApplied",
                    &format!(
                        "网络 {nwid:016x} 域 {domain} 解析已分流至 {}(NRPT)",
                        servers.join(", ")
                    ),
                ),
                Err(m) => events.push(
                    planet_id,
                    "dnsFailed",
                    &format!("网络 {nwid:016x} NRPT 规则已建立但标记写入失败(重复应用不去重):{m}"),
                ),
            }
        }
        Err(m) => events.push(
            planet_id,
            "dnsFailed",
            &format!("网络 {nwid:016x} NRPT 规则应用失败:{m}"),
        ),
    }
}

/// 移除 NRPT 规则(net_leave / planet 停止 / 删除路径,须持 `dns_gate` 并在
/// `spawn_blocking` 内调用)。`nwid = None` 表示整个 planet(该 planet 名下
/// 全部记录)。owner 不匹配 → 不动系统。
pub(super) fn restore(config_dir: &Path, events: &Ring, planet_id: &str, nwid: Option<u64>) {
    let recs = read_records(config_dir);
    let mut kept: Vec<NrptRecord> = Vec::new();
    let mut removed: Vec<NrptRecord> = Vec::new();
    let mut err: Option<String> = None;
    for r in recs {
        // 已有失败即停止删规则:标记与系统状态必须同进退,留下的记录供
        // 下次 restore 重试(规则按 Comment 匹配,重复删幂等)
        if err.is_some() {
            kept.push(r);
            continue;
        }
        let owned = r.planet_id == planet_id && nwid.is_none_or(|n| r.nwid == n);
        if !owned {
            kept.push(r);
            continue;
        }
        match remove_rules_of_nwid(r.nwid) {
            Ok(()) => removed.push(r),
            Err(m) => {
                err = Some(m);
                kept.push(r);
            }
        }
    }
    if let Some(m) = err {
        if removed.is_empty() {
            events.push(
                planet_id,
                "dnsFailed",
                &format!("网络 {} NRPT 规则还原失败:{m}", fmt_targets(planet_id, nwid)),
            );
        } else {
            events.push(
                planet_id,
                "dnsFailed",
                &format!(
                    "网络 {} NRPT 规则已移除 {},其余还原失败:{m}",
                    fmt_targets(planet_id, nwid),
                    fmt_nwids(&removed),
                ),
            );
        }
        let _ = write_records(config_dir, &kept);
        return;
    }
    if removed.is_empty() {
        return; // 无本 owner 的记录:静默跳过
    }
    if kept.is_empty() {
        let _ = fs::remove_file(marker_path(config_dir));
    } else {
        let _ = write_records(config_dir, &kept);
    }
    for r in &removed {
        events.push(
            planet_id,
            "dnsRestored",
            &format!(
                "网络 {:016x} 域 {} 的 NRPT 分流已移除",
                r.nwid, r.domain
            ),
        );
    }
}

/// 启动残留清理(kill -9 未走 stop 路径):按 Comment 前缀无条件全清本产品
/// 规则并清标记文件——不依赖标记(kill -9 后标记可能与规则集脱节),
/// 前缀匹配保证不误删他产品规则。
pub(super) fn restore_leftover(config_dir: &Path, events: &Ring) {
    let recs = read_records(config_dir);
    let _ = fs::remove_file(marker_path(config_dir));
    match remove_rules(&format!("$_.Comment -like '{}*'", COMMENT_PREFIX)) {
        Ok(()) => {
            if recs.is_empty() {
                return;
            }
            // 一条事件占一位 owner 即可:残留是全局清理,无 per-network 语义
            events.push(
                &recs[0].planet_id,
                "dnsRestored",
                &format!("启动检测到 NRPT 残留,已按 {COMMENT_PREFIX} 前缀全清(涉及 {} 条记录)", recs.len()),
            );
        }
        Err(m) => {
            eprintln!("[dns] 残留 NRPT 规则清理失败: {m}");
            if let Some(r) = recs.first() {
                events.push(
                    &r.planet_id,
                    "dnsFailed",
                    &format!("启动检测到 NRPT 残留,清理失败:{m}"),
                );
            }
        }
    }
}

/// 事件文案:还原目标(nwid 已知给十六进制,未知 = 整 planet)
fn fmt_targets(planet_id: &str, nwid: Option<u64>) -> String {
    match nwid {
        Some(n) => format!("{n:016x}"),
        None => format!("planet {planet_id} 全部网络"),
    }
}

fn fmt_nwids(recs: &[NrptRecord]) -> String {
    recs.iter()
        .map(|r| format!("{:016x}", r.nwid))
        .collect::<Vec<_>>()
        .join(",")
}
