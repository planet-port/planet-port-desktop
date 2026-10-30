//! 网络面端点:GET /networks、join / leave / set、GET /events。
//! 错误码、错误体、响应结构({"result":"ok"}、{"networks":[...]}、
//! NetworkEntry 字段)以 planet-port-api 类型为唯一基准。
//!
//! 已知限制(一期接受):POST /networks/{nwid}(set)只写 net_local,
//! 生效时机 = 下一轮控制器 ConfigUpdate(sink 侧没有主动重配通道);
//! routeViaZeroTier 的变更同样延迟生效,README 注明。

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use planet_port_api::types::{NetworkEntry, RouteLine};
use serde_json::{json, Value};
use zt_core::NetConfig;

use crate::registry::{NetLocal, RegistryEntry};
use crate::runtime;
use crate::state::{err, runtime_of, AppState, PlanetRuntime};
use crate::world;

type ApiErr = (StatusCode, Json<planet_port_api::types::ApiError>);

// ---- 定位链与校验 ----

/// 变更端点定位链:显式 ?planet= → 存在且 running;省略 → 单 running 自动选,
/// 多 running 409 planetAmbiguous(带候选),零 running 409 noPlanetRunning。
/// 查找复用 world::resolve_planet_id(id/name/worldId 均可作为定位输入)。
///
/// 锁纪律:registry 锁只在函数内部短持;歧义分支需要每个 running planet 的
/// 实际网络数(= 核心网络快照条数,非 registry net_local 条数),
/// 那是 node 查询,须在不持 registry 锁时做 —— 故先短锁收集候选并 clone
/// 运行时 Arc,放锁后再查询计数。
pub(crate) fn resolve_running_planet(
    st: &AppState,
    planet: Option<&str>,
) -> Result<String, ApiErr> {
    let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(p) = planet {
        return match world::resolve_planet_id(&reg, p).and_then(|cid| {
            reg.planets.iter().find(|x| x.id == cid).map(|x| (cid, x))
        }) {
            Some((cid, x)) if x.running => Ok(cid),
            Some((_, x)) => Err(err(
                StatusCode::CONFLICT,
                "planetNotRunning",
                &format!("planet \"{}\" is not running", crate::api_planet::name_of(x)),
                None,
            )),
            None => Err(err(
                StatusCode::NOT_FOUND,
                "planetNotFound",
                &format!("planet \"{p}\" not found"),
                None,
            )),
        };
    }
    let running: Vec<&RegistryEntry> = reg.planets.iter().filter(|p| p.running).collect();
    match running.len() {
        1 => Ok(running[0].id.clone()),
        0 => Err(err(
            StatusCode::CONFLICT,
            "noPlanetRunning",
            "no running planet; specify ?planet=",
            None,
        )),
        _ => {
            let named: Vec<(String, Arc<PlanetRuntime>)> = running
                .iter()
                .filter_map(|p| {
                    runtime_of(st, &p.id).map(|rt| (crate::api_planet::name_of(p), rt))
                })
                .collect();
            drop(reg);
            // 实际网络数以核心快照为准(leave 后 net_local 仍保留配置,不计数)
            let candidates: Vec<String> = named
                .into_iter()
                .map(|(name, rt)| format!("{name} ({})", rt.networks().len()))
                .collect();
            Err(err(
                StatusCode::CONFLICT,
                "planetAmbiguous",
                "multiple running planets; specify ?planet=",
                Some(candidates),
            ))
        }
    }
}

/// 查询端点 ?planet= 过滤:解析为 canonical id,未知名 404(过滤语义,不查 running)
pub(crate) fn filter_planet(
    reg: &crate::registry::Registry,
    q: &HashMap<String, String>,
) -> Result<Option<String>, ApiErr> {
    match q.get("planet") {
        None => Ok(None),
        Some(p) => Ok(Some(
            world::resolve_planet_id(reg, p).ok_or_else(|| {
                err(
                    StatusCode::NOT_FOUND,
                    "planetNotFound",
                    &format!("planet \"{p}\" not found"),
                    None,
                )
            })?,
        )),
    }
}

