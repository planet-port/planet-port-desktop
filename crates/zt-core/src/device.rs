//! 设备层:每网络一张 feth L2 设备对(tun-rs),内核处理 ARP;
//! managed 地址/路由差分应用;设备帧与核心的双向桥接。
//!
//! # 架构红线
//!
//! - 方向红线(两条通道不可混):
//!   设备读出的帧 -> `FrameIn` 通道 -> 驱动线程 `processVirtualNetworkFrame`(设备→核心);
//!   核心产出的帧 -> sink `on_frame` -> `DevicePlane::send_frame` 写设备(核心→设备);
//! - 关停硬顺序:先 `teardown_all`(停 reader 线程 → 删路由 → 销毁 feth),
//!   再 `NodeHandle::shutdown` —— 设备线程必须先于驱动线程消失;
//! - reader 线程只做「收帧 → FrameInTx::send」,不回调核心;读错误退避重试,
//!   不因一次 IO 抖动永久退出;
//! - 回调 glue(驱动线程)对 `send_frame` 的调用发生在驱动线程内,sink 不跨线程。
//!
//! tun-rs 在 macOS 的 L2 实现即 ZeroTier 官方同款 feth 对:低号 feth 为虚拟接口
//! (配 IP/MAC,`name()` 返回它),peer feth 仅用于 I/O——读走 BPF、写走 AF_NDRV,
//! 帧均含完整 14 字节以太头。Drop 时自动 `ifconfig fethX destroy`。

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use tun_rs::SyncDevice;

use crate::config::{IpNet, NetConfig};
use crate::routes::RouteApplier;

const ETH_HDR: usize = 14;
/// reader 读错误退避重试间隔
const READER_BACKOFF: Duration = Duration::from_secs(1);
/// reader 读错误中,fd 失效类错误(EBADF/ENXIO)连续出现的放弃阈值
const READER_FD_FAIL_MAX: u32 = 3;

/// 设备读出的待回喂核心的帧(方向:设备→核心)。
#[derive(Debug, Clone)]
pub struct FrameIn {
    pub nwid: u64,
    pub src: u64,
    pub dst: u64,
    pub eth: u16,
    pub payload: Vec<u8>,
}

/// `FrameIn` 通道发送端:发出后写唤醒管道,让驱动线程的 poll 立即返回。
pub struct FrameInTx {
    tx: Sender<FrameIn>,
    wake: File,
}

impl Clone for FrameInTx {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), wake: self.wake.try_clone().expect("复制唤醒管道写端失败") }
    }
}

impl FrameInTx {
    pub(crate) fn new(tx: Sender<FrameIn>, wake: File) -> Self {
        Self { tx, wake }
    }

    /// 驱动线程已退出返回 false(发送失败被静默忽略,POC 无需处理)。
    pub fn send(&self, f: FrameIn) -> bool {
        if self.tx.send(f).is_err() {
            return false;
        }
        let mut b = [1u8; 1];
        let _ = unsafe { libc::write(self.wake.as_raw_fd(), b.as_mut_ptr() as *const _, 1) };
        true
    }
}

/// 设备线程取发送端的槽位:NodeHandle 在 start 后才能给出 FrameInTx,
/// 设备线程开跑可能更早,首帧前自旋等待一次即可。
pub type FrameInTxSlot = Arc<OnceLock<FrameInTx>>;

/// 单网络的 feth L2 设备 + managed 地址/路由应用状态。
struct NetDevice {
    dev: Arc<SyncDevice>,
    if_name: String,
    if_index: u32,
    mtu: u16,
    applied_addrs: Vec<IpNet>,
    routes: Option<RouteApplier>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    /// 设备→核心方向:reader 线程成功投递 frame_in 的次数(POC 验证用)。
    frames_in: Arc<AtomicU64>,
}

