use crate::client::{CliError, Client};
use crate::cmd::{planet_label, resolve_single};
use crate::output::{fmt_value, print_json};
use clap::Subcommand;
use planet_port_api::endpoints;
use planet_port_api::types::{ApiError, NetworkEntry};
use serde_json::{json, Value};

/// 变更类回执:文本带 planet 落点 `name(id) 200 join OK`(定位链②③生效时
/// 用户看不到目标,回执必须自报家门;落点拿不到时退化为 `200 join OK`),
/// -j 输出真实响应体
fn ok(client: &Client, verb: &str, planet: &Option<String>, json: bool, body: &Value) {
    if json {
        print_json(body);
        return;
    }
    match planet_label(client, planet) {
        Some(label) => println!("{label} 200 {verb} OK"),
        None => println!("200 {verb} OK"),
    }
}

/// 204 空响应体(deorbit/remove)在 -j 下代以 {"result":"ok"},消除 null
fn body_or_ok(v: Value) -> Value {
    if v.is_null() {
        json!({ "result": "ok" })
    } else {
        v
    }
}

pub fn join(client: &Client, nwid: &str, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let resp = client.post_empty(&endpoints::network_join(nwid, planet.as_deref()))?;
    ok(client, "join", planet, json, &resp);
    Ok(())
}

pub fn leave(client: &Client, nwid: &str, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let resp = client.post_empty(&endpoints::network_leave(nwid, planet.as_deref()))?;
    ok(client, "leave", planet, json, &resp);
    Ok(())
}

/// zerotier-cli 老用户的 key 肌肉记忆 → 本项目等价 key(报错时提示映射)
fn legacy_key_hint(k: &str) -> Option<&'static str> {
    match k {
        "allowDefault" | "allowManaged" => Some("routeViaZeroTier"),
        "allowDNS" => Some("dnsMode"),
        _ => None,
    }
}

pub fn set(
    client: &Client,
    nwid: &str,
    values: &[String],
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    if values.is_empty() {
        return Err(CliError::Usage(
            "set requires at least one key=value (routeViaZeroTier, dnsMode, customDns)".into(),
        ));
    }
    let mut body = serde_json::Map::new();
    for v in values {
        let (k, val) = v
            .split_once('=')
            .ok_or_else(|| CliError::Usage(format!("expected key=value, got \"{v}\"")))?;
        match k {
            "routeViaZeroTier" => {
                if !matches!(val, "0" | "1") {
                    return Err(CliError::Usage(format!(
                        "routeViaZeroTier must be 0|1, got \"{val}\""
                    )));
                }
                body.insert(k.into(), json!(val == "1"));
            }
            "dnsMode" => {
                if !matches!(val, "none" | "network" | "custom") {
                    return Err(CliError::Usage(format!(
                        "dnsMode must be none|network|custom, got \"{val}\""
                    )));
                }
                body.insert(k.into(), json!(val));
            }
            "customDns" => {
                let list: Vec<String> = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                body.insert(k.into(), json!(list));
            }
            _ => {
                return Err(match legacy_key_hint(k) {
                    Some(h) => CliError::Usage(format!(
                        "unknown key \"{k}\"; ZeroTier's {k} ≈ \"{h}\" here; valid: routeViaZeroTier, dnsMode, customDns"
                    )),
                    None => CliError::Usage(format!(
                        "unknown key \"{k}\"; valid: routeViaZeroTier, dnsMode, customDns"
                    )),
                });
            }
        }
    }
    let resp = client.post_json(&endpoints::network_set(nwid, planet.as_deref()), &Value::Object(body))?;
    ok(client, "set", planet, json, &resp);
    Ok(())
}

/// 网络号归一化:容忍 0x 前缀与缺省前导零,统一为 16 位小写 hex
fn norm_nwid(s: &str) -> Result<String, CliError> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u64::from_str_radix(hex, 16)
        .map(|v| format!("{v:016x}"))
        .map_err(|_| CliError::Usage(format!("invalid network id \"{s}\" (expect hex digits)")))
}

