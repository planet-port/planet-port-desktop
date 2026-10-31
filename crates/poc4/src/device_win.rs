//! wintun(L3)设备封装:tun-rs 同步设备 + managed 地址(复用其
//! address-management/netconfig-rs)。Gate 1 自验入口也在此。
//!
//! 与 zt-core 的 device.rs 的关键差异:wintun 是 L3 设备——
//! - 无以太网头:send/recv 都是裸 IP 包(ARP 见 arp.rs);
//! - 适配器随设备对象存活,drop 即销毁,无 feth pair 概念。
use std::io;

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
    /// L3:send 写裸 IP 包(写以太网帧无意义,驱动只认 IP)
    #[allow(dead_code)] // Task 7 帧桥使用
    pub fn send(&self, pkt: &[u8]) -> io::Result<usize> {
        self.dev.send(pkt)
    }
}

/// Gate 1 自验 CLI:`poc4_windows --gate1-create|--gate1-ip a.b.c.d/nn`
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
        _ => {
            eprintln!("Gate 0 已随 zt-sys 构建通过;当前版本先提供 --gate1-create / --gate1-ip");
            2
        }
    }
}
