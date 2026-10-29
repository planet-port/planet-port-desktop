use crate::client::{CliError, Client};
use crate::emoji::icon_to_emoji;
use crate::output::{new_table, print_json};
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
}

pub fn run(client: &Client, cmd: &PlanetCmd, json: bool) -> Result<(), CliError> {
    match cmd {
        PlanetCmd::List => list(client, json),
        PlanetCmd::Show { id } => show(client, id, json),
        PlanetCmd::Rename { id, new_name } => rename(client, id, new_name, json),
        PlanetCmd::Export { id, out } => export(client, id, out.clone()),
        PlanetCmd::Share { id, ttl } => share(client, id, *ttl, json),
    }
}

// start/stop/remove 已提级为顶层命令(add/remove/start/stop,对齐用户习惯、
// 减少层级),实现仍留在本模块
pub fn start(client: &Client, id: &str, json: bool) -> Result<(), CliError> {
    let resp = client.post_empty(&endpoints::planet_action(id, "start"))?;
    print_result("start", json, &resp);
    Ok(())
}

pub fn stop(client: &Client, id: &str, json: bool) -> Result<(), CliError> {
    let resp = client.post_empty(&endpoints::planet_action(id, "stop"))?;
    print_result("stop", json, &resp);
    Ok(())
}

fn print_result(verb: &str, json: bool, body: &Value) {
    if json {
        print_json(body);
    } else {
        println!("200 {verb} OK");
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
        t.add_row(vec![
            icon_to_emoji(&p.icon).to_string(),
            p.name,
            p.id,
            if p.status == "running" { "running".into() } else { "stopped".into() },
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
    println!("added planet \"{}\" (id {}) → running", d.name, d.id);
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
    println!("revision  {} (ms epoch)", d.revision_time_ms);
    println!("lastNodeId {}", d.last_node_id.unwrap_or_else(|| "—".into()));
    println!("networks  {}", d.network_count);
    println!("port      {}", d.port);
    Ok(())
}

fn rename(client: &Client, id: &str, new_name: &str, json: bool) -> Result<(), CliError> {
    let body = json!({ "name": new_name });
    let resp = client.patch_json(&endpoints::planet(id), &body)?;
    print_result("rename", json, &resp);
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
        eprint!("delete planet \"{}\" and its config? [y/N] ", id);
        std::io::stderr().flush().ok();
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|e| CliError::Io(e.to_string()))?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            eprintln!("aborted");
            return Ok(());
        }
    }
    let _ = client.delete(&endpoints::planet(id))?;
    print_result("rm", json, &Value::Null);
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
    let resp = client.post_json(&endpoints::planet_share(id), &body)?;
    if json {
        print_json(&resp);
        return Ok(());
    }
    let link: ShareLink = serde_json::from_value(resp)
        .map_err(|e| CliError::Io(e.to_string()))?;
    println!("{}", link.url);
    eprintln!("expires_at: {}", link.expires_at);
    Ok(())
}
