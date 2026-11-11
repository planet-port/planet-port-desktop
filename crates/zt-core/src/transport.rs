//! UDP 受限网络降级通道:副端口策略、TCP 隧道线协议、wire 选路决策。
//!
//! 行为与参数对齐官方 OneService 1.14.2,实现为纯 Rust(不引入 OneService):
//! - 副端口:随机 20000-65500,离线 >120s 换绑(30s 周期检查),online 不换;
//! - TCP 隧道:60s 无 global IPv4 直收触发 connect(2.5s 节流),UDP 恢复
//!   (30s 窗口内有直收)主动关闭(fail forward);
//! - TCP 线协议:伪 TLS 记录层,见 codec。
//!
//! # 架构红线
//! - 本模块不触线程/Inner:决策是纯函数,执行(node.rs 驱动线程)与
//!   传输(TCP 线程)由 node.rs 装配;
//! - codec 的 Decoder 对脏流必须容错:长度越界/前缀非 0x04 返回错误由
//!   调用方断开重连,不得 panic。

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

/// TCP 路径在核心 localSocket 语义中的哨兵。
/// 核心把 localSocket 当不透明路径标识(官方 Phy 即 socket 句柄),
/// 该值远离真实 fd/SOCKET 数值域,绝不冲突。
pub const TCP_LOCAL_SOCKET: i64 = 0x7a5a_0000_0000_0001;

// ---- 官方对齐参数(OneService.cpp 1.14.2 / tcp-proxy.cpp) ----

/// 副端口随机起点下限(官方 _getRandomPort:20000 + rand%45500)
pub const SECONDARY_PORT_MIN: u16 = 20_000;
/// 随机窗口:首试落在 20000..=65499
pub const SECONDARY_PORT_SPAN: u32 = 45_500;
/// 副端口连续绑定失败上限(官方:失败 +1 顺试,1000 次放弃)
pub const SECONDARY_PORT_TRIALS: u32 = 1000;
/// 副端口上限(含)
pub const SECONDARY_PORT_MAX: u16 = 65_535;
// 以下常量由 node.rs 驱动线程消费;SECONDARY_REBIND_AFTER_MS/TRANSPORT_TICK_MS
// 已在 Task 2 接线(换绑巡检),其余 TCP 组待 Task 3 消费,暂标 dead_code
/// 离线多久换绑副口(官方 ZT_PEER_PING_PERIOD*2)
pub const SECONDARY_REBIND_AFTER_MS: i64 = 120_000;
/// 通道层巡检周期(官方 Binder refresh)
pub const TRANSPORT_TICK_MS: i64 = 30_000;
/// TCP 触发:持续无 global 直收时长(官方 ZT_TCP_FALLBACK_AFTER)
#[allow(dead_code)]
pub const TCP_AFTER_MS: i64 = 60_000;
/// TCP connect 节流(官方 ZT_PING_CHECK_INTERVAL/2)
#[allow(dead_code)]
pub const TCP_RETRY_THROTTLE_MS: i64 = 2_500;
/// TCP 回退窗口:窗口内有过直收即关隧道(官方 30s fail forward)
#[allow(dead_code)]
pub const TCP_FALLBACK_WINDOW_MS: i64 = 30_000;
/// TCP 发送队列等效上限(try_send 满即丢帧;官方 writeq 满同样丢弃)
#[allow(dead_code)]
pub const TCP_QUEUE_LIMIT: usize = 64 * 1024;

/// 是否「global 范围 IPv4」:非环回/私网/链路本地/组播/文档段。
/// 用于 TCP 触发计时与隧道镜像目标判定(官方 IP_SCOPE_GLOBAL 的常用覆盖)。
pub fn is_global_ipv4(ip: Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || ip.is_unspecified())
}

/// 随机选绑副口:20000+rand%45500 起,bind 失败 +1 顺试,上限 1000 次。
/// 返回已绑定的 socket(阻塞/非阻塞设置由调用方平台分支处理)。
pub fn bind_secondary(mut rng: impl FnMut() -> u32) -> Option<UdpSocket> {
    let start = u32::from(SECONDARY_PORT_MIN) + rng() % SECONDARY_PORT_SPAN;
    for off in 0..SECONDARY_PORT_TRIALS {
        let Some(p) = start.checked_add(off) else { break };
        if p < u32::from(SECONDARY_PORT_MIN) || p > u32::from(SECONDARY_PORT_MAX) {
            break;
        }
        if let Ok(s) = UdpSocket::bind(("0.0.0.0", p as u16)) {
            return Some(s);
        }
    }
    None
}

