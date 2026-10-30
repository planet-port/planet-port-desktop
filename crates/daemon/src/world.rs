//! World(planet/moon)二进制格式解析。
//! 格式以 Core/node/World.hpp 的 `World::deserialize` 为准逐字段平移:
//! 类型(1 字节)| worldId(BE u64)| revision 时间戳(BE u64)| updatesMustBeSignedBy(64B)
//! | 签名(96B)| 根数(1B)| 每根 { identity + 稳定端点数(1B) + 端点列表 } | moon 附加字典(u16 + N)。
//! identity 段按 Core/node/Identity.hpp 的 `Identity::deserialize` 消费:
//! 地址(5B)+ 保留零字节(1B)+ 公钥(64B)+ 私钥长度标记(1B,0=无 / 64=有私钥)。
//! 端点按 Core/node/InetAddress.hpp 的 `InetAddress::deserialize`:
//! 0=占位(1B)、1/2=MAC 等前向兼容(7B)、3=扩展(1B + u16 长度 + N)、4=IPv4(7B)、6=IPv6(19B)。
//!
//! 解析通过即视为合法 planet 文件(import 的全部校验);本模块只提取
//! worldId / revision / 类型 / 各根稳定端点地址,identity 与签名不校验。

use crate::registry::Registry;

/// world 文件大小上限(与 switch-planet PlanetImporter 的 8MB 上限一致)
pub const MAX_WORLD_BYTES: usize = 8 * 1024 * 1024;

// Core/node/Constants.hpp 与 ECC.hpp(非 FIPS 分支)定义
const ZT_ADDRESS_LENGTH: usize = 5;
const ZT_ECC_PUBLIC_KEY_SET_LEN: usize = 64;
const ZT_ECC_PRIVATE_KEY_SET_LEN: usize = 64;
const ZT_ECC_SIGNATURE_LEN: usize = 96;

// World.hpp 的合理性上限
const ZT_WORLD_MAX_ROOTS: usize = 4;
const ZT_WORLD_MAX_STABLE_ENDPOINTS_PER_ROOT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldKind {
    /// World::TYPE_PLANET = 1
    Planet,
    /// World::TYPE_MOON = 127(brief 中约定的 2 也按 moon 接受)
    Moon,
}

#[derive(Debug, Clone)]
pub struct WorldInfo {
    pub world_id: u64,
    pub revision_ms: u64,
    pub kind: WorldKind,
    /// 各根的稳定端点,"ip:port"(IPv6 为 "[ip]:port")
    pub roots: Vec<String>,
}

/// 带中文名称的边界检查游标
struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }

    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self
            .p
            .checked_add(n)
            .ok_or_else(|| format!("{what}:长度溢出"))?;
        if end > self.b.len() {
            return Err(format!(
                "{what}:数据越界,需要 {n} 字节,剩余 {} 字节",
                self.b.len() - self.p
            ));
        }
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }

    fn u8(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }

    fn u16(&mut self, what: &str) -> Result<u16, String> {
        let s = self.take(2, what)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }

    fn u64(&mut self, what: &str) -> Result<u64, String> {
        let s = self.take(8, what)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_be_bytes(a))
    }
}

