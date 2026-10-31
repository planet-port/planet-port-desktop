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
use std::sync::Arc;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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

/// 设备→核心帧通道的槽位:node start 后由主线程注入 frame_in_tx,
/// sink(驱动线程)与设备读线程共用(对齐 poc3 的 FrameInTxSlot)。
#[cfg(windows)]
type FrameTxSlot = Arc<std::sync::OnceLock<std::sync::mpsc::Sender<driver::FrameIn>>>;

/// 数据面平面:WinDevice + 路由应用器 + ARP 垫层 + 设备读线程,
/// 协作方式对齐 poc3 的 DevicePlane(sink 与主线程经 Arc<Mutex> 共享)。
#[cfg(windows)]
struct Plane {
    frame_tx: FrameTxSlot,
    /// ARP 垫层:sink(驱动线程)与设备读线程双侧共用
    shim: Arc<std::sync::Mutex<arp::ArpShim>>,
    dev: Option<Arc<device_win::WinDevice>>,
    ra: Option<routes_win::RouteApplier>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
    /// 已应用到设备的 managed IPv4(地址差分 + local_ipv4s)
    applied_v4s: Vec<std::net::Ipv4Addr>,
    /// 核心→设备已写出帧数(状态打印;sink 侧自增)
    frames_from_core: Arc<AtomicU64>,
    /// 设备→核心已投递帧数(状态打印;读线程出向 IP 帧 + ARP 应答双侧自增)
    frames_to_core: Arc<AtomicU64>,
    /// 范围外帧丢弃数(POC 只桥 IPv4/ARP;IPv6 RS/NS 等静默丢弃,仅计数,
    /// 双向共享:读线程侧 wintun 产出的非 IPv4 包 + sink 侧核心来的其他 ethertype)
    v6_drop: Arc<AtomicU64>,
}

#[cfg(windows)]
impl Plane {
    fn new(frame_tx: FrameTxSlot) -> Self {
        Self {
            frame_tx,
            shim: Arc::new(std::sync::Mutex::new(arp::ArpShim::new(0))),
            dev: None,
            ra: None,
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            reader: None,
            applied_v4s: Vec::new(),
            frames_from_core: Arc::new(AtomicU64::new(0)),
            frames_to_core: Arc::new(AtomicU64::new(0)),
            v6_drop: Arc::new(AtomicU64::new(0)),
        }
    }

    fn lock_shim(&self) -> std::sync::MutexGuard<'_, arp::ArpShim> {
        self.shim.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 建/取 wintun 设备并启动读线程(幂等;首次创建后复用)。
    fn ensure_dev(&mut self, cfg: &netcfg::NetConfig) -> Result<(), String> {
        if self.dev.is_some() {
            return Ok(());
        }
        // 读线程要把帧回喂核心,槽位未就绪时不建设备(下次 ConfigUpdate 重试)
        let Some(tx) = self.frame_tx.get() else {
            return Err("帧通道槽位未就绪,设备读线程无法回喂核心(跳过建设备)".into());
        };
        let tx = tx.clone();
        let mtu = cfg.mtu.max(1280);
        let dev = Arc::new(
            device_win::WinDevice::new("planet-port", mtu)
                .map_err(|e| format!("创建 wintun 失败: {e}"))?,
        );
        let nwid = cfg.nwid;
        let shim = Arc::clone(&self.shim);
        let stop = Arc::new(AtomicBool::new(false));
        let rd_stop = Arc::clone(&stop);
        let rd_dev = Arc::clone(&dev);
        let rd_to_core = Arc::clone(&self.frames_to_core);
        let rd_v6_drop = Arc::clone(&self.v6_drop);
        // try_recv 轮询读(tun-rs Windows 专有):置停后 join 立即返回,不挂收尾
        let reader = std::thread::Builder::new()
            .name("wintun-reader".into())
            .spawn(move || run_reader(rd_dev, shim, tx, nwid, rd_stop, rd_to_core, rd_v6_drop))
            .map_err(|e| format!("设备读线程启动失败: {e}"))?;
        self.dev = Some(dev);
        self.stop = stop;
        self.reader = Some(reader);
        Ok(())
    }

