//! planet 生命周期端点:import / show / patch / delete / start / stop / export / share。
//! 错误码、错误体(`{"error","message","candidates"}`)与响应结构(`{"planet": PlanetDetail}`、
//! 201 CREATED 等)以 planet-port-api 契约为基准,CLI 按同一契约消费。

use std::fs;
use std::net::{Ipv4Addr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};

/// handlers 统一的错误类型(体积小,避免 clippy result_large_err)
type ApiErr = (StatusCode, Json<planet_port_api::types::ApiError>);
use planet_port_api::types::{PlanetDetail, PlanetKind, PlanetStatus, ShareLink, TransportStatus};
use serde_json::{json, Value};

use crate::registry::{Registry, RegistryEntry};
use crate::runtime;
use crate::state::{err, AppState, LiveInfo, ShareRecord};
use crate::world;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// worldId 命中官方常量即 Official(展示标签;world 签名不校验,见常量注释)
fn planet_kind(world_id: u64) -> PlanetKind {
    if world_id == planet_port_api::types::OFFICIAL_WORLD_ID {
        PlanetKind::Official
    } else {
        PlanetKind::Custom
    }
}

/// 取 n 字节加密随机数并 hex 编码为 2n 字符(不引入 getrandom 依赖;
/// 随机源平台分叉见 main.rs 的 fill_random)
fn random_hex(n: usize) -> std::io::Result<String> {
    let mut buf = vec![0u8; n];
    crate::fill_random(&mut buf)?;
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

/// 列表项投影:online/core_version/latency/node_id 取自运行时实时数据
/// (无运行时一律 false/None,offline 分支同样形态)
pub(crate) fn to_status(e: &RegistryEntry, live: Option<&LiveInfo>) -> PlanetStatus {
    let online = live.map(|l| l.online).unwrap_or(false);
    PlanetStatus {
        id: e.id.clone(),
        name: name_of(e),
        icon: e.icon.clone().unwrap_or_default(),
        color: e.color.clone().unwrap_or_default(),
        kind: planet_kind(e.world_id),
        status: if e.running { "running" } else { "stopped" }.into(),
        enabled: e.enabled,
        online,
        // 节点地址/核心版本是本地事实:实例运行即有(connecting 阶段也显示)。
        // latency 才依赖根可达,保持 online 门控。空串 = 节点槽位已空(停止
        // 流程中),归一为 None
        node_id: live.map(|l| l.node_id.clone()).filter(|s| !s.is_empty()),
        core_version: live.map(|l| l.core_version.clone()),
        latency_ms: if online { live.unwrap().latency_ms } else { None },
        network_count: live.map(|l| l.network_count).unwrap_or_else(|| e.net_local.len()),
        port: e.port,
        transport: transport_of(live),
    }
}

/// live → transport 投影(实例未运行或节点槽位已空 → None;tcp 枚举转小写字符串)
fn transport_of(live: Option<&LiveInfo>) -> Option<TransportStatus> {
    live.and_then(|l| l.transport.as_ref())
        .map(|t| TransportStatus {
            primary_port: t.primary_port,
            secondary_port: t.secondary_port,
            tcp: format!("{:?}", t.tcp).to_lowercase(),
            relay: t.relay.map(|r| r.to_string()),
        })
}

/// 详情投影:roots 来自导入时解析的 world 文件
fn to_detail(e: &RegistryEntry, live: Option<&LiveInfo>) -> PlanetDetail {
    PlanetDetail {
        id: e.id.clone(),
        name: name_of(e),
        icon: e.icon.clone().unwrap_or_default(),
        color: e.color.clone().unwrap_or_default(),
        kind: planet_kind(e.world_id),
        world_id: e.world_id,
        roots: e.roots.clone(),
        file_size: e.file_size,
        revision_time_ms: e.revision_time_ms,
        last_node_id: e.last_node_id.clone(),
        source: e.source.clone(),
        network_count: live.map(|l| l.network_count).unwrap_or_else(|| e.net_local.len()),
        port: e.port,
        status: if e.running { "running" } else { "stopped" }.into(),
        enabled: e.enabled,
        online: live.map(|l| l.online).unwrap_or(false),
        transport: transport_of(live),
        tcp_fallback_relay: e.tcp_fallback_relay.clone(),
    }
}

/// 展示名(name 缺省回退 id)
pub(crate) fn name_of(e: &RegistryEntry) -> String {
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

/// 单 planet 的实时运行信息;node 查询在 registry 锁外做(锁纪律)。
/// 未找到即 404(与 find_entry 同款),未运行 → Ok(None)。
fn live_of(st: &AppState, id: &str) -> Result<Option<LiveInfo>, ApiErr> {
    let cid = {
        let reg = st.registry.lock().unwrap();
        world::resolve_planet_id(&reg, id).ok_or_else(|| not_found(id))?
    };
    let live = crate::runtime::live_infos(st);
    Ok(live.get(&cid).cloned())
}

// ---- GET /planets ----

pub async fn planets_list(State(st): State<AppState>) -> Json<Value> {
    // node 查询(online/peers/networks)先于 registry 锁:持锁期间不做 node 调用
    let live = crate::runtime::live_infos(&st);
    let reg = st.registry.lock().unwrap();
    let items: Vec<PlanetStatus> = reg
        .planets
        .iter()
        .map(|e| to_status(e, live.get(&e.id)))
        .collect();
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

/// 端口分配:body 指定则用之(与其它 planet 冲突 → 409),否则从 9993 起取
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
    // 9993 起:对齐官方默认口(本机被官方 zerotier-one 占用时
    // udp_bindable 探测自然跳过,不冲突)
    for p in 9993..=u16::MAX {
        if !port_taken(reg, p) && udp_bindable(p) {
            return Ok(p);
        }
    }
    Err(err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "noAvailablePort",
        "no free udp port from 9993",
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
    // 一期不支持容器导入(README 注明);container 为字符串即明确拒绝
    if body.get("container").and_then(|v| v.as_str()).is_some() {
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
                "body must contain \"source\" or \"container\"",
                None,
            )
        })?;
    // 含 `://` 视为 URL 源:下载后走与本地文件完全相同的解析校验;
    // 其余按本地路径读(行为不变)
    let is_url = source.contains("://");
    let bytes = if is_url {
        // 下载含网络 IO,REST handler 是异步上下文,放阻塞线程池;
        // spawn_blocking 要求 'static,source 从 body 借来,先取 owned
        let url = source.to_string();
        tokio::task::spawn_blocking(move || world::fetch_url(&url))
            .await
            .map_err(|e| internal(format!("fetch task failed: {e}")))?
            .map_err(|e| match e {
                world::FetchError::TooLarge => err(
                    StatusCode::BAD_REQUEST,
                    "sourceTooLarge",
                    &format!(
                        "downloaded planet file from \"{source}\" exceeds the limit of {} bytes",
                        world::MAX_WORLD_BYTES
                    ),
                    None,
                ),
                world::FetchError::Fetch(m) => err(
                    StatusCode::BAD_REQUEST,
                    "sourceFetchFailed",
                    &format!("cannot fetch \"{source}\": {m}"),
                    None,
                ),
            })?
    } else {
        // 先看大小再读入,避免为超限文件白白吃内存
        let meta = fs::metadata(source).map_err(|e| {
            err(
                StatusCode::BAD_REQUEST,
                "sourceUnreadable",
                &format!("cannot read source file \"{source}\": {e}"),
                None,
            )
        })?;
        if meta.len() > world::MAX_WORLD_BYTES as u64 {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "fileTooLarge",
                &format!(
                    "planet file is {} bytes, limit is {} bytes",
                    meta.len(),
                    world::MAX_WORLD_BYTES
                ),
                None,
            ));
        }
        fs::read(source).map_err(|e| {
            err(
                StatusCode::BAD_REQUEST,
                "sourceUnreadable",
                &format!("cannot read source file \"{source}\": {e}"),
                None,
            )
        })?
    };
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
    // registry 锁只包登记判定与写盘,不跨后续 start 的 await(MutexGuard 非 Send);
    // 块结束时一并归还,name 供事件消息使用
    let name = {
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

    // dataDir 覆写:先建目录再 canonicalize,registry 只存归一化后的绝对路径
    // (杜绝 `..`/相对路径混入,DELETE 侧的目录守卫才能做可靠的逐组件比较);
    // 缺省 <config>/data/<worldId>/(config_dir 启动时已 canonicalize,本身即安全)
    let data_dir = match body.get("dataDir").and_then(|v| v.as_str()) {
        Some(d) if !d.is_empty() => {
            let pb = PathBuf::from(d);
            let pb = if pb.is_absolute() {
                pb
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(pb)
            };
            fs::create_dir_all(&pb)
                .map_err(|e| internal(format!("cannot create data dir {}: {e}", pb.display())))?;
            fs::canonicalize(&pb)
                .map_err(|e| internal(format!("cannot resolve data dir {}: {e}", pb.display())))?
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
        source: if is_url { "url".into() } else { "file".into() },
        roots: info.roots.clone(),
        file_size: bytes.len() as u64,
        revision_time_ms: info.revision_ms,
        imported_at_ms: now_unix() * 1000,
        last_node_id: None,
        data_dir,
        port,
        port_explicit: requested_port.is_some(),
        tcp_fallback_relay: None,
        running: false,
        enabled: true,
        net_local: Default::default(),
        moons: Vec::new(),
        ifaces: Vec::new(),
    };
    reg.planets.push(entry);
    reg.save(&st.config_dir)
        .map_err(|e| internal(format!("cannot save registry: {e}")))?;
    name
    };

    st.events
        .push(&id, "planetImported", &format!("planet \"{name}\" imported"));
    // 派生运行规则:导入只登记,不拉起(无网络不运行);首次 join 或显式
    // enable 时按规则自然上线
    let reg = st.registry.lock().unwrap();
    let e = find_entry(&reg, &id)?;
    let detail = to_detail(e, None);
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
    let live = live_of(&st, &id)?;
    let reg = st.registry.lock().unwrap();
    let e = find_entry(&reg, &id)?;
    Ok(Json(json!({ "planet": to_detail(e, live.as_ref()) })))
}

