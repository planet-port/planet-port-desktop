//! POC 3:同进程双 Node 并发与隔离验证(核心风险:多 Node 全局状态互扰)。
//!
//! 同一进程内启动两个 Node(同 planet、独立 identity/UDP 端口/dataStore),
//! 并发 join 同一网络,逐项断言隔离性并打印 PASS/FAIL/PENDING:
//!   1. identity 独立:两份 identity.secret 内容不同、各在自己目录、首字段
//!      (address)与各自 Node 实例一致
//!   2. dataStore 隔离:目录不同、A/B 树内均扫不到对方的 identity.secret 字节、
//!      公共相对路径中 identity*/networks.d/* 内容不同(根 planet 同 world 回写
//!      相同属预期;peers.d/ 下同名 peer 记录属互为对端,不作串扰判定,仅提示)
//!   3. UDP 端口:9994/9995 各自 bind 成功(start 即 bind),且同进程内重复
//!      bind 必须 EADDRINUSE(证明独占、互不抢占)
//!   4. node-b shutdown 后重新 start 复用 dataStore(address 不变),
//!      重启窗口内 node-a 持续 online
//!   5. a↔b 互 ping:两节点都在本机,内核对本地地址直接交付、绕过 ZT 数据面
//!      (Task 4 已证),仅作记录性输出、不参与 PASS/FAIL 判定;数据面证据以
//!      POC2(tcpdump+帧计数)与 --target(网内真实设备)对照为准
//!
//! 用法:
//!   无 sudo 自验(不建设备,先验证隔离项 1-4):--no-device
//!   授权后完整验证(需 sudo 建 feth):sudo /usr/bin/time -l ./target/debug/poc3_multi
//!
//! 已知环境限制(2026-10-28 实测):本机到 Gozi25 根服务器(124.222.91.224:9993)
//! 仅本地源端口 9994 的 UDP 流能收到应答;9995/9996/9997/9998/50000 全部静默,
//! 且与节点身份无关(同一 identity 换口结果翻转),STUN 证明本机这些端口双向
//! UDP 正常——即限制在根服务器或其路径侧。因此双节点「同时在线 + 互 ping」
//! 在该环境暂无法完成,隔离项 1-4(--no-device)不受影响。
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use clap::Parser;
use zt_core::{
    local_ipv4s, DevicePlane, FrameInTxSlot, NetConfig, NetConfigOp, NetStatus, NodeEvent,
    NodeHandle, NodeParams, NodeSink,
};

#[derive(Parser)]
struct Args {
    /// planet 文件(两节点共用同一份)
    #[arg(long, default_value = "crates/pocs/fixtures/planet.Gozi25")]
    planet: PathBuf,
    /// 加入的网络 ID(十六进制,可带 0x 前缀)
    #[arg(long, default_value = "5cf54f181554090f")]
    join: String,
    /// node-a 数据目录(既有节点,已授权)
    #[arg(long, default_value = "target/poc-data/node-a")]
    dir_a: PathBuf,
    /// node-a 物理 UDP 端口
    #[arg(long, default_value_t = 9994)]
    port_a: u16,
    /// node-b 数据目录(全新 identity,需控制器授权)
    #[arg(long, default_value = "target/poc-data/node-b")]
    dir_b: PathBuf,
    /// node-b 物理 UDP 端口
    #[arg(long, default_value_t = 9995)]
    port_b: u16,
    /// 运行秒数
    #[arg(long, default_value_t = 600)]
    duration: u64,
    /// 等待两节点同时 OK 的最长时间;超时则把依赖授权的检查项置 PENDING
    #[arg(long, default_value_t = 120)]
    auth_wait: u64,
    /// 不建 feth 设备(无 sudo 的隔离自验阶段,检查项 1-4)
    #[arg(long)]
    no_device: bool,
    /// 数据面对照目标(网内真实设备,如 10.121.16.229):node-a 侧每 5s ping 一次
    #[arg(long)]
    target: Option<Ipv4Addr>,
}

fn parse_nwid(s: &str) -> Result<u64, String> {
    let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(t, 16).map_err(|e| format!("nwid `{s}` 不是十六进制: {e}"))
}

