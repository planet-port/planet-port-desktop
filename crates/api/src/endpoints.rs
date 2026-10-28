fn q(planet: Option<&str>) -> String {
    match planet {
        Some(p) => format!("?planet={p}"),
        None => String::new(),
    }
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
    format!("/planets/{id}")
}
pub fn planet_export(id: &str) -> String {
    format!("/planets/{id}/export")
}
pub fn planet_share(id: &str) -> String {
    format!("/planets/{id}/share")
}
pub fn planet_action(id: &str, action: &str) -> String {
    format!("/planets/{id}/{action}")
}
pub fn networks(planet: Option<&str>) -> String {
    format!("/networks{}", q(planet))
}
pub fn network_join(nwid: &str, planet: Option<&str>) -> String {
    format!("/networks/{nwid}/join{}", q(planet))
}
pub fn network_leave(nwid: &str, planet: Option<&str>) -> String {
    format!("/networks/{nwid}/leave{}", q(planet))
}
pub fn network_set(nwid: &str, planet: Option<&str>) -> String {
    format!("/networks/{nwid}{}", q(planet))
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
pub fn dump(planet: Option<&str>) -> String {
    format!("/dump{}", q(planet))
}
