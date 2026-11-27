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
use zt_core::{NetConfig, NetStatus};

use crate::registry::{NetDetached, NetLocal, RegistryEntry};
use crate::runtime;
use crate::state::{err, runtime_of, AppState, PlanetRuntime};
use crate::world;

type ApiErr = (StatusCode, Json<planet_port_api::types::ApiError>);

// ---- 定位链与校验 ----

/// 变更端点定位链:显式 ?planet= → 存在且 running;省略 → 按**登记数**自动选
/// (登记 1 个即选中,stopped 报 409 planetNotRunning;登记 ≥2 报 409
/// planetAmbiguous 列全部候选,零登记 409 noPlanetRunning)。
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
    let (cid, entry) = resolve_planet_entry(st, planet)?;
    if !entry.running {
        return Err(err(
            StatusCode::CONFLICT,
            "planetNotRunning",
            &format!(
                "planet \"{}\" is not running",
                crate::api_planet::name_of(&entry)
            ),
            None,
        ));
    }
    Ok(cid)
}

/// 定位目标 planet(不校验运行状态,供 join 的自动拉起路径复用):
/// 显式指定按 id/name/worldId 解析,未命中 404;自动选择按**登记数**计
/// (1 个即中,≥2 一律报歧义要求显式指定)。返回 canonical id + entry
/// 克隆,registry 锁内即放。
pub(crate) fn resolve_planet_entry(
    st: &AppState,
    planet: Option<&str>,
) -> Result<(String, RegistryEntry), ApiErr> {
    let reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(p) = planet {
        return match world::resolve_planet_id(&reg, p).and_then(|cid| {
            reg.planets
                .iter()
                .find(|x| x.id == cid)
                .cloned()
                .map(|x| (cid, x))
        }) {
            Some((cid, x)) => Ok((cid, x)),
            None => Err(err(
                StatusCode::NOT_FOUND,
                "planetNotFound",
                &format!("planet \"{p}\" not found"),
                None,
            )),
        };
    }
    match reg.planets.len() {
        0 => Err(err(
            StatusCode::CONFLICT,
            "noPlanetRunning",
            "no planet registered; add one first",
            None,
        )),
        1 => {
            let p = &reg.planets[0];
            Ok((p.id.clone(), p.clone()))
        }
        _ => {
            // 候选列全部登记行星(带运行状态;running 的附实际网络数,以核心
            // 快照为准——net_local 条目数与实际加入数可能短暂不一致)。网络数是
            // node 查询,须在不持 registry 锁时做
            let cands: Vec<(String, bool, String)> = reg
                .planets
                .iter()
                .map(|p| (crate::api_planet::name_of(p), p.running, p.id.clone()))
                .collect();
            drop(reg);
            let candidates: Vec<String> = cands
                .into_iter()
                .map(|(name, running, id)| {
                    if running {
                        match runtime_of(st, &id) {
                            Some(rt) => format!("{name} ({id}, running, {} networks)", rt.networks().len()),
                            None => format!("{name} ({id}, running)"),
                        }
                    } else {
                        format!("{name} ({id}, stopped)")
                    }
                })
                .collect();
            Err(err(
                StatusCode::CONFLICT,
                "planetAmbiguous",
                "multiple planets registered; specify ?planet=",
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

/// leave 的扩展定位:优先级 = 合法 nwid 直用 → 已加入网络名(resolve_nwid
/// 语义)→ 断开区 name 快照(断开的网络不在核心快照里,名字只活在快照中)
fn resolve_nwid_ext(
    rt: &crate::state::PlanetRuntime,
    entry: &RegistryEntry,
    input: &str,
) -> Result<u64, ApiErr> {
    if let Ok(v) = parse_nwid(input) {
        return Ok(v);
    }
    if let Ok(v) = resolve_nwid(rt, input) {
        return Ok(v);
    }
    resolve_detached_nwid(entry, input)
}

/// 按断开区 name 快照解析(不区分大小写);合法 nwid 直用,同名多条 409
/// 列候选,未命中 404
fn resolve_detached_nwid(entry: &RegistryEntry, input: &str) -> Result<u64, ApiErr> {
    if let Ok(v) = parse_nwid(input) {
        return Ok(v);
    }
    let hits: Vec<(&String, &NetDetached)> = entry
        .net_detached
        .iter()
        .filter(|(_, nd)| {
            nd.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(input.trim()))
        })
        .collect();
    match hits.as_slice() {
        [(k, _)] => u64::from_str_radix(k, 16)
            .map_err(|_| internal(format!("invalid detached nwid key \"{k}\""))),
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
                    .map(|(k, nd)| format!("{k} ({})", nd.name.clone().unwrap_or_default()))
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
        cached_name: None,
        alias: None,
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

/// 核心 NetConfig + net_local → NetworkEntry(planet-port-api 类型)。
/// device_error:该网络最近的设备层错误(PlanetRuntime.device_errors 快照)
fn to_network_entry(
    planet_id: &str,
    cfg: &NetConfig,
    nl: &NetLocal,
    conflict: Option<String>,
    if_name: Option<String>,
    device_error: Option<String>,
) -> NetworkEntry {
    // 核心已 OK 但设备层失败(feth 没建起来/地址路由没挂上):数据面不通,
    // 状态覆盖为 DEVICE_ERROR。REQUESTING 等中间态保持核心语义(那是
    // 「配置还没到」,与本地设备面失败是两回事)
    let status = if cfg.status == NetStatus::Ok && device_error.is_some() {
        "DEVICE_ERROR"
    } else {
        runtime::status_name(&cfg.status)
    };
    NetworkEntry {
        planet: planet_id.to_string(),
        nwid: format!("{:016x}", cfg.nwid),
        // 显示优先级:别名(用户设) > netconf 名 > 缓存名(无名过渡兜底),
        // 融合进 name 单字段下发,各端(卡片/托盘/CLI 定位)零改动生效
        name: nl.alias.clone().or_else(|| {
            if cfg.name.is_empty() { nl.cached_name.clone() } else { Some(cfg.name.clone()) }
        }),
        status: status.into(),
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
        let entry = match entries.iter().find(|e| e.id == pid) {
            Some(e) => e.clone(),
            None => continue,
        };
        let Some(rt) = runtime_of(&st, &pid) else {
            // 非运行(禁用/idle 停):按 registry net_local 出 STOPPED 占位
            // 条目 + net_detached 出 DISCONNECTED 占位条目——"已添加/已断开的
            // 网络"仍是用户的资产,不可因行星停止而从列表消失
            items.extend(placeholder_entries(&pid, &entry));
            items.extend(detached_entries(&pid, &entry));
            continue;
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
                rt.device_error(cfg.nwid),
            ));
        }
        items.extend(detached_entries(&pid, &entry));
    }
    Ok(Json(json!({ "networks": items })))
}

/// 非运行 planet 的网络占位条目:核心不在,拿不到 name/mac/地址等动态
/// 字段,只回 nwid + 本地配置;status=STOPPED 标识来源
fn placeholder_entries(pid: &str, entry: &RegistryEntry) -> Vec<NetworkEntry> {
    // HashMap 遍历无序:按 nwid 排序保证列表输出稳定
    let mut keys: Vec<&String> = entry.net_local.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| {
            let nl = &entry.net_local[k];
            placeholder_entry(pid, k, nl)
        })
        .collect()
}

/// 断开区投影:DISCONNECTED 占位行(name 用断开时快照,动态字段无核心可取)。
/// 断开 = 原 zt 的 leave(用户层词汇),差异是本地记录保留(net_local
/// 迁入 net_detached),连接即可恢复;记录仅可查询/连接/删除。
/// 防御:同 nwid 双区残留(理论不可达)以 net_local 为准,跳过重复行;
/// HashMap 遍历无序,按 nwid 排序保证列表输出稳定
fn detached_entries(pid: &str, entry: &RegistryEntry) -> Vec<NetworkEntry> {
    let mut keys: Vec<&String> = entry
        .net_detached
        .keys()
        .filter(|k| !entry.net_local.contains_key(*k))
        .collect();
    keys.sort();
    keys.into_iter()
        .map(|k| {
            let nd = &entry.net_detached[k];
            NetworkEntry {
                planet: pid.to_string(),
                nwid: k.to_string(),
                // 显示优先级:别名 > 断开时快照名(stopped 态断开无快照)> 缓存
                name: nd
                    .local
                    .alias
                    .clone()
                    .or(nd.name.clone())
                    .or(nd.local.cached_name.clone()),
                status: "DISCONNECTED".into(),
                // 断开原因(网段冲突自动断开时为冲突级别);用户主动断开为 None
                conflict: nd.block_reason.clone(),
                kind: "—".into(),
                mac: "—".into(),
                interface: None,
                mtu: 0,
                assigned_addrs: Vec::new(),
                managed_routes: Vec::new(),
                dns_servers: Vec::new(),
                route_via_zero_tier: nd.local.route_via_zero_tier,
                route_via_locked: false,
                dns_mode: nd.local.dns_mode.clone(),
                custom_dns: nd.local.custom_dns.clone(),
            }
        })
        .collect()
}

fn placeholder_entry(pid: &str, nwid: &str, nl: &NetLocal) -> NetworkEntry {
    NetworkEntry {
        planet: pid.to_string(),
        nwid: nwid.to_string(),
        // 显示优先级:别名 > 缓存名(从未收到过 netconf 则均 None → nwid)
        name: nl.alias.clone().or(nl.cached_name.clone()),
        status: "STOPPED".into(),
        conflict: None,
        kind: "—".into(),
        mac: "—".into(),
        interface: None,
        mtu: 0,
        assigned_addrs: Vec::new(),
        managed_routes: Vec::new(),
        dns_servers: Vec::new(),
        route_via_zero_tier: nl.route_via_zero_tier,
        route_via_locked: false,
        dns_mode: nl.dns_mode.clone(),
        custom_dns: nl.custom_dns.clone(),
    }
}

// ---- POST /networks/{nwid}/join ----

/// 网段预检:目标网络有全局快照(曾经连接,网段已知)且与其它激活网络
/// 精确重叠时返回第一个命中(对方 pid/nwid/重叠网段串)。判定口径与
/// runtime 冲突扫描一致(IpNet 精确相等),无快照则放行(netconf 兜底)
fn subnet_overlap(st: &AppState, pid: &str, nwid_u: u64) -> Option<(String, u64, String)> {
    let cache = st
        .net_cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let snap = cache.get(&(pid.to_string(), nwid_u))?;
    for ((p2, n2), s2) in cache.iter() {
        if p2 == pid && *n2 == nwid_u {
            continue;
        }
        if let Some(s) = snap.subnets.iter().find(|s| s2.subnets.contains(*s)) {
            return Some((p2.clone(), *n2, format!("{}/{}", s.addr, s.prefix)));
        }
    }
    None
}

pub async fn net_join(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    // 定位(registry 锁在 resolve_planet_entry 内短持)与运行时获取(planets
    // 锁)分开:两锁不得嵌套。join 是明确的联网意图,目标未运行时自动拉起
    // 再加入;禁用是用户的"别跑"意志,报错指引
    let (pid, entry) = resolve_planet_entry(&st, q.get("planet").map(|s| s.as_str()))?;
    if !entry.enabled {
        return Err(err(
            StatusCode::CONFLICT,
            "planetDisabled",
            &format!(
                "planet \"{}\" is disabled; enable it first",
                crate::api_planet::name_of(&entry)
            ),
            None,
        ));
    }
    let nwid_u = parse_nwid(&nwid)?;
    let key = format!("{nwid_u:016x}");
    // 网段预检:相同网段仅允许一个激活。目标网络有全局网段快照(曾经连接,
    // 网段已知)且与其它激活网络精确重叠 → 409 拒绝,指引先断开对方;全新
    // 网络无快照可查,由 netconf 到达时的冲突自动断开兜底
    if let Some(pair) = subnet_overlap(&st, &pid, nwid_u) {
        let (other_pid, other_nwid, subnet) = pair;
        let other_planet = st
            .net_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&(other_pid.clone(), other_nwid))
            .map(|s| s.planet_name.clone())
            .unwrap_or_default();
        return Err(err(
            StatusCode::CONFLICT,
            "subnetConflict",
            &format!(
                "network {nwid} subnet {subnet} already in use by network {other_nwid:016x} (planet \"{other_planet}\"); disconnect it first"
            ),
            None,
        ));
    }
    // 断开区命中 → 自动恢复(等价 connect):回迁记录后走正常 join 路径;
    // 未运行时随后的 start_planet 按 net_local 全量 re-join(含本网络),
    // 已运行时镜像在锁外插入、下方 node_join 生效
    let mut restored = None;
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) {
            if let Some(nd) = e.net_detached.remove(&key) {
                e.net_local.entry(key.clone()).or_insert(nd.local.clone());
                if let Err(e) = reg.save(&st.config_dir) {
                    eprintln!("[net] registry 写盘失败: {e}");
                }
                restored = Some(nd.local);
            }
        }
    }
    // 运行时镜像与事件在 registry 锁外补(runtime_of 取 planets 锁,
    // 不得嵌套);planet 未运行时无镜像可插,start_planet 按 net_local 灌注
    if let Some(nl) = restored {
        if let Some(rt) = runtime_of(&st, &pid) {
            rt.net_local
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(nwid_u)
                .or_insert(nl);
        }
        st.events.push(&pid, "networkConnected", &format!("restored {nwid} (via join)"));
    }
    // 未运行(running=false:leave 清空后的资源性停止等)→ 自动拉起;
    // running=true 但运行时缺失的残留(上次启动失败等)同样由幂等的
    // start_planet 纠正(其内部在门闩内重读 registry,禁用竞态不放行)
    if !entry.running || runtime_of(&st, &pid).is_none() {
        runtime::start_planet(&st, &pid)
            .await
            .map_err(|m| err(StatusCode::INTERNAL_SERVER_ERROR, "startFailed", &m, None))?;
    }
    let rt = not_running(&st, &pid)?;
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
    // net_local 无记录则建默认项(registry 持久层 + 运行时镜像;断开区
    // 回迁的记录已在上方写入,此处 or_insert 不覆盖恢复配置)
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

/// 底层脱离与资源回收(leave/disconnect 共用收尾):核心已加入该网络时走
/// node_leave + 设备面拆除 + 快照/冲突清理 + DNS 还原;随后按派生规则
/// 判定 Idle 停机(net_local 镜像清空时;断开区不算「有事可做」)。
/// 前置:调用方已完成记录迁移/删除并同步运行时镜像
async fn detach_and_idle(st: &AppState, rt: &Arc<PlanetRuntime>, nwid_u: u64) -> Result<(), ApiErr> {
    let nwid = format!("{nwid_u:016x}");
    if rt.networks().iter().any(|c| c.nwid == nwid_u) {
        let rt2 = Arc::clone(rt);
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
        // 设备面拆除 + 快照/冲突清理(记录由调用方处理)
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
        // DNS:该网络的系统 resolver 若归本 planet 所有则还原(失败仅记
        // 事件)。持 dns_gate:与 netconf apply / stop 的 restore 互斥,标记
        // 文件的 check-then-act 不能交叠。守卫必须收在本块内:下方 idle
        // 停机会经 stop_planet 再次拿 dns_gate,tokio Mutex 不可重入
        {
            let _dns_gate = st.dns_gate.lock().await;
            let events = Arc::clone(&st.events);
            let config_dir = st.config_dir.clone();
            let pid = rt.planet_id.clone();
            let _ = tokio::task::spawn_blocking(move || {
                crate::dns::restore(&config_dir, &events, &pid, Some(nwid_u))
            })
            .await;
        }
    }
    // 派生运行规则:net_local(镜像)清空 → 资源性停机(并发 join 落盘则
    // 门闩内重评不放行)
    if rt
        .net_local
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_empty()
    {
        runtime::stop_planet(st, &rt.planet_id, runtime::StopMark::Idle).await;
    }
    Ok(())
}

// ---- POST /networks/{nwid}/leave ----

/// 断开(表层与 zt leave 同名,语义为用户层"断开"):复用断开核心动作,
/// net_local → net_detached 迁移保留本地配置与名字快照,join 可恢复。
/// 彻底删除走 DELETE /networks/{nwid}(remove)
pub async fn net_leave(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    // 定位不要求 running:stopped planet 上的网络也能断开(纯记录迁移)
    let (pid, entry) = resolve_planet_entry(&st, q.get("planet").map(|s| s.as_str()))?;
    let rt = runtime_of(&st, &pid);
    // nwid 解析:核心在 → nwid / 已加入名 / 断开区快照名;核心不在 →
    // nwid / 断开区快照名
    let nwid_u = match &rt {
        Some(rt) => resolve_nwid_ext(rt, &entry, &nwid)?,
        None => resolve_detached_nwid(&entry, &nwid)?,
    };
    disconnect_network_inner(&st, &pid, nwid_u, None).await?;
    Ok(Json(json!({"result": "ok"})))
}

// ---- DELETE /networks/{nwid} ----

/// 彻底删除:在网先脱离(记录迁入断开区),随后清除 net_local 与
/// net_detached 全部记录;列表不再显示,重新 join 按控制器默认配置重来。
/// 幂等:两区皆无记录 → 成功
pub async fn net_remove(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    let (pid, entry) = resolve_planet_entry(&st, q.get("planet").map(|s| s.as_str()))?;
    let rt = runtime_of(&st, &pid);
    let nwid_u = match &rt {
        Some(rt) => resolve_nwid_ext(rt, &entry, &nwid)?,
        None => resolve_detached_nwid(&entry, &nwid)?,
    };
    let key = format!("{nwid_u:016x}");
    // 在网或 net_local 仍有记录 → 先走断开收尾(底层脱离 + 记录迁移);
    // 断开区记录直接落下方删除
    let joined = rt.as_ref().is_some_and(|r| r.networks().iter().any(|c| c.nwid == nwid_u));
    if joined || entry.net_local.contains_key(&key) {
        disconnect_network_inner(&st, &pid, nwid_u, None).await?;
    }
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) {
            let hit = e.net_local.remove(&key).is_some() | e.net_detached.remove(&key).is_some();
            if hit {
                if let Err(err) = reg.save(&st.config_dir) {
                    eprintln!("[net] registry 写盘失败: {err}");
                }
            }
        }
    }
    st.events.push(&pid, "networkRemoved", &format!("removed {nwid_u:016x}"));
    Ok(Json(json!({"result": "ok"})))
}