/// wire_send 单包选路决策(纯函数,驱动线程执行)。
///
/// 官方语义对照:
/// - localSocket==0:核心不指定 → 主口必发,副口并存则冗余发(Phy sock=-1);
/// - localSocket==TCP 哨兵:核心要求走 TCP 学到的路径 → 仅入隧道;
/// - 其它:指定 socket(副口 fd 匹配则副口发,否则主口);
/// - mirror_tcp:隧道 Established 期间,global IPv4 的 UDP 发包同时镜像进
///   隧道(官方 nodeWirePacketSendFunction:隧道存在即追加 writeq)。
pub fn plan_wire(
    local_socket: i64,
    secondary_local: Option<i64>,
    tcp_open: bool,
    dst: SocketAddr,
) -> WirePlan {
    if local_socket == TCP_LOCAL_SOCKET {
        return WirePlan { udp: vec![WireRoute::Tcp], mirror_tcp: false };
    }
    let udp = if local_socket == 0 {
        let mut v = vec![WireRoute::Primary];
        if secondary_local.is_some() {
            v.push(WireRoute::Secondary);
        }
        v
    } else if Some(local_socket) == secondary_local {
        vec![WireRoute::Secondary]
    } else {
        vec![WireRoute::Primary]
    };
    let mirror_tcp = tcp_open
        && matches!(dst, SocketAddr::V4(v4) if is_global_ipv4(*v4.ip()));
    WirePlan { udp, mirror_tcp }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireRoute {
    Primary,
    Secondary,
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WirePlan {
    /// 应发送的通道;含 Tcp 表示核心指定走隧道路径
    pub udp: Vec<WireRoute>,
    /// 额外镜像一份进 TCP 隧道(隧道激活期的 global IPv4 包)
    pub mirror_tcp: bool,
}

/// 伪 TLS 记录层线协议(对照官方 tcp-proxy.cpp:200-256 与
/// OneService.cpp:3054-3126):
///
/// - 帧:`17 03 03 | len16(大端) | payload`,len = 7 + ZT 包长;
/// - payload 前缀:`0x04 | IPv4(网络序 4B) | port(网络序 2B)`——
///   客户端发=目的地址,服务端回=来源地址;
/// - 连接建立先发 9B HELLO(一条 len=4 的记录):`17 03 03 00 04 | major
///   minor rev_hi rev_lo`(服务端仅以 len==4 识别新协议,不校验版本)。
pub mod codec {
    use std::net::SocketAddrV4;

    /// TCP 单帧可承载的最大 payload(ZT_MAX_PHYSPAYLOAD+headroom 之外再
    /// 留余量;超过视为脏流)
    pub const MAX_FRAME: usize = 16_384;

    pub fn encode_hello(major: u8, minor: u8, rev: u16, out: &mut Vec<u8>) {
        out.extend_from_slice(&[0x17, 0x03, 0x03, 0x00, 0x04, major, minor]);
        out.extend_from_slice(&rev.to_be_bytes());
    }

    pub fn encode_frame(dst: &SocketAddrV4, payload: &[u8], out: &mut Vec<u8>) {
        let mlen = (7 + payload.len()) as u16;
        out.extend_from_slice(&[0x17, 0x03, 0x03]);
        out.extend_from_slice(&mlen.to_be_bytes());
        out.push(0x04);
        out.extend_from_slice(&dst.ip().octets());
        out.extend_from_slice(&dst.port().to_be_bytes());
        out.extend_from_slice(payload);
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Frame {
        /// 对端 HELLO(服务端视角;客户端忽略对端回显)
        Hello { major: u8, minor: u8, rev: u16 },
        /// 一个 ZT 物理包 + 来源地址(剥 7B 前缀后)
        Packet { peer: SocketAddrV4, payload: Vec<u8> },
    }

    #[derive(Debug, PartialEq, Eq)]
    pub enum DecodeErr {
        /// 长度越界:流脏,调用方应断开重连
        Oversize,
        /// 前缀非法(非 0x04 / 长度不足)
        BadPrefix,
    }

    /// 字节流 → 帧的增量解码器(处理半包/粘包)
    pub struct Decoder {
        buf: Vec<u8>,
    }

    impl Decoder {
        pub fn new() -> Self {
            Self { buf: Vec::new() }
        }

        pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, DecodeErr> {
            self.buf.extend_from_slice(chunk);
            let mut frames = Vec::new();
            loop {
                if self.buf.len() < 5 {
                    return Ok(frames);
                }
                let mlen = u16::from_be_bytes([self.buf[3], self.buf[4]]) as usize;
                if mlen == 4 {
                    // HELLO:5B 头 + 4B 版本
                    if self.buf.len() < 9 {
                        return Ok(frames);
                    }
                    frames.push(Frame::Hello {
                        major: self.buf[5],
                        minor: self.buf[6],
                        rev: u16::from_be_bytes([self.buf[7], self.buf[8]]),
                    });
                    self.buf.drain(..9);
                    continue;
                }
                if !(7..=MAX_FRAME).contains(&mlen) {
                    return Err(DecodeErr::Oversize);
                }
                if self.buf.len() < 5 + mlen {
                    return Ok(frames);
                }
                if self.buf[5] != 0x04 {
                    return Err(DecodeErr::BadPrefix);
                }
                let peer = SocketAddrV4::new(
                    std::net::Ipv4Addr::new(self.buf[6], self.buf[7], self.buf[8], self.buf[9]),
                    u16::from_be_bytes([self.buf[10], self.buf[11]]),
                );
                let payload = self.buf[12..5 + mlen].to_vec();
                frames.push(Frame::Packet { peer, payload });
                self.buf.drain(..5 + mlen);
            }
        }
    }

    impl Default for Decoder {
        fn default() -> Self {
            Self::new()
        }
    }
}

/// TCP 隧道状态(驱动线程独占写,快照经 TransportInfo 对外)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpTunnelState {
    /// 未配置 relay
    Disabled,
    /// 未激活(UDP 正常或未到触发条件)
    Idle,
    /// connect 进行中
    Connecting,
    /// 隧道已建立
    Established,
}

/// 通道状态快照(NodeHandle::transport() 返回,REST/诊断面消费)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportInfo {
    pub primary_port: u16,
    pub secondary_port: Option<u16>,
    pub tcp: TcpTunnelState,
    pub relay: Option<SocketAddr>,
}

/// 驱动线程 → TCP 线程命令
#[derive(Debug)]
pub enum TcpCmd {
    Connect(SocketAddr),
    /// 已编码的完整帧(含记录头与地址前缀)
    Send(Vec<u8>),
    Close,
}

/// TCP 线程 → 驱动线程上抛
#[derive(Debug)]
pub enum TcpUp {
    Connected,
    /// 断开(connect 失败/IO 错误/驱动主动关),带原因
    Down(String),
    /// 隧道收到的 ZT 包 + 来源地址(解自帧前缀)
    Packet(Vec<u8>, SocketAddr),
}