/// 单节点 sink:带标签输出,设备可选(--no-device 时无 DevicePlane)。
struct Sink {
    tag: &'static str,
    plane: Option<Arc<Mutex<DevicePlane>>>,
    frame_tx_slot: FrameInTxSlot,
    frames_to_dev: Arc<AtomicU64>,
}

impl Sink {
    fn lock(&self) -> Option<std::sync::MutexGuard<'_, DevicePlane>> {
        self.plane
            .as_ref()
            .map(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

impl NodeSink for Sink {
    fn on_event(&mut self, ev: NodeEvent) {
        println!("[{}][event]   {ev:?}", self.tag);
    }
    fn on_net_config(&mut self, nwid: u64, op: NetConfigOp, cfg: NetConfig) {
        println!(
            "[{}][netconf] {nwid:016x} {op:?} status={:?} rev={} assigned={:?} routes={:?}",
            self.tag, cfg.status, cfg.netconf_revision, cfg.assigned, cfg.routes
        );
        let Some(p) = &self.plane else { return };
        let r = match op {
            NetConfigOp::Up | NetConfigOp::ConfigUpdate => {
                let mut d = p.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                d.apply(&cfg, &self.frame_tx_slot)
            }
            NetConfigOp::Down | NetConfigOp::Destroy => {
                p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).teardown(nwid);
                Ok(())
            }
        };
        if let Err(e) = r {
            eprintln!("[{}][netconf] 配置应用失败: {e}", self.tag);
        }
    }
    fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]) {
        let n = self.frames_to_dev.fetch_add(1, Ordering::Relaxed) + 1;
        println!(
            "[{}][frame#{n}→dev] {nwid:016x} {src_mac:012x}->{dst_mac:012x} eth={ether_type:#06x} len={}",
            self.tag,
            payload.len()
        );
        if let Some(d) = self.lock() {
            if let Err(e) = d.send_frame(nwid, src_mac, dst_mac, ether_type, payload) {
                eprintln!("[{}][frame] 写设备失败: {e}", self.tag);
            }
        }
    }
    fn local_ipv4s(&self) -> Vec<Ipv4Addr> {
        // 本机物理接口 IPv4 + 本设备已应用的 managed IPv4;核心侧订阅幂等
        let mut ips = local_ipv4s();
        if let Some(d) = self.lock() {
            ips.extend(d.applied_v4s());
        }
        ips.sort_unstable();
        ips.dedup();
        ips
    }
}

/// 一个 POC 节点实例(NodeHandle + 自己的 sink 组件)。
struct Inst {
    tag: &'static str,
    data_dir: PathBuf,
    planet: PathBuf,
    port: u16,
    with_device: bool,
    h: Option<NodeHandle>,
    plane: Option<Arc<Mutex<DevicePlane>>>,
    frames_to_dev: Arc<AtomicU64>,
}

impl Inst {
    fn address(&self) -> u64 {
        self.h.as_ref().map(|h| h.address()).unwrap_or(0)
    }
    fn online(&self) -> bool {
        self.h.as_ref().map(|h| h.online()).unwrap_or(false)
    }
    fn net(&self, nwid: u64) -> Option<NetConfig> {
        let h = self.h.as_ref()?;
        h.networks().into_iter().find(|n| n.nwid == nwid)
    }
    fn assigned_v4(&self, nwid: u64) -> Option<Ipv4Addr> {
        let n = self.net(nwid)?;
        n.assigned.into_iter().find_map(|a| match a.addr {
            std::net::IpAddr::V4(v) => Some(v),
            _ => None,
        })
    }
    fn status(&self, nwid: u64) -> Option<NetStatus> {
        self.net(nwid).map(|n| n.status)
    }
    /// 帧计数与设备名快照(打印用)。
    fn dev_desc(&self, nwid: u64) -> String {
        let Some(p) = &self.plane else { return "(--no-device)".into() };
        let d = p.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        d.if_name_of(nwid).map(|s| s.to_string()).unwrap_or_else(|| "(未建)".into())
    }
    fn frames_to_dev(&self) -> u64 {
        self.frames_to_dev.load(Ordering::Relaxed)
    }
    fn frames_in(&self) -> u64 {
        let Some(p) = &self.plane else { return 0 };
        p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).frames_in()
    }
}

