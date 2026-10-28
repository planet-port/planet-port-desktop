//! POC 1:控制面验证 —— Node 构造 → join → 状态轮询 → 干净关停。
//!
//! 每 2s 打印 address/online/各网络 status/netconfRevision/assigned/routes;
//! 事件与网络配置变更由驱动线程送达 PrintSink 即时打印;
//! 结束时打印最终 networks 快照并 shutdown(驱动线程 join 后 delete)。
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use zt_core::{
    NodeHandle, NetConfig, NetConfigOp, NetStatus, NodeEvent, NodeParams, NodeSink, local_ipv4s,
};

#[derive(Parser)]
struct Args {
    /// 数据目录(identity/planet/网络配置落盘)
    #[arg(long, default_value = "target/poc-data/node-a")]
    data_dir: PathBuf,
    /// 物理 UDP 端口(本机 9993 被官方 zerotier-one 占用,POC 用 9994/9995)
    #[arg(long, default_value_t = 9994)]
    port: u16,
    /// planet 文件
    #[arg(long, default_value = "crates/pocs/fixtures/planet.Gozi25")]
    planet: PathBuf,
    /// 加入的网络 ID(十六进制,可带 0x 前缀)
    #[arg(long)]
    join: Option<String>,
    /// 运行秒数
    #[arg(long, default_value_t = 300)]
    duration: u64,
}

fn parse_nwid(s: &str) -> Result<u64, String> {
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(t, 16).map_err(|e| format!("nwid `{s}` 不是十六进制: {e}"))
}

/// 打印 sink:事件/网络配置即时输出;帧仅计数(POC1 无设备,Task 4 换 DevicePlane sink)。
/// 计数器用 Arc 共享,便于主线程轮询展示。
struct PrintSink {
    frames: Arc<AtomicU64>,
}

impl NodeSink for PrintSink {
    fn on_event(&mut self, ev: NodeEvent) {
        println!("[event]   {ev:?}");
    }
    fn on_net_config(&mut self, nwid: u64, op: NetConfigOp, cfg: NetConfig) {
        println!("[netconf] {nwid:016x} {op:?} status={:?} rev={} assigned={:?} routes={:?}",
            cfg.status, cfg.netconf_revision, cfg.assigned, cfg.routes);
    }
    fn on_frame(&mut self, nwid: u64, src: u64, dst: u64, ether_type: u16, payload: &[u8]) {
        let n = self.frames.fetch_add(1, Ordering::Relaxed) + 1;
        println!("[frame#{n}] {nwid:016x} {src:012x}->{dst:012x} eth={ether_type:#06x} len={}", payload.len());
    }
    fn local_ipv4s(&self) -> Vec<Ipv4Addr> {
        local_ipv4s()
    }
}

fn print_status(h: &NodeHandle, frames: &AtomicU64) {
    println!(
        "[t] address={:010x} online={} frames_out={}",
        h.address(),
        h.online(),
        frames.load(Ordering::Relaxed)
    );
    for n in h.networks() {
        print_net(&n);
    }
}

fn print_net(n: &NetConfig) {
    let status = status_name(&n.status);
    let assigned: Vec<String> =
        n.assigned.iter().map(|a| format!("{}/{}", a.addr, a.prefix)).collect();
    let routes: Vec<String> = n
        .routes
        .iter()
        .map(|r| match r.via {
            Some(v) => format!("{}/{} via {}", r.target.addr, r.target.prefix, v),
            None => format!("{}/{}", r.target.addr, r.target.prefix),
        })
        .collect();
    println!(
        "  net {:016x} \"{}\" status={status} rev={} mtu={} mac={:012x}",
        n.nwid, n.name, n.netconf_revision, n.mtu, n.mac
    );
    println!("    assigned: [{}]", assigned.join(", "));
    println!("    routes:   [{}]", routes.join(", "));
}

fn status_name(s: &NetStatus) -> &'static str {
    match s {
        NetStatus::RequestingConfiguration => "REQUESTING_CONFIGURATION",
        NetStatus::Ok => "OK",
        NetStatus::AccessDenied => "ACCESS_DENIED",
        NetStatus::NotFound => "NOT_FOUND",
        NetStatus::PortError => "PORT_ERROR",
        NetStatus::ClientTooOld => "CLIENT_TOO_OLD",
        NetStatus::AuthenticationRequired => "AUTHENTICATION_REQUIRED",
    }
}

fn main() {
    let args = Args::parse();
    let (major, minor, rev) = NodeHandle::core_version();
    println!("core version = {major}.{minor}.{rev}");
    println!(
        "data-dir={} port={} planet={}",
        args.data_dir.display(),
        args.port,
        args.planet.display()
    );

    let frames = Arc::new(AtomicU64::new(0));
    let h = match NodeHandle::start(
        NodeParams {
            data_dir: args.data_dir.clone(),
            planet_file: args.planet.clone(),
            bind_port: args.port,
        },
        PrintSink { frames: Arc::clone(&frames) },
    ) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("node start 失败: {e}");
            std::process::exit(1);
        }
    };
    println!("node address = {:010x}", h.address());
    println!("本机 IPv4(组播订阅目标): {:?}", local_ipv4s());

    if let Some(nw) = &args.join {
        match parse_nwid(nw).and_then(|id| h.join(id).map_err(|e| e.to_string())) {
            Ok(()) => println!("join {nw} 已下发"),
            Err(e) => {
                eprintln!("join 失败: {e}");
                h.shutdown();
                std::process::exit(1);
            }
        }
    }

    let end = Instant::now() + Duration::from_secs(args.duration);
    while Instant::now() < end {
        print_status(&h, &frames);
        std::thread::sleep(Duration::from_secs(2));
    }

    println!("===== 最终 networks 快照 =====");
    for n in h.networks() {
        print_net(&n);
    }
    println!("===== shutdown =====");
    h.shutdown();
    println!("exit clean");
}
