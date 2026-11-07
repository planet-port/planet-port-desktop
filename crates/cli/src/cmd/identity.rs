//! identity 子命令:身份模式与身份列表(缺省)、官方身份导入、全局模式切换。

use clap::{Subcommand, ValueEnum};
use serde_json::json;

use crate::client::{CliError, Client};
use planet_port_api::endpoints;
use planet_port_api::types::{GlobalIdentity, IdentityOverview};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum IdentityMode {
    Shared,
    Independent,
}

impl IdentityMode {
    fn as_str(&self) -> &'static str {
        match self {
            IdentityMode::Shared => "shared",
            IdentityMode::Independent => "independent",
        }
    }

    fn label(&self) -> String {
        match self {
            IdentityMode::Shared => rust_i18n::t!("cli.identity.modeShared").to_string(),
            IdentityMode::Independent => rust_i18n::t!("cli.identity.modeIndependent").to_string(),
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
    /// Switch global identity mode (shared | independent)
    Use { mode: IdentityMode },
}

pub fn run(client: &Client, cmd: &Option<IdentityCmd>, json: bool) -> Result<(), CliError> {
    match cmd.as_ref().unwrap_or(&IdentityCmd::List) {
        IdentityCmd::List => list(client, json),
        IdentityCmd::Import { path, force } => import(client, path.as_deref(), *force, json),
        IdentityCmd::Use { mode } => switch(client, *mode, json),
    }
}

fn list(client: &Client, json: bool) -> Result<(), CliError> {
    let v = client.get_json(endpoints::identity())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(());
    }
    let ov: IdentityOverview = serde_json::from_value(v).unwrap_or(IdentityOverview {
        mode: "-".into(),
        global: GlobalIdentity {
            exists: false,
            node_id: None,
        },
        planets: Vec::new(),
    });
    let mode_label = match ov.mode.as_str() {
        "shared" => rust_i18n::t!("cli.identity.modeShared").to_string(),
        "independent" => rust_i18n::t!("cli.identity.modeIndependent").to_string(),
        other => other.to_string(),
    };
    println!("{}", rust_i18n::t!("cli.identity.mode", mode = mode_label));
    match ov.global.node_id {
        Some(node) => println!("{}", rust_i18n::t!("cli.identity.global", node = node)),
        None => println!("{}", rust_i18n::t!("cli.identity.globalNone")),
    }
    for p in ov.planets {
        println!(
            "{}  {}  {}",
            p.name,
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
    // 官方客户端当前 planet 的处置提示:身份迁移保留原节点地址、成员
    // 资格随之保留,但网络挂在哪个 planet 需要指引用户(已导入 → 直接
    // join;未导入 → 先 add 官方目录里的 planet 文件)
    if let Some(p) = resp["officialPlanet"].as_object() {
        let world_id = p
            .get("worldId")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .to_string();
        let kind = if p.get("official").and_then(|v| v.as_bool()).unwrap_or(false) {
            rust_i18n::t!("cli.identity.planetKindOfficial").to_string()
        } else {
            rust_i18n::t!("cli.identity.planetKindCustom").to_string()
        };
        match p.get("registeredName").and_then(|v| v.as_str()) {
            Some(name) => println!(
                "{}",
                rust_i18n::t!("cli.identity.planetRegistered", name = name)
            ),
            None => {
                let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("-");
                println!(
                    "{}",
                    rust_i18n::t!(
                        "cli.identity.planetMissing",
                        kind = kind,
                        worldId = world_id,
                        path = path
                    )
                );
            }
        }
    }
    // Independent → Shared 的自动切换提示(导入语义 = 整端改用官方身份)
    if resp["modeSwitched"].as_bool().unwrap_or(false) {
        println!("{}", rust_i18n::t!("cli.identity.modeAutoSwitched"));
    }
    // 游离成员记录指引:被取代的 id 在控制器各网络的成员记录不会自行
    // 消失(节点无权删自身记录),提示到控制器侧清理
    let superseded: Vec<&str> = resp["supersededNodeIds"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !superseded.is_empty() {
        println!(
            "{}",
            rust_i18n::t!("cli.identity.supersededHint", ids = superseded.join(", "))
        );
    }
    Ok(())
}

fn switch(client: &Client, mode: IdentityMode, json: bool) -> Result<(), CliError> {
    let body = json!({ "mode": mode.as_str() });
    let resp = client.put_json(endpoints::identity(), &body)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
        return Ok(());
    }
    // 成功后再提示:切换被 409 拒绝时打印警告会误导(什么都没发生)
    eprintln!("{}", rust_i18n::t!("cli.identity.switchWarn"));
    println!(
        "{}",
        rust_i18n::t!("cli.identity.switched", mode = mode.label())
    );
    Ok(())
}
