use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use planet_port_api::types::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<Store>>,
    seq: Arc<AtomicU32>,
}

struct Store {
    planets: Vec<PlanetDetail>,
    networks: Vec<NetworkEntry>,
    peers: Vec<PeerEntry>,
    moons: Vec<MoonEntry>,
    events: Vec<EventEntry>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn err(
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

impl Store {
    fn init() -> Self {
        let mk_planet = |id: &str,
                         name: &str,
                         icon: &str,
                         kind: PlanetKind,
                         world_id: u64,
                         running: bool,
                         online: bool,
                         nets: usize,
                         port: u16|
         -> PlanetDetail {
            PlanetDetail {
                id: id.into(),
                name: name.into(),
                icon: icon.into(),
                color: "#4C7BD9".into(),
                kind,
                world_id,
                roots: vec!["1.2.3.4/9993".into(), "5.6.7.8/9993".into()],
                file_size: 1024,
                revision_time_ms: 1760000000000,
                last_node_id: Some("8d3f2a1b2c".into()),
                source: "builtin".into(),
                network_count: nets,
                port,
                status: if running { "running".into() } else { "stopped".into() },
                online,
            }
        };
        Store {
            planets: vec![
                mk_planet("official", "official", "animal.panda", PlanetKind::Official, 149604618, true, true, 2, 9994),
                mk_planet("2187945993", "company", "animal.fox", PlanetKind::Custom, 2187945993, true, true, 1, 9995),
                mk_planet("3129115810", "home-lab", "animal.owl", PlanetKind::Custom, 3129115810, false, false, 0, 9996),
            ],
            networks: vec![
                NetworkEntry {
                    planet: "official".into(),
                    nwid: "8056c2e21c000001".into(),
                    name: Some("dev".into()),
                    status: "OK".into(),
                    conflict: None,
                    kind: "PUBLIC".into(),
                    mac: "ba:16:37:84:6b:53".into(),
                    mtu: 2800,
                    assigned_addrs: vec!["10.147.17.30/24".into()],
                    managed_routes: vec![RouteLine { target: "10.147.17.0/24".into(), via: None }],
                    dns_servers: vec!["10.147.17.1".into()],
                    route_via_zero_tier: false,
                    route_via_locked: false,
                    dns_mode: "network".into(),
                    custom_dns: vec![],
                },
                NetworkEntry {
                    planet: "company".into(),
                    nwid: "9f77ac39c000001".into(),
                    name: Some("prod".into()),
                    status: "OK".into(),
                    conflict: None,
                    kind: "PRIVATE".into(),
                    mac: "ba:16:37:84:6b:54".into(),
                    mtu: 2800,
                    assigned_addrs: vec!["172.16.10.5/24".into()],
                    managed_routes: vec![RouteLine { target: "172.16.10.0/24".into(), via: None }],
                    dns_servers: vec![],
                    route_via_zero_tier: false,
                    route_via_locked: true,
                    dns_mode: "none".into(),
                    custom_dns: vec![],
                },
            ],
            peers: vec![
                PeerEntry { planet: "official".into(), address: "7cee7c9f1a".into(), role: "PLANET".into(), version: Some("1.16.2".into()), latency_ms: Some(38), path: Some("udp/1.2.3.4:9993".into()) },
                PeerEntry { planet: "official".into(), address: "4a2b8c9d0e".into(), role: "LEAF".into(), version: Some("1.16.2".into()), latency_ms: Some(6), path: Some("udp/5.6.7.8:9993".into()) },
                PeerEntry { planet: "company".into(), address: "a1b2c3d4e5".into(), role: "PLANET".into(), version: Some("1.16.2".into()), latency_ms: Some(12), path: None },
            ],
            moons: vec![MoonEntry {
                planet: "official".into(),
                moon_id: "0123456789abcdef".into(),
                seed: Some("cafebabe00000000".into()),
                from_file: false,
                cached: true,
            }],
            events: vec![
                EventEntry { planet: "official".into(), ts_ms: 1761000000000, kind: "status".into(), message: "node ONLINE".into() },
                EventEntry { planet: "company".into(), ts_ms: 1761000001000, kind: "conflict".into(), message: "subnet conflict avoided on 9f77ac39c000001".into() },
            ],
        }
    }
}

/// 定位链 ③④:显式指定 → 校验;省略 → 单 running 自动选,多 running 409
fn resolve_planet(
    store: &Store,
    planet: Option<&str>,
) -> Result<String, (StatusCode, Json<ApiError>)> {
    if let Some(p) = planet {
        return match store.planets.iter().find(|x| x.id == p || x.name == p) {
            Some(x) => Ok(x.id.clone()),
            None => Err(err(
                StatusCode::NOT_FOUND,
                "planetNotFound",
                &format!("planet \"{p}\" not found"),
                None,
            )),
        };
    }
    let running: Vec<&PlanetDetail> = store
        .planets
        .iter()
        .filter(|p| p.status == "running")
        .collect();
    match running.len() {
        1 => Ok(running[0].id.clone()),
        0 => Err(err(
            StatusCode::CONFLICT,
            "noPlanetRunning",
            "no running planet; specify ?planet=",
            None,
        )),
        _ => {
            let candidates = running
                .iter()
                .map(|p| format!("{} ({})", p.name, p.network_count))
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

async fn auth_mw(req: Request<Body>, next: Next) -> Response {
    let ok = req
        .headers()
        .get("x-zt1-auth")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if ok {
        next.run(req).await
    } else {
        err(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing X-ZT1-Auth header",
            None,
        )
        .into_response()
    }
}

fn log_event(store: &mut Store, planet: &str, kind: &str, message: &str) {
    store.events.push(EventEntry {
        planet: planet.into(),
        ts_ms: now_ms(),
        kind: kind.into(),
        message: message.into(),
    });
}

// ---- handlers ----

async fn version() -> Json<Value> {
    Json(json!({"version": "0.1.0-mock"}))
}

async fn status(State(st): State<AppState>) -> Json<Value> {
    let store = st.inner.lock().unwrap();
    let latency = |p: &PlanetDetail| if p.online { Some(38u64) } else { None };
    let items: Vec<PlanetStatus> = store
        .planets
        .iter()
        .map(|p| PlanetStatus {
            id: p.id.clone(),
            name: p.name.clone(),
            icon: p.icon.clone(),
            color: p.color.clone(),
            kind: p.kind.clone(),
            status: p.status.clone(),
            online: p.online,
            node_id: if p.online { p.last_node_id.clone() } else { None },
            core_version: if p.online { Some("1.16.2".into()) } else { None },
            latency_ms: latency(p),
            network_count: p.network_count,
            port: p.port,
        })
        .collect();
    Json(json!({ "planets": items }))
}

async fn planets_list(State(st): State<AppState>) -> Json<Value> {
    let store = st.inner.lock().unwrap();
    Json(json!({ "planets": store.planets }))
}

async fn planets_import(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<ApiError>)> {
    let has_source = body.get("source").and_then(|v| v.as_str()).is_some();
    let has_container = body.get("container").and_then(|v| v.as_str()).is_some();
    if !has_source && !has_container {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "badRequest",
            "body must contain \"source\" or \"container\"",
            None,
        ));
    }
    let n = st.seq.fetch_add(1, Ordering::SeqCst);
    let mut store = st.inner.lock().unwrap();
    let id = format!("{}", 5000000000u64 + n as u64);
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("planet-{n}"));
    let detail = PlanetDetail {
        id: id.clone(),
        name,
        icon: "animal.rabbit".into(),
        color: "#59A96A".into(),
        kind: PlanetKind::Custom,
        world_id: 5000000000 + n as u64,
        roots: vec!["9.9.9.9/9993".into()],
        file_size: 1024,
        revision_time_ms: now_ms(),
        last_node_id: None,
        source: if has_container {
            "qr".into()
        } else {
            body["source"].as_str().unwrap().into()
        },
        network_count: 0,
        port: 10000 + n as u16,
        status: "running".into(),
        online: false,
    };
    log_event(
        &mut store,
        &id,
        "planetImported",
        &format!("planet \"{}\" imported", detail.name),
    );
    store.planets.push(detail.clone());
    Ok((StatusCode::CREATED, Json(json!({ "planet": detail }))))
}

fn find_planet(
    store: &Store,
    id: &str,
) -> Result<usize, (StatusCode, Json<ApiError>)> {
    store
        .planets
        .iter()
        .position(|p| p.id == id || p.name == id)
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "planetNotFound",
                &format!("planet \"{id}\" not found"),
                None,
            )
        })
}

