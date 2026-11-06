//! 本机接口地址枚举(组播订阅用:对本机每个 IPv4 订阅 (ffffffffffff, ADI=IPv4))。

use std::net::Ipv4Addr;

/// 非回环接口上的全部 IPv4(去重升序)。枚举失败返回空(仅影响组播接收,不致命)。
///
/// Unix 用 libc::getifaddrs(仅 Unix 提供);Windows 无该 API,用 `getifaddrs`
/// crate(crates.io,MIT,内部走 GetAdaptersAddresses),语义一致。
#[cfg(not(windows))]
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

/// (接口名, IPv4) 对(排除回环),macOS 托管路由防护用:判定目标网段是否
/// 覆盖其它接口上的本机地址。与 local_ipv4s 同源遍历,多取 ifa_name。
#[cfg(target_os = "macos")]
pub fn local_v4_by_if() -> Vec<(String, Ipv4Addr)> {
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
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                let a = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                out.push((name, Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr))));
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }
    out
}

/// Windows 版:GetAdaptersAddresses 封装,过滤回环后取全部 IPv4。
/// 失败返回空(仅影响组播接收,不致命),与 Unix 分支语义一致。
#[cfg(windows)]
pub fn local_ipv4s() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if let Ok(it) = getifaddrs::getifaddrs() {
        for ifa in it {
            if ifa.flags.contains(getifaddrs::InterfaceFlags::LOOPBACK) {
                continue;
            }
            if let std::net::IpAddr::V4(v4) = ifa.address {
                out.push(v4);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}
