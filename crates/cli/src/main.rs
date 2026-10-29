mod client;
mod cmd;
mod emoji;
mod output;
mod picker;

use clap::{Parser, Subcommand};
use client::{CliError, Client};
use std::io::IsTerminal;

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
    /// 启用交互式 planet 选择器(默认非交互;仍要求 stdin 为 TTY)
    #[arg(long = "ui", global = true)]
    ui: bool,
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
    /// 管理 planet-portd 系统服务(LaunchDaemon 安装/卸载/启停/状态)
    #[command(name = "service")]
    Service {
        #[command(subcommand)]
        cmd: cmd::service::ServiceCmd,
    },
    /// Show event log
    Events {
        #[arg(long, default_value_t = 40)]
        limit: usize,
        #[arg(long)]
        follow: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    // 定位链 ①>②:--planet 显式指定,否则取 PLANET_PORT_PLANET 环境变量;
    // ③(单 running 自动选)与 ④(歧义)默认由 daemon 执行;交互式 TTY 下由
    // CLI 弹选择列表代替 ④(见 resolve_planets)
    let planet_flag = cli
        .planet
        .clone()
        .or_else(|| std::env::var("PLANET_PORT_PLANET").ok().filter(|s| !s.is_empty()));

    // service 子命令不经 REST client(全新机器上无 token 也须能 install/status)
    let result = match &cli.command {
        Command::Service { cmd } => cmd::service::run((*cmd).clone(), json),
        other => {
            let client = match Client::new(cli.api_url.clone()) {
                Ok(c) => c,
                Err(e) => die(&e),
            };
            let (planet_flag, query_filter) = resolve_planets(&client, other, planet_flag, json, cli.ui);
            dispatch(&client, other, &planet_flag, &query_filter, json)
        }
    };
    if let Err(e) = result {
        die(&e);
    }
}

// ---- 交互式 planet 选择 ----

/// 命令对 planet 的依赖形态:变更类单选(running 候选),events 单选(全量候选,
/// 仅一个 planet 时自动),可批量查询类多选(含「全部」)
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
        | Command::Orbit { .. }
        | Command::Deorbit { .. }
        | Command::Dump
        | Command::Moon { .. } => PlanetNeed::Single,
        _ => PlanetNeed::None,
    }
}

/// 命令的短标签(选择器提示用)
fn planet_verb(cmd: &Command) -> &'static str {
    match cmd {
        Command::Info => "info",
        Command::List => "list",
        Command::Listnetworks => "listnetworks",
        Command::Listpeers => "listpeers",
        Command::Listmoons => "listmoons",
        Command::Events { .. } => "events",
        Command::Join { .. } => "join",
        Command::Leave { .. } => "leave",
        Command::Set { .. } => "set",
        Command::Orbit { .. } => "orbit",
        Command::Deorbit { .. } => "deorbit",
        Command::Dump => "dump",
        Command::Moon { .. } => "moon import",
        _ => "",
    }
}

/// 取 planet 清单(/status);daemon 不可达等失败返回 None(跳过选择器,
/// 让后续请求自身报错,行为与非交互路径一致)
fn fetch_options(client: &Client, running_only: bool) -> Option<Vec<picker::PlanetOption>> {
    let v = client.get_json(planet_port_api::endpoints::status()).ok()?;
    let rows: Vec<planet_port_api::types::PlanetStatus> = serde_json::from_value(
        v.get("planets").cloned().unwrap_or(serde_json::Value::Null),
    )
    .unwrap_or_default();
    let out: Vec<picker::PlanetOption> = rows
        .into_iter()
        .filter(|p| !running_only || p.status == "running")
        .map(|p| picker::PlanetOption {
            label: format!("{} ({}) · {}", p.name, p.id, p.status),
            id: p.id,
        })
        .collect();
    Some(out)
}

