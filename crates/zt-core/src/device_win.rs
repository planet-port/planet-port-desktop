//! 设备层 Windows 实现:每网络一张 wintun L3 适配器(tun-rs) + 独立 reader
//! 线程 + 独立 ArpShim 用户态 ARP 垫层(见 arp.rs);managed 地址/路由差分
//! 应用;设备帧与核心的双向桥接。
//!
//! # 架构红线(与 device_unix 对齐,公共 API 两侧一致)
//!
//! - 方向红线(两条通道不可混):
//!   wintun 读出的裸 IP 包 -> ArpShim 补 14 字节以太头 -> `FrameIn` 通道 ->
//!   驱动线程 `processVirtualNetworkFrame`(设备→核心);
//!   核心产出的帧 -> sink `on_frame` -> `DevicePlane::send_frame`:学习
//!   (src MAC, src IP)、ARP request 命中本机 managed IP 由 ArpShim 应答并
//!   经 `FrameInTx` 回喂核心,IPv4 剥掉以太头裸写 wintun(核心→设备);
//! - 关停硬顺序:先 `teardown_all`(停 reader 线程 → 删路由 → 销毁适配器),
//!   再 `NodeHandle::shutdown` —— 设备线程必须先于驱动线程消失;
//! - reader 线程只做「收包 → 补以太头 → FrameInTx::send」,不回调核心;
//!   读错误退避重试,不因一次 IO 抖动永久退出;
//! - 回调 glue(驱动线程)对 `send_frame` 的调用发生在驱动线程内,不跨线程。
//!
//! wintun 是 L3 设备:send/recv 均为裸 IP 包,无以太网头,ARP 由用户态
//! ArpShim 垫层承接;适配器随设备对象存活,drop 即销毁。DLL 从 exe 同目录
//! 加载(tun-rs 默认行为,POC 已证)。reader 用 tun-rs Windows 专有的
//! `try_recv` 轮询——阻塞 recv 会让 stop 后 join 永挂(POC 收尾修复结论)。

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use tun_rs::SyncDevice;

use super::arp::{self, ArpShim};
use crate::config::{IpNet, NetConfig};
use crate::routes::RouteApplier;

/// reader 无包(WouldBlock)时的轮询间隔:阻塞 recv 会让 stop 后 join 永挂
const READER_POLL: Duration = Duration::from_millis(20);
/// reader 读错误退避重试间隔
const READER_BACKOFF: Duration = Duration::from_millis(200);
/// wintun 适配器名前缀(便于实测时 netsh 查找;多网络并存按 nwid 区分)
const ADAPTER_PREFIX: &str = "planet-port";

/// 设备读出的待回喂核心的帧(方向:设备→核心)。
#[derive(Debug, Clone)]
pub struct FrameIn {
    pub nwid: u64,
    pub src: u64,
    pub dst: u64,
    pub eth: u16,
    pub payload: Vec<u8>,
}

/// `FrameIn` 通道发送端:发出后写唤醒通道,让驱动线程的
/// recv_timeout 立即返回。
pub struct FrameInTx {
    tx: Sender<FrameIn>,
    wake: Sender<u8>,
}

impl Clone for FrameInTx {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), wake: self.wake.clone() }
    }
}

impl FrameInTx {
    pub(crate) fn new(tx: Sender<FrameIn>, wake: Sender<u8>) -> Self {
        Self { tx, wake }
    }

    /// 驱动线程已退出返回 false(发送失败被静默忽略,POC 无需处理)。
    pub fn send(&self, f: FrameIn) -> bool {
        if self.tx.send(f).is_err() {
            return false;
        }
        let _ = self.wake.send(1);
        true
    }
}

/// 设备线程取发送端的槽位:NodeHandle 在 start 后才能给出 FrameInTx,
/// 设备线程开跑可能更早,首帧前自旋等待一次即可。
pub type FrameInTxSlot = Arc<OnceLock<FrameInTx>>;

/// 垫层锁:reader 线程(出向补头)与驱动线程(send_frame 学习/应答)双侧
/// 共用;poison 不传播(锁内只做纯内存操作,持锁 panic 视为可恢复)。
fn lock_arp(a: &Mutex<ArpShim>) -> MutexGuard<'_, ArpShim> {
    a.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 单网络的 wintun L3 适配器 + ARP 垫层 + managed 地址/路由应用状态。
