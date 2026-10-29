use std::collections::HashMap;

use crate::client::{CliError, Client};
use crate::output::{new_table, print_json};
use planet_port_api::endpoints;
use planet_port_api::types::{MoonEntry, NetworkEntry, PeerEntry, PlanetStatus};
use serde_json::{json, Value};

fn dash<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "—".into())
}

/// planet id → 名称映射,列表输出显示 `id (name)`;/status 拿不到时回退纯 id
fn planet_names(client: &Client) -> HashMap<String, String> {
    planet_statuses(client)
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect()
}

/// /status 的 planets 全量(含 stopped):登记即返回,list 分组展示用
fn planet_statuses(client: &Client) -> Vec<PlanetStatus> {
    let Ok(v) = client.get_json(endpoints::status()) else {
        return Vec::new();
    };
    serde_json::from_value(v.get("planets").cloned().unwrap_or(Value::Null))
        .unwrap_or_default()
}

fn planet_display(names: &HashMap<String, String>, id: &str) -> String {
    match names.get(id) {
        Some(n) => format!("{id} ({n})"),
        None => id.to_string(),
    }
}

/// 列表型查询的统一取数:过滤集为 None → 全量;单成员 → ?planet= 过滤;
/// 多成员 → 逐 planet 请求合并数组
fn fetch_list(
    client: &Client,
    endpoint: fn(Option<&str>) -> String,
    key: &str,
    filter: &Option<Vec<String>>,
) -> Result<Value, CliError> {
    match filter {
        None => client.get_json(&endpoint(None)),
        Some(ids) if ids.len() == 1 => client.get_json(&endpoint(Some(&ids[0]))),
        Some(ids) => {
            let mut merged: Vec<Value> = Vec::new();
            for id in ids {
                let v = client.get_json(&endpoint(Some(id)))?;
                merged.extend(
                    v.get(key)
                        .and_then(|x| x.as_array())
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            Ok(json!({ key: merged }))
        }
    }
}

pub fn info(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let mut v = client.get_json(endpoints::status())?;
    // 多选子集:客户端侧过滤 /status 的 planets 数组(选择器返回 canonical id,
    // 但显式 --planet 可能是 name,双匹配兼容两者)
    if let Some(ids) = filter {
        if let Some(arr) = v.get_mut("planets").and_then(|p| p.as_array_mut()) {
            arr.retain(|p| {
                ids.iter().any(|id| {
                    p.get("id").and_then(Value::as_str) == Some(id.as_str())
                        || p.get("name").and_then(Value::as_str) == Some(id.as_str())
                })
            });
        }
    }
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<PlanetStatus> = serde_json::from_value(
        v.get("planets").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let mut t = new_table(&["PLANET", "STATUS", "ADDRESS", "VERSION", "LATENCY"]);
    for p in rows {
        let status = if p.status != "running" {
            "stopped".to_string()
        } else if p.online {
            "ONLINE".to_string()
        } else {
            "OFFLINE".to_string()
        };
        let latency = p
            .latency_ms
            .map(|l| format!("{l}ms"))
            .unwrap_or_else(|| "—".into());
        t.add_row(vec![p.name, status, dash(p.node_id), dash(p.core_version), latency]);
    }
    println!("{t}");
    Ok(())
}

/// 按行星分组列出网络(listpeers 同款分组样式)。
/// 分组覆盖**全部登记行星**(不只在线的):未连接的行星也列出并标注状态,
/// 其下无网络表;`--planet` 过滤对分组生效
pub fn list(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let v = fetch_list(client, endpoints::networks, "networks", filter)?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<NetworkEntry> = serde_json::from_value(
        v.get("networks").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    // /status 只取一次:分组与名称展示共用,避免重复请求
    let planets = planet_statuses(client);
    let names: HashMap<String, String> = planets
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    for p in planets {
        if let Some(f) = filter {
            if !f.contains(&p.id) && !f.contains(&p.name) {
                continue;
            }
        }
        // 三态对齐 info:运行且根可达=已连接;运行但离线=连接中;停止=未连接
        let state = if p.status != "running" {
            rust_i18n::t!("status.disconnected")
        } else if p.online {
            rust_i18n::t!("status.connected")
        } else {
            rust_i18n::t!("status.connecting")
        };
        println!("planet: {} [{state}]", planet_display(&names, &p.id));
        let nets: Vec<_> = rows.iter().filter(|n| n.planet == p.id).collect();
        if nets.is_empty() {
            println!("  {}\n", rust_i18n::t!("cli.list.noNetworks"));
            continue;
        }
        let mut t = new_table(&["NETWORK", "NAME", "STATUS", "TYPE", "MANAGED-IPS"]);
        for n in nets {
            let mut ips = n.assigned_addrs.iter().take(2).cloned().collect::<Vec<_>>();
            if n.assigned_addrs.len() > 2 {
                ips.push("…".into());
            }
            t.add_row(vec![
                n.nwid.clone(),
                n.name.clone().unwrap_or_else(|| "—".into()),
                n.status.clone(),
                n.kind.clone(),
                ips.join(", "),
            ]);
        }
        println!("{t}\n");
    }
    Ok(())
}

pub fn listnetworks(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let v = fetch_list(client, endpoints::networks, "networks", filter)?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<NetworkEntry> = serde_json::from_value(
        v.get("networks").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let names = planet_names(client);
    let mut t = new_table(&["PLANET", "NETWORK", "NAME", "STATUS", "TYPE", "MANAGED-IPS"]);
    for n in rows {
        let mut ips = n.assigned_addrs.iter().take(2).cloned().collect::<Vec<_>>();
        if n.assigned_addrs.len() > 2 {
            ips.push("…".into());
        }
        t.add_row(vec![
            planet_display(&names, &n.planet),
            n.nwid,
            n.name.unwrap_or_else(|| "—".into()),
            n.status,
            n.kind,
            ips.join(", "),
        ]);
    }
    println!("{t}");
    Ok(())
}

pub fn listpeers(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let v = fetch_list(client, endpoints::peers, "peers", filter)?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<PeerEntry> = serde_json::from_value(
        v.get("peers").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    // 按 planet 分组展示
    let names = planet_names(client);
    let mut order: Vec<String> = Vec::new();
    for p in &rows {
        if !order.contains(&p.planet) {
            order.push(p.planet.clone());
        }
    }
    for pid in order {
        println!("planet: {}", planet_display(&names, &pid));
        let mut t = new_table(&["ROLE", "ADDRESS", "LATENCY", "VERSION", "PATH"]);
        for p in rows.iter().filter(|p| p.planet == pid) {
            let latency = p
                .latency_ms
                .map(|l| format!("{l}ms"))
                .unwrap_or_else(|| "—".into());
            t.add_row(vec![
                p.role.clone(),
                p.address.clone(),
                latency,
                dash(p.version.clone()),
                p.path.clone().unwrap_or_else(|| "(relay)".into()),
            ]);
        }
        println!("{t}\n");
    }
    Ok(())
}

pub fn listmoons(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let v = fetch_list(client, endpoints::moons, "moons", filter)?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<MoonEntry> = serde_json::from_value(
        v.get("moons").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let names = planet_names(client);
    let mut t = new_table(&["PLANET", "MOONID", "SEED", "SOURCE", "CACHED"]);
    for m in rows {
        t.add_row(vec![
            planet_display(&names, &m.planet),
            m.moon_id,
            m.seed.unwrap_or_else(|| "—".into()),
            if m.from_file { "file" } else { "manual" }.into(),
            if m.cached { "yes" } else { "no" }.into(),
        ]);
    }
    println!("{t}");
    Ok(())
}