async fn planets_show(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let store = st.inner.lock().unwrap();
    let i = find_planet(&store, &id)?;
    Ok(Json(json!({ "planet": store.planets[i] })))
}

async fn planets_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let i = find_planet(&store, &id)?;
    if let Some(n) = body.get("name").and_then(|v| v.as_str()) {
        store.planets[i].name = n.into();
    }
    if let Some(c) = body.get("icon").and_then(|v| v.as_str()) {
        store.planets[i].icon = c.into();
    }
    if let Some(c) = body.get("color").and_then(|v| v.as_str()) {
        store.planets[i].color = c.into();
    }
    let pid = store.planets[i].id.clone();
    log_event(&mut store, &pid, "planetUpdated", "planet metadata updated");
    Ok(Json(json!({ "planet": store.planets[i] })))
}

async fn planet_start(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let i = find_planet(&store, &id)?;
    store.planets[i].status = "running".into();
    let pid = store.planets[i].id.clone();
    log_event(&mut store, &pid, "planetStarted", "planet started");
    Ok(Json(json!({ "planet": store.planets[i] })))
}

async fn planet_stop(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let i = find_planet(&store, &id)?;
    store.planets[i].status = "stopped".into();
    store.planets[i].online = false;
    let pid = store.planets[i].id.clone();
    log_event(&mut store, &pid, "planetStopped", "planet stopped");
    Ok(Json(json!({ "planet": store.planets[i] })))
}

