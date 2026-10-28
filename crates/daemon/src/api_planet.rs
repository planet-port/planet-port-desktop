//! planet 生命周期端点:import / show / patch / delete / start / stop / export / share。
//! 错误码、错误体(`{"error","message","candidates"}`)与响应结构(`{"planet": PlanetDetail}`、
//! 201 CREATED 等)对齐 crates/mockd 的同名 handler,保证 CLI 双端互通。

use std::fs;
use std::io::Read;
use std::net::{Ipv4Addr, UdpSocket};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};

/// handlers 统一的错误类型(与 mockd 一致,体积小,避免 clippy result_large_err)
type ApiErr = (StatusCode, Json<planet_port_api::types::ApiError>);
use planet_port_api::types::{PlanetDetail, PlanetKind, PlanetStatus, ShareLink};
use serde_json::{json, Value};

use crate::registry::{Registry, RegistryEntry};
use crate::state::{err, AppState, ShareRecord};
use crate::world;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// 从 /dev/urandom 取 n 字节并 hex 编码为 2n 字符(不引入 getrandom 依赖)
fn random_hex(n: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; n];
    fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

fn not_found(id: &str) -> ApiErr {
    err(
        StatusCode::NOT_FOUND,
        "planetNotFound",
        &format!("planet \"{id}\" not found"),
        None,
    )
}

// ---- 投影 ----

/// 列表项投影:无运行时信息,一律 online=false、core_version/latency/node_id 不下发
/// (与 mockd to_status 的 offline 分支形态一致);source=builtin 视为官方 planet
pub(crate) fn to_status(e: &RegistryEntry) -> PlanetStatus {
    PlanetStatus {
        id: e.id.clone(),
        name: e.name.clone().unwrap_or_else(|| e.id.clone()),
        icon: e.icon.clone().unwrap_or_default(),
        color: e.color.clone().unwrap_or_default(),
        kind: if e.source == "builtin" {
            PlanetKind::Official
        } else {
            PlanetKind::Custom
        },
        status: if e.running { "running" } else { "stopped" }.into(),
        online: false,
        node_id: None,
        core_version: None,
        latency_ms: None,
        network_count: e.net_local.len(),
        port: e.port,
    }
}

/// 详情投影:roots 来自导入时解析的 world 文件
fn to_detail(e: &RegistryEntry) -> PlanetDetail {
    PlanetDetail {
        id: e.id.clone(),
        name: e.name.clone().unwrap_or_else(|| e.id.clone()),
        icon: e.icon.clone().unwrap_or_default(),
        color: e.color.clone().unwrap_or_default(),
        kind: if e.source == "builtin" {
            PlanetKind::Official
        } else {
            PlanetKind::Custom
        },
        world_id: e.world_id,
        roots: e.roots.clone(),
        file_size: e.file_size,
        revision_time_ms: e.revision_time_ms,
        last_node_id: e.last_node_id.clone(),
        source: e.source.clone(),
        network_count: e.net_local.len(),
        port: e.port,
        status: if e.running { "running" } else { "stopped" }.into(),
        online: false,
    }
}

fn name_of(e: &RegistryEntry) -> String {
    e.name.clone().unwrap_or_else(|| e.id.clone())
}

/// 路径/查询参数 → canonical id(id / name / worldId 数值均可),未命中 404 planetNotFound
fn find_entry<'a>(reg: &'a Registry, id: &str) -> Result<&'a RegistryEntry, ApiErr> {
    let cid = world::resolve_planet_id(reg, id).ok_or_else(|| not_found(id))?;
    reg.planets
        .iter()
        .find(|e| e.id == cid)
        .ok_or_else(|| not_found(id))
}

/// 同一定位逻辑的可变版本(返回下标,规避借用冲突)
fn find_entry_mut(reg: &mut Registry, id: &str) -> Result<usize, ApiErr> {
    let cid = world::resolve_planet_id(reg, id).ok_or_else(|| not_found(id))?;
    reg.planets
        .iter()
        .position(|e| e.id == cid)
        .ok_or_else(|| not_found(id))
}

// ---- GET /planets ----

pub async fn planets_list(State(st): State<AppState>) -> Json<Value> {
    let reg = st.registry.lock().unwrap();
    let items: Vec<PlanetStatus> = reg.planets.iter().map(to_status).collect();
    Json(json!({ "planets": items }))
}

