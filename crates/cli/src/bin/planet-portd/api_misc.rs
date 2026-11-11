//! 杂项端点:GET /peers、GET/POST /moons、POST /moons/{id}/orbit、
//! DELETE /moons/{id}、GET /dump。
//! 响应结构({"peers":[...]}、{"moons":[...]}、201 {"moon":...}、
//! {"result":"ok"}、204、纯文本 dump)与错误码、错误体以 planet-port-api 契约为基准
//! 同名 handler,保证 CLI 双端互通。
//!
//! # moon 落盘真源
//!
//! daemon 只把 moon 文件写到 zt-core StateStore 的 ZT_STATE_OBJECT_MOON
//! 同款路径 `<data_dir>/<planet worldId>/moons.d/<moonId:016x>.moon`,核心
//! stateGet 自会命中;registry.moons 只是清单投影(列表/删除判定用)。
//!
//! # 锁与线程纪律
//!
//! - registry / planets 锁只短临界区取克隆,不跨 await、不嵌套;
//! - orbit/deorbit 是阻塞应答命令,一律 spawn_blocking(红线);
//! - peers/networks 是只读查询接口,与驱动线程无共享可变状态,可直接调。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use planet_port_api::types::{MoonEntry, PeerEntry};
use serde_json::{json, Value};
use zt_core::{PeerRole, TcpTunnelState};

use crate::api_net::{filter_planet, net_local_of, not_running, resolve_running_planet};
use crate::registry::MoonRecord;
use crate::state::{err, runtime_of, AppState};
use crate::world;

type ApiErr = (StatusCode, Json<planet_port_api::types::ApiError>);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn internal(msg: String) -> ApiErr {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internalError",
        &msg,
        None,
    )
}

/// moonid 路径参数解析:十六进制 1~16 位(容错 0x 前缀);非法 → 400 badRequest
fn parse_moon_id(s: &str) -> Result<u64, ApiErr> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if hex.is_empty() || hex.len() > 16 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "badRequest",
            &format!("invalid moon id \"{s}\" (expect 1-16 hex digits)"),
            None,
        ));
    }
    u64::from_str_radix(hex, 16).map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            "badRequest",
            &format!("invalid moon id \"{s}\": {e}"),
            None,
        )
    })
}

/// seed 解析:按 moon 惯例先按十六进制(容错 0x 前缀),失败再按十进制
fn parse_seed(s: &str) -> Option<u64> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u64::from_str_radix(hex, 16)
        .ok()
        .or_else(|| t.parse().ok())
}

/// StateStore 同款 moon 落盘路径:<data_dir>/<planet worldId>/moons.d/<moonId:016x>.moon
/// (StateStore::new 的 world_dir 即按 PLANET worldId 建目录,文件名才是 moon 的 id)
fn moon_file(data_dir: &std::path::Path, world_id: u64, moon_id: u64) -> PathBuf {
    data_dir
        .join(world_id.to_string())
        .join("moons.d")
        .join(format!("{moon_id:016x}.moon"))
}

/// MoonRecord → MoonEntry(cached 即时探测 moons.d 文件,seed 16 位 hex)
fn to_moon_entry(planet_id: &str, entry: &crate::registry::RegistryEntry, r: &MoonRecord) -> MoonEntry {
    let cached = u64::from_str_radix(&r.moon_id, 16)
        .map(|mid| moon_file(&entry.data_dir, entry.world_id, mid).exists())
        .unwrap_or(false);
    MoonEntry {
        planet: planet_id.to_string(),
        moon_id: r.moon_id.clone(),
        seed: r.seed.map(|s| format!("{s:016x}")),
        // source=="file" 即从 moon 文件导入;orbit 来源记 "orbit"
        from_file: r.source == "file",
        cached,
    }
}

fn role_str(r: PeerRole) -> &'static str {
    match r {
        PeerRole::Planet => "PLANET",
        PeerRole::Moon => "MOON",
        PeerRole::Leaf => "LEAF",
    }
}

// ---- GET /peers ----