/// 多网络设备表:每网络一张 feth 设备、各自 reader 线程与 applied 状态。
///
/// 线程模型:`Arc<SyncDevice>` 由本对象与设备读线程共享(tun-rs 全部 `&self`
/// 方法且内部有 op_lock,Send+Sync);读线程阻塞 poll BPF fd 收帧;
/// `apply/send_frame` 可从驱动线程调用;`apply` 需 `&mut`(POC 里套 Mutex)。
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

    /// 当前全部网络的 feth 设备名快照(dev 端;I/O peer 端由内核配对,
    /// 清扫时经 ifconfig 解析)
    pub fn iface_names(&self) -> Vec<String> {
        self.devices.values().map(|d| d.if_name.clone()).collect()
    }

    /// 指定网络的设备名(未建返回 None)。
    pub fn if_name_of(&self, nwid: u64) -> Option<&str> {
        self.devices.get(&nwid).map(|d| d.if_name.as_str())
    }

    /// 设备→核心方向已成功投递核心的帧数(全部网络合计;reader 线程计数)。
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
        // 只对 Ok 状态应用地址/路由(REQUESTING 等阶段 assigned/routes 无意义)
        if cfg.status == crate::config::NetStatus::Ok {
            d.apply_addrs(cfg)?;
            d.apply_routes(cfg);
        }
        Ok(())
    }

    fn create_device(&mut self, cfg: &NetConfig, frame_tx: &FrameInTxSlot) -> io::Result<NetDevice> {
        let mtu = cfg.mtu.max(1280);
        // MAC 必须等于核心分配的虚拟网 MAC,否则内核 ARP/交换行为与核心不一致
        let mac = mac_bytes(cfg.mac);
        // 设备名不指定:内核 SIOCIFCREATE 自动取空闲 feth 单元号,与本机官方
        // zerotier-one 的 feth194/345/1986/2527/4684 天然不冲突;Drop 自动销毁
        let mut b = tun_rs::DeviceBuilder::new()
            .layer(tun_rs::Layer::L2)
            .mtu(mtu)
            .mac_addr(mac)
            .enable(true);
        // L2(feth)下 tun-rs 强制 associate_route=false,路由全部由本模块管理
        b = b.with(|o| {
            #[cfg(target_os = "macos")]
            o.associate_route(false);
        });
        let dev = b.build_sync()?;
        let name = dev.name()?;
        let if_index = dev.if_index().map_err(|e| {
            io::Error::other(format!("获取 {name} if_index 失败: {e}"))
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        let frames_in = Arc::new(AtomicU64::new(0));
        let dev = Arc::new(dev);
        let reader = spawn_reader(
            Arc::clone(&dev),
            Arc::clone(&stop),
            cfg.nwid,
            mtu,
            Arc::clone(frame_tx),
            Arc::clone(&frames_in),
        );
        eprintln!("[device] {:016x} feth 对已建立:{}(peer 用于 I/O)mtu={}", cfg.nwid, name, mtu);
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
        })
    }

    /// 核心产出的帧写设备(方向:核心→设备)。设备未建或网络不符时静默丢弃。
    pub fn send_frame(
        &self,
        nwid: u64,
        src: u64,
        dst: u64,
        eth: u16,
        payload: &[u8],
    ) -> io::Result<()> {
        let Some(d) = self.devices.get(&nwid) else { return Ok(()) };
        if payload.len() > (d.mtu as usize) {
            return Ok(());
        }
        let mut frame = Vec::with_capacity(ETH_HDR + payload.len());
        frame.extend_from_slice(&mac_bytes(dst));
        frame.extend_from_slice(&mac_bytes(src));
        frame.extend_from_slice(&eth.to_be_bytes());
        frame.extend_from_slice(payload);
        d.dev.send(&frame).map(|_| ())
    }

    /// 拆除单个网络的设备(停线程 → 删路由 → 销毁 feth)。网络不存在则无操作。
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
                IpAddr::V6(v6) => self.dev.add_address_v6(v6, a.prefix)?,
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

    /// 停线程 → 删路由 → 释放设备(feth 随最后一个 Arc 释放而销毁)。
    fn teardown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        if let Some(mut r) = self.routes.take() {
            r.teardown();
        }
        self.applied_addrs.clear();
        self.if_index = 0;
        self.if_name.clear();
        // self.dev 是最后一个引用时随本对象 drop 而销毁 feth
    }
}

impl Drop for DevicePlane {
    fn drop(&mut self) {
        // 兜底:调用方漏 teardown 时仍保证停线程、删路由、销毁 feth
        self.teardown_all();
    }
}

/// 48 位 MAC -> 6 字节(网络序,即帧内字节序)。
fn mac_bytes(mac: u64) -> [u8; 6] {
    let b = (mac & 0x0000_ffff_ffff_ffff).to_be_bytes();
    [b[2], b[3], b[4], b[5], b[6], b[7]]
}