/// 未显式指定 planet 时的定位。默认非交互(多 planet 走聚合 / 报错);
/// 仅显式传 --ui 且 stdin 为 TTY 时弹选择列表。返回(单选 planet, 查询过滤集)。
/// 单选结果回填 planet_flag(等价 --planet);多选子集作为查询过滤集(None = 全部)。
/// 取消(Esc/Ctrl+C)退出码 130
fn resolve_planets(
    client: &Client,
    command: &Command,
    planet_flag: Option<String>,
    json: bool,
    ui: bool,
) -> (Option<String>, Option<Vec<String>>) {
    // 交互选择器三重前置:显式 --ui、非 --json、stdin 为 TTY
    let interactive = ui && !json && std::io::stdin().is_terminal();
    match planet_need(command) {
        PlanetNeed::None => (planet_flag, None),
        PlanetNeed::Single => {
            if planet_flag.is_none() && interactive {
                if let Some(opts) = fetch_options(client, true) {
                    // 多 running 才需要选;单个/零个沿用 daemon 自动选/报错
                    if opts.len() > 1 {
                        return match picker::pick_single(&opts, planet_verb(command)) {
                            Ok(id) => (Some(id), None),
                            Err(()) => std::process::exit(130),
                        };
                    }
                }
            }
            (planet_flag, None)
        }
        PlanetNeed::Events => {
            // 与 dump 同款定位:显式 > 单 running 自动 > 交互单选 / 非交互报错。
            // daemon 的 /events 端点是过滤语义(无歧义判定),歧义由 CLI 兜住
            if planet_flag.is_some() {
                return (planet_flag, None);
            }
            if let Some(opts) = fetch_options(client, true) {
                match opts.len() {
                    1 => return (Some(opts[0].id.clone()), None),
                    0 => {}
                    _ if interactive => {
                        return match picker::pick_single(&opts, "events") {
                            Ok(id) => (Some(id), None),
                            Err(()) => std::process::exit(130),
                        };
                    }
                    n => {
                        eprintln!("error: events 需要指定 --planet(当前 {n} 个 running planet)");
                        std::process::exit(2);
                    }
                }
            }
            (None, None)
        }
        PlanetNeed::Multi => {
            if let Some(p) = planet_flag {
                // 显式 --planet / 环境变量:等价单成员过滤集
                return (None, Some(vec![p]));
            }
            if interactive {
                if let Some(opts) = fetch_options(client, false) {
                    if opts.len() > 1 {
                        return match picker::pick_multi(&opts, planet_verb(command)) {
                            Ok(picker::MultiPick::All) => (None, None),
                            Ok(picker::MultiPick::Subset(ids)) => (None, Some(ids)),
                            Err(()) => std::process::exit(130),
                        };
                    }
                }
            }
            (None, None)
        }
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
        Command::Service { .. } => unreachable!("service 在 main 中单独分流"),
        Command::Info => cmd::query::info(client, query_filter, json),
        Command::List => cmd::query::list(client, query_filter, json),
        Command::Listnetworks => cmd::query::listnetworks(client, query_filter, json),
        Command::Listpeers => cmd::query::listpeers(client, query_filter, json),
        Command::Listmoons => cmd::query::listmoons(client, query_filter, json),
        Command::Join { nwid } => cmd::mutate::join(client, nwid, planet_flag, json),
        Command::Leave { nwid } => cmd::mutate::leave(client, nwid, planet_flag, json),
        Command::Set { nwid, values } => cmd::mutate::set(client, nwid, values, planet_flag, json),
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
        Command::Use { planet } => cmd::misc::use_planet(client, planet),
        Command::Events { limit, follow } => cmd::misc::events(client, planet_flag, *limit, *follow, json),
    }
}

fn die(e: &CliError) -> ! {
    match e {
        CliError::Offline => {
            eprintln!("planet-portd is not running (try: planet-port service start)")
        }
        CliError::Api(code, api) => {
            eprintln!("error {code}: {}", api.message);
            if let Some(c) = &api.candidates {
                for line in c {
                    eprintln!("  {line}");
                }
            }
        }
        CliError::Usage(m) => eprintln!("error: {m}"),
        CliError::Io(m) => eprintln!("error: {m}"),
    }
    std::process::exit(e.exit_code());
}
