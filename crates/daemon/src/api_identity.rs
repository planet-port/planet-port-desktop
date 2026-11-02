//! 身份端点:GET /identity(模式与身份列表)、POST /identity/import
//! (官方身份导入,含停禁官方服务)、PUT /planets/{id}/identity(切换绑定)。
//! 语义见 docs/superpowers/specs/2026-11-02-identity-management-design.md。
//!
//! # 锁与线程纪律
//!
//! - registry 锁短临界区取克隆/改写后即放,不跨 await;
//! - 官方服务停用是子进程阻塞操作,一律 spawn_blocking(红线);
//! - 切换绑定要求 planet stopped:registry.running 与 planets 运行时表都查。

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde_json::{json, Value};

use crate::registry::IdentityBinding;
use crate::service_control::{self, OfficialServiceAction};
use crate::state::{err, runtime_of, AppState};

type ApiErr = (StatusCode, Json<planet_port_api::types::ApiError>);

fn internal(msg: String) -> ApiErr {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internalError", &msg, None)
}

fn bad_request(code: &str, msg: &str) -> ApiErr {
    err(StatusCode::BAD_REQUEST, code, msg, None)
}

/// identity.secret/public 首字段(冒号前)即十位 hex 节点地址
fn identity_addr(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    s.split(':')
        .next()
        .map(|a| a.trim().to_lowercase())
        .filter(|a| a.len() == 10 && a.chars().all(|c| c.is_ascii_hexdigit()))
}

/// 官方客户端数据目录(以 identity.secret 存在为准);未安装返回 None
fn official_dir() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ZeroTier")
    } else if cfg!(target_os = "linux") {
        PathBuf::from("/var/lib/zerotier-one")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\ProgramData\ZeroTier\One")
    } else {
        return None;
    };
    dir.join("identity.secret").exists().then_some(dir)
}

/// 全局身份节点地址:优先 identity.public,回落 identity.secret
fn global_node_id(config_dir: &std::path::Path) -> Option<String> {
    fs::read(config_dir.join("identity.public"))
        .ok()
        .as_deref()
        .and_then(identity_addr)
        .or_else(|| {
            fs::read(config_dir.join("identity.secret"))
                .ok()
                .as_deref()
                .and_then(identity_addr)
        })
}

fn binding_str(b: IdentityBinding) -> &'static str {
    match b {
        IdentityBinding::Own => "own",
        IdentityBinding::Global => "global",
    }
}

// ---- GET /identity ----

pub async fn identity_list(State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    let entries = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets.clone()
    };
    let exists = st.config_dir.join("identity.secret").exists();
    let g_node = global_node_id(&st.config_dir);
    let planets: Vec<Value> = entries
        .iter()
        .map(|e| {
            // own 绑定看自己的 last_node_id;global 绑定即全局身份地址
            let node_id = match e.identity {
                IdentityBinding::Own => e.last_node_id.clone(),
                IdentityBinding::Global => g_node.clone(),
            };
            json!({
                "id": e.id,
                "name": crate::api_planet::name_of(e),
                "identity": binding_str(e.identity),
                "nodeId": node_id,
                "running": e.running,
            })
        })
        .collect();
    Ok(Json(json!({
        "global": { "exists": exists, "nodeId": g_node },
        "planets": planets,
    })))
}

// ---- POST /identity/import ----