/// 解析单个稳定端点;返回 Some("ip:port") 或 None(非 IP 类端点,仅跳过)
fn parse_endpoint(r: &mut Reader) -> Result<Option<String>, String> {
    let family = r.u8("端点地址族")?;
    match family {
        // InetAddress::deserialize case 0:空端点占位,共 1 字节
        0x00 => Ok(None),
        // case 0x01/0x02:以太网/蓝牙地址,前向兼容,共 7 字节
        0x01 | 0x02 => {
            r.take(6, "端点地址")?;
            Ok(None)
        }
        // case 0x03:扩展地址类型,1B 族 + 2B 长度 + N 字节
        0x03 => {
            let n = r.u16("扩展端点长度")? as usize;
            r.take(n, "扩展端点数据")?;
            Ok(None)
        }
        // case 0x04:IPv4,4B 地址 + 2B 端口
        0x04 => {
            let ip = r.take(4, "IPv4 地址")?;
            let port = r.u16("端点端口")?;
            Ok(Some(format!("{}.{}.{}.{}:{}", ip[0], ip[1], ip[2], ip[3], port)))
        }
        // case 0x06:IPv6,16B 地址 + 2B 端口
        0x06 => {
            let ip = r.take(16, "IPv6 地址")?;
            let port = r.u16("端点端口")?;
            let mut a = [0u8; 16];
            a.copy_from_slice(ip);
            Ok(Some(format!("[{}]:{}", std::net::Ipv6Addr::from(a), port)))
        }
        other => Err(format!("非法端点地址族 {other}(应为 0/1/2/3/4/6)")),
    }
}

/// 解析单个根的 identity 段并跳过(消费字节数与 Identity::deserialize 一致)
fn skip_identity(r: &mut Reader, k: usize) -> Result<(), String> {
    r.take(ZT_ADDRESS_LENGTH, &format!("根 {k} identity 地址"))?;
    // Identity.hpp:`if (b[p++] != 0) throw` —— 保留标记字节必须为 0
    let flag = r.u8(&format!("根 {k} identity 标记字节"))?;
    if flag != 0 {
        return Err(format!("根 {k} identity 标记字节非零({flag}),不是公开 identity 序列化"));
    }
    r.take(ZT_ECC_PUBLIC_KEY_SET_LEN, &format!("根 {k} identity 公钥"))?;
    let priv_len = r.u8(&format!("根 {k} identity 私钥长度标记"))?;
    match priv_len {
        0 => {}
        x if x as usize == ZT_ECC_PRIVATE_KEY_SET_LEN => {
            r.take(ZT_ECC_PRIVATE_KEY_SET_LEN, &format!("根 {k} identity 私钥"))?;
        }
        other => {
            return Err(format!(
                "根 {k} identity 私钥长度标记非法({other},应为 0 或 64)"
            ))
        }
    }
    Ok(())
}

/// 解析 world 二进制;任何字段非法即返回 Err(中文描述)。上限 8MB。
pub fn parse_world(bytes: &[u8]) -> Result<WorldInfo, String> {
    if bytes.len() > MAX_WORLD_BYTES {
        return Err(format!(
            "world 文件过大:{} 字节(上限 {} 字节)",
            bytes.len(),
            MAX_WORLD_BYTES
        ));
    }
    if bytes.is_empty() {
        return Err("world 文件为空".into());
    }

    let mut r = Reader::new(bytes);

    let type_byte = r.u8("world 类型")?;
    let kind = match type_byte {
        1 => WorldKind::Planet,
        // World.hpp 定义 TYPE_MOON=127;brief 约定的 2 也归入 moon
        2 | 127 => WorldKind::Moon,
        other => {
            return Err(format!(
                "非法 world 类型 {other}(应为 1=planet 或 127=moon)"
            ))
        }
    };

    let world_id = r.u64("worldId")?;
    if world_id == 0 {
        return Err("worldId 不能为 0".into());
    }
    let revision_ms = r.u64("revision 时间戳")?;
    if revision_ms == 0 {
        return Err("revision 时间戳不能为 0".into());
    }

    r.take(ZT_ECC_PUBLIC_KEY_SET_LEN, "updatesMustBeSignedBy 公钥")?;
    r.take(ZT_ECC_SIGNATURE_LEN, "world 签名")?;

    let num_roots = r.u8("根节点数")? as usize;
    if num_roots == 0 {
        return Err("world 至少需要 1 个根节点".into());
    }
    if num_roots > ZT_WORLD_MAX_ROOTS {
        return Err(format!(
            "根节点数 {num_roots} 超过上限 {ZT_WORLD_MAX_ROOTS}"
        ));
    }

    let mut roots = Vec::new();
    for k in 0..num_roots {
        skip_identity(&mut r, k)?;
        let n_ep = r.u8(&format!("根 {k} 稳定端点数"))? as usize;
        if n_ep > ZT_WORLD_MAX_STABLE_ENDPOINTS_PER_ROOT {
            return Err(format!(
                "根 {k} 稳定端点数 {n_ep} 超过上限 {ZT_WORLD_MAX_STABLE_ENDPOINTS_PER_ROOT}"
            ));
        }
        for _ in 0..n_ep {
            if let Some(ep) = parse_endpoint(&mut r)? {
                roots.push(ep);
            }
        }
    }

    // moon 末尾的附加字典(16 位非包含长度),planet 无此段
    if kind == WorldKind::Moon {
        let dict_len = r.u16("moon 附加字典长度")? as usize;
        r.take(dict_len, "moon 附加字典")?;
    }

    // 与 World::deserialize 语义一致:尾部多余字节不视为错误
    Ok(WorldInfo {
        world_id,
        revision_ms,
        kind,
        roots,
    })
}

