//! 本机接口地址枚举(组播订阅用:对本机每个 IPv4 订阅 (ffffffffffff, ADI=IPv4))。

use std::net::Ipv4Addr;

/// 非回环接口上的全部 IPv4(去重升序)。枚举失败返回空(仅影响组播接收,不致命)。
pub fn local_ipv4s() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return out;
        }
        let mut cur = ifap;
        while !cur.is_null() {
            let ifa = &*cur;
            if !ifa.ifa_addr.is_null()
                && (ifa.ifa_flags & libc::IFF_LOOPBACK as u32) == 0
                && (*ifa.ifa_addr).sa_family as i32 == libc::AF_INET
            {
                let a = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                out.push(Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)));
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
    out.sort_unstable();
    out.dedup();
    out
}
