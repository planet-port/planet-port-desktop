pub mod misc;
pub mod mutate;
pub mod planet;
pub mod query;
pub mod service;

use crate::client::{CliError, Client};
use planet_port_api::endpoints;
use planet_port_api::types::{ApiError, PlanetStatus};
use serde_json::Value;

/// /status 的 planets 全量(含 stopped);daemon 不可达返回 None,调用方自行回退
fn statuses(client: &Client) -> Option<Vec<PlanetStatus>> {
    let v = client.get_json(endpoints::status()).ok()?;
    Some(
        serde_json::from_value(v.get("planets").cloned().unwrap_or(Value::Null))
            .unwrap_or_default(),
    )
}

/// 变更回执的 planet 落点 `name(id)`:显式 flag 按 id/name 匹配,无 flag 时
/// 单 running 自动选。拿不到(离线/多 running)返回 None,回执退化为无落点样式
pub fn planet_label(client: &Client, flag: &Option<String>) -> Option<String> {
    let rows = statuses(client)?;
    if let Some(f) = flag {
        let p = rows
            .into_iter()
            .find(|p| p.id == *f || p.name.eq_ignore_ascii_case(f))?;
        return Some(format!("{}({})", p.name, p.id));
    }
    let mut running = rows.into_iter().filter(|p| p.status == "running");
    let one = running.next()?;
    running
        .next()
        .is_none()
        .then(|| format!("{}({})", one.name, one.id))
}

/// 单目标解析:显式 flag 命中登记项则校验 running(镜像 daemon 变更端点的
/// 409 planetNotRunning;未命中的输入(如 worldId)透传给 daemon 判定);
/// 无 flag 时按**登记数**自动选——登记 1 个即选中(stopped 则报
/// planetNotRunning),登记 ≥2 报 planetAmbiguous,不看连接状态。
/// 供 CLI 侧自行取数的命令用:dump -j / get / network show
pub fn resolve_single(client: &Client, flag: &Option<String>) -> Result<String, CliError> {
    let rows = statuses(client).ok_or(CliError::Offline)?;
    if let Some(f) = flag {
        if let Some(p) = rows
            .iter()
            .find(|p| p.id == *f || p.name.eq_ignore_ascii_case(f))
        {
            if p.status != "running" {
                return Err(CliError::Api(
                    409,
                    ApiError {
                        error: "planetNotRunning".into(),
                        message: format!("planet \"{}\" is not running", p.name),
                        candidates: None,
                    },
                ));
            }
            return Ok(p.id.clone());
        }
        return Ok(f.clone());
    }
    // 自动选择按**登记数**计,不看连接状态:登记 1 个(即使未启用)自动选中,
    // 由上面的 running 校验兜底报 planetNotRunning;登记 ≥2 一律要求显式指定
    match rows.as_slice() {
        [one] if one.status != "running" => Err(CliError::Api(
            409,
            ApiError {
                error: "planetNotRunning".into(),
                message: format!("planet \"{}\" is not running", one.name),
                candidates: None,
            },
        )),
        [one] => Ok(one.id.clone()),
        [] => Err(CliError::Api(
            409,
            ApiError {
                error: "noPlanetRunning".into(),
                message: "no planet registered".into(),
                candidates: None,
            },
        )),
        many => Err(CliError::Api(
            409,
            ApiError {
                error: "planetAmbiguous".into(),
                message: "multiple planets registered".into(),
                candidates: Some(
                    many.iter()
                        .map(|p| {
                            let state = if p.status == "running" { "running" } else { "stopped" };
                            format!("{} ({}, {})", p.name, p.id, state)
                        })
                        .collect(),
                ),
            },
        )),
    }
}