pub async fn peers_list(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    // registry 只在开头短锁取克隆;peers 查询在锁外逐 planet 做
    let (filter, entries) = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (filter_planet(&reg, &q)?, reg.planets.clone())
    };
    let mut items: Vec<PeerEntry> = Vec::new();
    for e in &entries {
        if filter.as_ref().map(|f| *f != e.id).unwrap_or(false) {
            continue;
        }
        let Some(rt) = runtime_of(&st, &e.id) else { continue };
        for p in rt.peers() {
            items.push(PeerEntry {
                planet: e.id.clone(),
                address: format!("{:010x}", p.address),
                role: role_str(p.role).into(),
                version: p.version.map(|(a, b, c)| format!("{a}.{b}.{c}")),
                latency_ms: p.latency_ms.map(|v| v.max(0) as u64),
                // 有 active 路径取第一条,否则 None = relay
                path: p
                    .paths
                    .iter()
                    .find(|pp| pp.active)
                    .map(|pp| format!("udp/{}", pp.addr)),
            });
        }
    }
    Ok(Json(json!({ "peers": items })))
}

// ---- GET /moons ----

pub async fn moons_list(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    let (filter, entries) = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (filter_planet(&reg, &q)?, reg.planets.clone())
    };
    let mut items: Vec<MoonEntry> = Vec::new();
    for e in &entries {
        if filter.as_ref().map(|f| *f != e.id).unwrap_or(false) {
            continue;
        }
        for r in &e.moons {
            items.push(to_moon_entry(&e.id, e, r));
        }
    }
    Ok(Json(json!({ "moons": items })))
}

// ---- POST /moons ----

pub async fn moon_import(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
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
    // 定位链 + entry(data_dir 决定 moon 落哪个 planet):running 校验在前
    let pid = resolve_running_planet(&st, q.get("planet").map(|s| s.as_str()))?;
    let entry = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets.iter().find(|e| e.id == pid).cloned().ok_or_else(|| {
            err(
                StatusCode::CONFLICT,
                "planetNotRunning",
                &format!("planet \"{pid}\" is not running"),
                None,
            )
        })?
    };
    // 先看大小再读入(与 /planets import 同款,8MB 上限)
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
                "moon file is {} bytes, limit is {} bytes",
                meta.len(),
                world::MAX_WORLD_BYTES
            ),
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
                "moon file is {} bytes, limit is {} bytes",
                bytes.len(),
                world::MAX_WORLD_BYTES
            ),
            None,
        ));
    }
    let info = world::parse_world(&bytes).map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            "invalidWorld",
            &format!("not a valid moon file: {e}"),
            None,
        )
    })?;
    // /moons 只收 moon 文件;planet 走 /planets
    if info.kind != world::WorldKind::Moon {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "notAMoon",
            "not a moon world file (type=planet); use the planets endpoint instead",
            None,
        ));
    }

    // 落盘到该 planet 的 dataDir(StateStore 同款路径:目录段是 PLANET 的
    // worldId,文件名才是 moon 的 worldId;mkdir -p 父目录)
    let dest = moon_file(&entry.data_dir, entry.world_id, info.world_id);
    let parent = dest
        .parent()
        .ok_or_else(|| internal(format!("非法 moon 路径 {}", dest.display())))?;
    fs::create_dir_all(parent).map_err(|e| internal(format!("cannot create {}: {e}", parent.display())))?;
    fs::write(&dest, &bytes).map_err(|e| internal(format!("cannot write {}: {e}", dest.display())))?;

    // registry upsert(同 moon_id 覆写)+ 事件 + 201
    let record = MoonRecord {
        moon_id: format!("{:016x}", info.world_id),
        seed: None,
        source: "file".into(),
        cached_at_ms: now_ms(),
    };
    let moon = to_moon_entry(&pid, &entry, &record);
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) {
            match e.moons.iter_mut().find(|m| m.moon_id == record.moon_id) {
                Some(m) => *m = record,
                None => e.moons.push(record),
            }
            if let Err(e) = reg.save(&st.config_dir) {
                eprintln!("[moons] registry 写盘失败: {e}");
            }
        }
    }
    st.events
        .push(&pid, "moonImported", &format!("moon imported from {source}"));
    Ok((StatusCode::CREATED, Json(json!({ "moon": moon }))))
}