struct NetDevice {
    dev: Arc<SyncDevice>,
    if_name: String,
    if_index: u32,
    mtu: u16,
    /// 已处理的 managed 地址(含 Windows 侧跳过的 v6:标记为已处理避免每次
    /// 刷配置重复告警/重复尝试拆除)
    applied_addrs: Vec<IpNet>,
    routes: Option<RouteApplier>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    /// 设备→核心方向:成功投递 frame_in 的次数(reader 出向 IP 帧 + ARP 应答双侧自增)
    frames_in: Arc<AtomicU64>,
    /// 范围外帧丢弃数(只桥 IPv4/ARP;IPv6 RS/NS 等静默丢弃,仅计数留排查线索,
    /// reader 侧 wintun 产出的非 IPv4 包与 send_frame 侧核心来的其他 ethertype 共用)
    v6drop: Arc<AtomicU64>,
    /// ARP 垫层(reader 线程与驱动线程双侧共用)
    arp: Arc<Mutex<ArpShim>>,
    /// 回喂核心的发送端槽位(reader 出向帧与 ARP 应答共用)
    frame_tx: FrameInTxSlot,
}

/// 多网络设备表:每网络一张 wintun 适配器、各自 reader 线程、ArpShim 与
/// applied 状态。
///
/// 线程模型:`Arc<SyncDevice>` 由本对象与设备读线程共享(tun-rs 全部 `&self`
/// 方法且内部有 op_lock,Send+Sync);读线程 try_recv 轮询收包;
/// `apply/send_frame` 可从驱动线程调用;`apply` 需 `&mut`(上层套 Mutex)。
pub struct DevicePlane {
    devices: HashMap<u64, NetDevice>,
}

impl Default for DevicePlane {
    fn default() -> Self {
        Self::new()
    }
}

impl DevicePlane {
    pub fn new() -> Self {
        Self { devices: HashMap::new() }
    }

    /// 是否有任一网络设备在用。
    pub fn is_up(&self) -> bool {
        !self.devices.is_empty()
    }

    /// 当前全部网络的 wintun 适配器名快照。
    pub fn iface_names(&self) -> Vec<String> {
        self.devices.values().map(|d| d.if_name.clone()).collect()
    }

    /// 指定网络的适配器名(未建返回 None)。
    pub fn if_name_of(&self, nwid: u64) -> Option<&str> {
        self.devices.get(&nwid).map(|d| d.if_name.as_str())
    }

    /// 设备→核心方向已成功投递核心的帧数(全部网络合计;reader/应答双侧计数)。
    pub fn frames_in(&self) -> u64 {
        self.devices.values().map(|d| d.frames_in.load(Ordering::Relaxed)).sum()
    }

    /// 全部设备已应用的 IPv4(sink 的 local_ipv4s 用,跨网络聚合)。
    pub fn applied_v4s(&self) -> Vec<Ipv4Addr> {
        let mut out = Vec::new();
        for d in self.devices.values() {
            out.extend(d.applied_addrs.iter().filter_map(|n| match n.addr {
                IpAddr::V4(v) => Some(v),
                _ => None,
            }));
        }
        out
    }

    /// on_net_config(Up/ConfigUpdate)调用;设备按 `cfg.nwid` 找到/建设备,
    /// 地址/路由差分应用。(Down/Destroy 由调用方走 `teardown`。)
    pub fn apply(&mut self, cfg: &NetConfig, frame_tx: &FrameInTxSlot) -> io::Result<()> {
        if !self.devices.contains_key(&cfg.nwid) {
            let d = self.create_device(cfg, frame_tx)?;
            self.devices.insert(cfg.nwid, d);
        }
        let d = self.devices.get_mut(&cfg.nwid).expect("上方已插入");
        // 本网虚拟 MAC 必须等于核心分配值,垫层的 ARP 应答/学习语义才与核心一致
        lock_arp(&d.arp).local_mac = cfg.mac;
        // 只对 Ok 状态应用地址/路由(REQUESTING 等阶段 assigned/routes 无意义)
        if cfg.status == crate::config::NetStatus::Ok {
            d.apply_addrs(cfg)?;
            d.apply_routes(cfg);
        }
        Ok(())
    }

