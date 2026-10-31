//! POC 4:Windows 支持可行性(wintun L3 + ZeroTier 核心,最低 Win10)。
//! 三道 Gate:G0 构建链路(已由 zt-sys 构建通过)/ G1 设备层(Task 4/5)/
//! G2 组网互通(Task 6 控制面 → Task 7 数据面)。
//! 本 crate 独立于 pocs,可整包 `cargo xwin check -p poc4` 交叉编译检查;
//! 非 Windows 平台仅编译占位 main,保证 workspace 全平台可编译。

#[cfg(windows)]
mod arp;
#[cfg(windows)]
mod device_win;
#[cfg(windows)]
mod driver;
#[cfg(windows)]
mod netcfg;
#[cfg(windows)]
mod routes_win;
#[cfg(windows)]
mod sa;
#[cfg(windows)]
mod store;

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Gate 1 设备层自验入口保留;其余走控制面(Task 6)
    match args.get(1).map(String::as_str) {
        Some("--gate1-create") | Some("--gate1-ip") | Some("--gate1-read") => {
            std::process::exit(device_win::gate1_cli());
        }
        _ => std::process::exit(control_cli(&args)),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("poc4_windows 仅在 Windows 上运行(Windows 支持评估 POC)");
}

#[cfg(windows)]
const USAGE: &str = "用法:\n  \
    控制面: poc4_windows --planet <file> --join <nwid> --dir <dir> [--port 9994] [--duration 600] [--target ip]\n  \
    设备层: poc4_windows --gate1-create | --gate1-ip a.b.c.d/nn | --gate1-read a.b.c.d/nn [probe_last]";

/// 控制面入口:join planet → 每 2s 打印 address/online/netconf → 到点关停。
/// 设备应用(netcfg → wintun 地址/路由/帧桥)留 Task 7,本阶段 netcfg 只打印。
#[cfg(windows)]
fn control_cli(args: &[String]) -> i32 {
    let (mut planet, mut join, mut dir) = (None, None, None);
    let (mut port, mut duration, mut target) = (9994u16, 600u64, None::<String>);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--planet" => {
                i += 1;
                planet = args.get(i).cloned();
            }
            "--join" => {
                i += 1;
                join = args.get(i).cloned();
            }
            "--dir" => {
                i += 1;
                dir = args.get(i).cloned();
            }
            "--port" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    port = v;
                }
            }
            "--duration" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    duration = v;
                }
            }
            "--target" => {
                i += 1;
                target = args.get(i).cloned();
            }
            other => {
                eprintln!("未知参数: {other}\n{USAGE}");
                return 2;
            }
        }
        i += 1;
    }
    let (Some(planet), Some(join), Some(dir)) = (planet, join, dir) else {
        eprintln!("{USAGE}");
        return 2;
    };
    // nwid 以 16 进制解析为准(如 5cf54f181554090f);纯数字串一律先按 hex 解,
    // 仅当 hex 溢出(>16 位的纯 10 进制长串)或含非法字符时才退 10 进制试一次
    let nwid = match u64::from_str_radix(&join, 16) {
        Ok(v) => v,
        Err(_) => match join.parse::<u64>() {
            Ok(v) => v,
            Err(_) => {
                eprintln!("[ctl] 非法 nwid: {join}");
                return 2;
            }
        },
    };
    if let Some(t) = &target {
        println!("[ctl] --target {t} 需数据面(Task 7)接入后才有意义,本阶段忽略");
    }
    let (v, m, r) = driver::PocNode::core_version();
    println!(
        "[ctl] zt-core {v}.{m}.{r} planet={} nwid={nwid:016x} dir={dir} port={port} duration={duration}s",
        planet
    );
    let node = match driver::PocNode::start(dir.into(), planet.into(), port, CtlSink) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[ctl] start 失败: {e}");
            return 1;
        }
    };
    println!("[ctl] address={:010x}", node.address());
    if let Err(e) = node.join(nwid) {
        eprintln!("[ctl] join 失败: {e}");
        node.shutdown();
        return 1;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(duration);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_secs(2));
        print_status(&node, nwid);
    }
    node.shutdown();
    println!("[ctl] 已关停");
    0
}

/// 状态打印(对齐 poc3 print_status 风格):节点级一行 + 目标网络一行。
#[cfg(windows)]
fn print_status(node: &driver::PocNode, nwid: u64) {
    let peers = node.peers();
    let planets = peers.iter().filter(|p| p.role == driver::PeerRole::Planet).count();
    let active = peers
        .iter()
        .flat_map(|p| p.paths.iter())
        .filter(|pp| pp.active)
        .count();
    println!(
        "[ctl] address={:010x} online={} peers={} planet={} 活跃路径={}",
        node.address(),
        node.online(),
        peers.len(),
        planets,
        active
    );
    match node.networks().into_iter().find(|n| n.nwid == nwid) {
        Some(n) => {
            let assigned: Vec<String> =
                n.assigned.iter().map(|a| format!("{}/{}", a.addr, a.prefix)).collect();
            println!(
                "  net {:016x} status={} rev={} mtu={} assigned=[{}]",
                n.nwid,
                status_name(n.status),
                n.netconf_revision,
                n.mtu,
                assigned.join(", ")
            );
        }
        None => println!("  net {nwid:016x} (未查询到配置)"),
    }
}

#[cfg(windows)]
fn status_name(s: netcfg::NetStatus) -> &'static str {
    use netcfg::NetStatus as S;
    match s {
        S::RequestingConfiguration => "REQUESTING_CONFIGURATION",
        S::Ok => "OK",
        S::AccessDenied => "ACCESS_DENIED",
        S::NotFound => "NOT_FOUND",
        S::PortError => "PORT_ERROR",
        S::ClientTooOld => "CLIENT_TOO_OLD",
        S::AuthenticationRequired => "AUTHENTICATION_REQUIRED",
    }
}

/// 控制面 sink:事件/netcfg/帧全部打印;设备面(Task 7)接入前
/// local_ipv4s 返回空(组播订阅自然跳过,不影响 netconf 流程)。
#[cfg(windows)]
struct CtlSink;

#[cfg(windows)]
impl driver::PocSink for CtlSink {
    fn on_event(&mut self, ev: &str) {
        eprintln!("[event] {ev}");
    }

    fn on_net_config(&mut self, cfg: netcfg::NetConfig) {
        let assigned: Vec<String> =
            cfg.assigned.iter().map(|a| format!("{}/{}", a.addr, a.prefix)).collect();
        let routes: Vec<String> =
            cfg.routes.iter().map(|r| format!("{}/{}", r.target.addr, r.target.prefix)).collect();
        eprintln!(
            "[netcfg] {:016x} {} rev={} name={:?} assigned=[{}] routes=[{}]",
            cfg.nwid,
            status_name(cfg.status),
            cfg.netconf_revision,
            cfg.name,
            assigned.join(", "),
            routes.join(", ")
        );
    }

    fn on_frame(&mut self, nwid: u64, src: u64, dst: u64, eth: u16, payload: &[u8]) {
        // Task 6 无设备面:核心不应产出帧,出现即打印一条便于发现异常
        eprintln!("[frame] {nwid:016x} {src:012x}->{dst:012x} eth={eth:04x} len={}", payload.len());
    }

    fn local_ipv4s(&self) -> Vec<std::net::Ipv4Addr> {
        // Task 7:返回设备已应用的 IPv4,驱动线程据此做组播订阅
        Vec::new()
    }
}
