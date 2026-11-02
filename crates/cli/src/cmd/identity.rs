//! identity 子命令:身份模式与身份列表(缺省)、官方身份导入、切换绑定。

use clap::{Subcommand, ValueEnum};
use serde_json::{json, Value};

use crate::client::{CliError, Client};
use planet_port_api::endpoints;
use planet_port_api::types::{GlobalIdentity, PlanetIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum IdentityMode {
    Own,
    Global,
}

impl IdentityMode {
    fn as_str(&self) -> &'static str {
        match self {
            IdentityMode::Own => "own",
            IdentityMode::Global => "global",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum IdentityCmd {
    /// List identity mode and identities (default)
    List,
    /// Import official ZeroTier identity as global identity
    Import {
        /// Path to official identity.secret (or its dir); omit to auto-detect
        #[arg(long)]
        path: Option<String>,
        /// Overwrite existing global identity
        #[arg(long)]
        force: bool,
    },
    /// Switch a planet's identity binding
    Use {
        planet: String,
        mode: IdentityMode,
    },
}

pub fn run(client: &Client, cmd: &Option<IdentityCmd>, json: bool) -> Result<(), CliError> {
    match cmd.as_ref().unwrap_or(&IdentityCmd::List) {
        IdentityCmd::List => list(client, json),
        IdentityCmd::Import { path, force } => import(client, path.as_deref(), *force, json),
        IdentityCmd::Use { planet, mode } => switch(client, planet, *mode, json),
    }
}

/// /identity 的 planets 段里按 id 或 name(忽略大小写)找 canonical id
fn resolve_id(client: &Client, planet: &str) -> Result<String, CliError> {
    let v = client.get_json(endpoints::identity())?;
    for p in v["planets"].as_array().unwrap_or(&Vec::new()) {
        if p["id"].as_str() == Some(planet)
            || p["name"].as_str().map(|n| n.eq_ignore_ascii_case(planet)) == Some(true)
        {
            return Ok(p["id"].as_str().unwrap_or(planet).to_string());
        }
    }
    // 未命中也透传,由 daemon 404 planetNotFound 统一报错
    Ok(planet.to_string())
}

fn list(client: &Client, json: bool) -> Result<(), CliError> {
    let v = client.get_json(endpoints::identity())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }
    let g: Option<GlobalIdentity> = serde_json::from_value(
        v.get("global").cloned().unwrap_or(Value::Null),
    )
    .ok();
    let rows: Vec<PlanetIdentity> = serde_json::from_value(
        v.get("planets").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    match g.and_then(|g| g.node_id) {
        Some(node) => println!("{}", rust_i18n::t!("cli.identity.global", node = node)),
        None => println!("{}", rust_i18n::t!("cli.identity.globalNone")),
    }
    for p in rows {
        println!(
            "{}  {}  {}  {}",
            p.name,
            p.identity,
            p.node_id.as_deref().unwrap_or("-"),
            if p.running { "running" } else { "stopped" },
        );
    }
    Ok(())
}

fn import(client: &Client, path: Option<&str>, force: bool, json: bool) -> Result<(), CliError> {
    let mut body = json!({});
    if let Some(p) = path {
        body["path"] = json!(p);
    }
    if force {
        body["force"] = json!(true);
    }
    // 长超时路径:daemon 侧要停禁官方服务,systemctl stop 最长约 90s
    let resp = client.post_json_timeout(endpoints::identity_import(), &body)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
        return Ok(());
    }
    let node = resp["nodeId"].as_str().unwrap_or("-");
    match resp["serviceAction"].as_str() {
        Some("stoppedAndDisabled") => println!(
            "{}",
            rust_i18n::t!("cli.identity.importedStopped", node = node)
        ),
        Some("notInstalled") => {
            println!("{}", rust_i18n::t!("cli.identity.imported", node = node));
            println!("{}", rust_i18n::t!("cli.identity.officialAbsent"));
        }
        _ => println!("{}", rust_i18n::t!("cli.identity.imported", node = node)),
    }
    Ok(())
}

fn switch(client: &Client, planet: &str, mode: IdentityMode, json: bool) -> Result<(), CliError> {
    let id = resolve_id(client, planet)?;
    if mode == IdentityMode::Own {
        // 独立身份 = 新节点地址(或复用留盘旧 own 身份),成员资格需重新授权
        eprintln!("{}", rust_i18n::t!("cli.identity.switchOwnWarn"));
    }
    let body = json!({ "identity": mode.as_str() });
    let resp = client.put_json(&endpoints::planet_identity(&id), &body)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
        return Ok(());
    }
    println!(
        "{}",
        rust_i18n::t!("cli.identity.switched", planet = planet, mode = mode.as_str())
    );
    Ok(())
}
