mod client;
mod cmd;
mod emoji;
mod output;

use clap::{Parser, Subcommand};
use client::{CliError, Client};

// 多语言:文案在 crates/cli/locales/<locale>.yml,新增语言加文件即可。
// 源语言 zh-Hans(与 iOS Localizable.xcstrings 一致),缺失键回退到它
rust_i18n::i18n!("locales", fallback = "zh-Hans");

#[derive(Parser)]
#[command(
    name = "planet-port",
    version,
    about = "Planet Port CLI - ZeroTier multi-planet client"
)]
struct Cli {
    /// Target planet (name or worldId); filter for queries, selector for mutations
    #[arg(long, global = true)]
    planet: Option<String>,
    /// JSON output
    #[arg(short = 'j', long = "json", global = true)]
    json: bool,
    /// Override daemon API URL (debug)
    #[arg(long, global = true, env = "PLANET_PORT_URL")]
    api_url: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Aggregate status of all planets
    Info,
    /// List networks grouped by planet
    List,
    /// Show one joined network's full detail
    Network {
        #[command(subcommand)]
        cmd: cmd::query::NetworkCmd,
    },
    /// List networks across planets
    Listnetworks,
    /// List peers, grouped by planet
    Listpeers,
    /// Join a network
    Join { nwid: String },
    /// Leave a network
    Leave { nwid: String },
    /// Set client-side config: routeViaZeroTier / dnsMode / customDns
    Set { nwid: String, values: Vec<String> },
    /// Get one config value of a joined network (script-friendly)
    Get { nwid: String, key: String },
    /// List moons
    Listmoons,
    /// Orbit a moon
    Orbit { moon_id: String, seed: String },
    /// Deorbit a moon
    Deorbit { moon_id: String },
    /// Dump state of a single planet
    Dump,
    /// Import a planet from local file and bring it online
    Add {
        source: String,
        /// Optional alias
        #[arg(long)]
        name: Option<String>,
        /// Override data dir (absolute path); omit to use daemon default
        #[arg(long)]
        data_dir: Option<String>,
        /// Override UDP port; omit to use daemon auto-assignment
        #[arg(long)]
        port: Option<u16>,
    },
    /// Remove a planet registration
    Remove {
        id: String,
        /// Skip confirmation (scripts)
        #[arg(long)]
        yes: bool,
    },
    /// Start a stopped planet
    Start { id: String },
    /// Stop a running planet (keep registered)
    Stop { id: String },
    /// Planet management
    Planet {
        #[command(subcommand)]
        cmd: cmd::planet::PlanetCmd,
    },
    /// Moon import
    Moon {
        #[command(subcommand)]
        cmd: cmd::mutate::MoonCmd,
    },
    /// Print shell export line to set default planet
    #[command(name = "use")]
    Use { planet: String },
    /// 管理 planet-portd 系统服务(launchd/systemd 安装/卸载/启停/状态)
    #[command(name = "service")]
    Service {
        #[command(subcommand)]
        cmd: cmd::service::ServiceCmd,
    },
    /// 一键安装:同目录二进制套装落盘预设路径 + PATH 软链 + 装服务(需 root)
    Install {
        /// 跳过版本差异确认(脚本用;与已装版本相同时本就不提示)
        #[arg(long)]
        yes: bool,
    },
    /// Show event log
    Events {
        #[arg(long, default_value_t = 40)]
        limit: usize,
        #[arg(long)]
        follow: bool,
    },
}

/// 语言跟随系统,无需配置:LC_ALL > LC_MESSAGES > LANG > sys-locale 探测,
/// 均无法判定时回退 zh-Hans(源语言)
fn init_locale() {
    // 映射放进 find_map:某变量虽已设置但不可映射(如 C / POSIX / 空串)时
    // 继续尝试下一个变量,而不是直接跳到 sys-locale
    let detected = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().as_deref().and_then(map_locale))
        .or_else(|| sys_locale::get_locale().and_then(|l| map_locale(&l)));
    rust_i18n::set_locale(&detected.unwrap_or_else(|| "zh-Hans".into()));
}