    fn create_device(&mut self, cfg: &NetConfig, frame_tx: &FrameInTxSlot) -> io::Result<NetDevice> {
        let mtu = cfg.mtu.max(1280);
        // wintun 同名适配器会复用 ring,多网络并存按 nwid 区分命名;需要管理员权限
        let want_name = format!("{ADAPTER_PREFIX}{:016x}", cfg.nwid);
        let dev = tun_rs::DeviceBuilder::new()
            .layer(tun_rs::Layer::L3)
            .name(&want_name)
            .mtu(mtu)
            .enable(true)
            .build_sync()?;
        let name = dev.name()?;
        let if_index = dev
            .if_index()
            .map_err(|e| io::Error::other(format!("获取 {name} if_index 失败: {e}")))?;
        let stop = Arc::new(AtomicBool::new(false));
        let frames_in = Arc::new(AtomicU64::new(0));
        let v6drop = Arc::new(AtomicU64::new(0));
        let dev = Arc::new(dev);
        let arp = Arc::new(Mutex::new(ArpShim::new(cfg.mac)));
        let reader = spawn_reader(
            Arc::clone(&dev),
            Arc::clone(&arp),
            Arc::clone(frame_tx),
            Arc::clone(&stop),
            cfg.nwid,
            Arc::clone(&frames_in),
            Arc::clone(&v6drop),
        );
        eprintln!("[device] {:016x} wintun L3 适配器已建立:{} mtu={}", cfg.nwid, name, mtu);
        Ok(NetDevice {
            dev,
            if_name: name,
            if_index,
            mtu,
            applied_addrs: Vec::new(),
            routes: None,
            stop,
            reader,
            frames_in,
            v6drop,
            arp,
            frame_tx: Arc::clone(frame_tx),
        })
    }

