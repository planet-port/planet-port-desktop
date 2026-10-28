use crate::client::{CliError, Client};
use crate::output::print_json;
use clap::Subcommand;
use planet_port_api::endpoints;
use serde_json::{json, Value};

/// 变更类命令:成功打印 "<code> <verb> OK";-j 输出响应体
fn ok(code: u16, verb: &str, json: bool, body: &Value) {
    if json {
        print_json(body);
    } else {
        println!("{code} {verb} OK");
    }
}

pub fn join(client: &Client, nwid: &str, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let _ = client.post_empty(&endpoints::network_join(nwid, planet.as_deref()))?;
    ok(200, "join", json, &Value::Null);
    Ok(())
}

pub fn leave(client: &Client, nwid: &str, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let _ = client.post_empty(&endpoints::network_leave(nwid, planet.as_deref()))?;
    ok(200, "leave", json, &Value::Null);
    Ok(())
}

pub fn set(
    client: &Client,
    nwid: &str,
    values: &[String],
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    if values.is_empty() {
        return Err(CliError::Usage(
            "set requires at least one key=value (routeViaZeroTier, dnsMode, customDns)".into(),
        ));
    }
    let mut body = serde_json::Map::new();
    for v in values {
        let (k, val) = v
            .split_once('=')
            .ok_or_else(|| CliError::Usage(format!("expected key=value, got \"{v}\"")))?;
        match k {
            "routeViaZeroTier" => {
                if !matches!(val, "0" | "1") {
                    return Err(CliError::Usage(format!(
                        "routeViaZeroTier must be 0|1, got \"{val}\""
                    )));
                }
                body.insert(k.into(), json!(val == "1"));
            }
            "dnsMode" => {
                if !matches!(val, "none" | "network" | "custom") {
                    return Err(CliError::Usage(format!(
                        "dnsMode must be none|network|custom, got \"{val}\""
                    )));
                }
                body.insert(k.into(), json!(val));
            }
            "customDns" => {
                let list: Vec<String> = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                body.insert(k.into(), json!(list));
            }
            _ => {
                return Err(CliError::Usage(format!(
                    "unknown key \"{k}\"; valid: routeViaZeroTier, dnsMode, customDns"
                )));
            }
        }
    }
    let resp = client.post_json(&endpoints::network_set(nwid, planet.as_deref()), &Value::Object(body))?;
    ok(200, "set", json, &resp);
    Ok(())
}

pub fn orbit(
    client: &Client,
    moon_id: &str,
    seed: &str,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    let body = json!({ "seed": seed });
    let _ = client.post_json(&endpoints::moon_orbit(moon_id, planet.as_deref()), &body)?;
    ok(200, "orbit", json, &Value::Null);
    Ok(())
}

pub fn deorbit(client: &Client, moon_id: &str, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    let _ = client.delete(&endpoints::moon_deorbit(moon_id, planet.as_deref()))?;
    ok(200, "deorbit", json, &Value::Null);
    Ok(())
}

pub fn dump(client: &Client, planet: &Option<String>, json: bool) -> Result<(), CliError> {
    // /dump 契约返回纯文本,须按文本读取;JSON 模式包装为字符串输出
    let s = client.get_text(&endpoints::dump(planet.as_deref()))?;
    if json {
        print_json(&json!(s));
    } else {
        println!("{s}");
    }
    Ok(())
}

#[derive(Subcommand)]
pub enum MoonCmd {
    /// Import a .moon file
    Import { source: String },
}

pub fn run_moon(
    client: &Client,
    cmd: &MoonCmd,
    planet: &Option<String>,
    json: bool,
) -> Result<(), CliError> {
    match cmd {
        MoonCmd::Import { source } => {
            let body = json!({ "source": source });
            let resp = client.post_json(&endpoints::moons(planet.as_deref()), &body)?;
            ok(201, "moon import", json, &resp);
            Ok(())
        }
    }
}