    /// netcfg Up/ConfigUpdate:设备 → 本网 MAC → 地址差分 → 路由差分 → ARP 垫层。
    fn apply(&mut self, cfg: &netcfg::NetConfig) {
        if let Err(e) = self.ensure_dev(cfg) {
            eprintln!("[plane] {e}");
            return;
        }
        self.lock_shim().local_mac = cfg.mac;
        if cfg.status != netcfg::NetStatus::Ok {
            return;
        }
        let dev = match &self.dev {
            Some(d) => d,
            None => return,
        };
        let want_v4: Vec<(std::net::Ipv4Addr, u8)> = cfg
            .assigned
            .iter()
            .filter_map(|a| match a.addr {
                std::net::IpAddr::V4(v) => Some((v, a.prefix)),
                _ => None,
            })
            .collect();
        // 地址差分:新增
        for (ip, pfx) in &want_v4 {
            if !self.applied_v4s.contains(ip) {
                match dev.add_address_v4(*ip, *pfx) {
                    Ok(()) => {
                        println!("[plane] 地址已应用 {ip}/{pfx} on {}", dev.name());
                        self.applied_v4s.push(*ip);
                    }
                    Err(e) => eprintln!("[plane] add {ip}/{pfx} 失败(忽略): {e}"),
                }
            }
        }
        // 地址差分:移除(netconf 撤销的)
        let stale: Vec<std::net::Ipv4Addr> = self
            .applied_v4s
            .iter()
            .filter(|ip| !want_v4.iter().any(|(w, _)| w == *ip))
            .copied()
            .collect();
        for ip in stale {
            match dev.remove_address(std::net::IpAddr::V4(ip)) {
                Ok(()) => self.applied_v4s.retain(|x| *x != ip),
                Err(e) => eprintln!("[plane] remove {ip} 失败(忽略): {e}"),
            }
        }
        // 路由差分(默认路由由 RouteApplier 跳过,绝不劫持)
        if self.ra.is_none() {
            match routes_win::RouteApplier::new(dev.if_index()) {
                Ok(r) => self.ra = Some(r),
                Err(e) => eprintln!("[plane] 路由应用器创建失败(忽略): {e}"),
            }
        }
        let specs: Vec<routes_win::RouteSpec> = cfg
            .routes
            .iter()
            .map(|r| routes_win::RouteSpec {
                target: r.target.addr,
                prefix: r.target.prefix,
                via: r.via,
            })
            .collect();
        if let Some(ra) = &mut self.ra {
            let n = ra.apply(&specs);
            if n > 0 {
                println!("[plane] managed 路由新装 {n} 条");
            }
        }
        // ARP 垫层:更新需要应答的 managed IPv4
        let ips = self.applied_v4s.clone();
        self.lock_shim().set_ips(ips);
    }

