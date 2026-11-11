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
pub(crate) fn planet_names(client: &Client) -> HashMap<String, String> {
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

pub(crate) fn planet_display(names: &HashMap<String, String>, id: &str) -> String {
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
                        || p.get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|n| n.eq_ignore_ascii_case(id))
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
    let mut t = new_table(&["PLANET", "STATUS", "ADDRESS", "VERSION", "LATENCY", "TRANSPORT"]);
    for p in rows {
        // 三态对齐 list:停止=未连接、运行且根可达=已连接、运行但离线=连接中
        // (旧版 ONLINE/OFFLINE/stopped 混在一列,口径与 list 对不上)
        let status = if p.status != "running" {
            rust_i18n::t!("status.disconnected").to_string()
        } else if p.online {
            rust_i18n::t!("status.connected").to_string()
        } else {
            rust_i18n::t!("status.connecting").to_string()
        };
        let latency = p
            .latency_ms
            .map(|l| format!("{l}ms"))
            .unwrap_or_else(|| "—".into());
        // 通道形态:主口[+副口],TCP 隧道激活再标 /tcp;未运行 —
        let transport = p
            .transport
            .as_ref()
            .map(|t| {
                let mut s = format!("udp/{}", t.primary_port);
                if let Some(sec) = t.secondary_port {
                    s.push_str(&format!("+{sec}"));
                }
                if t.tcp == "established" {
                    s.push_str("/tcp");
                }
                s
            })
            .unwrap_or_else(|| "—".into());
        t.add_row(vec![
            p.name,
            status,
            dash(p.node_id),
            dash(p.core_version),
            latency,
            transport,
        ]);
    }
    println!("{t}");
    Ok(())
}

// ---- 分组列表(listnetworks / listpeers / listmoons 共用骨架) ----

/// 过滤集是否命中该行星(id 精确 / name 忽略大小写;--planet 的别名在
/// daemon 侧已解析为 canonical id 传回数据,这里只需对上分组维度)
fn match_filter(filter: &Option<Vec<String>>, p: &PlanetStatus) -> bool {
    filter.as_ref().is_none_or(|f| {
        f.iter().any(|id| id == &p.id || p.name.eq_ignore_ascii_case(id))
    })
}

/// 分组头:三态对齐 info(运行且根可达=已连接;运行但离线=连接中;停止=未连接)
fn group_state(p: &PlanetStatus) -> String {
    if p.status != "running" {
        rust_i18n::t!("status.disconnected").to_string()
    } else if p.online {
        rust_i18n::t!("status.connected").to_string()
    } else {
        rust_i18n::t!("status.connecting").to_string()
    }
}

/// 分组 JSON(planets[] 各带条目数组;文本模式同口径)
fn grouped_json<T, F>(
    planets: &[PlanetStatus],
    filter: &Option<Vec<String>>,
    rows: &[T],
    key: &str,
    row_planet: F,
) -> Value
where
    T: serde::Serialize,
    F: Fn(&T) -> &str,
{
    json!({
        "planets": planets
            .iter()
            .filter(|p| match_filter(filter, p))
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name,
                    "status": p.status,
                    "online": p.online,
                    key: rows
                        .iter()
                        .filter(|r| row_planet(r) == p.id)
                        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>()
    })
}