// ---- POST /moons/{moonid}/orbit ----

pub async fn moon_orbit(
    State(st): State<AppState>,
    Path(moon_id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    let mid = parse_moon_id(&moon_id)?;
    // seed 可省(=0);契约用字符串,这里同时容忍 JSON 数字
    let seed = match body.get("seed") {
        None | Some(Value::Null) => 0,
        Some(Value::String(s)) => parse_seed(s).ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "badRequest",
                &format!("invalid seed \"{s}\" (expect hex or decimal u64)"),
                None,
            )
        })?,
        Some(v) => v.as_u64().ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "badRequest",
                "seed must be a hex or decimal string",
                None,
            )
        })?,
    };
    // 定位(registry 锁在 resolve_running_planet 内短持)与运行时获取(planets
    // 锁)分开:两锁不得嵌套
    let pid = resolve_running_planet(&st, q.get("planet").map(|s| s.as_str()))?;
    let rt = not_running(&st, &pid)?;
    // orbit 在驱动线程执行并阻塞等应答,放阻塞线程池
    let rt2 = std::sync::Arc::clone(&rt);
    tokio::task::spawn_blocking(move || rt2.node_orbit(mid, seed))
        .await
        .map_err(|e| internal(format!("orbit 任务失败: {e}")))?
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "orbitFailed",
                &format!("orbit {moon_id} failed: {e}"),
                None,
            )
        })?;
    // registry 增/改记录(同 moon_id 覆写)+ 事件
    let record = MoonRecord {
        moon_id: format!("{mid:016x}"),
        seed: Some(seed),
        source: "orbit".into(),
        cached_at_ms: now_ms(),
    };
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|x| x.id == pid) {
            match e.moons.iter_mut().find(|m| m.moon_id == record.moon_id) {
                Some(m) => *m = record,
                None => e.moons.push(record),
            }
            if let Err(e) = reg.save(&st.config_dir) {
                eprintln!("[moons] registry 写盘失败: {e}");
            }
        }
    }
    st.events.push(&pid, "orbit", &format!("orbit {moon_id}"));
    Ok(Json(json!({ "result": "ok" })))
}

// ---- DELETE /moons/{moonid} ----

pub async fn moon_deorbit(
    State(st): State<AppState>,
    Path(moon_id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<StatusCode, ApiErr> {
    let mid = parse_moon_id(&moon_id)?;
    let mid_hex = format!("{mid:016x}");
    let pid = resolve_running_planet(&st, q.get("planet").map(|s| s.as_str()))?;
    let entry = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets.iter().find(|e| e.id == pid).cloned().ok_or_else(|| {
            err(
                StatusCode::CONFLICT,
                "planetNotRunning",
                &format!("planet \"{pid}\" is not running"),
                None,
            )
        })?
    };
    if !entry.moons.iter().any(|m| m.moon_id == mid_hex) {
        return Err(err(
            StatusCode::NOT_FOUND,
            "moonNotFound",
            &format!("moon {moon_id} not found on this planet"),
            None,
        ));
    }
    let rt = not_running(&st, &pid)?;
    let rt2 = std::sync::Arc::clone(&rt);
    tokio::task::spawn_blocking(move || rt2.node_deorbit(mid))
        .await
        .map_err(|e| internal(format!("deorbit 任务失败: {e}")))?
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "deorbitFailed",
                &format!("deorbit {moon_id} failed: {e}"),
                None,
            )
        })?;
    // 删 moons.d 文件(有则删)+ 删 registry 记录 + 事件
    let _ = fs::remove_file(moon_file(&entry.data_dir, entry.world_id, mid));
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|x| x.id == pid) {
            e.moons.retain(|m| m.moon_id != mid_hex);
            if let Err(e) = reg.save(&st.config_dir) {
                eprintln!("[moons] registry 写盘失败: {e}");
            }
        }
    }
    st.events.push(&pid, "deorbit", &format!("deorbit {moon_id}"));
    Ok(StatusCode::NO_CONTENT)
}

// ---- GET /dump ----

