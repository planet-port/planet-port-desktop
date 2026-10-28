//! POC 2:设备数据面 —— feth L2 设备 + managed 地址/路由应用 + 帧双向桥接。
//!
//! 流程:start node → join → netconf(Up/ConfigUpdate)时 DevicePlane::apply
//! (建 feth 对、设 MAC/地址、写 managed 路由)→ 设备线程收帧回喂核心、
//! 核心产出帧写设备 → 可选 --target 定时 ping 网内既有成员。
//! 退出(或 netconf Down/Destroy)时 teardown:停线程、删路由、销毁 feth。
//! 需 sudo(建 feth/BPF/AF_NDRV/路由)。
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use clap::Parser;
use zt_core::{
    local_ipv4s, DevicePlane, FrameInTxSlot, NetConfig, NetConfigOp, NodeEvent, NodeHandle,
    NodeParams, NodeSink,
};

#[derive(Parser)]
struct Args {
    /// 数据目录(identity/planet/网络配置落盘;复用 poc1 的目录即同一节点身份)
    #[arg(long, default_value = "target/poc-data/node-a")]
    data_dir: PathBuf,
    /// 物理 UDP 端口(本机 9993 被官方 zerotier-one 占用,POC 用 9994/9995)
    #[arg(long, default_value_t = 9994)]
    port: u16,
    /// planet 文件
    #[arg(long, default_value = "crates/pocs/fixtures/planet.Gozi25")]
    planet: PathBuf,
    /// 加入的网络 ID(十六进制,可带 0x 前缀)
    #[arg(long, default_value = "5cf54f181554090f")]
    join: String,
    /// 网内既有成员 IPv4,每 5s ping 一次验证数据面
    #[arg(long)]
    target: Option<Ipv4Addr>,
    /// 运行秒数
    #[arg(long, default_value_t = 600)]
    duration: u64,
}

fn parse_nwid(s: &str) -> Result<u64, String> {
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(t, 16).map_err(|e| format!("nwid `{s}` 不是十六进制: {e}"))
}

/// 桥接 sink:netconf → 设备配置;核心帧 → 写设备;组播订阅地址含设备自身。
struct DeviceSink {
    plane: Arc<Mutex<DevicePlane>>,
    frame_tx_slot: FrameInTxSlot,
    frames_to_dev: Arc<AtomicU64>,
}

impl DeviceSink {
    fn lock(&self) -> std::sync::MutexGuard<'_, DevicePlane> {
        self.plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl NodeSink for DeviceSink {
    fn on_event(&mut self, ev: NodeEvent) {
        println!("[event]   {ev:?}");
    }
    fn on_net_config(&mut self, nwid: u64, op: NetConfigOp, cfg: NetConfig) {
        println!(
            "[netconf] {nwid:016x} {op:?} status={:?} rev={} assigned={:?} routes={:?}",
            cfg.status, cfg.netconf_revision, cfg.assigned, cfg.routes
        );
        let r = match op {
            NetConfigOp::Up | NetConfigOp::ConfigUpdate => self.lock().apply(&cfg, &self.frame_tx_slot),
            NetConfigOp::Down | NetConfigOp::Destroy => {
                self.lock().teardown();
                Ok(())
            }
        };
        if let Err(e) = r {
            eprintln!("[netconf] 配置应用失败: {e}");
        }
    }
    fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]) {
        let n = self.frames_to_dev.fetch_add(1, Ordering::Relaxed) + 1;
        println!(
            "[frame#{n}→dev] {nwid:016x} {src_mac:012x}->{dst_mac:012x} eth={ether_type:#06x} len={}",
            payload.len()
        );
        if let Err(e) = self.lock().send_frame(nwid, src_mac, dst_mac, ether_type, payload) {
            eprintln!("[frame] 写设备失败: {e}");
        }
    }
    fn local_ipv4s(&self) -> Vec<Ipv4Addr> {
        // 本机物理接口 IPv4 + 本设备已应用的 managed IPv4;核心侧订阅幂等,去重即可
        let mut ips = local_ipv4s();
        ips.extend(self.lock().applied_v4s());
        ips.sort_unstable();
        ips.dedup();
        ips
    }
}

fn ping_once(target: Ipv4Addr) {
    let out = Command::new("/sbin/ping").args(["-c", "1", "-t", "2"]).arg(target.to_string()).output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let line = s.lines().find(|l| l.contains("time=")).unwrap_or("");
            println!("[ping] {target} 通:{line}");
        }
        Ok(_) => println!("[ping] {target} 不通"),
        Err(e) => println!("[ping] {target} 执行失败: {e}"),
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

    let frame_tx_slot: FrameInTxSlot = Arc::new(OnceLock::new());
    let plane = Arc::new(Mutex::new(DevicePlane::new()));
    let frames_to_dev = Arc::new(AtomicU64::new(0));
    let h = match NodeHandle::start(
        NodeParams {
            data_dir: args.data_dir.clone(),
            planet_file: args.planet.clone(),
            bind_port: args.port,
        },
        DeviceSink {
            plane: Arc::clone(&plane),
            frame_tx_slot: Arc::clone(&frame_tx_slot),
            frames_to_dev: Arc::clone(&frames_to_dev),
        },
    ) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("node start 失败: {e}");
            std::process::exit(1);
        }
    };
    println!("node address = {:010x}", h.address());
    let tx = h.frame_in_tx();
    frame_tx_slot
        .set(tx)
        .map_err(|_| "FrameInTx 已被设置")
        .expect("FrameInTx 槽位冲突");

    let nwid = match parse_nwid(&args.join) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner).teardown();
            h.shutdown();
            std::process::exit(1);
        }
    };
    if let Err(e) = h.join(nwid) {
        eprintln!("join 失败: {e}");
        plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner).teardown();
        h.shutdown();
        std::process::exit(1);
    }
    println!("join {} 已下发", args.join);

    let end = Instant::now() + Duration::from_secs(args.duration);
    let mut next_ping = Instant::now();
    while Instant::now() < end {
        let dev = plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dev_name = if dev.is_up() { dev.if_name().to_string() } else { "(未建)".to_string() };
        let frames_in = dev.frames_in();
        println!(
            "[t] address={:010x} online={} frames_to_dev={} frames_in={} dev={}",
            h.address(),
            h.online(),
            frames_to_dev.load(Ordering::Relaxed),
            frames_in,
            dev_name
        );
        for n in h.networks() {
            let assigned: Vec<String> =
                n.assigned.iter().map(|a| format!("{}/{}", a.addr, a.prefix)).collect();
            println!("  net {:016x} status={:?} assigned=[{}]", n.nwid, n.status, assigned.join(", "));
        }
        drop(dev);
        if let Some(t) = args.target {
            if Instant::now() >= next_ping {
                next_ping = Instant::now() + Duration::from_secs(5);
                ping_once(t);
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    println!("===== teardown(停线程→删路由→销毁 feth)=====");
    plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner).teardown();
    println!("===== shutdown(驱动线程 join → ZT_Node_delete)=====");
    h.shutdown();
    println!("exit clean");
}
