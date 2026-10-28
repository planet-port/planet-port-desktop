//! 设备层:每网络一张 feth L2 设备对(tun-rs),内核处理 ARP;
//! managed 地址/路由差分应用;设备帧与核心的双向桥接。
//!
//! 方向红线(两条通道不可混):
//! - 设备读出的帧 -> `FrameIn` 通道 -> 驱动线程 `processVirtualNetworkFrame`(设备→核心)
//! - 核心产出的帧 -> sink `on_frame` -> `DevicePlane::send_frame` 写设备(核心→设备)
//!
//! tun-rs 在 macOS 的 L2 实现即 ZeroTier 官方同款 feth 对:低号 feth 为虚拟接口
//! (配 IP/MAC,`name()` 返回它),peer feth 仅用于 I/O——读走 BPF、写走 AF_NDRV,
//! 帧均含完整 14 字节以太头。Drop 时自动 `ifconfig fethX destroy`。

use std::fs::File;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

use tun_rs::SyncDevice;

use crate::config::{IpNet, NetConfig};
use crate::routes::RouteApplier;

const ETH_HDR: usize = 14;

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

/// 每网络一张 feth L2 设备 + managed 地址/路由应用器。
///
/// 线程模型:`Arc<SyncDevice>` 由本对象与设备读线程共享(tun-rs 全部 `&self`
/// 方法且内部有 op_lock,Send+Sync);读线程阻塞 poll BPF fd 收帧;
/// `apply/send_frame` 可从驱动线程调用;`apply` 需 `&mut`(POC 里套 Mutex)。
pub struct DevicePlane {
    nwid: u64,
    dev: Option<Arc<SyncDevice>>,
    if_name: String,
    if_index: u32,
    mtu: u16,
    applied_addrs: Vec<IpNet>,
    routes: Option<RouteApplier>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl Default for DevicePlane {
    fn default() -> Self {
        Self::new()
    }
}

impl DevicePlane {
    pub fn new() -> Self {
        Self {
            nwid: 0,
            dev: None,
            if_name: String::new(),
            if_index: 0,
            mtu: 0,
            applied_addrs: Vec::new(),
            routes: None,
            stop: Arc::new(AtomicBool::new(false)),
            reader: None,
        }
    }

    pub fn is_up(&self) -> bool {
        self.dev.is_some()
    }

    pub fn if_name(&self) -> &str {
        &self.if_name
    }

    /// 本设备已应用的 IPv4(sink 的 local_ipv4s 用)。
    pub fn applied_v4s(&self) -> Vec<Ipv4Addr> {
        self.applied_addrs
            .iter()
            .filter_map(|n| match n.addr {
                IpAddr::V4(v) => Some(v),
                _ => None,
            })
            .collect()
    }

    /// on_net_config(Up/ConfigUpdate)调用;设备按需创建,地址/路由差分应用。
    /// (Down/Destroy 由调用方走 `teardown`。)
    pub fn apply(&mut self, cfg: &NetConfig, frame_tx: &FrameInTxSlot) -> io::Result<()> {
        if self.dev.is_none() {
            let dev = self.create_device(cfg)?;
            let if_index = dev.if_index().map_err(|e| {
                io::Error::other(format!("获取 {} if_index 失败: {e}", self.if_name))
            })?;
            self.if_index = if_index;
            self.spawn_reader(Arc::clone(&dev), Arc::clone(frame_tx));
            self.dev = Some(dev);
        }
        // 只对 Ok 状态应用地址/路由(REQUESTING 等阶段 assigned/routes 无意义)
        if cfg.status == crate::config::NetStatus::Ok {
            self.apply_addrs(cfg)?;
            self.apply_routes(cfg)?;
        }
        Ok(())
    }

    fn create_device(&mut self, cfg: &NetConfig) -> io::Result<Arc<SyncDevice>> {
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
        self.nwid = cfg.nwid;
        self.if_name = name.clone();
        self.mtu = mtu;
        self.stop = Arc::new(AtomicBool::new(false));
        eprintln!("[device] {:016x} feth 对已建立:{}(peer 用于 I/O)mtu={}", cfg.nwid, name, mtu);
        Ok(Arc::new(dev))
    }