/// 按行星分组列出网络。分组覆盖**全部登记行星**(不只在线的):未连接的
/// 行星也列出并标注状态,其下网络按 registry 出 STOPPED 占位行;
/// `--planet` 过滤对分组生效
pub fn listnetworks(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let v = fetch_list(client, endpoints::networks, "networks", filter)?;
    let rows: Vec<NetworkEntry> = serde_json::from_value(
        v.get("networks").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let planets = planet_statuses(client);
    if json {
        print_json(&grouped_json(&planets, filter, &rows, "networks", |n| &n.planet));
        return Ok(());
    }
    let names: HashMap<String, String> = planets
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    for p in &planets {
        if !match_filter(filter, p) {
            continue;
        }
        println!("planet: {} [{}]", planet_display(&names, &p.id), group_state(p));
        let nets: Vec<_> = rows.iter().filter(|n| n.planet == p.id).collect();
        if nets.is_empty() {
            println!("  {}\n", rust_i18n::t!("cli.list.noNetworks"));
            continue;
        }
        // 列序对齐 zerotier-cli listnetworks(MAC 在 STATUS 前,DEV 紧随 TYPE)
        let mut t = new_table(&["NETWORK", "NAME", "MAC", "STATUS", "TYPE", "DEV", "MANAGED-IPS"]);
        for n in nets {
            // 多 managed IP 全量堆叠(换行续行对齐本列),不再截断 2 个
            t.add_row(vec![
                n.nwid.clone(),
                n.name.clone().unwrap_or_else(|| "—".into()),
                n.mac.clone(),
                n.status.clone(),
                n.kind.clone(),
                n.interface.clone().unwrap_or_else(|| "—".into()),
                n.assigned_addrs.join("\n"),
            ]);
        }
        println!("{t}\n");
    }
    auth_pending_hint(&rows);
    Ok(())
}

/// 授权等待状态的行动提示(排障闭环:用户第一反应是"为什么没 IP"):
/// 卡在 AUTHENTICATION_REQUIRED 时告知去控制器放行。走 stderr,stdout 保持数据纯净
fn auth_pending_hint(rows: &[NetworkEntry]) {
    for n in rows.iter().filter(|n| n.status == "AUTHENTICATION_REQUIRED") {
        eprintln!("{}", rust_i18n::t!("cli.list.authPending", nwid = n.nwid));
    }
}

/// 按行星分组列出 peer。停止/离线的行星没有 peer,占位提示;`--planet`
/// 过滤对分组生效
pub fn listpeers(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let v = fetch_list(client, endpoints::peers, "peers", filter)?;
    let rows: Vec<PeerEntry> = serde_json::from_value(
        v.get("peers").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let planets = planet_statuses(client);
    if json {
        print_json(&grouped_json(&planets, filter, &rows, "peers", |p| &p.planet));
        return Ok(());
    }
    let names: HashMap<String, String> = planets
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    for p in &planets {
        if !match_filter(filter, p) {
            continue;
        }
        println!("planet: {} [{}]", planet_display(&names, &p.id), group_state(p));
        let peers: Vec<_> = rows.iter().filter(|r| r.planet == p.id).collect();
        if peers.is_empty() {
            println!("  {}\n", rust_i18n::t!("cli.listpeers.noPeers"));
            continue;
        }
        let mut t = new_table(&["ROLE", "ADDRESS", "LATENCY", "VERSION", "PATH"]);
        for r in peers {
            let latency = r
                .latency_ms
                .map(|l| format!("{l}ms"))
                .unwrap_or_else(|| "—".into());
            t.add_row(vec![
                r.role.clone(),
                r.address.clone(),
                latency,
                dash(r.version.clone()),
                r.path.clone().unwrap_or_else(|| "(relay)".into()),
            ]);
        }
        println!("{t}\n");
    }
    Ok(())
}

/// 按行星分组列出 moon。停止的行星没有 moon 数据,占位提示;`--planet`
/// 过滤对分组生效
pub fn listmoons(client: &Client, filter: &Option<Vec<String>>, json: bool) -> Result<(), CliError> {
    let v = fetch_list(client, endpoints::moons, "moons", filter)?;
    let rows: Vec<MoonEntry> = serde_json::from_value(
        v.get("moons").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let planets = planet_statuses(client);
    if json {
        print_json(&grouped_json(&planets, filter, &rows, "moons", |m| &m.planet));
        return Ok(());
    }
    let names: HashMap<String, String> = planets
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    for p in &planets {
        if !match_filter(filter, p) {
            continue;
        }
        println!("planet: {} [{}]", planet_display(&names, &p.id), group_state(p));
        let moons: Vec<_> = rows.iter().filter(|m| m.planet == p.id).collect();
        if moons.is_empty() {
            println!("  {}\n", rust_i18n::t!("cli.listmoons.noMoons"));
            continue;
        }
        let mut t = new_table(&["MOONID", "SEED", "SOURCE", "CACHED"]);
        for m in moons {
            t.add_row(vec![
                m.moon_id.clone(),
                m.seed.clone().unwrap_or_else(|| "—".into()),
                if m.from_file { "file" } else { "manual" }.into(),
                if m.cached { "yes" } else { "no" }.into(),
            ]);
        }
        println!("{t}\n");
    }
    Ok(())
}

// ---- networkinfo:单网络详情视图 ----

/// 单网络详情(IP / 路由 / DNS / routeViaZeroTier);planet 定位与 join/leave
/// 同链(--planet 显式 > 环境变量 > daemon 自动选/报歧义)。行星未运行时
/// 不拒绝:展示 STOPPED 占位详情(仅 registry 登记配置)并 stderr 反馈
/// 动态字段缺失的原因
pub fn networkinfo(
    client: &Client,
    nwid: &str,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    let pid = crate::cmd::resolve_registered(client, planet)?;
    let n = crate::cmd::mutate::find_network(client, nwid, &pid)?;
    let stopped = n.status == "STOPPED";
    if stopped {
        eprintln!("{}", rust_i18n::t!("cli.networkinfo.stoppedPlanet"));
    }
    if json {
        print_json(&serde_json::to_value(&n).map_err(|e| CliError::Io(e.to_string()))?);
        return Ok(());
    }
    let label = crate::cmd::planet_label(client, &Some(pid.clone()))
        .unwrap_or_else(|| pid.clone());
    let routes = n
        .managed_routes
        .iter()
        .map(|r| match &r.via {
            Some(v) => format!("{} via {}", r.target, v),
            None => r.target.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    println!("network   {}", n.nwid);
    println!("planet    {label}");
    println!("name      {}", n.name.clone().unwrap_or_else(|| "—".into()));
    println!("status    {}", n.status);
    println!("type      {}", n.kind);
    println!("mac       {}", n.mac);
    println!("device    {}", n.interface.clone().unwrap_or_else(|| "—".into()));
    // 占位条目 mtu=0(核心不在拿不到),显示为 — 而非误导性的 0
    println!("mtu       {}", if stopped { "—".into() } else { n.mtu.to_string() });
    println!("assigned  {}", n.assigned_addrs.join(", "));
    println!("routes    {}", if routes.is_empty() { "—".into() } else { routes });
    println!("dns       mode={} servers=[{}] custom=[{}]",
        n.dns_mode,
        n.dns_servers.join(", "),
        n.custom_dns.join(", "),
    );
    println!("routeVia  {}", n.route_via_zero_tier);
    if let Some(c) = &n.conflict {
        println!("conflict  {c}");
    }
    Ok(())
}
