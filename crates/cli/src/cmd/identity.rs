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
        /// Overwrite existing global identity (interactive confirm if omitted)
        #[arg(long)]
        force: bool,
        /// Identity mode after import; required in non-interactive contexts
        #[arg(long)]
        mode: Option<IdentityMode>,
        /// With --mode independent and a matching registered planet: replace
        /// that planet's own identity with the official one (default keep)
        #[arg(long)]
        planet_identity: Option<PlanetIdentityChoice>,
    },
    /// Switch global identity mode (shared | independent)
    Use { mode: IdentityMode },
}

/// 独立模式下相同 planet 的身份选择(导入交互二选一的参数化形态)
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PlanetIdentityChoice {
    Official,
    Keep,
}

pub fn run(client: &Client, cmd: &Option<IdentityCmd>, json: bool) -> Result<(), CliError> {
    match cmd.as_ref().unwrap_or(&IdentityCmd::List) {
        IdentityCmd::List => list(client, json),
        IdentityCmd::Import { path, force, mode, planet_identity } => import(
            client,
            path.as_deref(),
            *force,
            *mode,
            *planet_identity,
            json,
        ),
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

/// 交互提问:读一行 stdin,trim;EOF 视为空串(走各问题的缺省/取消分支)
fn ask(prompt: &str) -> String {
    use std::io::Write as _;
    print!("{prompt} ");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s.trim().to_string()
}

fn import(
    client: &Client,
    path: Option<&str>,
    force: bool,
    mode: Option<IdentityMode>,
    planet_identity: Option<PlanetIdentityChoice>,
    json: bool,
) -> Result<(), CliError> {
    use std::io::IsTerminal as _;
    // 1. 预检(只读):拿导入决策事实。预检是独立路由,旧 daemon 对其
    //    404(旧版会把未知请求体字段的 /identity/import 当正式导入执行,
    //    靠路由区分才能拦住);响应缺 preflight 标记同样拒绝,双保险
    let mut pre = json!({ "preflight": true });
    if let Some(p) = path {
        pre["path"] = json!(p);
    }
    let pre = client
        .post_json_timeout(endpoints::identity_import_preflight(), &pre)
        .map_err(|e| match e {
            CliError::Api(404, _) => {
                CliError::Usage(rust_i18n::t!("cli.identity.needNewDaemon").to_string())
            }
            other => other,
        })?;
    if pre.get("preflight").and_then(|v| v.as_bool()) != Some(true) {
        return Err(CliError::Usage(
            rust_i18n::t!("cli.identity.needNewDaemon").to_string(),
        ));
    }
    let interactive = std::io::stdin().is_terminal() && !json;
    let official_node = pre["nodeId"].as_str().unwrap_or("-").to_string();
    // 2. 覆盖确认:已有全局身份且未 --force,交互下确认(回车=不继续)
    let mut force = force;
    if !force
        && pre["globalIdentity"]["exists"].as_bool() == Some(true)
    {
        let old = pre["globalIdentity"]["nodeId"].as_str().unwrap_or("-");
        if !interactive {
            return Err(CliError::Usage(
                rust_i18n::t!("cli.identity.needForce", old = old).to_string(),
            ));
        }
        let ans = ask(&rust_i18n::t!("cli.identity.confirmOverwrite", old = old).to_string());
        if !ans.eq_ignore_ascii_case("y") && !ans.eq_ignore_ascii_case("yes") {
            println!("{}", rust_i18n::t!("cli.identity.cancelled"));
            return Ok(());
        }
        force = true;
    }
    // 3. 模式选择(交还用户):共享=全部 planet 复用导入身份(独立模式下
    //    各 planet 运行地址会变);独立=各 planet 身份保持不变
    let mode = match mode {
        Some(m) => m,
        None => {
            if !interactive {
                return Err(CliError::Usage(
                    rust_i18n::t!("cli.identity.needModeFlag").to_string(),
                ));
            }
            loop {
                let ans = ask(&rust_i18n::t!("cli.identity.chooseMode").to_string());
                match ans.as_str() {
                    "1" => break IdentityMode::Shared,
                    "2" => break IdentityMode::Independent,
                    _ => println!("{}", rust_i18n::t!("cli.identity.invalidChoice", input = ans)),
                }
            }
        }
    };
    // 4. 相同 planet 二选一(仅独立模式且该 planet 自有身份与官方不同):
    //    换官方身份 / 保留原身份
    let mut planet_official = planet_identity == Some(PlanetIdentityChoice::Official);
    if mode == IdentityMode::Independent && planet_identity.is_none() && interactive {
        let cur = pre["matchingPlanet"]["nodeId"].as_str();
        if let Some(cur) = cur.filter(|c| *c != official_node) {
            let name = pre["matchingPlanet"]["name"].as_str().unwrap_or("-");
            loop {
                let ans = ask(&rust_i18n::t!(
                    "cli.identity.choosePlanetIdentity",
                    name = name,
                    old = cur,
                    new = official_node
                )
                .to_string());
                match ans.as_str() {
                    "1" => {
                        planet_official = true;
                        break;
                    }
                    "2" => break,
                    _ => println!("{}", rust_i18n::t!("cli.identity.invalidChoice", input = ans)),
                }
            }
        }
    }
    // 5. 正式导入(长超时:daemon 停禁官方服务最长 90s)
    let mut body = json!({ "mode": mode.as_str() });
    if let Some(p) = path {
        body["path"] = json!(p);
    }
    if force {
        body["force"] = json!(true);
    }
    if mode == IdentityMode::Independent {
        body["planetIdentity"] =
            json!(if planet_official { "official" } else { "keep" });
    }
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
    // 模式切换结果提示(用户显式选择的落地回执)
    if resp["modeChanged"].as_bool().unwrap_or(false) {
        let label = match resp["identityMode"].as_str() {
            Some("independent") => rust_i18n::t!("cli.identity.modeIndependent").to_string(),
            _ => rust_i18n::t!("cli.identity.modeShared").to_string(),
        };
        println!("{}", rust_i18n::t!("cli.identity.switched", mode = label));
    }
    // 相同 planet 换用官方身份的回执(下次运行生效)
    if let Some(name) = resp["planetIdentityApplied"].as_str() {
        println!("{}", rust_i18n::t!("cli.identity.planetIdentityApplied", name = name));
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
