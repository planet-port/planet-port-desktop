//! 身份端点:GET /identity(模式与身份列表)、POST /identity/import
//! (官方身份导入,含停禁官方服务)、PUT /planets/{id}/identity(切换绑定)。
//! 语义见 docs/superpowers/specs/2026-11-02-identity-management-design.md。
//!
//! # 锁与线程纪律
//!
//! - registry 锁短临界区取克隆/改写后即放,不跨 await;
//! - 官方服务停用是子进程阻塞操作,一律 spawn_blocking(红线);
//! - 切换绑定要求 planet stopped:先取 runtime_gate 再判 runtime_of。
//!   仅查运行时表不够——start_planet 全程持 gate 且「先插 map 再置
//!   running=true」,启动窗口内 map 已有记录而 running 仍为 false,若此时
//!   放行改写 registry,实例会继续用旧身份而登记却是新身份;持 gate 后
//!   与并发 start 串行化,该半途态不再出现。

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
            // own 绑定读 dataDir 内 identity.public(显式 stop 后 last_node_id
            // 会被置 None,身份本体留盘可解析,列表不丢地址);都取不到回落
            // last_node_id,再无则 null。global 绑定即全局身份地址。
            let node_id = match e.identity {
                IdentityBinding::Own => fs::read(e.data_dir.join("identity.public"))
                    .ok()
                    .as_deref()
                    .and_then(identity_addr)
                    .or_else(|| e.last_node_id.clone()),
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
    // ZeroTier 身份串格式(vendored Core/node/Identity.cpp toString):
    // identity.public = 「addr:0:<pubhex>」3 段;identity.secret =
    // 「addr:0:<pubhex>:<privhex>」4 段。第 4 段(下标 3)是私钥段:
    // 误传 identity.public 时双文件同名同内容、地址校验全过,公钥一旦落盘会
    // 污染 config 根的 identity.secret,故此处校验确实含私钥段。
    if std::str::from_utf8(&secret)
        .ok()
        .and_then(|s| s.split(':').nth(3))
        .is_none_or(|priv_part| priv_part.trim().is_empty())
    {
        return Err(bad_request(
            "invalidIdentity",
            "file looks like identity.public, not identity.secret",
        ));
    }
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
    // 3.5 运行态守卫:global 绑定的 planet 仍在运行时导入(尤其 --force 覆盖)
    // 会出现「实例用旧地址、列表显新地址」的分裂,先拒绝并指引逐个 stop。
    // 判定 = runtime_of(实际运行)+ registry.running(期望状态,启动窗口内
    // runtime 表可能有空档)。锁纪律:短临界区——锁内只取 id/name/布尔即放,
    // runtime_of 另锁 planets 表,不嵌套持锁;全程无 await。
    let global_bound: Vec<(String, bool, String)> = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets
            .iter()
            .filter(|e| e.identity == IdentityBinding::Global)
            .map(|e| (e.id.clone(), e.running, crate::api_planet::name_of(e)))
            .collect()
    };
    let busy: Vec<String> = global_bound
        .into_iter()
        .filter(|(id, running, _)| *running || runtime_of(&st, id).is_some())
        .map(|(id, _, name)| format!("{name}(id {id})"))
        .collect();
    if !busy.is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            "globalIdentityInUse",
            &format!(
                "global identity is in use by running planet(s): {}; \
                 stop them first (`planet-port stop <planet>`) and retry",
                busy.join(", ")
            ),
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
    // 5. 写盘:secret/public 两文件必须成对生效。先各自写好不同名的 tmp
    // (不能用 with_extension("tmp"),它会把 identity.secret/public 都变成
    // identity.tmp 而互相覆盖),两个 tmp 都落定后再依次 rename;任一 rename
    // 失败清理两个 tmp 并中止,避免留下「新 secret 配旧 public」的半态。
    let tmp_for = |dest: &std::path::Path| -> PathBuf {
        let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("identity");
        dest.with_file_name(format!("{name}.tmp"))
    };
    let tmp_secret = tmp_for(&dest_secret);
    let tmp_public = tmp_for(&dest_public);
    let cleanup_tmps = || {
        let _ = fs::remove_file(&tmp_secret);
        let _ = fs::remove_file(&tmp_public);
    };
    if let Err(e) = fs::write(&tmp_secret, &secret) {
        cleanup_tmps();
        return Err(internal(format!("cannot write {}: {e}", tmp_secret.display())));
    }
    if let Err(e) = fs::write(&tmp_public, &public) {
        cleanup_tmps();
        return Err(internal(format!("cannot write {}: {e}", tmp_public.display())));
    }
    for (tmp, dest) in [(&tmp_secret, &dest_secret), (&tmp_public, &dest_public)] {
        if let Err(e) = fs::rename(tmp, dest) {
            cleanup_tmps();
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
    // 先取启停门闩(与 DELETE 对齐):start_planet 先插运行时表再置
    // running=true,启动窗口内只查运行时表会漏判运行中,持 gate 后判定
    // 与 registry 改写都被串行化,不会与并发 start 交错。门闩内不做 await。
    let _gate = st.runtime_gate.lock().await;
    // 运行中拒绝(临界区内无 await)
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
            // 半态可见:绑定已改在内存生效但未落盘,事件留痕供 events 排查
            eprintln!("[identity] registry 写盘失败: {e}");
            st.events
                .push(&id, "identitySwitchPersistFailed", &e.to_string());
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
