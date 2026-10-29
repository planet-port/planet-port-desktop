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
    filter: &Option<Vec<String>>,
    limit: usize,
    follow: bool,
    json: bool,
) -> Result<(), CliError> {
    // 过滤集多成员时逐 planet 请求并按 ts 合并;单成员/全量走单次请求
    let fetch = |after: Option<u64>, polling: bool| -> Result<Value, CliError> {
        let one = |id: Option<&str>| {
            if polling {
                client.get_json_polling(&endpoints::events(id, limit, after))
            } else {
                client.get_json(&endpoints::events(id, limit, after))
            }
        };
        match filter {
            None => one(None),
            Some(ids) if ids.len() == 1 => one(Some(&ids[0])),
            Some(ids) => {
                let mut merged: Vec<Value> = Vec::new();
                for id in ids {
                    let v = one(Some(id))?;
                    merged.extend(
                        v.get("events")
                            .and_then(|x| x.as_array())
                            .cloned()
                            .unwrap_or_default(),
                    );
                }
                merged.sort_by_key(|e| e.get("ts_ms").and_then(Value::as_u64).unwrap_or(0));
                Ok(serde_json::json!({ "events": merged }))
            }
        }
    };
    if !follow {
        let v = fetch(None, false)?;
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
        let v = fetch(after, true)?;
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
