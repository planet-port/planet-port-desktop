use crate::client::{CliError, Client};
use crate::cmd::planet_label;
use crate::emoji::icon_to_emoji;
use crate::output::{fmt_local_ms, fmt_local_secs, new_table, print_json};
use clap::Subcommand;
use planet_port_api::endpoints;
use planet_port_api::types::{PlanetDetail, PlanetStatus, ShareLink};
use serde_json::{json, Value};
use std::io::Write;

#[derive(Subcommand)]
pub enum PlanetCmd {
    /// List registered planets
    List,
    /// Show planet details
    Show { id: String },
    /// Rename a planet
    Rename { id: String, new_name: String },
    /// Set/clear TCP fallback relay (official tcp-proxy deployment)
    SetRelay {
        id: String,
        /// Relay address: full "ip:port"; ":port" or bare port reuses the
        /// planet's first root ip; bare flag = first root ip + default 9443.
        /// Omit --relay to clear.
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        relay: Option<String>,
    },
    /// Export planet binary to stdout or file
    Export {
        id: String,
        #[arg(short, long)]
        out: Option<String>,
    },
    /// Create a LAN temporary download link
    Share {
        #[arg(long, default_value_t = 300)]
        ttl: u64,
        id: String,
    },
    /// Enable a planet (persist; auto-start if it has networks)
    Enable { id: String },
    /// Disable a planet (persist; stop it and never run until enabled)
    Disable { id: String },
}

pub fn run(client: &Client, cmd: &PlanetCmd, json: bool) -> Result<(), CliError> {
    match cmd {
        PlanetCmd::List => list(client, json),
        PlanetCmd::Show { id } => show(client, id, json),
        PlanetCmd::Rename { id, new_name } => rename(client, id, new_name, json),
        PlanetCmd::SetRelay { id, relay } => set_relay(client, id, relay.clone(), json),
        PlanetCmd::Export { id, out } => export(client, id, out.clone()),
        PlanetCmd::Share { id, ttl } => share(client, id, *ttl, json),
        PlanetCmd::Enable { id } => enable(client, id, json),
        PlanetCmd::Disable { id } => disable(client, id, json),
    }
}

// remove 已提级为顶层命令(add/remove,对齐用户习惯、减少层级),实现仍
// 留在本模块;start/stop 语义已并入 enable/disable(派生运行规则下启停
// 不再是独立操作),命令移除
/// 启用(持久):有网络 → daemon 侧按派生规则自动拉起
pub fn enable(client: &Client, id: &str, json: bool) -> Result<(), CliError> {
    let resp = client.post_empty(&endpoints::planet_action(id, "enable"))?;
    print_result(client, "enable", id, json, &resp);
    Ok(())
}

/// 禁用(持久):在跑立即停;禁用期间 join/enable 恢复前/boot 一律不运行
pub fn disable(client: &Client, id: &str, json: bool) -> Result<(), CliError> {
    let resp = client.post_empty(&endpoints::planet_action(id, "disable"))?;
    print_result(client, "disable", id, json, &resp);
    Ok(())
}

/// 回执带落点 `name(id) 200 start OK`(id 解析为登记名;拿不到时退化无落点)
fn print_result(client: &Client, verb: &str, target: &str, json: bool, body: &Value) {
    if json {
        print_json(body);
        return;
    }
    match planet_label(client, &Some(target.to_string())) {
        Some(label) => println!("{label} 200 {verb} OK"),
        None => println!("200 {verb} OK"),
    }
}

/// PlanetKind 已按 serde lowercase 序列化,这里直接取其字符串形式
fn kind_str(kind: &planet_port_api::types::PlanetKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{kind:?}"))
}