async fn planets_delete(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let i = find_planet(&store, &id)?;
    let pid = store.planets[i].id.clone();
    let active = store.networks.iter().any(|n| n.planet == pid);
    if active {
        return Err(err(
            StatusCode::CONFLICT,
            "planetInUse",
            "planet has networks; leave them first",
            None,
        ));
    }
    let removed = store.planets.remove(i);
    log_event(
        &mut store,
        "official",
        "planetRemoved",
        &format!("planet \"{}\" removed", removed.name),
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn planet_export(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let store = st.inner.lock().unwrap();
    let i = find_planet(&store, &id)?;
    let mut bytes = vec![0x01u8];
    bytes.extend_from_slice(&store.planets[i].world_id.to_be_bytes());
    bytes.extend_from_slice(&store.planets[i].revision_time_ms.to_be_bytes());
    Ok(([("content-type", "application/octet-stream")], bytes).into_response())
}

async fn planet_share(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let store = st.inner.lock().unwrap();
    let i = find_planet(&store, &id)?;
    let ttl = body.get("ttlSeconds").and_then(|v| v.as_u64()).unwrap_or(300);
    let token = st.seq.fetch_add(1, Ordering::SeqCst);
    let url = format!("http://127.0.0.1:9998/share/{token}");
    let expires_at = now_ms() / 1000 + ttl;
    let _ = store.planets[i].id;
    Ok(Json(json!(ShareLink { url, expires_at })))
}

async fn networks_list(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let store = st.inner.lock().unwrap();
    let items: Vec<&NetworkEntry> = store
        .networks
        .iter()
        .filter(|n| q.get("planet").map(|p| n.planet == *p).unwrap_or(true))
        .collect();
    Json(json!({ "networks": items }))
}

async fn net_join(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let pid = resolve_planet(&store, q.get("planet").map(|s| s.as_str()))?;
    if store
        .networks
        .iter()
        .any(|n| n.planet == pid && n.nwid == nwid)
    {
        return Err(err(
            StatusCode::CONFLICT,
            "alreadyJoined",
            &format!("network {nwid} already joined on this planet"),
            None,
        ));
    }
    store.networks.push(NetworkEntry {
        planet: pid.clone(),
        nwid: nwid.clone(),
        name: None,
        status: "REQUESTING_CONFIGURATION".into(),
        conflict: None,
        kind: "PRIVATE".into(),
        mac: "ba:16:37:84:6b:55".into(),
        mtu: 2800,
        assigned_addrs: vec![],
        managed_routes: vec![],
        dns_servers: vec![],
        route_via_zero_tier: false,
        route_via_locked: false,
        dns_mode: "none".into(),
        custom_dns: vec![],
    });
    log_event(&mut store, &pid, "join", &format!("joined {nwid}"));
    Ok(Json(json!({"result": "ok"})))
}

async fn net_leave(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let pid = resolve_planet(&store, q.get("planet").map(|s| s.as_str()))?;
    let before = store.networks.len();
    store
        .networks
        .retain(|n| !(n.planet == pid && n.nwid == nwid));
    if store.networks.len() == before {
        return Err(err(
            StatusCode::NOT_FOUND,
            "networkNotFound",
            &format!("network {nwid} not joined on this planet"),
            None,
        ));
    }
    log_event(&mut store, &pid, "leave", &format!("left {nwid}"));
    Ok(Json(json!({"result": "ok"})))
}

async fn net_set(
    State(st): State<AppState>,
    Path(nwid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let pid = resolve_planet(&store, q.get("planet").map(|s| s.as_str()))?;
    let i = store
        .networks
        .iter()
        .position(|n| n.planet == pid && n.nwid == nwid)
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "networkNotFound",
                &format!("network {nwid} not joined on this planet"),
                None,
            )
        })?;
    if let Some(v) = body.get("routeViaZeroTier").and_then(|v| v.as_bool()) {
        store.networks[i].route_via_zero_tier = v;
    }
    if let Some(v) = body.get("dnsMode").and_then(|v| v.as_str()) {
        store.networks[i].dns_mode = v.into();
    }
    if let Some(v) = body.get("customDns").and_then(|v| v.as_array()) {
        store.networks[i].custom_dns =
            v.iter().filter_map(|x| x.as_str().map(String::from)).collect();
    }
    log_event(&mut store, &pid, "set", &format!("network {nwid} settings updated"));
    Ok(Json(json!({"network": store.networks[i]})))
}

