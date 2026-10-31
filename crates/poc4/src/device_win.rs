//! wintun(L3)设备封装:tun-rs 同步设备 + managed 地址(复用其
//! address-management/netconfig-rs)。Gate 1 自验入口也在此。
//!
//! 与 zt-core 的 device.rs 的关键差异:wintun 是 L3 设备——
//! - 无以太网头:send/recv 都是裸 IP 包(ARP 见 arp.rs);
//! - 适配器随设备对象存活,drop 即销毁,无 feth pair 概念。
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::routes_win;

/// 一个 wintun 适配器。命名固定带 planet-port 前缀,便于实测时 netsh 查找。
pub struct WinDevice {
    dev: tun_rs::SyncDevice,
    name: String,
    if_index: u32,
}

impl WinDevice {
    /// 创建(已存在同名适配器会复用 wintun 的 ring)。需要管理员权限。
    pub fn new(name: &str, mtu: u16) -> io::Result<Self> {
        let dev = tun_rs::DeviceBuilder::new()
            .layer(tun_rs::Layer::L3)
            .name(name)
            .mtu(mtu)
            .enable(true)
            .build_sync()?;
        let dn = dev.name()?;
        let if_index = dev
            .if_index()
            .map_err(|e| io::Error::other(format!("获取 {dn} if_index 失败: {e}")))?;
        Ok(Self { dev, name: dn, if_index })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn if_index(&self) -> u32 {
        self.if_index
    }
    /// 设备引用(读线程/写帧共用;调用方自行 Arc)
    #[allow(dead_code)] // Task 5/7 数据面使用
    pub fn device(&self) -> &tun_rs::SyncDevice {
        &self.dev
    }
    #[allow(dead_code)] // Task 5/7 数据面使用
    pub fn into_inner(self) -> tun_rs::SyncDevice {
        self.dev
    }

    pub fn add_address_v4(&self, addr: std::net::Ipv4Addr, prefix: u8) -> io::Result<()> {
        self.dev.add_address_v4(addr, prefix)
    }
    #[allow(dead_code)] // Task 5 地址差分移除使用
    pub fn remove_address(&self, addr: std::net::IpAddr) -> io::Result<()> {
        self.dev.remove_address(addr)
    }

    /// L3:recv 返回裸 IP 包(无以太网头)
    #[allow(dead_code)] // Task 5 --gate1-read / Task 7 读线程使用
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.dev.recv(buf)
    }
    /// L3:非阻塞读(仅 tun-rs Windows 提供);无包返回 WouldBlock。
    /// 收包自验的轮询读用它,保证置停后 join 可返回,不挂死收尾。
    #[allow(dead_code)] // Task 5 --gate1-read 轮询读使用
    pub fn try_recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.dev.try_recv(buf)
    }
    /// L3:send 写裸 IP 包(写以太网帧无意义,驱动只认 IP)
    #[allow(dead_code)] // Task 7 帧桥使用
    pub fn send(&self, pkt: &[u8]) -> io::Result<usize> {
        self.dev.send(pkt)
    }
}