// ---- POST /planets ----

/// 端口是否已被其它 planet 记录占用
fn port_taken(reg: &Registry, port: u16) -> bool {
    reg.planets.iter().any(|e| e.port == port)
}

/// 本机 UDP 临时 bind 探测(成功即释放),确认端口当前真的可用
fn udp_bindable(port: u16) -> bool {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok()
}

/// 端口分配:body 指定则用之(与其它 planet 冲突 → 409),否则从 9994 起取
/// registry 未占用且本机可 bind 的最低端口
fn allocate_port(reg: &Registry, requested: Option<u16>) -> Result<u16, ApiErr> {
    if let Some(p) = requested {
        if port_taken(reg, p) {
            return Err(err(
                StatusCode::CONFLICT,
                "portInUse",
                &format!("udp port {p} is already used by another planet"),
                None,
            )
            );
        }
        return Ok(p);
    }
    for p in 9994..=u16::MAX {
        if !port_taken(reg, p) && udp_bindable(p) {
            return Ok(p);
        }
    }
    Err(err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "noAvailablePort",
        "no free udp port from 9994",
        None,
    ))
}

/// planets/<worldId>.bin 的存储路径
fn planet_bin_path(st: &AppState, id: &str) -> PathBuf {
    st.config_dir.join("planets").join(format!("{id}.bin"))
}

pub async fn planets_import(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    // 一期不支持容器导入(README 注明);出现 container 字段即明确拒绝
    if body.get("container").is_some() {
        return Err(err(
            StatusCode::NOT_IMPLEMENTED,
            "notImplemented",
            "container import is not implemented; provide a local planet file via \"source\"",
            None,
        ));
    }
    let source = body
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "badRequest",
                "body must contain \"source\"",
                None,
            )
        })?;
    // 一期仅支持本地路径;URL 源推迟到后续任务(README 注明)
    if source.contains("://") {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "unsupportedSource",
            "only local file paths are supported; url sources are not implemented yet",
            None,
        ));
    }

    let bytes = fs::read(source).map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            "sourceUnreadable",
            &format!("cannot read source file \"{source}\": {e}"),
            None,
        )
    })?;
    if bytes.len() > world::MAX_WORLD_BYTES {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "fileTooLarge",
            &format!(
                "planet file is {} bytes, limit is {} bytes",
                bytes.len(),
                world::MAX_WORLD_BYTES
            ),
            None,
        ));
    }
    // 解析通过 = 合法 planet 文件(import 的全部校验)
    let info = world::parse_world(&bytes).map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            "invalidWorld",
            &format!("not a valid planet file: {e}"),
            None,
        )
    })?;

    // /planets 仅收 planet;moon 文件走后续的 /moons 导入(Task 5)
    if info.kind != world::WorldKind::Planet {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalidWorld",
            "not a planet world file (type=moon); use the moons endpoint instead",
            None,
        ));
    }

    let id = info.world_id.to_string();
    let mut reg = st.registry.lock().unwrap();

    if reg.planets.iter().any(|e| e.world_id == info.world_id) {
        return Err(err(
            StatusCode::CONFLICT,
            "planetExists",
            &format!("planet with worldId {} already exists", info.world_id),
            None,
        ));
    }
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("planet-{}", info.world_id));
    if reg.planets.iter().any(|e| name_of(e) == name) {
        return Err(err(
            StatusCode::CONFLICT,
            "nameTaken",
            &format!("planet name \"{name}\" is already taken"),
            None,
        ));
    }

    let requested_port = match body.get("port").and_then(|v| v.as_u64()) {
        Some(p) if p > 0 && p <= u16::MAX as u64 => Some(p as u16),
        // 缺省或非法(0/越界)均回退到自动分配
        _ => None,
    };
    let port = allocate_port(&reg, requested_port)?;

    // 拷贝源文件到 <config>/planets/<worldId>.bin
    let dest = planet_bin_path(&st, &id);
    fs::create_dir_all(st.config_dir.join("planets"))
        .map_err(|e| internal(format!("cannot create planets dir: {e}")))?;
    fs::write(&dest, &bytes).map_err(|e| internal(format!("cannot write {}: {e}", dest.display())))?;

    // dataDir 覆写:统一转绝对路径;默认 <config>/data/<worldId>/
    let data_dir = match body.get("dataDir").and_then(|v| v.as_str()) {
        Some(d) if !d.is_empty() => {
            let pb = PathBuf::from(d);
            if pb.is_absolute() {
                pb
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(pb)
            }
        }
        _ => st.config_dir.join("data").join(&id),
    };

    let entry = RegistryEntry {
        id: id.clone(),
        world_id: info.world_id,
        name: Some(name.clone()),
        icon: body.get("icon").and_then(|v| v.as_str()).map(String::from),
        color: body
            .get("color")
            .and_then(|v| v.as_str())
            .map(String::from),
        source: "file".into(),
        roots: info.roots.clone(),
        file_size: bytes.len() as u64,
        revision_time_ms: info.revision_ms,
        imported_at_ms: now_unix() * 1000,
        last_node_id: None,
        data_dir,
        port,
        running: false,
        net_local: Default::default(),
        moons: Vec::new(),
    };
    let detail = to_detail(&entry);
    reg.planets.push(entry);
    reg.save(&st.config_dir)
        .map_err(|e| internal(format!("cannot save registry: {e}")))?;

    st.events
        .push(&id, "planetImported", &format!("planet \"{name}\" imported"));
    Ok((StatusCode::CREATED, Json(json!({ "planet": detail }))))
}