    /// 核心帧入:学习 (src MAC, src IP) → ARP request 应答回喂核心 / IPv4 裸包写设备。
    fn on_frame(&mut self, nwid: u64, src: u64, _dst: u64, eth: u16, payload: &[u8]) {
        let mut shim = self.lock_shim();
        shim.learn(src, eth, payload);
        let reply =
            if eth == arp::ether::ETHERTYPE_ARP { shim.answer(payload) } else { None };
        let local_mac = shim.local_mac;
        drop(shim);
        if let Some((req_mac, r)) = reply {
            println!("[plane] ARP request 命中本机 IP → reply → {req_mac:012x}");
            if let Some(tx) = self.frame_tx.get() {
                let _ = tx.send(driver::FrameIn {
                    nwid,
                    src: local_mac,
                    dst: req_mac,
                    eth: arp::ether::ETHERTYPE_ARP,
                    payload: r,
                });
                self.frames_to_core.fetch_add(1, Ordering::Relaxed);
            }
            return; // ARP 不进 L3 设备
        }
        if eth == arp::ether::ETHERTYPE_IPV4 {
            if let Some(dev) = &self.dev {
                match dev.send(payload) {
                    Ok(_) => {
                        self.frames_from_core.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => eprintln!("[plane] 写设备失败: {e}"),
                }
            }
        } else if eth != arp::ether::ETHERTYPE_ARP {
            // 范围外 ethertype(POC 只桥 IPv4/ARP):静默丢弃,仅计数留排查线索
            self.v6_drop.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 关停(红线顺序):设备读线程 stop → 路由 teardown → drop 设备。
    /// Down/Destroy 与主循环退出共用;drop WinDevice 即销毁适配器。
    fn teardown(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        if let Some(ra) = &mut self.ra {
            ra.teardown();
            self.ra = None;
        }
        self.dev = None;
        self.applied_v4s.clear();
        self.lock_shim().set_ips(Vec::new());
    }
}

/// 控制面入口:join planet → netcfg 驱动设备应用(数据面)→ 每 2s 打印状态
/// + 每 5s ping target → 到点按红线顺序关停。
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
    let target_ip: Option<std::net::Ipv4Addr> =
        target.as_deref().and_then(|t| t.parse().ok());
    if target.is_some() && target_ip.is_none() {
        eprintln!("[ctl] 非法 --target(需 IPv4): {:?}", target);
        return 2;
    }
    let (v, m, r) = driver::PocNode::core_version();
    println!(
        "[ctl] zt-core {v}.{m}.{r} planet={} nwid={nwid:016x} dir={dir} port={port} duration={duration}s target={}",
        planet,
        target_ip.map(|t| t.to_string()).unwrap_or_else(|| "(未设)".into())
    );
    let slot: FrameTxSlot = Arc::new(std::sync::OnceLock::new());
    let plane = Arc::new(std::sync::Mutex::new(Plane::new(Arc::clone(&slot))));
    let node = match driver::PocNode::start(
        dir.into(),
        planet.into(),
        port,
        PlaneSink { plane: Arc::clone(&plane) },
    ) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[ctl] start 失败: {e}");
            return 1;
        }
    };
    println!("[ctl] address={:010x}", node.address());
    // 注入帧通道(须在 join 前完成:netcfg 到达即可能建设备 + 启动读线程)
    if slot.set(node.frame_in_tx()).is_err() {
        eprintln!("[ctl] FrameInTx 槽位冲突");
        {
            let mut p = plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            p.teardown();
        }
        node.shutdown();
        return 1;
    }
    if let Err(e) = node.join(nwid) {
        eprintln!("[ctl] join 失败: {e}");
        {
            let mut p = plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            p.teardown();
        }
        node.shutdown();
        return 1;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(duration);
    let mut last_ping = std::time::Instant::now() - std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if std::time::Instant::now() >= deadline {
            break;
        }
        print_status(&node, nwid, &plane);
        // 每 5s 对网内目标单发 ping(Windows ping 参数)
        if let Some(t) = target_ip {
            if last_ping.elapsed() >= std::time::Duration::from_secs(5) {
                last_ping = std::time::Instant::now();
                ping_once(t);
            }
        }
    }
    // 关停红线:设备读线程 stop → 路由 teardown → drop 设备 → PocNode::shutdown
    {
        let mut p = plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        p.teardown();
    }
    node.shutdown();
    println!("[ctl] 已关停");
    0
}