// ---- leave/remove 共用的断开核心动作 ----

/// 断开核心动作(net_leave 与冲突自动断开共用):net_local →
/// net_detached 记录迁移(保留本地配置与名字快照,可携断开原因)+ 底层
/// 脱离;net_local 清空则 idle 停机(派生规则同 leave 收尾)。planet 未
/// 运行时无核心,仅做记录迁移。幂等:已在断开区 → 仅补做底层脱离
/// (上次 node_leave 失败的幽灵加入防御)。前置:pid 已注册
pub(crate) async fn disconnect_network_inner(
    st: &AppState,
    pid: &str,
    nwid_u: u64,
    block_reason: Option<String>,
) -> Result<(), ApiErr> {
    let rt = runtime_of(st, pid);
    let nwid = format!("{nwid_u:016x}");
    let key = format!("{nwid_u:016x}");
    let entry = {
        let reg = st
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match reg.planets.iter().find(|e| e.id == pid) {
            Some(e) => e.clone(),
            None => {
                return Err(err(
                    StatusCode::NOT_FOUND,
                    "planetNotFound",
                    &format!("planet \"{pid}\" is not registered"),
                    None,
                ))
            }
        }
    };
    if !entry.enabled {
        return Err(err(
            StatusCode::CONFLICT,
            "planetDisabled",
            &format!(
                "planet \"{}\" is disabled; enable it first",
                crate::api_planet::name_of(&entry)
            ),
            None,
        ));
    }
    // 幂等:已在断开区 → 成功。但记录在断开区不代表核心已脱离:上次
    // disable 的 node_leave 若失败,记录已迁移而核心仍 joined,重试命中
    // 本分支时必须补做底层脱离,否则核心"幽灵加入"直至 stop/connect
    if entry.net_detached.contains_key(&key) {
        if let Some(rt) = rt.as_ref().filter(|rt| rt.networks().iter().any(|c| c.nwid == nwid_u)) {
            // 同步运行时镜像(detach_and_idle 前置:调用方完成记录/镜像处理)
            rt.net_local
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&nwid_u);
            detach_and_idle(st, rt, nwid_u).await?;
        }
        st.events.push(pid, "networkDisconnected", &format!("already disconnected {nwid} (idempotent)"));
        return Ok(());
    }
    // 名字快照取核心(可得时)
    let name = rt
        .as_ref()
        .and_then(|rt| {
            rt.networks()
                .into_iter()
                .find(|c| c.nwid == nwid_u)
                .map(|c| if c.name.is_empty() { None } else { Some(c.name) })
        })
        .flatten();
    // 记录迁移(net_local → net_detached;半态防御:核心已加入但记录缺失
    // 时按现行配置快照入库)
    {
        let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) {
            // registry 锁内读 rt.networks()(node 锁)依赖「sink 绝不取
            // registry」的单向保证;勿在 sink 侧引入反向取 registry 的调用
            let joined = rt
                .as_ref()
                .is_some_and(|r| r.networks().iter().any(|c| c.nwid == nwid_u));
            let nl = match e.net_local.remove(&key) {
                Some(nl) => nl,
                None if !joined => {
                    return Err(err(
                        StatusCode::NOT_FOUND,
                        "networkNotFound",
                        &format!("network {nwid} not joined on this planet"),
                        None,
                    ))
                }
                None => net_local_of(rt.as_ref(), e, nwid_u),
            };
            e.net_detached.insert(key, NetDetached { local: nl, name, block_reason });
            if let Err(e) = reg.save(&st.config_dir) {
                eprintln!("[net] registry 写盘失败: {e}");
            }
        }
    }
    st.events.push(pid, "networkDisconnected", &format!("disconnected {nwid}"));
    // 无核心:记录迁移即完成
    let Some(rt) = rt else {
        return Ok(());
    };
    // 底层脱离(与 leave 同款收尾;记录已在断开区,不删)
    rt.net_local
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&nwid_u);
    detach_and_idle(st, &rt, nwid_u).await?;
    Ok(())
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
    // 别名:字符串 trim 后空串 = 清除(None);显示优先于控制器名/缓存名
    if let Some(v) = body.get("alias") {
        let Some(s) = v.as_str() else {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "badRequest",
                "alias must be a string",
                None,
            ));
        };
        let s = s.trim();
        nl.alias = if s.is_empty() { None } else { Some(s.to_string()) };
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
        Some(c) => to_network_entry(&pid, &c, &nl, conflict, rt.if_name(c.nwid), rt.device_error(c.nwid)),
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
        .route("/networks/{nwid}", post(net_set).delete(net_remove))
        .route("/events", get(events))
}