    /// 核心产出的帧入(方向:核心→设备)。设备未建或网络不符时静默丢弃。
    /// 学习 (src MAC, src IP) → ARP request 命中本机 IP 应答回喂核心 /
    /// IPv4 剥以太头裸写 wintun / 其余 ethertype 计 v6drop。
    pub fn send_frame(
        &self,
        nwid: u64,
        src: u64,
        _dst: u64,
        eth: u16,
        payload: &[u8],
    ) -> io::Result<()> {
        let Some(d) = self.devices.get(&nwid) else { return Ok(()) };
        if payload.len() > (d.mtu as usize) {
            return Ok(());
        }
        let (reply, local_mac) = {
            let mut shim = lock_arp(&d.arp);
            shim.learn(src, eth, payload);
            let r = if eth == arp::ether::ETHERTYPE_ARP { shim.answer(payload) } else { None };
            (r, shim.local_mac)
        };
        if let Some((req_mac, r)) = reply {
            // ARP 不进 L3 设备:以 (src=本网 MAC, dst=请求者, ARP) 回喂核心
            if let Some(tx) = d.frame_tx.get() {
                let f = FrameIn {
                    nwid,
                    src: local_mac,
                    dst: req_mac,
                    eth: arp::ether::ETHERTYPE_ARP,
                    payload: r,
                };
                if tx.send(f) {
                    d.frames_in.fetch_add(1, Ordering::Relaxed);
                }
            }
            return Ok(());
        }
        if eth == arp::ether::ETHERTYPE_IPV4 {
            d.dev.send(payload).map(|_| ())?;
        } else if eth != arp::ether::ETHERTYPE_ARP {
            // 范围外 ethertype:静默丢弃,仅计数留排查线索
            d.v6drop.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// 拆除单个网络的设备(停线程 → 删路由 → 销毁适配器)。网络不存在则无操作。
    /// 必须先于 NodeHandle::shutdown。
    pub fn teardown(&mut self, nwid: u64) {
        if let Some(mut d) = self.devices.remove(&nwid) {
            d.teardown();
        }
    }

    /// 拆除全部网络设备(Drop 兜底也走这里)。
    pub fn teardown_all(&mut self) {
        for (_, mut d) in self.devices.drain() {
            d.teardown();
        }
    }
}

impl NetDevice {
    fn apply_addrs(&mut self, cfg: &NetConfig) -> io::Result<()> {
        for a in &cfg.assigned {
            if self.applied_addrs.contains(a) {
                continue;
            }
            match a.addr {
                IpAddr::V4(v4) => self.dev.add_address_v4(v4, a.prefix)?,
                IpAddr::V6(_) => {
                    // Windows 设备面 v6 帧是双向黑洞(wintun L3 无 NDP 垫层,
                    // reader/send 侧对 v6 帧丢弃计 v6drop),v6 地址不应用,
                    // 否则系统会把 v6 流量路由进黑洞适配器。与 routes.rs 的
                    // 「不安装 v6 托管路由」是同一平台差异;待 NDP 垫层落地放开。
                    eprintln!(
                        "[device] {} 跳过 v6 地址 {}/{}(Windows L3 面无 v6 垫层,不应用)",
                        self.if_name, a.addr, a.prefix
                    );
                    continue;
                }
            }
            eprintln!("[device] {} + {}/{}", self.if_name, a.addr, a.prefix);
        }
        let removed: Vec<IpNet> = self
            .applied_addrs
            .iter()
            .filter(|a| !cfg.assigned.contains(a))
            .cloned()
            .collect();
        for a in &removed {
            let _ = self.dev.remove_address(a.addr);
            eprintln!("[device] {} - {}/{}", self.if_name, a.addr, a.prefix);
        }
        self.applied_addrs = cfg.assigned.clone();
        // 垫层同步:需要应答 ARP 的本机 managed IPv4
        let ips: Vec<Ipv4Addr> = self
            .applied_addrs
            .iter()
            .filter_map(|n| match n.addr {
                IpAddr::V4(v) => Some(v),
                _ => None,
            })
            .collect();
        lock_arp(&self.arp).set_ips(ips);
        Ok(())
    }

    fn apply_routes(&mut self, cfg: &NetConfig) {
        if self.routes.is_none() {
            match RouteApplier::new(self.if_index) {
                Ok(r) => self.routes = Some(r),
                Err(e) => {
                    eprintln!("[device] {} 路由应用器创建失败(忽略): {e}", self.if_name);
                    return;
                }
            }
        }
        if let Some(r) = self.routes.as_mut() {
            let _ = r.apply(&cfg.routes);
        }
    }

    /// 停线程 → 删路由 → 释放设备(适配器随最后一个 Arc 释放而销毁)。
    fn teardown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        if let Some(mut r) = self.routes.take() {
            r.teardown();
        }
        self.applied_addrs.clear();
        lock_arp(&self.arp).set_ips(Vec::new());
        let v6drop = self.v6drop.load(Ordering::Relaxed);
        if v6drop > 0 {
            eprintln!("[device] {} 已拆除,v6drop={v6drop}", self.if_name);
        }
        self.if_index = 0;
        self.if_name.clear();
        // self.dev 是最后一个引用时随本对象 drop 而销毁适配器
    }
}

impl Drop for DevicePlane {
    fn drop(&mut self) {
        // 兜底:调用方漏 teardown 时仍保证停线程、删路由、销毁适配器
        self.teardown_all();
    }
}

/// 设备读线程:wintun `try_recv` 20ms 轮询(Windows 专有,WouldBlock = 无包),
/// 裸 IP 包 → ArpShim 补 14 字节以太头定目的 MAC → 回喂核心。
///
/// 计数:成功投递出向 IP 帧 → `frames_in`;垫层拒收(非 IPv4,如 IPv6 RS/NS)
/// → `v6drop`。读错误退避 200ms 重试,不因一次 IO 抖动永久退出。
fn spawn_reader(
    dev: Arc<SyncDevice>,
    arp: Arc<Mutex<ArpShim>>,
    frame_tx: FrameInTxSlot,
    stop: Arc<AtomicBool>,
    nwid: u64,
    frames_in: Arc<AtomicU64>,
    v6drop: Arc<AtomicU64>,
) -> Option<JoinHandle<()>> {
    match std::thread::Builder::new()
        .name(format!("wintun-reader-{nwid:016x}"))
        .spawn(move || run_reader(dev, arp, frame_tx, stop, nwid, frames_in, v6drop))
    {
        Ok(h) => Some(h),
        // 线程起不来不再静默吞掉:设备将收不到入向帧,必须可见
        Err(e) => {
            eprintln!("[device] {nwid:016x} reader 线程启动失败(设备单向): {e}");
            None
        }
    }
}

fn run_reader(
    dev: Arc<SyncDevice>,
    arp: Arc<Mutex<ArpShim>>,
    frame_tx: FrameInTxSlot,
    stop: Arc<AtomicBool>,
    nwid: u64,
    frames_in: Arc<AtomicU64>,
    v6drop: Arc<AtomicU64>,
) {
    // 单个 IP 包上限 65535 字节(wintun 只交付 IP 包,与 mtu 配置无关)
    let mut buf = vec![0u8; 65535];
    eprintln!("[device-reader] {nwid:016x} 启动(try_recv {}ms 轮询)", READER_POLL.as_millis());
    while !stop.load(Ordering::SeqCst) {
        match dev.try_recv(&mut buf) {
            Ok(len) if len > 0 => {
                let out = lock_arp(&arp).wrap_outbound(&buf[..len]);
                match out {
                    Some((dst, src, eth, payload)) => {
                        let Some(tx) = frame_tx.get() else {
                            eprintln!("[device-reader] FrameInTx 未就绪,丢帧");
                            std::thread::sleep(READER_POLL);
                            continue;
                        };
                        let f = FrameIn { nwid, src, dst, eth, payload };
                        if tx.send(f) {
                            frames_in.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    None => {
                        v6drop.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(READER_POLL);
            }
            Err(e) => {
                eprintln!("[device-reader] try_recv 错误(退避 200ms 继续): {e}");
                std::thread::sleep(READER_BACKOFF);
            }
        }
    }
    eprintln!("[device-reader] {nwid:016x} 已停止");
}
