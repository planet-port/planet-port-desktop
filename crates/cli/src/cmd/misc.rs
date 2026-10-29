use crate::client::{CliError, Client};
use crate::output::{new_table, print_json};
use planet_port_api::endpoints;
use planet_port_api::types::{EventEntry, PlanetStatus};
use serde_json::Value;
use std::thread::sleep;
use std::time::Duration;

pub fn use_planet(client: &Client, planet: &str) -> Result<(), CliError> {
    let v = client.get_json(endpoints::planets())?;
    let rows: Vec<PlanetStatus> = serde_json::from_value(
        v.get("planets").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let found = rows
        .iter()
        .find(|p| p.id == planet || p.name == planet)
        .ok_or_else(|| CliError::Usage(format!("planet \"{planet}\" not found")))?;
    // stdout 只输出 export 行(供 eval);提示走 stderr
    println!("export PLANET_PORT_PLANET={}", found.id);
    eprintln!("# default planet set to \"{}\" (this shell only)", found.name);
    eprintln!("# usage: eval $(planet-port use {})", found.name);
    Ok(())
}

pub fn events(
    client: &Client,
    planet: &Option<String>,
    limit: usize,
    follow: bool,
    json: bool,
) -> Result<(), CliError> {
    if !follow {
        let v = client.get_json(&endpoints::events(planet.as_deref(), limit, None))?;
        if json {
            print_json(&v);
            return Ok(());
        }
        render(&v);
        return Ok(());
    }
    // --follow:2s 轮询,按 ts 游标增量输出;走无超时轮询路径,daemon 偶发阻塞不中断
    let mut after: Option<u64> = None;
    loop {
        let v = client.get_json_polling(&endpoints::events(planet.as_deref(), limit, after))?;
        let entries: Vec<EventEntry> = serde_json::from_value(
            v.get("events").cloned().unwrap_or(Value::Null),
        )
        .unwrap_or_default();
        if let Some(last) = entries.iter().map(|e| e.ts_ms).max() {
            after = Some(last);
        }
        if json {
            print_json(&v);
        } else if !entries.is_empty() {
            render(&v);
        }
        sleep(Duration::from_secs(2));
    }
}

fn render(v: &Value) {
    let entries: Vec<EventEntry> = serde_json::from_value(
        v.get("events").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    let mut t = new_table(&["TS", "PLANET", "KIND", "MESSAGE"]);
    for e in entries {
        t.add_row(vec![e.ts_ms.to_string(), e.planet, e.kind, e.message]);
    }
    println!("{t}");
}