/// POSIX locale 值(如 zh_CN.UTF-8 / en_US / zh-Hant)→ 本项目 locale 名。
/// 只认 en / zh 两族;其余语言暂无文案,返回 None 走 zh-Hans 回退
fn map_locale(raw: &str) -> Option<String> {
    // 取首个 `.` / `@` 之前的 language_TERRITORY 部分
    let tag = raw.split(['.', '@']).next()?.replace('_', "-").to_lowercase();
    if tag.starts_with("en") {
        return Some("en".into());
    }
    if tag.starts_with("zh") {
        // 简体:大陆/新加坡;繁体:台湾/香港/澳门;无地区默认简体
        return Some(match tag.as_str() {
            t if t.contains("hant") || t.contains("tw") || t.contains("hk") || t.contains("mo") => {
                "zh-Hant".into()
            }
            _ => "zh-Hans".into(),
        });
    }
    None
}

fn main() {
    init_locale();
    let cli = Cli::parse();
    let json = cli.json;
    // 定位链 ①>②:--planet 显式指定,否则取 PLANET_PORT_PLANET 环境变量;
    // ③(单 running 自动选)与 ④(歧义报候选)由 daemon 执行
    let planet_flag = cli
        .planet
        .clone()
        .or_else(|| std::env::var("PLANET_PORT_PLANET").ok().filter(|s| !s.is_empty()));

    // service / install 子命令不经 REST client(全新机器上无 token 也须能安装)
    let result = match &cli.command {
        Command::Service { cmd } => cmd::service::run((*cmd).clone(), json),
        Command::Install { yes } => {
            cmd::install::run(*yes, json);
            Ok(())
        }
        other => {
            let client = match Client::new(cli.api_url.clone()) {
                Ok(c) => c,
                Err(e) => die(&e),
            };
            let (planet_flag, query_filter) = resolve_planets(&client, other, planet_flag);
            dispatch(&client, other, &planet_flag, &query_filter, json)
        }
    };
    if let Err(e) = result {
        die(&e);
    }
}

// ---- planet 定位 ----

/// 命令对 planet 的依赖形态:变更类单目标(daemon 自动选/报歧义),events
/// 需恰好一个目标(歧义由 CLI 兜住),可批量查询类按过滤集聚合
enum PlanetNeed {
    None,
    Single,
    Events,
    Multi,
}

fn planet_need(cmd: &Command) -> PlanetNeed {
    match cmd {
        Command::Info
        | Command::List
        | Command::Listnetworks
        | Command::Listpeers
        | Command::Listmoons => PlanetNeed::Multi,
        Command::Events { .. } => PlanetNeed::Events,
        Command::Join { .. }
        | Command::Leave { .. }
        | Command::Set { .. }
        | Command::Get { .. }
        | Command::Network { .. }
        | Command::Orbit { .. }
        | Command::Deorbit { .. }
        | Command::Dump
        | Command::Moon { .. } => PlanetNeed::Single,
        _ => PlanetNeed::None,
    }
}

/// 登记行星的 canonical id 清单(/status,不看连接状态——计数口径以登记为准);
/// daemon 不可达等失败返回 None,让后续请求自身报错
fn registered_planet_ids(client: &Client) -> Option<Vec<String>> {
    let v = client.get_json(planet_port_api::endpoints::status()).ok()?;
    let rows: Vec<planet_port_api::types::PlanetStatus> = serde_json::from_value(
        v.get("planets").cloned().unwrap_or(serde_json::Value::Null),
    )
    .unwrap_or_default();
    Some(rows.into_iter().map(|p| p.id).collect())
}