fn list(client: &Client, json: bool) -> Result<(), CliError> {
    let v = client.get_json(endpoints::planets())?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<PlanetStatus> = serde_json::from_value(
        v.get("planets").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let mut t = new_table(&["ICON", "PLANET", "ID", "STATUS", "NETWORKS", "PORT"]);
    for p in rows {
        // 禁用是持久开关,与运行状态正交:disabled 前缀标识,避免"stopped
        // 但其实被禁用"的误读
        let status = match (p.status.as_str(), p.enabled) {
            ("running", _) => "running".to_string(),
            (_, false) => "disabled".to_string(),
            _ => "stopped".to_string(),
        };
        t.add_row(vec![
            icon_to_emoji(&p.icon).to_string(),
            p.name,
            p.id,
            status,
            p.network_count.to_string(),
            p.port.to_string(),
        ]);
    }
    println!("{t}");
    Ok(())
}

pub fn import(
    client: &Client,
    source: &str,
    name: Option<String>,
    data_dir: Option<String>,
    port: Option<u16>,
    json: bool,
) -> Result<(), CliError> {
    // dataDir/port 为 daemon API 已支持的覆写项;无值时不写字段,行为不变
    let body = json!({
        "source": source,
        "name": name,
        "dataDir": data_dir,
        "port": port,
    });
    let resp = client.post_json(endpoints::planets(), &body)?;
    if json {
        print_json(&resp);
        return Ok(());
    }
    let d: PlanetDetail = serde_json::from_value(
        resp.get("planet").cloned().unwrap_or(Value::Null),
    )
    .map_err(|e| CliError::Io(e.to_string()))?;
    // 导入后 daemon 自动拉起;status 如实反映(启动失败为 stopped)
    println!("added planet \"{}\" (id {}) → {}", d.name, d.id, d.status);
    Ok(())
}

fn show(client: &Client, id: &str, json: bool) -> Result<(), CliError> {
    let v = client.get_json(&endpoints::planet(id))?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let d: PlanetDetail = serde_json::from_value(
        v.get("planet").cloned().unwrap_or(Value::Null),
    )
    .map_err(|e| CliError::Io(e.to_string()))?;
    println!("planet    {} {}", icon_to_emoji(&d.icon), d.name);
    println!("id        {}", d.id);
    println!("worldId   {}", d.world_id);
    println!("type      {}", kind_str(&d.kind));
    println!("status    {} (online: {})", d.status, d.online);
    println!("roots     {}", d.roots.join(", "));
    println!("source    {}", d.source);
    println!("size      {} bytes", d.file_size);
    println!("revision  {}", fmt_local_ms(d.revision_time_ms));
    println!("lastNodeId {}", d.last_node_id.unwrap_or_else(|| "—".into()));
    println!("networks  {}", d.network_count);
    println!("port      {}", d.port);
    let transport = d
        .transport
        .as_ref()
        .map(|t| {
            let mut s = format!("udp/{}", t.primary_port);
            if let Some(sec) = t.secondary_port {
                s.push_str(&format!("+{sec}"));
            }
            s.push_str(&format!(" tcp={}", t.tcp));
            if let Some(r) = &t.relay {
                s.push_str(&format!("({r})"));
            }
            s
        })
        .unwrap_or_else(|| "—".into());
    println!("transport {transport}");
    println!("relay     {}", d.tcp_fallback_relay.clone().unwrap_or_else(|| "—".into()));
    Ok(())
}

fn rename(client: &Client, id: &str, new_name: &str, json: bool) -> Result<(), CliError> {
    let body = json!({ "name": new_name });
    let resp = client.patch_json(&endpoints::planet(id), &body)?;
    print_result(client, "rename", id, json, &resp);
    Ok(())
}

/// relay 精简形展开默认口:取 tcp-proxy 编译产物的默认监听口
/// (scripts/build-tcp-proxy.sh,改默认口须同步这里)
const DEFAULT_RELAY_PORT: &str = "9443";

/// 展开 relay 精简形为完整 ip:port:完整形原样;":port" 与纯数字端口复用
/// planet 首个根端点的 ip(relay 与根同机部署是文档推荐形态);展开发生在
/// CLI 层,registry 永远存完整形,daemon 不感知省略语法
fn expand_relay(client: &Client, id: &str, raw: &str) -> Result<String, CliError> {
    let shorthand = raw.is_empty()
        || raw.starts_with(':')
        || raw.chars().all(|c| c.is_ascii_digit());
    if !shorthand {
        return Ok(raw.to_string());
    }
    let port = if raw.is_empty() {
        DEFAULT_RELAY_PORT.to_string()
    } else {
        raw.trim_start_matches(':').to_string()
    };
    if port.parse::<u16>().is_err() {
        return Err(CliError::Usage(format!("relay 端口非法: {raw}")));
    }
    let v = client.get_json(&endpoints::planet(id))?;
    let d: PlanetDetail = serde_json::from_value(v.get("planet").cloned().unwrap_or(Value::Null))
        .map_err(|e| CliError::Io(e.to_string()))?;
    // roots 元素为 "ip:port";rsplit 防 ip 含冒号的形态(未来 IPv6)
    let ip = d
        .roots
        .iter()
        .find_map(|r| r.rsplit_once(':').map(|(ip, _)| ip))
        .ok_or_else(|| {
            CliError::Usage("planet 无可用根端点,无法展开 relay 精简形,请用完整 ip:port".into())
        })?;
    Ok(format!("{ip}:{port}"))
}

/// 设置/清除 per-planet TCP 兜底 relay(--relay 缺省时序列化为 null,
/// daemon 侧按清除处理;地址格式由服务端校验)
fn set_relay(client: &Client, id: &str, relay: Option<String>, json: bool) -> Result<(), CliError> {
    let relay = match relay {
        None => None,
        Some(raw) => Some(expand_relay(client, id, &raw)?),
    };
    let body = json!({ "tcpFallbackRelay": relay });
    let resp = client.patch_json(&endpoints::planet(id), &body)?;
    print_result(client, "set-relay", id, json, &resp);
    Ok(())
}

pub fn rm(client: &Client, id: &str, yes: bool, json: bool) -> Result<(), CliError> {
    // -j 时 stdout 只允许输出 JSON,交互确认会污染输出,必须显式 --yes
    if json && !yes {
        return Err(CliError::Usage(
            "--json requires --yes for remove (confirmation prompt would corrupt stdout)".into(),
        ));
    }
    if !yes {
        // 确认提示与人话反馈都走 stderr,stdout 保持纯净
        eprint!("{}", rust_i18n::t!("cli.planet.removeConfirm", id = id));
        std::io::stderr().flush().ok();
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|e| CliError::Io(e.to_string()))?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            // 拒绝 ≠ 成功:退出码 130(用户中止,与交互选择器取消一致),
            // 批量脚本不会把「没删」误判为「删了」
            eprintln!("{}", rust_i18n::t!("cli.planet.removeAborted"));
            std::process::exit(130);
        }
    }
    let resp = client.delete(&endpoints::planet(id))?;
    let body = if resp.is_null() { json!({ "result": "ok" }) } else { resp };
    print_result(client, "rm", id, json, &body);
    Ok(())
}