pub async fn planets_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    // relay 解析/校验前置:必须在任何内存变更之前完成,非法值立即 400。
    // 否则「带非法 relay 的多字段 PATCH」会先改掉 name/icon 再失败,
    // 造成 GET 看到新值而盘上未写、重启回退的部分应用
    let relay: Option<Option<String>> = match body.get("tcpFallbackRelay") {
        None => None,
        Some(Value::Null) => Some(None),            // null = 清除
        Some(Value::String(s)) if s.is_empty() => Some(None), // 空串 = 清除
        Some(Value::String(s)) => {
            if s.parse::<std::net::SocketAddr>().is_err() {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "badRelayAddr",
                    &format!("tcpFallbackRelay must be \"ip:port\", got: {s:?}"),
                    None,
                ));
            }
            Some(Some(s.clone()))
        }
        // 非字符串非 null(数字/布尔等)类型不符,不再静默清除
        Some(_) => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "badRelayAddr",
                "tcpFallbackRelay must be a string (\"ip:port\") or null",
                None,
            ));
        }
    };
    let live = live_of(&st, &id)?;
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
    if let Some(parsed) = relay {
        reg.planets[i].tcp_fallback_relay = parsed;
    }
    let pid = reg.planets[i].id.clone();
    let detail = to_detail(&reg.planets[i], live.as_ref());
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
    // 先取启停门闩:start_planet 全程持门闩(先插 map 再置 running=true),
    // 拿到门闩后 running 判定即不含并发 start 的半途态,杜绝「delete 读到
    // running=false 却把刚插入的运行时整条 remove → NodeHandle 走 Drop 兜底」
    let _gate = st.runtime_gate.lock().await;
    // 定位 + 克隆基本信息(随后的 leave/teardown 会写 registry,不能长期持锁)
    let (pid, name, data_dir) = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let e = find_entry(&reg, &id)?;
        (e.id.clone(), name_of(e), e.data_dir.clone())
    };

    // 自动批量 leave:以核心快照为准逐网络退出(阻塞应答,spawn_blocking 顺序执行)。
    // 单个 leave 失败不阻断删除:记录事件后继续,随后的实例级 teardown 兜底收尾
    let mut left = 0usize;
    if let Some(rt) = crate::state::runtime_of(&st, &pid) {
        let nwids: Vec<u64> = rt.networks().iter().map(|c| c.nwid).collect();
        for nwid in nwids {
            let rt2 = Arc::clone(&rt);
            let res = tokio::task::spawn_blocking(move || rt2.node_leave(nwid)).await;
            match res {
                Ok(Ok(())) => {
                    left += 1;
                    st.events
                        .push(&pid, "leave", &format!("left {:016x} (planet removing)", nwid));
                }
                Ok(Err(e)) => st.events.push(
                    &pid,
                    "leaveFailed",
                    &format!("leave {:016x} failed during removal: {e}", nwid),
                ),
                Err(e) => st.events.push(
                    &pid,
                    "leaveFailed",
                    &format!("leave task {:016x} panicked: {e}", nwid),
                ),
            }
            // 设备拆除 + 快照/冲突清理(leave 失败同样执行;实例级 teardown 兜底)
            rt.plane
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .teardown(nwid);
            st.net_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&(pid.clone(), nwid));
            rt.conflicts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&nwid);
        }
    }

    // 运行中/异常残留一并红线收尾(gate 已持,teardown_leftover 直接可用,
    // 不可调 stop_planet —— 它会再次取 gate 死锁)
    runtime::teardown_leftover(&st, &pid).await;

    // 摘记录(teardown_runtime 已把 running 置 false 并落盘,此处直接移除)
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let i = find_entry_mut(&mut reg, &id)?;
        reg.planets.remove(i);
        reg.save(&st.config_dir)
            .map_err(|e| internal(format!("cannot save registry: {e}")))?;
    }

    // planet 文件必删;data 目录只在「canonicalize 后严格位于 config_dir 内」才整目录删除。
    // 两侧都 canonicalize(macOS 下 /var → /private/var),并把 data_dir == config_dir
    // 视为非法配置:此时只摘记录、绝不动 config 目录本身(registry.json/token 都在里面)。
    // canonicalize 已解析符号链接与 `..`,之后 Path::starts_with 的逐组件比较即安全。
    let _ = fs::remove_file(planet_bin_path(&st, &pid));
    if let (Ok(cfg), Ok(dd)) = (fs::canonicalize(&st.config_dir), fs::canonicalize(&data_dir)) {
        if dd.starts_with(&cfg) && dd != cfg {
            let _ = fs::remove_dir_all(&dd);
        }
    }
    st.planets.lock().unwrap().remove(&pid);

    st.events.push(
        &pid,
        "planetRemoved",
        &format!("planet \"{name}\" removed ({left} networks left)"),
    );
    Ok(StatusCode::NO_CONTENT)
}