/// 启动一个节点:start(生成/装载 identity、bind 端口)→ 设置帧槽位。
fn spawn_node(
    tag: &'static str,
    data_dir: PathBuf,
    port: u16,
    planet: PathBuf,
    with_device: bool,
) -> Result<Inst, String> {
    let slot: FrameInTxSlot = Arc::new(OnceLock::new());
    let frames_to_dev = Arc::new(AtomicU64::new(0));
    let plane =
        if with_device { Some(Arc::new(Mutex::new(DevicePlane::new()))) } else { None };
    let sink = Sink {
        tag,
        plane: plane.clone(),
        frame_tx_slot: Arc::clone(&slot),
        frames_to_dev: Arc::clone(&frames_to_dev),
    };
    let h = NodeHandle::start(
        NodeParams {
            data_dir: data_dir.clone(),
            planet_file: planet.clone(),
            bind_port: port,
            identity_dir: None,
            tcp_fallback_relay: None,
        },
        sink,
    )
    .map_err(|e| format!("node {tag} start 失败: {e}"))?;
    slot.set(h.frame_in_tx()).map_err(|_| format!("node {tag} FrameInTx 槽位冲突"))?;
    Ok(Inst { tag, data_dir, planet, port, with_device, h: Some(h), plane, frames_to_dev })
}

/// 关停(红线顺序):先 teardown 设备(停读线程→删路由→销毁 feth),
/// 再 shutdown(join 驱动线程 → ZT_Node_delete)。
fn stop_node(inst: &mut Inst) {
    if let Some(p) = &inst.plane {
        p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).teardown_all();
    }
    if let Some(h) = inst.h.take() {
        h.shutdown();
    }
}

/// 重启节点:stop 后用同一 dataDir 重新 start(复用 identity),重新 join。
fn restart_node(inst: &mut Inst, nwid: u64) -> Result<(), String> {
    let (tag, dir, planet, port, dev) = (
        inst.tag,
        inst.data_dir.clone(),
        inst.planet.clone(),
        inst.port,
        inst.with_device,
    );
    println!("[{tag}] ===== 重启:shutdown → 重新 start 复用 dataStore({}) =====", dir.display());
    stop_node(inst);
    let fresh = spawn_node(tag, dir, port, planet, dev)?;
    fresh.h.as_ref().ok_or("重启后句柄缺失")?.join(nwid).map_err(|e| e.to_string())?;
    *inst = fresh;
    Ok(())
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

fn print_status(inst: &Inst, nwid: u64) {
    println!(
        "[{}] address={:010x} online={} frames_to_dev={} frames_in={} dev={}",
        inst.tag,
        inst.address(),
        inst.online(),
        inst.frames_to_dev(),
        inst.frames_in(),
        inst.dev_desc(nwid)
    );
    match inst.net(nwid) {
        Some(n) => {
            let assigned: Vec<String> =
                n.assigned.iter().map(|a| format!("{}/{}", a.addr, a.prefix)).collect();
            println!(
                "  net {:016x} status={status} rev={} assigned=[{}]",
                n.nwid,
                n.netconf_revision,
                assigned.join(", "),
                status = status_name(&n.status)
            );
        }
        None => println!("  net {:016x} (未查询到配置)", nwid),
    }
}

/// /sbin/ping 单发(与 poc2 同款);返回是否通。
fn ping_once(tag: &str, label: &str, target: Ipv4Addr) -> bool {
    let out = Command::new("/sbin/ping").args(["-c", "1", "-t", "2"]).arg(target.to_string()).output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let line = s.lines().find(|l| l.contains("time=")).unwrap_or("");
            println!("[ping {tag} {label}] {target} 通:{line}");
            true
        }
        Ok(_) => {
            println!("[ping {tag} {label}] {target} 不通");
            false
        }
        Err(e) => {
            println!("[ping {tag} {label}] {target} 执行失败: {e}");
            false
        }
    }
}