/// Gate 1 自验 CLI:`poc4_windows --gate1-create|--gate1-ip a.b.c.d/nn`
/// | `--gate1-read a.b.c.d/nn [probe_last]`
/// 设备创建/地址应用以实际系统状态为准,不内置断言——观察即结论。
pub fn gate1_cli() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--gate1-create") => {
            let d = WinDevice::new("planet-port", 2800);
            match d {
                Ok(d) => {
                    println!(
                        "[gate1] PASS 适配器已建立: name={} if_index={}",
                        d.name(),
                        d.if_index()
                    );
                    println!("[gate1] 按任意键销毁并退出(检查 Get-NetAdapter 后再按)");
                    let _ = std::io::stdin().read_line(&mut String::new());
                    0
                }
                Err(e) => {
                    eprintln!("[gate1] FAIL 设备创建失败: {e}");
                    1
                }
            }
        }
        Some(a) if a == "--gate1-ip" => {
            let ip_arg = args.get(2).expect("用法: --gate1-ip a.b.c.d/nn").clone();
            let (ip, prefix) = ip_arg
                .split_once('/')
                .expect("前缀用 / 分隔,如 10.121.16.50/24");
            let d = match WinDevice::new("planet-port", 2800) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[gate1] FAIL {e}");
                    return 1;
                }
            };
            match d.add_address_v4(ip.parse().expect("非法 IPv4"), prefix.parse().expect("非法前缀"))
            {
                Ok(()) => {
                    println!("[gate1] PASS 地址已应用: {}/{} on {}", ip, prefix, d.name());
                    println!("[gate1] 请另开终端验证: ipconfig / route print;回车后销毁退出");
                    let _ = std::io::stdin().read_line(&mut String::new());
                    0
                }
                Err(e) => {
                    eprintln!("[gate1] FAIL 地址应用失败: {e}");
                    1
                }
            }
        }
        Some(a) if a == "--gate1-read" => {
            let ip_arg = args.get(2).expect("用法: --gate1-read a.b.c.d/nn [probe_last]").clone();
            let (ip, prefix) = ip_arg.split_once('/').expect("前缀用 / 分隔");
            let ip: std::net::Ipv4Addr = ip.parse().expect("非法 IPv4");
            let prefix: u8 = prefix.parse().expect("非法前缀");
            // 探针网尾字节可参数化(默认 200);选地址时避开它,否则探针路由与自身地址重叠收不到包
            let probe_last: u8 = args
                .get(3)
                .map(|s| s.parse().expect("非法探针尾字节(0-255)"))
                .unwrap_or(200);
            let d = match WinDevice::new("planet-port", 2800) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[gate1] FAIL {e}");
                    return 1;
                }
            };
            if let Err(e) = d.add_address_v4(ip, prefix) {
                eprintln!("[gate1] FAIL 地址应用失败: {e}");
                return 1;
            }
            // on-link 路由通常随地址自动生成;补一条对端测试网段路由,诱导内核发包进 ring
            let probe_net = std::net::Ipv4Addr::new(
                ip.octets()[0],
                ip.octets()[1],
                ip.octets()[2],
                probe_last,
            );
            let mut ra = match routes_win::RouteApplier::new(d.if_index()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[gate1] FAIL 路由应用器: {e}");
                    return 1;
                }
            };
            let n = ra.apply(&[routes_win::RouteSpec {
                target: probe_net.into(),
                prefix,
                via: None,
            }]);
            println!(
                "[gate1] 地址 {ip}/{prefix} 已应用;探针路由安装 {n} 条({probe_net}/{prefix} on-link)"
            );
            println!("[gate1] 60s 内请在另一终端执行: ping {probe_net}");
            let d = Arc::new(d);
            let stop = Arc::new(AtomicBool::new(false));
            let rd = Arc::clone(&d);
            let rd_stop = Arc::clone(&stop);
            // try_recv 非阻塞轮询(tun-rs Windows 专有):无包返回 WouldBlock,
            // 保证 60s 置位 stop 后 join 能立即返回,teardown 与退出码不被挂起。
            let reader = std::thread::spawn(move || {
                let mut buf = [0u8; 4000];
                let mut seen = 0u64;
                while !rd_stop.load(Ordering::SeqCst) {
                    match rd.try_recv(&mut buf) {
                        Ok(len) if len > 0 => {
                            seen += 1;
                            // L3:首字节高 4 位是 IP 版本;wintun 理论只交付 IP 包
                            let ver = buf[0] >> 4;
                            let proto = if len >= 10 { buf[9] } else { 0 };
                            println!(
                                "[gate1] 包#{seen}: len={len} ipver={ver} proto={proto}(1=ICMP 17=UDP 6=TCP)"
                            );
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                        Err(e) => {
                            eprintln!("[gate1] recv 错误(继续): {e}");
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                    }
                }
                seen
            });
            std::thread::sleep(std::time::Duration::from_secs(60));
            stop.store(true, Ordering::SeqCst);
            let seen = reader.join().unwrap_or_else(|_| {
                eprintln!("[gate1] reader 线程异常退出");
                0
            });
            println!("[gate1] 60s 共读到 {seen} 个包(>0 即 Gate 1 读路径通过)");
            ra.teardown();
            if seen > 0 { 0 } else { 1 }
        }
        _ => {
            eprintln!(
                "Gate 0 已随 zt-sys 构建通过;当前版本提供 --gate1-create / --gate1-ip / --gate1-read"
            );
            2
        }
    }
}
