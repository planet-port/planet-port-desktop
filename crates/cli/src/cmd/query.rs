use std::collections::HashMap;

use crate::client::{CliError, Client};
use crate::cmd::{planet_label, resolve_single};
use crate::output::{new_table, print_json};
use clap::Subcommand;
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
    let mut t = new_table(&["PLANET", "STATUS", "ADDRESS", "VERSION", "LATENCY"]);
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
    let rows: Vec<NetworkEntry> = serde_json::from_value(
        v.get("networks").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    if json {
        // 分组结构(与文本模式同口径):planets[] 各带 networks[];
        // 平铺数组是 listnetworks -j 的职责,两个入口不再同源重复。
        // /status 失败时让错误冒出(退出码承载),不吞成「成功但零数据」
        let sv = client.get_json(endpoints::status())?;
        let planets: Vec<PlanetStatus> = serde_json::from_value(
            sv.get("planets").cloned().unwrap_or(Value::Null),
        )
        .unwrap_or_default();
        let arr: Vec<Value> = planets
            .iter()
            .filter(|p| match filter {
                Some(f) => f.iter().any(|id| id == &p.id || p.name.eq_ignore_ascii_case(id)),
                None => true,
            })
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name,
                    "status": p.status,
                    "online": p.online,
                    "networks": rows
                        .iter()
                        .filter(|n| n.planet == p.id)
                        .map(|n| serde_json::to_value(n).unwrap_or(Value::Null))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        print_json(&json!({ "planets": arr }));
        return Ok(());
    }
    // /status 只取一次:分组与名称展示共用,避免重复请求
    let planets = planet_statuses(client);
    let names: HashMap<String, String> = planets
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    for p in planets {
        if let Some(f) = filter {
            if !f.iter().any(|id| id == &p.id || p.name.eq_ignore_ascii_case(id)) {
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
    let mut t = new_table(&["PLANET", "NETWORK", "NAME", "MAC", "STATUS", "TYPE", "DEV", "MANAGED-IPS"]);
    // rows 随后被 move 进循环,先取出待提示的授权等待网络
    let pending: Vec<String> = rows
        .iter()
        .filter(|n| n.status == "AUTHENTICATION_REQUIRED")
        .map(|n| n.nwid.clone())
        .collect();
    for n in rows {
        t.add_row(vec![
            planet_display(&names, &n.planet),
            n.nwid,
            n.name.unwrap_or_else(|| "—".into()),
            n.mac,
            n.status,
            n.kind,
            n.interface.unwrap_or_else(|| "—".into()),
            // 多 managed IP 全量堆叠(换行续行对齐本列),与 list 同口径
            n.assigned_addrs.join("\n"),
        ]);
    }
    println!("{t}");
    for nwid in pending {
        eprintln!("{}", rust_i18n::t!("cli.list.authPending", nwid = nwid));
    }
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

// ---- network show:单网络详情视图 ----

#[derive(Subcommand)]
pub enum NetworkCmd {
    /// Show one joined network's full detail (IP / routes / DNS / routeViaZeroTier)
    Show { nwid: String },
}

pub fn run_network(
    client: &Client,
    cmd: &NetworkCmd,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    match cmd {
        NetworkCmd::Show { nwid } => network_show(client, nwid, planet, json),
    }
}

fn network_show(
    client: &Client,
    nwid: &str,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    let pid = resolve_single(client, planet)?;
    let n = crate::cmd::mutate::find_network(client, nwid, &pid)?;
    if json {
        print_json(&serde_json::to_value(&n).map_err(|e| CliError::Io(e.to_string()))?);
        return Ok(());
    }
    let label = planet_label(client, &Some(pid.clone()))
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
    println!("mtu       {}", n.mtu);
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