/// 未显式指定 planet 时的定位。返回(单选 planet, 查询过滤集):
/// - Single:原样透传,自动选/歧义由 daemon 判定;
/// - Events:与 dump 同款定位,daemon 的 /events 端点是过滤语义(无歧义
///   判定),登记 ≥2 时由 CLI 报错兜住(计数只看登记,不看连接状态);
/// - Multi:显式 planet 等价单成员过滤集,未指定则全量聚合
fn resolve_planets(
    client: &Client,
    command: &Command,
    planet_flag: Option<String>,
) -> (Option<String>, Option<Vec<String>>) {
    match planet_need(command) {
        PlanetNeed::None | PlanetNeed::Single => (planet_flag, None),
        PlanetNeed::Events => {
            if planet_flag.is_some() {
                return (planet_flag, None);
            }
            if let Some(ids) = registered_planet_ids(client) {
                match ids.len() {
                    1 => return (Some(ids[0].clone()), None),
                    0 => {}
                    n => {
                        eprintln!("{}", rust_i18n::t!("cli.events.needPlanet", n = n));
                        std::process::exit(2);
                    }
                }
            }
            (None, None)
        }
        PlanetNeed::Multi => match planet_flag {
            Some(p) => (None, Some(vec![p])),
            None => (None, None),
        },
    }
}

fn dispatch(
    client: &Client,
    command: &Command,
    planet_flag: &Option<String>,
    query_filter: &Option<Vec<String>>,
    json: bool,
) -> Result<(), CliError> {
    match command {
        Command::Service { .. } | Command::Install { .. } => {
            unreachable!("service/install 在 main 中单独分流")
        }
        Command::Info => cmd::query::info(client, query_filter, json),
        Command::List => cmd::query::list(client, query_filter, json),
        Command::Network { cmd } => cmd::query::run_network(client, cmd, planet_flag, json),
        Command::Listnetworks => cmd::query::listnetworks(client, query_filter, json),
        Command::Listpeers => cmd::query::listpeers(client, query_filter, json),
        Command::Listmoons => cmd::query::listmoons(client, query_filter, json),
        Command::Join { nwid } => cmd::mutate::join(client, nwid, planet_flag, json),
        Command::Leave { nwid } => cmd::mutate::leave(client, nwid, planet_flag, json),
        Command::Set { nwid, values } => cmd::mutate::set(client, nwid, values, planet_flag, json),
        Command::Get { nwid, key } => cmd::mutate::get(client, nwid, key, planet_flag, json),
        Command::Orbit { moon_id, seed } => cmd::mutate::orbit(client, moon_id, seed, planet_flag, json),
        Command::Deorbit { moon_id } => cmd::mutate::deorbit(client, moon_id, planet_flag, json),
        Command::Dump => cmd::mutate::dump(client, planet_flag, json),
        Command::Add { source, name, data_dir, port } => cmd::planet::import(
            client,
            source,
            name.clone(),
            data_dir.clone(),
            *port,
            json,
        ),
        Command::Remove { id, yes } => cmd::planet::rm(client, id, *yes, json),
        Command::Start { id } => cmd::planet::start(client, id, json),
        Command::Stop { id } => cmd::planet::stop(client, id, json),
        Command::Planet { cmd } => cmd::planet::run(client, cmd, json),
        Command::Moon { cmd } => cmd::mutate::run_moon(client, cmd, planet_flag, json),
        Command::Use { planet } => cmd::misc::use_planet(client, planet, json),
        Command::Events { limit, follow } => cmd::misc::events(client, planet_flag, *limit, *follow, json),
    }
}

fn die(e: &CliError) -> ! {
    match e {
        CliError::Offline => {
            eprintln!("planet-portd is not running (try: planet-port service start)")
        }
        CliError::Api(code, api) => {
            // 定位类错误改写成人话(--planet 指引 + 候选),不再透传 "?planet="
            // HTTP 行话;其余 API 错误原样(error/message/candidates 均为 API 数据)
            match api.error.as_str() {
                "planetAmbiguous" => {
                    eprintln!("{}", rust_i18n::t!("cli.error.planetAmbiguous"));
                    if let Some(c) = &api.candidates {
                        for line in c {
                            eprintln!("  {line}");
                        }
                    }
                }
                "noPlanetRunning" => {
                    eprintln!("{}", rust_i18n::t!("cli.error.noPlanetRunning"))
                }
                _ => {
                    eprintln!("error {code}: {}", api.message);
                    if let Some(c) = &api.candidates {
                        for line in c {
                            eprintln!("  {line}");
                        }
                    }
                }
            }
        }
        CliError::Usage(m) => eprintln!("error: {m}"),
        CliError::Io(m) => eprintln!("error: {m}"),
    }
    std::process::exit(e.exit_code());
}