/// nwid 解析:十六进制 3~16 位,容错 0x 前缀;非法 → 400 badRequest
fn parse_nwid(s: &str) -> Result<u64, ApiErr> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if hex.len() < 3
        || hex.len() > 16
        || hex.is_empty()
        || !hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "badRequest",
            &format!("invalid network id \"{s}\" (expect 3-16 hex digits)"),
            None,
        ));
    }
    u64::from_str_radix(hex, 16).map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            "badRequest",
            &format!("invalid network id \"{s}\": {e}"),
            None,
        )
    })
}

/// 网络定位(leave/set 等已加入网络的操作):合法 nwid(0x 容错)直用;
/// 否则按网络名(不区分大小写)在该 planet 已加入的网络中解析。同名多条 →
/// 409 列候选不猜;无命中 → 404。join 不走此路径:未加入的网络名字无从
/// 得知,只认 nwid
fn resolve_nwid(rt: &crate::state::PlanetRuntime, input: &str) -> Result<u64, ApiErr> {
    if let Ok(v) = parse_nwid(input) {
        return Ok(v);
    }
    let nets = rt.networks();
    let hits: Vec<_> = nets
        .iter()
        .filter(|c| c.name.eq_ignore_ascii_case(input.trim()))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one.nwid),
        [] => Err(err(
            StatusCode::NOT_FOUND,
            "networkNotFound",
            &format!("network \"{input}\" not found (by id or name)"),
            None,
        )),
        many => Err(err(
            StatusCode::CONFLICT,
            "networkAmbiguous",
            &format!("network name \"{input}\" matches {} networks", many.len()),
            Some(
                many.iter()
                    .map(|c| format!("{:016x} ({})", c.nwid, c.name))
                    .collect(),
            ),
        )),
    }
}

/// NetLocal 默认项:managed 路由走 ZeroTier 接口,无 DNS 覆写
fn default_net_local() -> NetLocal {
    NetLocal {
        route_via_zero_tier: true,
        dns_mode: "none".into(),
        custom_dns: Vec::new(),
    }
}

/// 读取某 planet 的 net_local(优先运行时镜像,回退 registry 持久层)
pub(crate) fn net_local_of(rt: Option<&Arc<PlanetRuntime>>, entry: &RegistryEntry, nwid: u64) -> NetLocal {
    if let Some(rt) = rt {
        if let Some(nl) = rt
            .net_local
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&nwid)
        {
            return nl.clone();
        }
    }
    entry
        .net_local
        .get(&format!("{nwid:016x}"))
        .cloned()
        .unwrap_or_else(default_net_local)
}

/// 取运行时;定位链已判 running 但运行时缺失(启动失败等)→ 409 planetNotRunning
pub(crate) fn not_running(st: &AppState, pid: &str) -> Result<Arc<PlanetRuntime>, ApiErr> {
    runtime_of(st, pid).ok_or_else(|| {
        err(
            StatusCode::CONFLICT,
            "planetNotRunning",
            &format!("planet \"{pid}\" is not running"),
            None,
        )
    })
}

// ---- 投影 ----

fn mac_str(mac: u64) -> String {
    let b = (mac & 0x0000_ffff_ffff_ffff).to_be_bytes();
    b[2..8]
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// 核心 NetConfig + net_local → NetworkEntry(planet-port-api 类型)
fn to_network_entry(
    planet_id: &str,
    cfg: &NetConfig,
    nl: &NetLocal,
    conflict: Option<String>,
    if_name: Option<String>,
) -> NetworkEntry {
    NetworkEntry {
        planet: planet_id.to_string(),
        nwid: format!("{:016x}", cfg.nwid),
        name: if cfg.name.is_empty() { None } else { Some(cfg.name.clone()) },
        status: runtime::status_name(&cfg.status).into(),
        conflict,
        kind: if cfg.is_private { "PRIVATE" } else { "PUBLIC" }.into(),
        mac: mac_str(cfg.mac),
        interface: if_name,
        mtu: u32::from(cfg.mtu),
        assigned_addrs: cfg
            .assigned
            .iter()
            .map(|a| format!("{}/{}", a.addr, a.prefix))
            .collect(),
        managed_routes: cfg
            .routes
            .iter()
            .map(|r| RouteLine {
                target: format!("{}/{}", r.target.addr, r.target.prefix),
                via: r.via.map(|v| v.to_string()),
            })
            .collect(),
        dns_servers: cfg.dns_servers.iter().map(|s| s.to_string()).collect(),
        route_via_zero_tier: nl.route_via_zero_tier,
        // 一期无锁定语义,恒 false
        route_via_locked: false,
        dns_mode: nl.dns_mode.clone(),
        custom_dns: nl.custom_dns.clone(),
    }
}

// ---- GET /networks ----

pub async fn networks_list(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    // registry 只在开头/结尾短锁:运行时列表与 net_local 各取一份克隆
    let (filter, entries) = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (filter_planet(&reg, &q)?, reg.planets.clone())
    };
    let order: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
    let mut items: Vec<NetworkEntry> = Vec::new();
    for pid in order {
        if let Some(f) = &filter {
            if *f != pid {
                continue;
            }
        }
        let Some(rt) = runtime_of(&st, &pid) else { continue };
        let entry = match entries.iter().find(|e| e.id == pid) {
            Some(e) => e.clone(),
            None => continue,
        };
        let conflicts = rt
            .conflicts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for cfg in rt.networks() {
            let nl = net_local_of(Some(&rt), &entry, cfg.nwid);
            items.push(to_network_entry(
                &pid,
                &cfg,
                &nl,
                conflicts.get(&cfg.nwid).cloned(),
                rt.if_name(cfg.nwid),
            ));
        }
    }
    Ok(Json(json!({ "networks": items })))
}