fn mac_of(bytes: &[u8]) -> u64 {
    let mut m: u64 = 0;
    for b in bytes {
        m = (m << 8) | u64::from(*b);
    }
    m
}

/// 设备读线程:非阻塞 BPF + poll 轮询,帧拆 14B 以太头后回喂核心。
///
/// 错误策略:单次读/poll 错误不再永久退出,退避 1s 重试;fd 失效类错误
/// (EBADF/ENXIO)连续 `READER_FD_FAIL_MAX` 次才放弃(设备已销毁,回天乏术)。
fn spawn_reader(
    dev: Arc<SyncDevice>,
    stop: Arc<AtomicBool>,
    nwid: u64,
    mtu: u16,
    frame_tx: FrameInTxSlot,
    frames_in: Arc<AtomicU64>,
) -> Option<JoinHandle<()>> {
    match std::thread::Builder::new()
        .name(format!("feth-reader-{nwid:016x}"))
        .spawn(move || run_reader(dev, stop, nwid, mtu, frame_tx, frames_in))
    {
        Ok(h) => Some(h),
        // 线程起不来不再静默吞掉:设备将收不到入向帧,必须可见
        Err(e) => {
            eprintln!("[device] {nwid:016x} reader 线程启动失败(设备单向): {e}");
            None
        }
    }
}

/// fd 失效类错误:设备已销毁/句柄不可用,重试无意义。
fn fd_invalid(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::EBADF) | Some(libc::ENXIO))
}

fn run_reader(
    dev: Arc<SyncDevice>,
    stop: Arc<AtomicBool>,
    nwid: u64,
    mtu: u16,
    frame_tx: FrameInTxSlot,
    frames_in: Arc<AtomicU64>,
) {
    let fd = dev.as_raw_fd();
    // 非阻塞:BPF 缓冲一帧读入后内部队列可能还有剩余帧,靠 EAGAIN 判断排空
    if let Err(e) = dev.set_nonblocking(true) {
        eprintln!("[device-reader] set_nonblocking 失败: {e}");
        return;
    }
    let mut buf = vec![0u8; mtu.max(1280) as usize + ETH_HDR + 64];
    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let mut fd_fail_streak: u32 = 0;
    eprintln!("[device-reader] {:016x} 启动,监听 fd={fd}", nwid);
    while !stop.load(Ordering::SeqCst) {
        pfd.revents = 0;
        let n = unsafe { libc::poll(&mut pfd, 1, 200) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            eprintln!("[device-reader] poll 错误,1s 后重试: {err}");
            std::thread::sleep(READER_BACKOFF);
            continue;
        }
        if n == 0 || pfd.revents & libc::POLLIN == 0 {
            continue;
        }
        loop {
            match dev.recv(&mut buf) {
                Ok(len) if len >= ETH_HDR => {
                    fd_fail_streak = 0;
                    let Some(tx) = frame_tx.get() else {
                        eprintln!("[device-reader] FrameInTx 未就绪,丢帧");
                        break;
                    };
                    let (dst, src) = (mac_of(&buf[..6]), mac_of(&buf[6..12]));
                    let eth = u16::from_be_bytes([buf[12], buf[13]]);
                    let f = FrameIn { nwid, src, dst, eth, payload: buf[ETH_HDR..len].to_vec() };
                    if tx.send(f) {
                        frames_in.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Ok(_) => break, // 短帧(不足以太头)丢弃;BPF 仍可读则下轮 poll 立即返回
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break, // 设备底层读空
                Err(e) => {
                    // 退避重试;fd 失效类错误连续 3 次放弃(设备已不在)
                    if fd_invalid(&e) {
                        fd_fail_streak += 1;
                    }
                    eprintln!(
                        "[device-reader] recv 错误(连续 fd 失效 {fd_fail_streak}/{READER_FD_FAIL_MAX}),1s 后重试: {e}"
                    );
                    if fd_fail_streak >= READER_FD_FAIL_MAX {
                        eprintln!("[device-reader] fd 持续失效,放弃: {e}");
                        return;
                    }
                    std::thread::sleep(READER_BACKOFF);
                    break;
                }
            }
        }
    }
    eprintln!("[device-reader] 已停止");
}