async fn peers(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let store = st.inner.lock().unwrap();
    let items: Vec<&PeerEntry> = store
        .peers
        .iter()
        .filter(|p| q.get("planet").map(|f| p.planet == *f).unwrap_or(true))
        .collect();
    Json(json!({ "peers": items }))
}

async fn moons_list(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let store = st.inner.lock().unwrap();
    let items: Vec<&MoonEntry> = store
        .moons
        .iter()
        .filter(|m| q.get("planet").map(|p| m.planet == *p).unwrap_or(true))
        .collect();
    Json(json!({ "moons": items }))
}

async fn moon_import(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<ApiError>)> {
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
    let mut store = st.inner.lock().unwrap();
    let pid = resolve_planet(&store, q.get("planet").map(|s| s.as_str()))?;
    let moon_id = format!(
        "{:016x}",
        0x1234_5678_9abc_u64 + st.seq.fetch_add(1, Ordering::SeqCst) as u64
    );
    let entry = MoonEntry {
        planet: pid.clone(),
        moon_id: moon_id.clone(),
        seed: None,
        from_file: true,
        cached: false,
    };
    store.moons.push(entry.clone());
    log_event(&mut store, &pid, "moonImported", &format!("moon imported from {source}"));
    Ok((StatusCode::CREATED, Json(json!({ "moon": entry }))))
}

