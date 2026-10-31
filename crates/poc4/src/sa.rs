//! sockaddr_storage <-> SocketAddr(winsock 布局:无 sin_len,in6_addr 是 u.Byte)。
//! 语义与 zt-core/sa.rs 相同;差异点即 Windows 适配面,见 Task 3 Step 3 实测记录。
//!
//! 绑定形态(bindgen on x86_64-pc-windows-msvc,ZeroTierOne.h → windows 头):
//! - `ss_family`/`sin_family`/`sin6_family` 均为 `ADDRESS_FAMILY`(u16),无 sin_len;
//! - `in6_addr.u.Byte` 为 `[u8; 16]`(union,读取需 unsafe);
//! - `in_addr.S_un.S_addr` 为 `ULONG`(union 字段,注意是 `S_addr` 大写 S,
//!   非 BSD/glibc 的 `s_addr`)。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use zt_sys as zt;

/// 读 in6_addr 的 16 字节(内存序即网络序)。union 字段访问需 unsafe。
fn in6_bytes(a: &zt::in6_addr) -> [u8; 16] {
    unsafe { a.u.Byte }
}

/// 写 in6_addr 的 16 字节(union 字段写入是安全操作,读取才需 unsafe)。
fn set_in6_bytes(dst: &mut zt::in6_addr, b: [u8; 16]) {
    dst.u.Byte = b
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
                    // S_un.S_addr 的内存字节即网络序;to_ne_bytes 按内存原序取回
                    // (LE 上等价于逐字节读,不可用 to_be_bytes 二次反转)
                    // (union 字段,外层 unsafe 块已覆盖)
                    IpAddr::V4(Ipv4Addr::from(a.sin_addr.S_un.S_addr.to_ne_bytes())),
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
/// `port_override`:收发物理包传真实端口;网络条目传前缀长度(核心编码在 sin_port)。
pub fn to_sockaddr_storage(addr: &SocketAddr, port_override: Option<u16>) -> zt::sockaddr_storage {
    let mut ss: zt::sockaddr_storage = unsafe { std::mem::zeroed() };
    let port = port_override.unwrap_or(addr.port()).to_be();
    match addr.ip() {
        IpAddr::V4(v4) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut zt::sockaddr_in);
            a.sin_family = zt::AF_INET as _;
            a.sin_port = port;
            // octets 即目标内存字节序(网络序);from_ne_bytes 保证存回去不变
            a.sin_addr.S_un.S_addr = u32::from_ne_bytes(v4.octets());
        },
        IpAddr::V6(v6) => unsafe {
            let a = &mut *(&mut ss as *mut _ as *mut zt::sockaddr_in6);
            a.sin6_family = zt::AF_INET6 as _;
            a.sin6_port = port;
            set_in6_bytes(&mut a.sin6_addr, v6.octets());
        },
    }
    ss
}