// ---- POST /networks/{nwid}/join ----

pub async fn net_join(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    // 定位(registry 锁在 resolve_running_planet 内短持)与运行时获取(planets
    // 锁)分开:两锁不得嵌套
    let pid = resolve_running_planet(&st, q.get("planet").map(|s| s.as_str()))?;
    let rt = not_running(&st, &pid)?;
    let nwid_u = parse_nwid(&nwid)?;
    // 已加入 → 幂等成功(对齐 zerotier-cli:重复 join 不报错,批量脚本可重入)
    if rt.networks().iter().any(|c| c.nwid == nwid_u) {
        st.events.push(&rt.planet_id, "join", &format!("already joined {nwid} (idempotent)"));
        return Ok(Json(json!({"result": "ok", "idempotent": true})));
    }
    // join 在驱动线程执行并等应答,放阻塞线程池
    let rt2 = Arc::clone(&rt);
    tokio::task::spawn_blocking(move || rt2.node_join(nwid_u))
        .await
        .map_err(|e| internal(format!("join 任务失败: {e}")))?
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "joinFailed",
                &format!("join {nwid} failed: {e}"),
                None,
            )
        })?;
    // net_local 无记录则建默认项(registry 持久层 + 运行时镜像)
    let key = format!("{nwid_u:016x}");
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == rt.planet_id) {
            e.net_local.entry(key.clone()).or_insert_with(default_net_local);
            if let Err(err) = reg.save(&st.config_dir) {
                eprintln!("[net] registry 写盘失败: {err}");
            }
        }
    }
    rt.net_local
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(nwid_u)
        .or_insert_with(default_net_local);
    st.events.push(&rt.planet_id, "join", &format!("joined {nwid}"));
    Ok(Json(json!({"result": "ok"})))
}

// ---- POST /networks/{nwid}/leave ----

pub async fn net_leave(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    // 定位(registry 锁在 resolve_running_planet 内短持)与运行时获取(planets
    // 锁)分开:两锁不得嵌套
    let pid = resolve_running_planet(&st, q.get("planet").map(|s| s.as_str()))?;
    let rt = not_running(&st, &pid)?;
    let nwid_u = resolve_nwid(&rt, &nwid)?;
    // 未加入 → 幂等成功(leave 的目标态就是"不在网络里",已达成)
    if !rt.networks().iter().any(|c| c.nwid == nwid_u) {
        st.events.push(&rt.planet_id, "leave", &format!("not joined {nwid} (idempotent)"));
        return Ok(Json(json!({"result": "ok", "idempotent": true})));
    }
    let rt2 = Arc::clone(&rt);
    tokio::task::spawn_blocking(move || rt2.node_leave(nwid_u))
        .await
        .map_err(|e| internal(format!("leave 任务失败: {e}")))?
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "leaveFailed",
                &format!("leave {nwid} failed: {e}"),
                None,
            )
        })?;
    // 设备面拆除 + 快照/冲突清理;net_local 保留(重新 join 恢复用户配置)
    rt.plane
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .teardown(nwid_u);
    st.net_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&(rt.planet_id.clone(), nwid_u));
    rt.conflicts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&nwid_u);
    st.events.push(&rt.planet_id, "leave", &format!("left {nwid}"));
    // DNS:该网络的系统 resolver 若归本 planet 所有则还原(失败仅记事件)。
    // 持 dns_gate:与 netconf apply / stop 的 restore 互斥,标记文件的
    // check-then-act 不能交叠
    let _dns_gate = st.dns_gate.lock().await;
    {
        let events = Arc::clone(&st.events);
        let config_dir = st.config_dir.clone();
        let pid = rt.planet_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            crate::dns::restore(&config_dir, &events, &pid, Some(nwid_u))
        })
        .await;
    }
    Ok(Json(json!({"result": "ok"})))
}

