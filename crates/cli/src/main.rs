mod client;
mod cmd;
mod emoji;
mod output;

use clap::{Parser, Subcommand};
use client::{CliError, Client};

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
    // ③(单 running 自动选)与 ④(歧义 409)由 daemon 执行
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
            dispatch(&client, other, &planet_flag, json)
        }
    };
    if let Err(e) = result {
        die(&e);
    }
}

fn dispatch(
    client: &Client,
    command: &Command,
    planet_flag: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    match command {
        Command::Service { .. } => unreachable!("service 在 main 中单独分流"),
        Command::Info => cmd::query::info(client, json),
        Command::Listnetworks => cmd::query::listnetworks(client, planet_flag, json),
        Command::Listpeers => cmd::query::listpeers(client, planet_flag, json),
        Command::Listmoons => cmd::query::listmoons(client, planet_flag, json),
        Command::Join { nwid } => cmd::mutate::join(client, nwid, planet_flag, json),
        Command::Leave { nwid } => cmd::mutate::leave(client, nwid, planet_flag, json),
        Command::Set { nwid, values } => cmd::mutate::set(client, nwid, values, planet_flag, json),
        Command::Orbit { moon_id, seed } => cmd::mutate::orbit(client, moon_id, seed, planet_flag, json),
        Command::Deorbit { moon_id } => cmd::mutate::deorbit(client, moon_id, planet_flag, json),
        Command::Dump => cmd::mutate::dump(client, planet_flag, json),
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