/// 内部错误 → 500 internalError
fn internal(msg: String) -> ApiErr {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internalError",
        &msg,
        None,
    )
}

// ---- GET/PATCH/DELETE /planets/{id} ----

pub async fn planets_show(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiErr> {
    let reg = st.registry.lock().unwrap();
    let e = find_entry(&reg, &id)?;
    Ok(Json(json!({ "planet": to_detail(e) })))
}

pub async fn planets_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    let mut reg = st.registry.lock().unwrap();
    let i = find_entry_mut(&mut reg, &id)?;
    if let Some(n) = body.get("name").and_then(|v| v.as_str()) {
        let self_id = reg.planets[i].id.clone();
        if reg
            .planets
            .iter()
            .any(|e| name_of(e) == n && e.id != self_id)
        {
            return Err(err(
                StatusCode::CONFLICT,
                "nameTaken",
                &format!("planet name \"{n}\" is already taken"),
                None,
            )
            );
        }
        reg.planets[i].name = Some(n.into());
    }
    if let Some(v) = body.get("icon").and_then(|v| v.as_str()) {
        reg.planets[i].icon = Some(v.into());
    }
    if let Some(v) = body.get("color").and_then(|v| v.as_str()) {
        reg.planets[i].color = Some(v.into());
    }
    let pid = reg.planets[i].id.clone();
    let detail = to_detail(&reg.planets[i]);
    reg.save(&st.config_dir)
        .map_err(|e| internal(format!("cannot save registry: {e}")))?;
    st.events
        .push(&pid, "planetUpdated", "planet metadata updated");
    Ok(Json(json!({ "planet": detail })))
}