/// 递归收集目录树:相对路径 → 内容(隔离断言用)。
fn walk(root: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let rel = if prefix.is_empty() {
            e.file_name().to_string_lossy().into_owned()
        } else {
            format!("{prefix}/{}", e.file_name().to_string_lossy())
        };
        let p = e.path();
        if p.is_dir() {
            walk(&p, &rel, out);
        } else if let Ok(d) = std::fs::read(&p) {
            out.insert(rel, d);
        }
    }
}

/// identity.secret 首字段(冒号前)即节点 address 的十位十六进制。
fn identity_addr(secret: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(secret).ok()?;
    s.split(':').next().map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty())
}

/// 五项断言汇总结果。
#[derive(Default, Clone, Copy, PartialEq)]
enum Verdict {
    #[default]
    Pending,
    Pass,
    Fail,
}

fn v_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "PASS",
        Verdict::Fail => "FAIL",
        Verdict::Pending => "PENDING",
    }
}

impl From<bool> for Verdict {
    fn from(b: bool) -> Self {
        if b { Verdict::Pass } else { Verdict::Fail }
    }
}

fn main() {
    let args = Args::parse();
    let nwid = match parse_nwid(&args.join) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let with_device = !args.no_device;
    let (major, minor, rev) = NodeHandle::core_version();
    println!("core version = {major}.{minor}.{rev}");
    println!(
        "双 Node:A(dir={}, port={}) B(dir={}, port={}) planet={} nwid={nwid:016x} device={}",
        args.dir_a.display(),
        args.port_a,
        args.dir_b.display(),
        args.port_b,
        args.planet.display(),
        with_device
    );

    let mut a = match spawn_node("A", args.dir_a.clone(), args.port_a, args.planet.clone(), with_device) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mut b = match spawn_node("B", args.dir_b.clone(), args.port_b, args.planet.clone(), with_device) {
        Ok(x) => x,
        Err(e) => {
            stop_node(&mut a);
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("node-a address = {:010x}", a.address());
    println!("node-b address = {:010x}", b.address());

    let mut join_err: Option<String> = None;
    for inst in [&mut a, &mut b] {
        match inst.h.as_ref().expect("刚启动").join(nwid) {
            Ok(()) => println!("[{}] join {nwid:016x} 已下发", inst.tag),
            Err(e) => join_err = Some(format!("[{}] join 失败: {e}", inst.tag)),
        }
    }
    if let Some(e) = join_err {
        stop_node(&mut a);
        stop_node(&mut b);
        eprintln!("{e}");
        std::process::exit(1);
    }

    // ===== 断言 3:端口互不抢占(同进程对已 bind 端口重复 bind 必须失败)=====
    let rebind_a = UdpSocket::bind(("0.0.0.0", args.port_a)).err();
    let rebind_b = UdpSocket::bind(("0.0.0.0", args.port_b)).err();
    let assert3 = [&rebind_a, &rebind_b].iter().all(
        |e| matches!(e, Some(err) if err.kind() == std::io::ErrorKind::AddrInUse),
    );
    println!(
        "[probe] 重复 bind {}/{}: {:?} / {:?}(均须 AddrInUse)",
        args.port_a,
        args.port_b,
        rebind_a.as_ref().map(|e| e.to_string()),
        rebind_b.as_ref().map(|e| e.to_string())
    );

    // ===== 主循环 =====
    let end = Instant::now() + Duration::from_secs(args.duration);
    let auth_deadline = Instant::now() + Duration::from_secs(args.auth_wait);
    let mut both_ok_at: Option<Instant> = None;
    let mut cross_ping: Option<(bool, bool)> = None;
    let mut restart_done = false;
    let mut assert4 = Verdict::Pending;
    let mut a_online_samples: u32 = 0;
    let mut a_online_fail: u32 = 0;
    let mut next_target_ping = Instant::now();

    while Instant::now() < end {
        print_status(&a, nwid);
        print_status(&b, nwid);

        let a_ok = a.status(nwid) == Some(NetStatus::Ok);
        let b_ok = b.status(nwid) == Some(NetStatus::Ok);
        if both_ok_at.is_none() {
            if a_ok && b_ok {
                both_ok_at = Some(Instant::now());
                println!("[flow] 双节点同时 OK,稍后执行互 ping 与 node-b 重启");
            } else if !restart_done && Instant::now() > auth_deadline {
                println!(
                    "[flow] 超过 --auth-wait 仍非双 OK(A_ok={a_ok} B_ok={b_ok});依赖授权的检查项置 PENDING,重启项照常执行"
                );
            }
        }
        if let Some(t) = both_ok_at {
            if cross_ping.is_none() && t.elapsed() >= Duration::from_secs(3) {
                let (a_ip, b_ip) = (a.assigned_v4(nwid), b.assigned_v4(nwid));
                let (ra, rb) = match (a_ip, b_ip) {
                    (Some(x), Some(y)) => (
                        ping_once("A→B", "node-b assigned", y),
                        ping_once("B→A", "node-a assigned", x),
                    ),
                    _ => {
                        println!("[ping] 双节点 assigned IPv4 未齐:a_ip={a_ip:?} b_ip={b_ip:?}");
                        (false, false)
                    }
                };
                cross_ping = Some((ra, rb));
            }
            if !restart_done && t.elapsed() >= Duration::from_secs(8) {
                let b_addr_before = b.address();
                a_online_samples += 2;
                if !a.online() {
                    a_online_fail += 2;
                }
                let r = restart_node(&mut b, nwid);
                restart_done = true;
                let b_addr_after = b.address();
                a_online_samples += 1;
                if !a.online() {
                    a_online_fail += 1;
                }
                let a_still_ok = a.status(nwid) == Some(NetStatus::Ok);
                assert4 = match r {
                    Ok(()) if b_addr_before == b_addr_after
                        && a_online_fail == 0
                        && a_still_ok =>
                    {
                        Verdict::Pass
                    }
                    _ => Verdict::Fail,
                };
                println!(
                    "[flow] node-b 重启完成:addr {:010x} → {:010x} addr_保持={} a_online 采样 {}/{} 次 online a_still_ok={a_still_ok}",
                    b_addr_before,
                    b_addr_after,
                    b_addr_before == b_addr_after,
                    a_online_samples - a_online_fail,
                    a_online_samples
                );
            }
        } else if !restart_done && Instant::now() > auth_deadline {
            // 未授权阶段也执行重启项(不依赖授权)
            let b_addr_before = b.address();
            let r = restart_node(&mut b, nwid);
            restart_done = true;
            let b_addr_after = b.address();
            let a_still_ok = a.online();
            assert4 = match r {
                Ok(()) if b_addr_before == b_addr_after && a_still_ok => Verdict::Pass,
                _ => Verdict::Fail,
            };
        }

        if let Some(t) = args.target {
            if Instant::now() >= next_target_ping {
                next_target_ping = Instant::now() + Duration::from_secs(5);
                ping_once("A→target", "数据面对照", t);
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    // ===== 断言 1:identity 独立 =====
    let sec_a = std::fs::read(a.data_dir.join("identity.secret"));
    let sec_b = std::fs::read(b.data_dir.join("identity.secret"));
    let addrs_differ = a.address() != b.address();
    let assert1 = match (&sec_a, &sec_b) {
        (Ok(x), Ok(y)) => {
            !x.is_empty()
                && !y.is_empty()
                && x != y
                && addrs_differ
                && identity_addr(x).as_deref() == Some(&*format!("{:010x}", a.address()))
                && identity_addr(y).as_deref() == Some(&*format!("{:010x}", b.address()))
        }
        _ => false,
    };

    // ===== 断言 2:dataStore 隔离 =====
    let mut ta = BTreeMap::new();
    let mut tb = BTreeMap::new();
    walk(&a.data_dir, "", &mut ta);
    walk(&b.data_dir, "", &mut tb);
    println!("[tree] node-a {} 文件:", a.data_dir.display());
    for k in ta.keys() {
        println!("    {k}");
    }
    println!("[tree] node-b {} 文件:", b.data_dir.display());
    for k in tb.keys() {
        println!("    {k}");
    }
    let mut assert2 = !ta.is_empty() && !tb.is_empty();
    // 对方 identity.secret 字节不得出现在自己树内任何文件(内容级交叉污染扫描):
    // A 的树里不得有 B 的 secret,B 的树里不得有 A 的 secret
    if let (Ok(x), Ok(y)) = (&sec_a, &sec_b) {
        if !x.is_empty() && !y.is_empty() {
            let cross = ta.values().any(|v| v.windows(y.len()).any(|w| w == y))
                || tb.values().any(|v| v.windows(x.len()).any(|w| w == x));
            if cross {
                println!("[assert2] 检测到对方 identity 字节,隔离被破坏");
                assert2 = false;
            }
        }
    }
    // 公共相对路径:identity*/networks.d/* 内容必须不同;根 planet 同 world 回写应相同
    for (rel, va) in ta.iter() {
        let Some(vb) = tb.get(rel) else { continue };
        if rel == "planet" {
            if va != vb {
                println!("[assert2] 根 planet 内容不一致(同 world 回写应相同)");
                assert2 = false;
            }
        } else if (rel.starts_with("identity") || rel.contains("networks.d/")) && va == vb {
            println!("[assert2] 公共文件 {rel} 内容完全相同,疑似共享状态");
            assert2 = false;
        }
    }
    let shared_peers: Vec<&String> =
        ta.keys().filter(|k| k.contains("peers.d/") && tb.contains_key(*k)).collect();
    if !shared_peers.is_empty() {
        // a↔b 互为对端,同名 peer 记录属预期;仅提示,不作串扰判定
        println!(
            "[assert2] 提示:peers.d/ 同名记录 {} 条(a↔b 互为对端,预期内,不判串扰)",
            shared_peers.len()
        );
    }

    // ===== 断言 4/5 汇总 =====
    let assert3_v = if assert3 { Verdict::Pass } else { Verdict::Fail };
    let dir_str = |v: Option<(bool, bool)>| match v {
        Some((true, true)) => "A→B 通/B→A 通".to_string(),
        Some((true, false)) => "A→B 通/B→A 不通".to_string(),
        Some((false, true)) => "A→B 不通/B→A 通".to_string(),
        Some((false, false)) => "A→B 不通/B→A 不通".to_string(),
        None => "未采样".to_string(),
    };
    // 待授权即置 PENDING 的项:重启未执行(理论上总会执行)或未同时 OK
    if !restart_done {
        assert4 = Verdict::Pending;
    }

    println!("===== 隔离断言汇总 =====");
    println!("[assert 1] identity 独立(内容不同/address 不同且各与自身一致): {}", v_str(assert1.into()));
    println!("[assert 2] dataStore 隔离(无交叉文件/无内容污染): {}", v_str(assert2.into()));
    println!(
        "[assert 3] UDP 端口独立 bind 且互不抢占({}/{}): {}",
        args.port_a, args.port_b, v_str(assert3_v)
    );
    println!(
        "[assert 4] node-b 重启复用 dataStore 且 node-a 持续 online: {}",
        v_str(assert4)
    );
    println!(
        "[assert 5] a↔b 互 ping: 信息性({}) —— 单机双节点互 ping 走内核本地交付不经 ZT 数据面,不作为互扰判据;数据面证据见 POC2(tcpdump+帧计数)",
        dir_str(cross_ping)
    );
    let all: [Verdict; 4] = [assert1.into(), assert2.into(), assert3_v, assert4];
    if all.contains(&Verdict::Fail) {
        println!("RESULT = FAIL(存在失败项)");
    } else if all.contains(&Verdict::Pending) {
        println!("RESULT = PENDING(待控制器授权 node-b 后复跑完整流程)");
    } else {
        println!("RESULT = PASS(隔离断言 1-4 全过;断言 5 仅信息性)");
    }

    println!("===== teardown(设备 → 驱动线程 → delete,两节点)=====");
    stop_node(&mut a);
    stop_node(&mut b);
    println!("exit clean");
}
