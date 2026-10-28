//! daemon 共享状态与统一错误体构造(Task 2 定义的 AppState 随 Task 3 扩展)。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum::Json;
use planet_port_api::types::ApiError;

use crate::events;
use crate::registry::Registry;

/// 每 planet 实例的运行时信息(Task 4 定义真实字段;当前仅占位撑起 AppState)
#[allow(dead_code)]
pub struct PlanetRuntime;

/// 一条分享链接记录:token → (planet id, 过期时间 unix 秒)
pub struct ShareRecord {
    pub planet_id: String,
    pub expires_unix: u64,
}

#[derive(Clone)]
pub struct AppState {
    /// 运行时配置目录(--config-dir / env / 三级解析结果)
    pub config_dir: PathBuf,
    /// daemon 实际监听的 API 端口(share 链接 url 需要它)
    pub api_port: u16,
    pub registry: Arc<Mutex<Registry>>,
    pub events: Arc<events::Ring>,
    /// planet id → 运行时(Task 4 起填充;当前恒为空)
    #[allow(dead_code)]
    pub planets: Arc<Mutex<HashMap<String, PlanetRuntime>>>,
    /// token → (planet id, 过期 unix 秒);仅内存,重启即失效
    pub shares: Arc<Mutex<HashMap<String, ShareRecord>>>,
    pub token: Arc<String>,
}

/// 统一错误响应体 `{"error","message","candidates"}`(形态与 mockd 完全一致)
pub fn err(
    code: StatusCode,
    error: &str,
    message: &str,
    candidates: Option<Vec<String>>,
) -> (StatusCode, Json<ApiError>) {
    (
        code,
        Json(ApiError {
            error: error.to_string(),
            message: message.to_string(),
            candidates,
        }),
    )
}
