fn q(planet: Option<&str>) -> String {
    match planet {
        Some(p) => format!("?planet={}", enc(p)),
        None => String::new(),
    }
}

/// RFC3986 percent-encode(仅保留 unreserved:A-Za-z0-9 - _ . ~)。
/// planet 别名与网络名可含空格/CJK,进 path 段或 query 值都必须转义;
/// 不引第三方 crate,实现仅 10 行
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn version() -> &'static str {
    "/version"
}
pub fn status() -> &'static str {
    "/status"
}
pub fn planets() -> &'static str {
    "/planets"
}
pub fn planet(id: &str) -> String {
    format!("/planets/{}", enc(id))
}
pub fn planet_export(id: &str) -> String {
    format!("/planets/{}/export", enc(id))
}
pub fn planet_share(id: &str) -> String {
    format!("/planets/{}/share", enc(id))
}
pub fn planet_action(id: &str, action: &str) -> String {
    format!("/planets/{}/{}", enc(id), action)
}
pub fn networks(planet: Option<&str>) -> String {
    format!("/networks{}", q(planet))
}
pub fn network_join(nwid: &str, planet: Option<&str>) -> String {
    format!("/networks/{}/join{}", enc(nwid), q(planet))
}
pub fn network_leave(nwid: &str, planet: Option<&str>) -> String {
    format!("/networks/{}/leave{}", enc(nwid), q(planet))
}
pub fn network_set(nwid: &str, planet: Option<&str>) -> String {
    format!("/networks/{}{}", enc(nwid), q(planet))
}
pub fn peers(planet: Option<&str>) -> String {
    format!("/peers{}", q(planet))
}
pub fn moons(planet: Option<&str>) -> String {
    format!("/moons{}", q(planet))
}
pub fn moon_orbit(moon_id: &str, planet: Option<&str>) -> String {
    format!("/moons/{moon_id}/orbit{}", q(planet))
}
pub fn moon_deorbit(moon_id: &str, planet: Option<&str>) -> String {
    format!("/moons/{moon_id}{}", q(planet))
}
pub fn events(planet: Option<&str>, limit: usize, after_ms: Option<u64>) -> String {
    let mut s = format!("/events?limit={limit}");
    if let Some(p) = planet {
        s.push_str(&format!("&planet={p}"));
    }
    if let Some(a) = after_ms {
        s.push_str(&format!("&after={a}"));
    }
    s
}
pub fn identity() -> &'static str {
    "/identity"
}
pub fn identity_import() -> &'static str {
    "/identity/import"
}
pub fn planet_identity(id: &str) -> String {
    format!("/planets/{}/identity", enc(id))
}
pub fn dump(planet: Option<&str>) -> String {
    format!("/dump{}", q(planet))
}