pub async fn identity_import(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let force = body.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    // 1. 定位身份文件:显式 path(identity.secret 路径或其目录)或自动探测
    let secret_path = match body.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => {
            let pb = PathBuf::from(p);
            if pb.is_dir() { pb.join("identity.secret") } else { pb }
        }
        _ => official_dir()
            .ok_or_else(|| {
                bad_request(
                    "officialNotFound",
                    "cannot detect official ZeroTier data dir; pass \"path\"",
                )
            })?
            .join("identity.secret"),
    };
    let public_path = secret_path.with_file_name("identity.public");
    // 2. 校验:两文件都在、地址可解析且一致
    let secret = fs::read(&secret_path).map_err(|e| {
        bad_request(
            "sourceUnreadable",
            &format!("cannot read {}: {e}", secret_path.display()),
        )
    })?;
    let public = fs::read(&public_path).map_err(|e| {
        bad_request(
            "sourceUnreadable",
            &format!("cannot read {}: {e}", public_path.display()),
        )
    })?;
    let addr_s = identity_addr(&secret)
        .ok_or_else(|| bad_request("invalidIdentity", "cannot parse identity.secret address"))?;
    let addr_p = identity_addr(&public)
        .ok_or_else(|| bad_request("invalidIdentity", "cannot parse identity.public address"))?;
    if addr_s != addr_p {
        return Err(bad_request(
            "invalidIdentity",
            "identity.secret and identity.public addresses do not match",
        ));
    }
    // 3. 覆盖保护:已有全局身份且未 force → 拒绝
    let dest_secret = st.config_dir.join("identity.secret");
    let dest_public = st.config_dir.join("identity.public");
    if dest_secret.exists() && !force {
        return Err(err(
            StatusCode::CONFLICT,
            "identityExists",
            "global identity already exists; pass force to overwrite",
            None,
        ));
    }
    // 4. 停+禁官方服务(子进程,放阻塞线程池);失败即中止导入
    let action = tokio::task::spawn_blocking(service_control::stop_and_disable)
        .await
        .map_err(|e| internal(format!("service stop 任务失败: {e}")))?;
    if let OfficialServiceAction::Failed(e) = &action {
        return Err(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "officialServiceStopFailed",
            &format!(
                "cannot stop official ZeroTier service ({e}); import aborted, \
                 stop it manually and retry"
            ),
            None,
        ));
    }
    // 5. 写盘(先 tmp 后 rename,对齐 StateStore/registry 原子写模式)
    for (dest, bytes) in [(&dest_secret, &secret), (&dest_public, &public)] {
        let tmp = dest.with_extension("tmp");
        fs::write(&tmp, bytes)
            .map_err(|e| internal(format!("cannot write {}: {e}", tmp.display())))?;
        if let Err(e) = fs::rename(&tmp, dest) {
            let _ = fs::remove_file(&tmp);
            return Err(internal(format!("cannot rename to {}: {e}", dest.display())));
        }
        #[cfg(unix)]
        let _ = fs::set_permissions(dest, fs::Permissions::from_mode(0o600));
    }
    st.events
        .push("global", "identityImported", &format!("global identity imported: {addr_s}"));
    let action_str = match action {
        OfficialServiceAction::StoppedAndDisabled => "stoppedAndDisabled",
        OfficialServiceAction::NotInstalled => "notInstalled",
        OfficialServiceAction::Failed(_) => unreachable!("failed 已在上方拦截"),
    };
    Ok((
        StatusCode::CREATED,
        Json(json!({ "nodeId": addr_s, "serviceAction": action_str })),
    ))
}

// ---- PUT /planets/{id}/identity ----

pub async fn planet_identity_set(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    let binding = match body.get("identity").and_then(|v| v.as_str()) {
        Some("own") => IdentityBinding::Own,
        Some("global") => IdentityBinding::Global,
        _ => {
            return Err(bad_request(
                "badRequest",
                "body must contain \"identity\": \"own\" | \"global\"",
            ))
        }
    };
    // 运行中拒绝:registry 标志与运行时表双查(与 DELETE 同款防御)
    if runtime_of(&st, &id).is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "planetRunning",
            &format!("planet \"{id}\" is running; stop it before switching identity"),
            None,
        ));
    }
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(e) = reg.planets.iter_mut().find(|e| e.id == id) else {
            return Err(err(
                StatusCode::NOT_FOUND,
                "planetNotFound",
                &format!("planet \"{id}\" not found"),
                None,
            ));
        };
        e.identity = binding;
        if let Err(e) = reg.save(&st.config_dir) {
            eprintln!("[identity] registry 写盘失败: {e}");
        }
    }
    st.events.push(
        &id,
        "identitySwitch",
        &format!("identity binding switched to {}", binding_str(binding)),
    );
    Ok(Json(json!({ "result": "ok", "id": id, "identity": binding_str(binding) })))
}

/// 身份路由:/identity、/identity/import、/planets/{id}/identity
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/identity", get(identity_list))
        .route("/identity/import", post(identity_import))
        .route("/planets/{id}/identity", put(planet_identity_set))
}
