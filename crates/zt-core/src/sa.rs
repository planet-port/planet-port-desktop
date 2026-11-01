//! sockaddr_storage <-> SocketAddr;ZeroTier 把前缀长度编码在 sin_port。
//!
//! # 架构红线
//!
//! - 只做纯转换,不持有核心内存;回调内调用 `from_sockaddr_storage` 属拷贝,允许;
//! - 网络条目(地址/路由)的前缀长度在 sin_port,收发包用真实端口,互不可混。
//!
//! 说明:zt-sys 的绑定自带平台版 `sockaddr_storage/sockaddr_in/...`(bindgen
//! 产物,布局随目标平台),FFI 签名必须用它,故本模块直接以 zt_sys
//! 的类型为边界,避免跨类型 transmute。
//!
//! 绑定形态差异(bindgen,MSVC/Windows,实证见 crates/poc4/src/sa.rs):
//! - `ss_family`/`sin_family`/`sin6_family` 均为 `ADDRESS_FAMILY`(u16),无 sin_len;
//! - `in6_addr.u.Byte` 为 `[u8; 16]`(union);
//! - `in_addr.S_un.S_addr` 为 `ULONG`(union,注意大写 `S_addr`,非 BSD/glibc
//!   的 `s_addr`)。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use zt_sys as zt;

// in6_addr 内层字段名与 sin_len 是平台布局差异:Darwin 为
// `__u6_addr.__u6_addr8` 且 sockaddr 带 sin_len;glibc 为 `__in6_u.__u6_addr8`
// 且无 sin_len;winsock 为 `u.Byte` 且无 sin_len。收束到 helper,转换逻辑
// 不重复分叉。
/// 读 in6_addr 的 16 字节(内存序即网络序)。union 字段访问需 unsafe。
fn in6_bytes(a: &zt::in6_addr) -> [u8; 16] {
    #[cfg(target_os = "macos")]
    {
        unsafe { a.__u6_addr.__u6_addr8 }
    }
    #[cfg(windows)]
    {
        unsafe { a.u.Byte }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        unsafe { a.__in6_u.__u6_addr8 }
    }
}

/// 写 in6_addr 的 16 字节(union 字段写入是安全操作,读取才需 unsafe)。
fn set_in6_bytes(dst: &mut zt::in6_addr, b: [u8; 16]) {
    #[cfg(target_os = "macos")]
    {
        dst.__u6_addr.__u6_addr8 = b;
    }
    #[cfg(windows)]
    {
        dst.u.Byte = b;
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        dst.__in6_u.__u6_addr8 = b;
    }
}

/// 读 in_addr 的 32 位 IPv4(内存序即网络序;winsock 是 union 字段 S_un.S_addr)。
fn in4_addr(a: &zt::in_addr) -> u32 {
    #[cfg(windows)]
    {
        unsafe { a.S_un.S_addr }
    }
    #[cfg(not(windows))]
    {
        a.s_addr
    }
}

/// 写 in_addr 的 32 位 IPv4(union 字段写入是安全操作,读取才需 unsafe)。
fn set_in4_addr(dst: &mut zt::in_addr, v: u32) {
    #[cfg(windows)]
    {
        dst.S_un.S_addr = v;
    }
    #[cfg(not(windows))]
    {
        dst.s_addr = v;
    }
}

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
                    // s_addr 的内存字节即网络序;to_ne_bytes 按内存原序取回(LE 上
                    // 等价于逐字节读,不可用 to_be_bytes 二次反转)
                    IpAddr::V4(Ipv4Addr::from(in4_addr(&a.sin_addr).to_ne_bytes())),
                    u16::from_be(a.sin_port),
                ))
            }
            zt::AF_INET6 => {
                let a = &*(ss as *const zt::sockaddr_in6);
                Some(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(in6_bytes(&a.sin6_addr))),
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
            // sin_len 仅 BSD 布局存在(glibc 无此字段)
            #[cfg(target_os = "macos")]
            {
                a.sin_len = std::mem::size_of::<zt::sockaddr_in>() as u8;
            }
            a.sin_family = zt::AF_INET as _;
            a.sin_port = port;
            // octets 即目标内存字节序(网络序);from_ne_bytes 保证存回去不变
            set_in4_addr(&mut a.sin_addr, u32::from_ne_bytes(v4.octets()));
        },
        IpAddr::V6(v6) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut zt::sockaddr_in6);
            #[cfg(target_os = "macos")]
            {
                a.sin6_len = std::mem::size_of::<zt::sockaddr_in6>() as u8;
            }
            a.sin6_family = zt::AF_INET6 as _;
            a.sin6_port = port;
            set_in6_bytes(&mut a.sin6_addr, v6.octets());
        },
    }
    ss
}
