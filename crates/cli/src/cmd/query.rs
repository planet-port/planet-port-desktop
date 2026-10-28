use crate::client::{CliError, Client};
use crate::output::{new_table, print_json};
use planet_port_api::endpoints;
use planet_port_api::types::{MoonEntry, NetworkEntry, PeerEntry, PlanetStatus};
use serde_json::Value;

fn dash<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "—".into())
}

pub fn info(client: &Client, json: bool) -> Result<(), CliError> {
    let v = client.get_json(endpoints::status())?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<PlanetStatus> = serde_json::from_value(
        v.get("planets").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let mut t = new_table(&["PLANET", "STATUS", "ADDRESS", "VERSION", "LATENCY"]);
    for p in rows {
        let status = if p.status != "running" {
            "stopped".to_string()
        } else if p.online {
            "ONLINE".to_string()
        } else {
            "OFFLINE".to_string()
        };
        let latency = p
            .latency_ms
            .map(|l| format!("{l}ms"))
            .unwrap_or_else(|| "—".into());
        t.add_row(vec![p.name, status, dash(p.node_id), dash(p.core_version), latency]);
    }
    println!("{t}");
    Ok(())
}

pub fn listnetworks(client: &Client, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let v = client.get_json(&endpoints::networks(planet.as_deref()))?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<NetworkEntry> = serde_json::from_value(
        v.get("networks").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let mut t = new_table(&["PLANET", "NETWORK", "NAME", "STATUS", "TYPE", "MANAGED-IPS"]);
    for n in rows {
        let mut ips = n.assigned_addrs.iter().take(2).cloned().collect::<Vec<_>>();
        if n.assigned_addrs.len() > 2 {
            ips.push("…".into());
        }
        t.add_row(vec![
            n.planet,
            n.nwid,
            n.name.unwrap_or_else(|| "—".into()),
            n.status,
            n.kind,
            ips.join(", "),
        ]);
    }
    println!("{t}");
    Ok(())
}

pub fn listpeers(client: &Client, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let v = client.get_json(&endpoints::peers(planet.as_deref()))?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<PeerEntry> = serde_json::from_value(
        v.get("peers").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    // 按 planet 分组展示
    let mut order: Vec<String> = Vec::new();
    for p in &rows {
        if !order.contains(&p.planet) {
            order.push(p.planet.clone());
        }
    }
    for pid in order {
        println!("planet: {pid}");
        let mut t = new_table(&["ROLE", "ADDRESS", "LATENCY", "VERSION", "PATH"]);
        for p in rows.iter().filter(|p| p.planet == pid) {
            let latency = p
                .latency_ms
                .map(|l| format!("{l}ms"))
                .unwrap_or_else(|| "—".into());
            t.add_row(vec![
                p.role.clone(),
                p.address.clone(),
                latency,
                dash(p.version.clone()),
                p.path.clone().unwrap_or_else(|| "(relay)".into()),
            ]);
        }
        println!("{t}\n");
    }
    Ok(())
}

pub fn listmoons(client: &Client, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let v = client.get_json(&endpoints::moons(planet.as_deref()))?;
    if json {
        print_json(&v);
        return Ok(());
    }
    let rows: Vec<MoonEntry> = serde_json::from_value(
        v.get("moons").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let mut t = new_table(&["PLANET", "MOONID", "SEED", "SOURCE", "CACHED"]);
    for m in rows {
        t.add_row(vec![
            m.planet,
            m.moon_id,
            m.seed.unwrap_or_else(|| "—".into()),
            if m.from_file { "file" } else { "manual" }.into(),
            if m.cached { "yes" } else { "no" }.into(),
        ]);
    }
    println!("{t}");
    Ok(())
}
