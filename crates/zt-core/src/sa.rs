//! sockaddr_storage <-> SocketAddr;ZeroTier 把前缀长度编码在 sin_port。
//!
//! 说明:zt-sys 的绑定自带 Darwin 版 `sockaddr_storage/sockaddr_in/...`(bindgen
//! 产物,与 libc 布局一致但类型不同),FFI 签名必须用它,故本模块直接以 zt_sys
//! 的类型为边界,避免跨类型 transmute。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use zt_sys as zt;

/// 读取 sockaddr_storage 指向的地址;family 未知(NULL/AF_UNSPEC 等)返回 None。
///
/// # Safety
/// `ss` 为 NULL 或指向核心提供的有效 sockaddr_storage。
pub unsafe fn from_sockaddr_storage(ss: *const zt::sockaddr_storage) -> Option<SocketAddr> {
    if ss.is_null() {
        return None;
    }
    unsafe {
        match (*ss).ss_family as u32 {
            zt::AF_INET => {
                let a = &*(ss as *const zt::sockaddr_in);
                Some(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::from(a.sin_addr.s_addr.to_be_bytes())),
                    u16::from_be(a.sin_port),
                ))
            }
            zt::AF_INET6 => {
                let a = &*(ss as *const zt::sockaddr_in6);
                Some(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(a.sin6_addr.__u6_addr.__u6_addr8)),
                    u16::from_be(a.sin6_port),
                ))
            }
            _ => None,
        }
    }
}

/// SocketAddr(或带 prefix 的网络条目)写回 sockaddr_storage。
/// `port` 参数:收发物理包用真实端口;网络条目用它承载前缀长度。
pub fn to_sockaddr_storage(addr: &SocketAddr, port_override: Option<u16>) -> zt::sockaddr_storage {
    let mut ss: zt::sockaddr_storage = unsafe { std::mem::zeroed() };
    let port = port_override.unwrap_or(addr.port()).to_be();
    match addr.ip() {
        IpAddr::V4(v4) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut zt::sockaddr_in);
            a.sin_len = std::mem::size_of::<zt::sockaddr_in>() as u8;
            a.sin_family = zt::AF_INET as _;
            a.sin_port = port;
            a.sin_addr.s_addr = u32::from_be_bytes(v4.octets());
        },
        IpAddr::V6(v6) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut zt::sockaddr_in6);
            a.sin6_len = std::mem::size_of::<zt::sockaddr_in6>() as u8;
            a.sin6_family = zt::AF_INET6 as _;
            a.sin6_port = port;
            a.sin6_addr.__u6_addr.__u6_addr8 = v6.octets();
        },
    }
    ss
}