async fn moon_orbit(
    State(st): State<AppState>,
    Path(moon_id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let seed = body
        .get("seed")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "badRequest",
                "body must contain \"seed\"",
                None,
            )
        })?;
    let mut store = st.inner.lock().unwrap();
    let pid = resolve_planet(&store, q.get("planet").map(|s| s.as_str()))?;
    store.moons.push(MoonEntry {
        planet: pid.clone(),
        moon_id: moon_id.clone(),
        seed: Some(seed.into()),
        from_file: false,
        cached: false,
    });
    log_event(&mut store, &pid, "orbit", &format!("orbit {moon_id}"));
    Ok(Json(json!({"result": "ok"})))
}

async fn moon_deorbit(
    State(st): State<AppState>,
    Path(moon_id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let mut store = st.inner.lock().unwrap();
    let pid = resolve_planet(&store, q.get("planet").map(|s| s.as_str()))?;
    let before = store.moons.len();
    store
        .moons
        .retain(|m| !(m.planet == pid && m.moon_id == moon_id));
    if store.moons.len() == before {
        return Err(err(
            StatusCode::NOT_FOUND,
            "moonNotFound",
            &format!("moon {moon_id} not found on this planet"),
            None,
        ));
    }
    log_event(&mut store, &pid, "deorbit", &format!("deorbit {moon_id}"));
    Ok(StatusCode::NO_CONTENT)
}

async fn events(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let store = st.inner.lock().unwrap();
    let after: Option<u64> = q.get("after").and_then(|s| s.parse().ok());
    let limit: usize = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(40);
    let items: Vec<&EventEntry> = store
        .events
        .iter()
        .filter(|e| q.get("planet").map(|p| e.planet == *p).unwrap_or(true))
        .filter(|e| after.map(|a| e.ts_ms > a).unwrap_or(true))
        .rev()
        .take(limit)
        .collect();
    Json(json!({ "events": items }))
}

async fn dump(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<String, (StatusCode, Json<ApiError>)> {
    let store = st.inner.lock().unwrap();
    let pid = resolve_planet(&store, q.get("planet").map(|s| s.as_str()))?;
    let p = store.planets.iter().find(|p| p.id == pid).unwrap();
    Ok(format!(
        "planet {} (world {}) mock dump\nnetworks:\n{}\npeers: {}\nevents: {}",
        p.name,
        p.world_id,
        store
            .networks
            .iter()
            .filter(|n| n.planet == pid)
            .map(|n| format!("  {}", n.nwid))
            .collect::<Vec<_>>()
            .join("\n"),
        store.peers.iter().filter(|x| x.planet == pid).count(),
        store.events.iter().filter(|e| e.planet == pid).count(),
    ))
}

#[tokio::main]
async fn main() {
    let state = AppState {
        inner: Arc::new(Mutex::new(Store::init())),
        seq: Arc::new(AtomicU32::new(0)),
    };
    let app = Router::new()
        .route("/version", get(version))
        .route("/status", get(status))
        .route("/planets", get(planets_list).post(planets_import))
        .route(
            "/planets/{id}",
            get(planets_show).patch(planets_patch).delete(planets_delete),
        )
        .route("/planets/{id}/start", post(planet_start))
        .route("/planets/{id}/stop", post(planet_stop))
        .route("/planets/{id}/export", get(planet_export))
        .route("/planets/{id}/share", post(planet_share))
        .route("/networks", get(networks_list))
        .route("/networks/{nwid}/join", post(net_join))
        .route("/networks/{nwid}/leave", post(net_leave))
        .route("/networks/{nwid}", post(net_set))
        .route("/peers", get(peers))
        .route("/moons", get(moons_list).post(moon_import))
        .route("/moons/{moonid}/orbit", post(moon_orbit))
        .route("/moons/{moonid}", delete(moon_deorbit))
        .route("/events", get(events))
        .route("/dump", get(dump))
        .layer(middleware::from_fn(auth_mw))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:9998").await.unwrap();
    println!("mockd listening on http://127.0.0.1:9998 (any non-empty X-ZT1-Auth accepted)");
    axum::serve(listener, app).await.unwrap();
}