/// 设备读线程(wintun,Windows 专有 try_recv 轮询):裸 IP 包 → ARP 垫层补
/// 以太网头定目的 MAC → 核心帧通道(驱动线程转 processVirtualNetworkFrame)。
/// 计数:成功投递出向 IP 帧 → `to_core`;垫层拒收(非 IPv4,如 IPv6 RS/NS)→ `v6_drop`。
#[cfg(windows)]
fn run_reader(
    dev: Arc<device_win::WinDevice>,
    shim: Arc<std::sync::Mutex<arp::ArpShim>>,
    tx: std::sync::mpsc::Sender<driver::FrameIn>,
    nwid: u64,
    stop: Arc<AtomicBool>,
    to_core: Arc<AtomicU64>,
    v6_drop: Arc<AtomicU64>,
) {
    let mut buf = vec![0u8; 65535];
    while !stop.load(Ordering::SeqCst) {
        match dev.try_recv(&mut buf) {
            Ok(len) if len > 0 => {
                let out = shim
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .wrap_outbound(&buf[..len]);
                match out {
                    Some((dst, src, eth, payload)) => {
                        let _ = tx.send(driver::FrameIn { nwid, src, dst, eth, payload });
                        to_core.fetch_add(1, Ordering::Relaxed);
                    }
                    None => {
                        v6_drop.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => {
                eprintln!("[reader] recv 错误(退避 200ms 继续): {e}");
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }
}

/// Windows ping 单发(-n 1 -w 2000);返回是否通。
/// 中文控制台输出为 GBK,只截取含 TTL= 的行(中英文 Windows 均含该字样)。
#[cfg(windows)]
fn ping_once(target: std::net::Ipv4Addr) -> bool {
    match std::process::Command::new("ping")
        .args(["-n", "1", "-w", "2000"])
        .arg(target.to_string())
        .output()
    {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let line = s.lines().find(|l| l.contains("TTL=")).unwrap_or("");
            println!("[ping] {target} 通:{line}");
            true
        }
        Ok(_) => {
            println!("[ping] {target} 不通");
            false
        }
        Err(e) => {
            println!("[ping] {target} 执行失败: {e}");
            false
        }
    }
}

/// 状态打印(对齐 poc3 print_status 风格):节点级一行 + 目标网络一行 + 数据面一行。
#[cfg(windows)]
fn print_status(
    node: &driver::PocNode,
    nwid: u64,
    plane: &Arc<std::sync::Mutex<Plane>>,
) {
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
    let p = plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    println!(
        "  plane dev={} 核心→设备={} 设备→核心={} v6drop={}",
        p.dev.as_ref().map(|d| d.name().to_string()).unwrap_or_else(|| "(未建)".into()),
        p.frames_from_core.load(Ordering::Relaxed),
        p.frames_to_core.load(Ordering::Relaxed),
        p.v6_drop.load(Ordering::Relaxed)
    );
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

/// 控制面 sink:netcfg 驱动设备应用(Up/Update 应用,Down/Destroy 拆机),
/// 帧双向桥接(学习 + ARP 应答 + IPv4 写设备),local_ipv4s 供组播订阅。
#[cfg(windows)]
struct PlaneSink {
    plane: Arc<std::sync::Mutex<Plane>>,
}

#[cfg(windows)]
impl driver::PocSink for PlaneSink {
    fn on_event(&mut self, ev: &str) {
        eprintln!("[event] {ev}");
    }

    fn on_net_config(&mut self, op: driver::NetConfigOp, cfg: netcfg::NetConfig) {
        let assigned: Vec<String> =
            cfg.assigned.iter().map(|a| format!("{}/{}", a.addr, a.prefix)).collect();
        let routes: Vec<String> =
            cfg.routes.iter().map(|r| format!("{}/{}", r.target.addr, r.target.prefix)).collect();
        eprintln!(
            "[netcfg] {:016x} {:?} {} rev={} name={:?} assigned=[{}] routes=[{}]",
            cfg.nwid,
            op,
            status_name(cfg.status),
            cfg.netconf_revision,
            cfg.name,
            assigned.join(", "),
            routes.join(", ")
        );
        let mut p = self.plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match op {
            driver::NetConfigOp::Up | driver::NetConfigOp::ConfigUpdate => p.apply(&cfg),
            driver::NetConfigOp::Down | driver::NetConfigOp::Destroy => p.teardown(),
        }
    }

    fn on_frame(&mut self, nwid: u64, src: u64, dst: u64, eth: u16, payload: &[u8]) {
        let mut p = self.plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        p.on_frame(nwid, src, dst, eth, payload);
    }

    fn local_ipv4s(&self) -> Vec<std::net::Ipv4Addr> {
        // POC 简化:返回设备已应用的 managed IPv4(对齐 poc3 applied_v4s 思路);
        // 物理口枚举(GetAdaptersAddresses)留正式实现,组播订阅覆盖面收窄
        // 不影响互通验证。此回调每 5s 发生在驱动线程,不得做慢操作。
        let p = self.plane.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        p.applied_v4s.clone()
    }
}