/// URL 下载的失败分类:TooLarge → 400 sourceTooLarge,Fetch → 400 sourceFetchFailed
#[derive(Debug)]
pub enum FetchError {
    /// 响应体超过 MAX_WORLD_BYTES
    TooLarge,
    /// 网络/HTTP 失败(带原因简述)
    Fetch(String),
}

/// 下载 URL 源的 planet 文件:整体超时 30s、跟随重定向(ureq 默认上限 10 跳)、
/// 响应体上限 MAX_WORLD_BYTES(超限即中断,不整块吃内存)。
/// 下载到的字节走与本地文件完全相同的 parse_world 校验(调用方负责)。
pub fn fetch_url(url: &str) -> Result<Vec<u8>, FetchError> {
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            // 非 2xx 不抛异常,统一由下方状态码分支转 sourceFetchFailed
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build(),
    );
    let mut resp = agent
        .get(url)
        .call()
        .map_err(|e| FetchError::Fetch(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(FetchError::Fetch(format!("HTTP {}", resp.status())));
    }
    // 上限 = MAX + 1:恰好 MAX 字节可读完,再多读 1 字节即 BodyExceedsLimit
    match resp
        .body_mut()
        .with_config()
        .limit(MAX_WORLD_BYTES as u64 + 1)
        .read_to_vec()
    {
        Ok(bytes) if bytes.len() > MAX_WORLD_BYTES => Err(FetchError::TooLarge),
        Ok(bytes) => Ok(bytes),
        Err(ureq::Error::BodyExceedsLimit(_)) => Err(FetchError::TooLarge),
        Err(e) => Err(FetchError::Fetch(e.to_string())),
    }
}

/// planet 定位:依次尝试 id 精确 → name(不区分大小写)→ 按 u64 解析匹配
/// world_id(十进制,或 0x 前缀 / 含字母的纯十六进制)
pub fn resolve_planet_id(registry: &Registry, input: &str) -> Option<String> {
    if let Some(e) = registry.planets.iter().find(|e| e.id == input) {
        return Some(e.id.clone());
    }
    if let Some(e) = registry
        .planets
        .iter()
        .find(|e| e.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(input)))
    {
        return Some(e.id.clone());
    }
    let parsed = if let Some(hex) = input.strip_prefix("0x").or_else(|| input.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else if !input.is_empty() && input.chars().all(|c| c.is_ascii_digit()) {
        input.parse::<u64>().ok()
    } else if input.len() > 1
        && input.chars().all(|c| c.is_ascii_hexdigit())
        && input.chars().any(|c| c.is_ascii_alphabetic())
    {
        u64::from_str_radix(input, 16).ok()
    } else {
        None
    };
    parsed.and_then(|w| {
        registry
            .planets
            .iter()
            .find(|e| e.world_id == w)
            .map(|e| e.id.clone())
    })
}