/// 取单个已加入网络的 NetworkEntry(dump/get/network show 共用);
/// 未加入 → 镜像 daemon 的 404 networkNotFound
pub(crate) fn find_network(
    client: &Client,
    nwid: &str,
    pid: &str,
) -> Result<NetworkEntry, CliError> {
    let nwid = norm_nwid(nwid)?;
    let v = client.get_json(&endpoints::networks(Some(pid)))?;
    let rows: Vec<NetworkEntry> = serde_json::from_value(
        v.get("networks").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    rows.into_iter()
        .find(|n| n.nwid == nwid)
        .ok_or_else(|| CliError::Api(
            404,
            ApiError {
                error: "networkNotFound".into(),
                message: format!("network {nwid} not joined on this planet"),
                candidates: None,
            },
        ))
}

/// `get <nwid> <key>`:读单个配置值,脚本友好;key 取 NetworkEntry 的
/// camelCase 字段名,未知 key 报 Usage 列全量合法 key
pub fn get(
    client: &Client,
    nwid: &str,
    key: &str,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    const VALID: &str = "nwid, name, status, conflict, type, mac, mtu, assignedAddrs, managedRoutes, dnsServers, routeViaZeroTier, routeViaLocked, dnsMode, customDns";
    let pid = resolve_single(client, planet)?;
    let n = find_network(client, nwid, &pid)?;
    let v = serde_json::to_value(&n).map_err(|e| CliError::Io(e.to_string()))?;
    let val = v.get(key).cloned().ok_or_else(|| {
        match legacy_key_hint(key) {
            Some(h) => CliError::Usage(format!(
                "unknown key \"{key}\"; ZeroTier's {key} ≈ \"{h}\" here; valid: {VALID}"
            )),
            None => CliError::Usage(format!("unknown key \"{key}\"; valid: {VALID}")),
        }
    })?;
    if json {
        print_json(&val);
    } else {
        println!("{}", fmt_value(&val));
    }
    Ok(())
}

pub fn orbit(
    client: &Client,
    moon_id: &str,
    seed: &str,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    let body = json!({ "seed": seed });
    let resp = client.post_json(&endpoints::moon_orbit(moon_id, planet.as_deref()), &body)?;
    ok(client, "orbit", planet, json, &resp);
    Ok(())
}

pub fn deorbit(client: &Client, moon_id: &str, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let resp = client.delete(&endpoints::moon_deorbit(moon_id, planet.as_deref()))?;
    ok(client, "deorbit", planet, json, &body_or_ok(resp));
    Ok(())
}

pub fn dump(client: &Client, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    // 文本模式:/dump 纯文本明细(daemon 生成)。
    // -j:结构化拼装(planet 概要 + networks + peers + events 各取自现有
    // JSON 端点),不再把整段文本包成 JSON 字符串(jq 不可用)
    if !json {
        let s = client.get_text(&endpoints::dump(planet.as_deref()))?;
        println!("{s}");
        return Ok(());
    }
    let pid = resolve_single(client, planet)?;
    let detail = client.get_json(&endpoints::planet(&pid))?;
    let networks = client
        .get_json(&endpoints::networks(Some(&pid)))?
        .get("networks")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let peers = client
        .get_json(&endpoints::peers(Some(&pid)))?
        .get("peers")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let events = client
        .get_json(&endpoints::events(Some(&pid), 10, None))?
        .get("events")
        .cloned()
        .unwrap_or_else(|| json!([]));
    print_json(&json!({
        "planet": detail.get("planet").cloned().unwrap_or(Value::Null),
        "networks": networks,
        "peers": peers,
        "events": events,
    }));
    Ok(())
}

#[derive(Subcommand)]
pub enum MoonCmd {
    /// Import a .moon file
    Import { source: String },
}

pub fn run_moon(
    client: &Client,
    cmd: &MoonCmd,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    match cmd {
        MoonCmd::Import { source } => {
            let body = json!({ "source": source });
            let resp = client.post_json(&endpoints::moons(planet.as_deref()), &body)?;
            if json {
                print_json(&resp);
            } else {
                println!("201 moon import OK");
            }
            Ok(())
        }
    }
}
