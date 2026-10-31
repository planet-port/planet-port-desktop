//! IPv4 ARP 垫层:wintun(L3)与 ZeroTier 核心(L2)之间的桥接层。
//!
//! 职责(POC 范围 IPv4):
//! 1. 应答远端 peer 的 ARP request(问我们的 managed IP → 以本网 MAC 回 reply);
//! 2. 出向(Windows IP 包 → 核心帧):维护 IP→MAC 学习表,来源是入向帧的
//!    (以太网 src MAC, IP src)绑定;查不到时用广播 MAC 直发(POC 策略,
//!    不做请求-缓存队列)。
//! 3. 入向 ARP reply(对我们在 Task 7b 主动发出的 probe request 的应答)入表。
pub mod ether {
    pub const ETHERTYPE_IPV4: u16 = 0x0800;
    pub const ETHERTYPE_ARP: u16 = 0x0806;
    pub const BROADCAST: u64 = 0xffff_ffff_ffff;
    pub fn mac_bytes(m: u64) -> [u8; 6] {
        m.to_be_bytes()[2..].try_into().unwrap()
    }
    pub fn mac_from_bytes(b: &[u8]) -> u64 {
        let mut wid = [0u8; 8];
        wid[2..].copy_from_slice(&b[..6]);
        u64::from_be_bytes(wid)
    }
}

pub struct ArpShim {
    /// 本网虚拟 MAC(核心分配的 cfg.mac)
    pub local_mac: u64,
    /// 需要应答 ARP 的本机 managed IPv4
    pub our_ips: Vec<std::net::Ipv4Addr>,
    /// 学习表:IP → MAC
    table: std::collections::HashMap<std::net::Ipv4Addr, u64>,
}

impl ArpShim {
    pub fn new(local_mac: u64) -> Self {
        Self { local_mac, our_ips: Vec::new(), table: Default::default() }
    }
    pub fn set_ips(&mut self, ips: Vec<std::net::Ipv4Addr>) {
        self.our_ips = ips;
    }

    /// 出向:Windows 裸 IP 包 → (dst_mac, src_mac, ethertype, payload)。
    /// src IP 来自 IP 头(20 字节起);dst 查表,miss 用广播。
    pub fn wrap_outbound(&mut self, pkt: &[u8]) -> Option<(u64, u64, u16, Vec<u8>)> {
        if pkt.len() < 20 || pkt[0] >> 4 != 4 {
            return None; // POC 只桥 IPv4
        }
        let dst_ip = std::net::Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]);
        // 不学 src:把本机自发包的 (src IP, local_mac) 入表只会污染学习表(表
        // 语义是「对端 IP → 对端 MAC」);学习只发生在入向 learn()/answer()。
        let dst = self.table.get(&dst_ip).copied().unwrap_or(ether::BROADCAST);
        Some((dst, self.local_mac, ether::ETHERTYPE_IPV4, pkt.to_vec()))
    }

    /// 入向帧学习:IP 帧的 (src MAC, src IP) 绑定入表。
    pub fn learn(&mut self, src_mac: u64, ether_type: u16, payload: &[u8]) {
        if ether_type != ether::ETHERTYPE_IPV4 || payload.len() < 20 {
            return;
        }
        let src_ip = std::net::Ipv4Addr::new(payload[12], payload[13], payload[14], payload[15]);
        self.table.insert(src_ip, src_mac);
    }

    /// 处理核心送来的 ARP 帧:问 our_ips 且是 request → 构造 reply payload。
    /// 返回 Some(reply_payload) 时,调用方以 (tha=req.sha, src=local_mac, ARP) 喂回核心。
    pub fn answer(&mut self, payload: &[u8]) -> Option<(u64, Vec<u8>)> {
        if payload.len() < 28 {
            return None;
        }
        let htype = u16::from_be_bytes([payload[0], payload[1]]);
        let ptype = u16::from_be_bytes([payload[2], payload[3]]);
        if htype != 1 || ptype != ether::ETHERTYPE_IPV4 {
            return None;
        }
        let oper = u16::from_be_bytes([payload[6], payload[7]]);
        let sha = ether::mac_from_bytes(&payload[8..14]);
        let spa = std::net::Ipv4Addr::new(payload[14], payload[15], payload[16], payload[17]);
        let tpa = std::net::Ipv4Addr::new(payload[24], payload[25], payload[26], payload[27]);
        match oper {
            1 => {
                // request:问的是不是我们的 IP
                if !self.our_ips.contains(&tpa) {
                    return None;
                }
                let mut r = [0u8; 28];
                r[0..2].copy_from_slice(&htype.to_be_bytes());
                r[2..4].copy_from_slice(&ptype.to_be_bytes());
                r[4] = 6; // hlen
                r[5] = 4; // plen
                r[6..8].copy_from_slice(&2u16.to_be_bytes()); // reply
                r[8..14].copy_from_slice(&ether::mac_bytes(self.local_mac)); // sha
                r[14..18].copy_from_slice(&tpa.octets()); // spa = 被问的 IP
                r[18..24].copy_from_slice(&ether::mac_bytes(sha)); // tha = 请求者
                r[24..28].copy_from_slice(&spa.octets()); // tpa = 请求者 IP
                // 顺带学习请求者绑定
                self.table.insert(spa, sha);
                Some((sha, r.to_vec()))
            }
            2 => {
                // reply(对我们 probe 的应答):学习 spa↔sha
                self.table.insert(spa, sha);
                None
            }
            _ => None,
        }
    }
}