pub async fn dump(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<String, ApiErr> {
    // 单 planet 定位链(running 校验)
    let pid = resolve_running_planet(&st, q.get("planet").map(|s| s.as_str()))?;
    let entry = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.planets.iter().find(|e| e.id == pid).cloned().ok_or_else(|| {
            err(
                StatusCode::CONFLICT,
                "planetNotRunning",
                &format!("planet \"{pid}\" is not running"),
                None,
            )
        })?
    };
    let rt = runtime_of(&st, &pid);
    let name = crate::api_planet::name_of(&entry);
    let status = if entry.running { "running" } else { "stopped" };

    let mut out = format!(
        "planet {name} (world {}) status={status} port={}\n",
        entry.world_id, entry.port
    );

    // transport:通道形态一行(主口/副口/TCP 隧道),UDP 受限排查的第一线索。
    // disabled(未配 relay)不带 relay 段;副口未启用整段省略
    if let Some(rt) = &rt {
        if let Some(t) = rt.transport() {
            let sec = t.secondary_port.map(|p| format!(" secondary=udp/{p}")).unwrap_or_default();
            let tcp = match t.tcp {
                TcpTunnelState::Disabled => " disabled".to_string(),
                other => format!(
                    " tcp={}{relay}",
                    format!("{other:?}").to_lowercase(),
                    relay = t.relay.map(|r| format!("({r})")).unwrap_or_default()
                ),
            };
            out.push_str(&format!(
                "transport: primary=udp/{port}{sec}{tcp}\n",
                port = t.primary_port
            ));
        }
    }

    // networks:每网络 nwid / status / assigned 地址
    out.push_str("networks:\n");
    if let Some(rt) = &rt {
        for cfg in rt.networks() {
            let nl = net_local_of(Some(rt), &entry, cfg.nwid);
            let assigned: Vec<String> = cfg
                .assigned
                .iter()
                .map(|a| format!("{}/{}", a.addr, a.prefix))
                .collect();
            out.push_str(&format!(
                "  {:016x} {} assigned=[{}] routeViaZeroTier={}\n",
                cfg.nwid,
                crate::runtime::status_name(&cfg.status),
                assigned.join(", "),
                nl.route_via_zero_tier
            ));
        }
    }

    // peers:地址 / role / latency / 路径
    out.push_str("peers:\n");
    let peers = rt.as_ref().map(|r| r.peers()).unwrap_or_default();
    if peers.is_empty() {
        out.push_str("  (none)\n");
    }
    for p in peers {
        let path = p
            .paths
            .iter()
            .find(|pp| pp.active)
            .map(|pp| format!("udp/{}", pp.addr))
            .unwrap_or_else(|| "relay".into());
        let latency = p
            .latency_ms
            .map(|v| format!("{}ms", v.max(0)))
            .unwrap_or_else(|| "-".into());
        out.push_str(&format!(
            "  {:010x} {} latency={latency} path={path}\n",
            p.address,
            role_str(p.role)
        ));
    }

    // net_local:registry 持久层的本地网络配置(键排序保证输出稳定)
    out.push_str("net_local:\n");
    let mut keys: Vec<&String> = entry.net_local.keys().collect();
    keys.sort();
    for k in keys {
        let nl = &entry.net_local[k];
        out.push_str(&format!(
            "  {k} routeViaZeroTier={} dnsMode={} customDns=[{}]\n",
            nl.route_via_zero_tier,
            nl.dns_mode,
            nl.custom_dns.join(", ")
        ));
    }

    // events:该 planet 尾部 10 条
    out.push_str("events (last 10):\n");
    for ev in st.events.tail(Some(&pid), None, 10) {
        out.push_str(&format!("  {} {} {}\n", ev.ts_ms, ev.kind, ev.message));
    }
    Ok(out)
}

/// 杂项路由:/peers、/moons 系列、/dump
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/peers", get(peers_list))
        .route("/moons", get(moons_list).post(moon_import))
        .route("/moons/{moonid}/orbit", post(moon_orbit))
        .route("/moons/{moonid}", delete(moon_deorbit))
        .route("/dump", get(dump))
}
