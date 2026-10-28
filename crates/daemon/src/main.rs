//! planet-portd:多 planet ZeroTier 桌面守护进程。
//! 本任务为骨架:config_dir 解析、token/port 文件、axum 路由(常量时间 token 校验)、
//! /version、/status、/planets(registry 静态投影,无运行时信息)。

mod events;
mod registry;

use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read};
use std::net::Ipv4Addr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use planet_port_api::types::{ApiError, PlanetKind, PlanetStatus};
use planet_port_api::DEFAULT_API_PORT;
use registry::Registry;

/// 每 planet 实例的运行时信息(Task 4 定义真实字段;本任务仅占位撑起 AppState)
#[allow(dead_code)]
struct PlanetRuntime;

#[derive(Clone)]
struct AppState {
    registry: Arc<Mutex<Registry>>,
    // events/planets 在 Task 3/4(事件上报与实例运行时)接线后启用
    #[allow(dead_code)]
    events: Arc<events::Ring>,
    /// planet id → 运行时(Task 4 起填充;当前恒为空)
    #[allow(dead_code)]
    planets: Arc<Mutex<HashMap<String, PlanetRuntime>>>,
    token: Arc<String>,
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

/// 常量时间字节比较,避免逐字节提前返回造成的时序侧信道
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn auth_mw(State(st): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let supplied = req
        .headers()
        .get("x-zt1-auth")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if constant_time_eq(supplied.as_bytes(), st.token.as_bytes()) {
        next.run(req).await
    } else {
        err(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or invalid X-ZT1-Auth header",
            None,
        )
        .into_response()
    }
}

// ---- handlers ----

async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "version": "0.1.0" }))
}

/// registry 静态投影:无运行时信息,一律 online=false、core_version/latency/node_id 不下发
/// (与 mockd to_status 的 offline 分支形态一致);source=builtin 视为官方 planet
fn to_status(e: &registry::RegistryEntry) -> PlanetStatus {
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

async fn status(State(st): State<AppState>) -> Json<serde_json::Value> {
    let reg = st.registry.lock().unwrap();
    let items: Vec<PlanetStatus> = reg.planets.iter().map(to_status).collect();
    Json(serde_json::json!({ "planets": items }))
}

async fn planets_list(State(st): State<AppState>) -> Json<serde_json::Value> {
    let reg = st.registry.lock().unwrap();
    let items: Vec<PlanetStatus> = reg.planets.iter().map(to_status).collect();
    Json(serde_json::json!({ "planets": items }))
}

// ---- 启动 ----

/// 从 /dev/urandom 取 32 字节随机数,hex 编码为 64 字符 token(不引入 getrandom 依赖)
fn gen_token() -> std::io::Result<String> {
    let mut buf = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// 写文件并设 0644。token/port 文件权限对齐官方 macOS zerotier authtoken.secret 的可读性:
/// CLI 以普通用户运行需能读到 token 才能调 API;API 仅绑 127.0.0.1,不暴露到网络
fn write_file_0644(path: &Path, data: &str) -> std::io::Result<()> {
    fs::write(path, data)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

fn parse_args() -> Option<PathBuf> {
    let mut config_dir = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config-dir" => match args.next() {
                Some(v) => config_dir = Some(PathBuf::from(v)),
                None => {
                    eprintln!("--config-dir 需要一个路径参数");
                    std::process::exit(2);
                }
            },
            other => {
                eprintln!("未知参数: {other}(仅支持 --config-dir <path>)");
                std::process::exit(2);
            }
        }
    }
    config_dir
}

#[tokio::main]
async fn main() {
    // 1. config dir:--config-dir 覆盖 env/系统目录/用户目录三级解析
    let dir = parse_args().unwrap_or_else(planet_port_api::config_dir);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("无法创建配置目录 {}: {e}", dir.display());
        std::process::exit(1);
    }
    let token_path = dir.join("authtoken.secret");
    let port_path = dir.join("port");

    // 2. token:存在则读入,不存在则生成并写 0644
    let token = match fs::read_to_string(&token_path) {
        Ok(t) => {
            let t = t.trim().to_string();
            if t.is_empty() {
                eprintln!("token 文件 {} 为空,重新生成", token_path.display());
                gen_token().unwrap()
            } else {
                t
            }
        }
        Err(_) => {
            let t = gen_token().unwrap();
            write_file_0644(&token_path, &t).unwrap();
            t
        }
    };

    // 3. 注册表:仅启动加载,不写盘(无变更不产生 registry.json)
    let state = AppState {
        registry: Arc::new(Mutex::new(Registry::load(&dir))),
        events: Arc::new(events::Ring::new()),
        planets: Arc::new(Mutex::new(HashMap::new())),
        token: Arc::new(token),
    };

    // 4. bind 127.0.0.1:DEFAULT_API_PORT,被占用则 +1 重试(上限 +100)
    let mut port = DEFAULT_API_PORT;
    let listener = loop {
        match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            Ok(l) => break l,
            Err(e) if e.kind() == ErrorKind::AddrInUse && port < DEFAULT_API_PORT + 100 => {
                port += 1;
            }
            Err(e) => {
                eprintln!("绑定 127.0.0.1:{port} 失败: {e}");
                std::process::exit(1);
            }
        }
    };
    write_file_0644(&port_path, &port.to_string()).unwrap();

    let app = Router::new()
        .route("/version", get(version))
        .route("/status", get(status))
        .route("/planets", get(planets_list))
        .layer(middleware::from_fn_with_state(state.clone(), auth_mw))
        .with_state(state);
    println!(
        "planet-portd listening on http://127.0.0.1:{port} (config_dir: {})",
        dir.display()
    );
    axum::serve(listener, app).await.unwrap();
}