// ---- POST /networks/{nwid}(set)----

pub async fn net_set(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
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
    let rt = runtime_of(&st, &pid).ok_or_else(|| {
        err(
            StatusCode::CONFLICT,
            "planetNotRunning",
            &format!("planet \"{pid}\" is not running"),
            None,
        )
    })?;
    let nwid_u = resolve_nwid(&rt, &nwid)?;
    // 仅已加入网络可配置(404 networkNotFound)
    if !rt.networks().iter().any(|c| c.nwid == nwid_u) {
        return Err(err(
            StatusCode::NOT_FOUND,
            "networkNotFound",
            &format!("network {nwid} not joined on this planet"),
            None,
        ));
    }
    // 契约字段:routeViaZeroTier bool / dnsMode 枚举 / customDns 数组
    let mut nl = net_local_of(Some(&rt), &entry, nwid_u);
    if let Some(v) = body.get("routeViaZeroTier") {
        match v.as_bool() {
            Some(b) => nl.route_via_zero_tier = b,
            None => {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "badRequest",
                    "routeViaZeroTier must be a boolean",
                    None,
                ))
            }
        }
    }
    if let Some(v) = body.get("dnsMode").and_then(|v| v.as_str()) {
        if !matches!(v, "none" | "network" | "custom") {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "badRequest",
                &format!("dnsMode must be one of none|network|custom, got \"{v}\""),
                None,
            ));
        }
        nl.dns_mode = v.into();
    }
    if let Some(v) = body.get("customDns") {
        let Some(arr) = v.as_array() else {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "badRequest",
                "customDns must be an array of strings",
                None,
            ));
        };
        let mut out = Vec::new();
        for x in arr {
            match x.as_str() {
                Some(s) => out.push(s.to_string()),
                None => {
                    return Err(err(
                        StatusCode::BAD_REQUEST,
                        "badRequest",
                        "customDns must be an array of strings",
                        None,
                    ))
                }
            }
        }
        nl.custom_dns = out;
    }
    // 写持久层 + 运行时镜像;生效 = 下一轮 ConfigUpdate(见模块注释)
    let key = format!("{nwid_u:016x}");
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) {
            e.net_local.insert(key, nl.clone());
            if let Err(err) = reg.save(&st.config_dir) {
                eprintln!("[net] registry 写盘失败: {err}");
            }
        }
    }
    rt.net_local
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(nwid_u, nl.clone());
    st.events.push(
        &pid,
        "set",
        &format!("network {nwid} settings updated"),
    );
    let cfg = rt
        .networks()
        .into_iter()
        .find(|c| c.nwid == nwid_u);
    let conflict = rt
        .conflicts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&nwid_u)
        .cloned();
    let network = match cfg {
        Some(c) => to_network_entry(&pid, &c, &nl, conflict, rt.if_name(c.nwid)),
        None => return Err(internal("network snapshot disappeared".into())),
    };
    Ok(Json(json!({ "network": network })))
}

// ---- GET /events ----

pub async fn events(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    let filter = {
        let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        filter_planet(&reg, &q)?
    };
    let after: Option<u64> = q.get("after").and_then(|s| s.parse().ok());
    let limit: usize = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(40);
    let items = st.events.tail(filter.as_deref(), after, limit);
    Ok(Json(json!({ "events": items })))
}

fn internal(msg: String) -> ApiErr {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internalError",
        &msg,
        None,
    )
}

/// 网络面路由:/networks 系列 + /events
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/networks", get(networks_list))
        .route("/networks/{nwid}/join", post(net_join))
        .route("/networks/{nwid}/leave", post(net_leave))
        .route("/networks/{nwid}", post(net_set))
        .route("/events", get(events))
}