    fn apply_addrs(&mut self, cfg: &NetConfig) -> io::Result<()> {
        let dev = self.dev.as_ref().expect("apply_addrs 需设备已建");
        for a in &cfg.assigned {
            if self.applied_addrs.contains(a) {
                continue;
            }
            match a.addr {
                IpAddr::V4(v4) => dev.add_address_v4(v4, a.prefix)?,
                IpAddr::V6(v6) => dev.add_address_v6(v6, a.prefix)?,
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
            let _ = dev.remove_address(a.addr);
            eprintln!("[device] {} - {}/{}", self.if_name, a.addr, a.prefix);
        }
        self.applied_addrs = cfg.assigned.clone();
        Ok(())
    }

    fn apply_routes(&mut self, cfg: &NetConfig) -> io::Result<()> {
        if self.routes.is_none() {
            self.routes = Some(RouteApplier::new(self.if_index)?);
        }
        if let Some(r) = self.routes.as_mut() {
            r.apply(&cfg.routes);
        }
        Ok(())
    }

    /// 核心产出的帧写设备(方向:核心→设备)。设备未建时静默丢弃。
    pub fn send_frame(
        &self,
        nwid: u64,
        src: u64,
        dst: u64,
        eth: u16,
        payload: &[u8],
    ) -> io::Result<()> {
        let Some(dev) = self.dev.as_ref() else { return Ok(()) };
        if nwid != self.nwid || payload.len() > (self.mtu as usize) {
            return Ok(());
        }
        let mut frame = Vec::with_capacity(ETH_HDR + payload.len());
        frame.extend_from_slice(&mac_bytes(dst));
        frame.extend_from_slice(&mac_bytes(src));
        frame.extend_from_slice(&eth.to_be_bytes());
        frame.extend_from_slice(payload);
        dev.send(&frame).map(|_| ())
    }

    /// 设备读线程:非阻塞 BPF + poll 轮询,帧拆 14B 以太头后回喂核心。
    fn spawn_reader(&mut self, dev: Arc<SyncDevice>, frame_tx: FrameInTxSlot) {
        let stop = Arc::clone(&self.stop);
        let nwid = self.nwid;
        let mtu = self.mtu;
        let handle = std::thread::Builder::new()
            .name("feth-reader".into())
            .spawn(move || unsafe { run_reader(dev, stop, nwid, mtu, frame_tx) })
            .ok();
        self.reader = handle;
    }

    /// 停线程 → 删路由 → drop 设备(feth 随 Drop 销毁)。必须先于 NodeHandle::shutdown。
    pub fn teardown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        if let Some(mut r) = self.routes.take() {
            r.teardown();
        }
        if let Some(dev) = self.dev.take() {
            let _ = dev;
        }
        self.applied_addrs.clear();
        self.if_index = 0;
        self.if_name.clear();
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

/// # Safety
/// `dev` 须为有效 SyncDevice;本函数在线程内独占读它,`stop` 置位后退出。
unsafe fn run_reader(
    dev: Arc<SyncDevice>,
    stop: Arc<AtomicBool>,
    nwid: u64,
    mtu: u16,
    frame_tx: FrameInTxSlot,
) {
    let fd = dev.as_raw_fd();
    // 非阻塞:BPF 缓冲一帧读入后内部队列可能还有剩余帧,靠 EAGAIN 判断排空
    if let Err(e) = dev.set_nonblocking(true) {
        eprintln!("[device-reader] set_nonblocking 失败: {e}");
        return;
    }
    let mut buf = vec![0u8; mtu.max(1280) as usize + ETH_HDR + 64];
    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    eprintln!("[device-reader] 启动,监听 fd={fd}");
    while !stop.load(Ordering::SeqCst) {
        pfd.revents = 0;
        let n = unsafe { libc::poll(&mut pfd, 1, 200) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            eprintln!("[device-reader] poll 错误,退出: {err}");
            return;
        }
        if n == 0 || pfd.revents & libc::POLLIN == 0 {
            continue;
        }
        loop {
            match dev.recv(&mut buf) {
                Ok(len) if len >= ETH_HDR => {
                    let Some(tx) = frame_tx.get() else {
                        eprintln!("[device-reader] FrameInTx 未就绪,丢帧");
                        break;
                    };
                    let (dst, src) = (mac_of(&buf[..6]), mac_of(&buf[6..12]));
                    let eth = u16::from_be_bytes([buf[12], buf[13]]);
                    let _ = tx.send(FrameIn {
                        nwid,
                        src,
                        dst,
                        eth,
                        payload: buf[ETH_HDR..len].to_vec(),
                    });
                }
                Ok(_) => break, // 短帧(不足以太头)丢弃;BPF 仍可读则下轮 poll 立即返回
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break, // 设备底层读空
                Err(e) => {
                    eprintln!("[device-reader] recv 错误,退出: {e}");
                    return;
                }
            }
        }
    }
    eprintln!("[device-reader] 已停止");
}