pub async fn planets_delete(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiErr> {
    let mut reg = st.registry.lock().unwrap();
    let i = find_entry_mut(&mut reg, &id)?;
    if reg.planets[i].running {
        return Err(err(
            StatusCode::CONFLICT,
            "planetRunning",
            &format!(
                "planet \"{}\" is running; stop it before deleting",
                name_of(&reg.planets[i])
            ),
            None,
        ));
    }
    let removed = reg.planets.remove(i);
    let pid = removed.id.clone();
    reg.save(&st.config_dir)
        .map_err(|e| internal(format!("cannot save registry: {e}")))?;

    // planet 文件必删;data 目录仅在 config_dir 前缀内才整目录删除
    // (用户覆写的自定义 dataDir 只摘记录、绝不动文件)
    let _ = fs::remove_file(planet_bin_path(&st, &pid));
    if removed.data_dir.starts_with(&st.config_dir) {
        let _ = fs::remove_dir_all(&removed.data_dir);
    }
    st.planets.lock().unwrap().remove(&pid);

    st.events.push(
        &pid,
        "planetRemoved",
        &format!("planet \"{}\" removed", name_of(&removed)),
    );
    Ok(StatusCode::NO_CONTENT)
}

// ---- start / stop(Task 3 占位) ----

/// TODO(task4):Task 4 接入 NodeRuntime 后,这里应真正拉起 zt-core 实例
/// (加载 planets/<id>.bin、绑定 entry.port、初始化 data_dir 并回传运行中的
/// PlanetDetail),而不是只翻转 registry.running 后返回 501。
pub async fn planet_start(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiErr> {
    let mut reg = st.registry.lock().unwrap();
    let i = find_entry_mut(&mut reg, &id)?;
    reg.planets[i].running = true;
    let pid = reg.planets[i].id.clone();
    reg.save(&st.config_dir)
        .map_err(|e| internal(format!("cannot save registry: {e}")))?;
    st.events.push(&pid, "planetStarted", "planet started");
    // TODO(task4):运行时就绪后移除此占位响应,返回 {"planet": ...} 200
    Err(err(
        StatusCode::NOT_IMPLEMENTED,
        "runtimeNotReady",
        "planet runtime is not available in this build; state marked as started only",
        None,
    ))
}

pub async fn planet_stop(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiErr> {
    let mut reg = st.registry.lock().unwrap();
    let i = find_entry_mut(&mut reg, &id)?;
    reg.planets[i].running = false;
    reg.planets[i].last_node_id = None;
    let pid = reg.planets[i].id.clone();
    let detail = to_detail(&reg.planets[i]);
    reg.save(&st.config_dir)
        .map_err(|e| internal(format!("cannot save registry: {e}")))?;
    st.events.push(&pid, "planetStopped", "planet stopped");
    Ok(Json(json!({ "planet": detail })))
}

// ---- export / share ----

pub async fn planet_export(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiErr> {
    let reg = st.registry.lock().unwrap();
    let e = find_entry(&reg, &id)?;
    let path = planet_bin_path(&st, &e.id);
    let bytes = fs::read(&path).map_err(|er| {
        err(
            StatusCode::NOT_FOUND,
            "planetFileMissing",
            &format!("planet file {}: {er}", path.display()),
            None,
        )
    })?;
    Ok(([("content-type", "application/octet-stream")], bytes).into_response())
}

pub async fn planet_share(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    let reg = st.registry.lock().unwrap();
    let e = find_entry(&reg, &id)?;
    let pid = e.id.clone();
    let name = name_of(e);
    drop(reg);

    let ttl = body
        .get("ttlSeconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(300);
    let token = random_hex(12)
        .map_err(|e| internal(format!("cannot generate share token: {e}")))?;
    let expires_unix = now_unix() + ttl;
    st.shares.lock().unwrap().insert(
        token.clone(),
        ShareRecord {
            planet_id: pid.clone(),
            expires_unix,
        },
    );
    st.events.push(
        &pid,
        "shareCreated",
        &format!("share link for planet \"{name}\" created (ttl {ttl}s)"),
    );
    Ok(Json(json!(ShareLink {
        url: format!("http://127.0.0.1:{}/share/{token}", st.api_port),
        expires_at: expires_unix,
    })))
}

/// GET /share/<token>:一次性下载,命中即删记录;不挂 auth 中间件(链接本身是凭证)
pub async fn share_download(
    State(st): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiErr> {
    let gone = || -> ApiErr {
        err(
            StatusCode::NOT_FOUND,
            "shareNotFound",
            "share link not found, expired or already used",
            None,
        )
    };
    // remove():取出即删,保证一次性语义(即使后续读盘失败也不再可用)
    let record = st
        .shares
        .lock()
        .unwrap()
        .remove(&token)
        .ok_or_else(gone)?;
    if now_unix() >= record.expires_unix {
        return Err(gone());
    }
    let path = planet_bin_path(&st, &record.planet_id);
    let bytes = fs::read(&path).map_err(|er| {
        err(
            StatusCode::NOT_FOUND,
            "planetFileMissing",
            &format!("planet file {}: {er}", path.display()),
            None,
        )
    })?;
    Ok(([("content-type", "application/octet-stream")], bytes).into_response())
}

/// planet 路由(GET/POST /planets、/planets/{id} 及 start/stop/export/share)
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/planets", get(planets_list).post(planets_import))
        .route(
            "/planets/{id}",
            get(planets_show).patch(planets_patch).delete(planets_delete),
        )
        .route("/planets/{id}/start", post(planet_start))
        .route("/planets/{id}/stop", post(planet_stop))
        .route("/planets/{id}/export", get(planet_export))
        .route("/planets/{id}/share", post(planet_share))
}