// ---- enable / disable(唯一的启停语义:启用 = 按派生规则运行,
// ---- 禁用 = 无条件停且不再运行;start/stop 已并入本对) ----

/// 禁用:落 enabled=false;在跑立即停(IdleStop,不产生挂起语义——
/// enable 后按派生规则可自动恢复)。join/boot 在禁用下一律不运行
pub async fn planet_disable(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiErr> {
    let pid = {
        let reg = st.registry.lock().unwrap();
        find_entry(&reg, &id)?.id.clone()
    };
    {
        let mut reg = st.registry.lock().unwrap();
        let e = find_entry_mut(&mut reg, &id)?;
        reg.planets[e].enabled = false;
        reg.save(&st.config_dir)
            .map_err(|e| internal(format!("cannot save registry: {e}")))?;
    }
    runtime::stop_planet(&st, &pid, runtime::StopMark::Stop).await;
    st.events.push(&pid, "planetDisabled", "planet disabled (won't run until enabled)");
    let reg = st.registry.lock().unwrap();
    let e = find_entry(&reg, &id)?;
    Ok(Json(json!({ "planet": to_detail(e, None) })))
}

/// 启用:落 enabled=true;非手动挂起且有网络 → 按派生规则自动拉起
pub async fn planet_enable(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiErr> {
    let entry = {
        let mut reg = st.registry.lock().unwrap();
        let e = find_entry_mut(&mut reg, &id)?;
        reg.planets[e].enabled = true;
        reg.save(&st.config_dir)
            .map_err(|e| internal(format!("cannot save registry: {e}")))?;
        reg.planets[e].clone()
    };
    if !entry.net_local.is_empty() {
        runtime::start_planet(&st, &entry.id)
            .await
            .map_err(|m| err(StatusCode::INTERNAL_SERVER_ERROR, "startFailed", &m, None))?;
    }
    st.events.push(&entry.id, "planetEnabled", "planet enabled");
    let live = live_of(&st, &entry.id)?;
    let reg = st.registry.lock().unwrap();
    let e = find_entry(&reg, &id)?;
    Ok(Json(json!({ "planet": to_detail(e, live.as_ref()) })))
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

/// planet 路由(GET/POST /planets、/planets/{id} 及 enable/disable/export/share)
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/planets", get(planets_list).post(planets_import))
        .route(
            "/planets/{id}",
            get(planets_show).patch(planets_patch).delete(planets_delete),
        )
        .route("/planets/{id}/enable", post(planet_enable))
        .route("/planets/{id}/disable", post(planet_disable))
        .route("/planets/{id}/export", get(planet_export))
        .route("/planets/{id}/share", post(planet_share))
}