fn export(client: &Client, id: &str, out: Option<String>) -> Result<(), CliError> {
    let bytes = client.get_bytes(&endpoints::planet_export(id))?;
    match out {
        Some(path) => std::fs::write(path, &bytes).map_err(|e| CliError::Io(e.to_string()))?,
        None => std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| CliError::Io(e.to_string()))?,
    }
    Ok(())
}

fn share(client: &Client, id: &str, ttl: u64, json: bool) -> Result<(), CliError> {
    let body = json!({ "ttlSeconds": ttl });
    let mut resp = client.post_json(&endpoints::planet_share(id), &body)?;
    // daemon 只知道自己的绑定地址(恒回环);CLI 按实际访问的 base 改写,
    // 经 PLANET_PORT_URL 指到远端 daemon 时链接才对他人可达
    if let Some(u) = resp.get("url").and_then(Value::as_str) {
        if let Some((_, rest)) = u.split_once("/share/") {
            resp["url"] = json!(format!("{}/share/{}", client.base(), rest));
        }
    }
    if json {
        print_json(&resp);
        return Ok(());
    }
    let link: ShareLink = serde_json::from_value(resp)
        .map_err(|e| CliError::Io(e.to_string()))?;
    println!("{}", link.url);
    eprintln!("expires_at: {} ({})", link.expires_at, fmt_local_secs(link.expires_at));
    // 回环地址发给他人打不开:明确提示换局域网 IP
    if link.url.contains("//127.0.0.1") || link.url.contains("//localhost") {
        eprintln!("{}", rust_i18n::t!("cli.planet.shareLoopHint"));
    }
    Ok(())
}
