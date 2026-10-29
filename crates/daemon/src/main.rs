//! planet-portd:多 planet ZeroTier 桌面守护进程。
//! config_dir 解析、token/port 文件、auth 中间件(常量时间 token 校验)、
//! /version、/status;planet 生命周期端点见 api_planet.rs,网络面见
//! api_net.rs,peers/moons/dump 见 api_misc.rs,实例运行时见 runtime.rs。

mod api_misc;
mod api_net;
mod api_planet;
mod cleanup;
mod dns;
mod events;
mod registry;
mod runtime;
mod state;
mod world;

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
use planet_port_api::types::PlanetStatus;
use planet_port_api::DEFAULT_API_PORT;
use state::{err, AppState};

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

async fn status(State(st): State<AppState>) -> Json<serde_json::Value> {
    // node 查询先于 registry 锁:持锁期间不做 node 调用(锁纪律)
    let live = runtime::live_infos(&st);
    let reg = st.registry.lock().unwrap();
    let items: Vec<PlanetStatus> = reg
        .planets
        .iter()
        .map(|e| api_planet::to_status(e, live.get(&e.id)))
        .collect();
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

/// acquire_singleton 的失败分类:「锁被其它实例持有」与「锁文件本身打不开」
/// 是两种不同故障,排查方向不同,文案必须可区分
enum SingletonError {
    /// 锁文件创建/打开失败(权限、磁盘等):不是「已有实例」
    Open(std::io::Error),
    /// flock 返回 EWOULDBLOCK(即 EAGAIN):锁被其它实例持有
    Locked,
    /// flock 的其它错误(如 EBADF/EINVAL):按锁文件故障报告
    Lock(std::io::Error),
}

/// 单实例守卫:对 `<config>/daemon.lock` 取 `flock(LOCK_EX | LOCK_NB)`。
/// 第二个实例加锁失败即退出,杜绝两个 daemon 共用同一 config 目录互相
/// 覆写 registry/token/port。锁文件不删除、内容不复用;返回的 File 须存活
/// 至进程结束(锁随进程退出由内核释放,显式删文件反而会引入竞态)。
fn acquire_singleton(dir: &Path) -> Result<fs::File, SingletonError> {
    use std::os::unix::io::AsRawFd;
    let path = dir.join("daemon.lock");
    let f = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(SingletonError::Open)?;
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        // 各平台 EWOULDBLOCK 与 EAGAIN 同值(Linux/macOS 均如此),单臂即可
        return match e.raw_os_error() {
            Some(libc::EWOULDBLOCK) => Err(SingletonError::Locked),
            _ => Err(SingletonError::Lock(e)),
        };
    }
    Ok(f)
}

/// 优雅退出:SIGINT/SIGTERM → 逐 planet 走 stop_planet 红线收尾 → registry
/// 兜底落盘 → 最后删 port 文件(存在即活性信号,CLI 据此快速判断)。
/// 作为 `with_graceful_shutdown` 的 future 传入:收尾在服务停止前完成,
/// 在途请求不会被打断,也不被收尾无限拖住(收尾本身有界)。
async fn shutdown(state: AppState, port_path: PathBuf) {
    use tokio::signal::unix::{signal, SignalKind};
    // 任一信号注册失败都不能让 shutdown future 立即 resolve(否则 daemon 会
    // 静默退出):注册成功的信号继续监听,全部失败则永不返回,daemon 持续服务
    let mut term = signal(SignalKind::terminate()).ok();
    let mut int = signal(SignalKind::interrupt()).ok();
    if term.is_none() {
        eprintln!("[shutdown] 注册 SIGTERM 监听失败,SIGTERM 优雅退出不可用");
    }
    if int.is_none() {
        eprintln!("[shutdown] 注册 SIGINT 监听失败,SIGINT 优雅退出不可用");
    }
    // select! 的 if 前置条件为 false 时 async 表达式仍会被求值,不能在其中
    // unwrap:先把 None 分支折叠成永不完成的 future,再进 select!
    if term.is_none() && int.is_none() {
        eprintln!("[shutdown] 信号监听全部不可用,daemon 将持续运行直至被强制结束");
    }
    let mut term_fut = Box::pin(async {
        match term.as_mut() {
            Some(s) => s.recv().await,
            None => std::future::pending().await,
        }
    });
    let mut int_fut = Box::pin(async {
        match int.as_mut() {
            Some(s) => s.recv().await,
            None => std::future::pending().await,
        }
    });
    tokio::select! {
        _ = &mut term_fut =>
            eprintln!("[shutdown] 收到 SIGTERM,开始优雅退出"),
        _ = &mut int_fut =>
            eprintln!("[shutdown] 收到 SIGINT,开始优雅退出"),
    }
    // 待停清单先克隆(短锁):id 取 registry 中标记 running 的(含上次启动
    // 失败的残留,stop_planet 会纠正),再并入运行时 map 中可能多出的 id
    let mut ids: Vec<String> = state
        .registry
        .lock()
        .unwrap()
        .planets
        .iter()
        .filter(|e| e.running)
        .map(|e| e.id.clone())
        .collect();
    for id in state.planets.lock().unwrap().keys().cloned().collect::<Vec<_>>() {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    for id in &ids {
        eprintln!("[shutdown] 停止 planet {id}");
        runtime::stop_planet(&state, id).await;
    }
    // registry 在每次变更/停止时均已落盘,此处兜底再存一次
    if let Err(e) = state.registry.lock().unwrap().save(&state.config_dir) {
        eprintln!("[shutdown] registry 兜底写盘失败: {e}");
    }
    if let Err(e) = fs::remove_file(&port_path) {
        if e.kind() != ErrorKind::NotFound {
            eprintln!("[shutdown] 删除 port 文件失败: {e}");
        }
    }
    eprintln!("[shutdown] 完成");
}

#[tokio::main]
async fn main() {
    // 1. config dir:--config-dir 覆盖 env/系统目录/用户目录三级解析
    let dir = parse_args().unwrap_or_else(planet_port_api::config_dir);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("无法创建配置目录 {}: {e}", dir.display());
        std::process::exit(1);
    }
    // 归一化为绝对路径(macOS 下 /var → /private/var):后续 planet dataDir 守卫
    // 依赖两侧 canonicalize 后的逐组件比较,启动时定死一次即可
    let dir = match fs::canonicalize(&dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("无法解析配置目录 {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    let token_path = dir.join("authtoken.secret");
    let port_path = dir.join("port");

    // 1.5 单实例守卫:在任何副作用(写 token/port、起 planet)之前。锁被
    // 占用才是「已有实例」;文件打不开是环境故障(权限/磁盘),文案须可区分
    let lock_path = dir.join("daemon.lock");
    let _singleton = match acquire_singleton(&dir) {
        Ok(f) => f,
        Err(SingletonError::Locked) => {
            eprintln!(
                "another planet-portd instance is running (lock: {})",
                lock_path.display()
            );
            std::process::exit(1);
        }
        Err(SingletonError::Open(e)) => {
            eprintln!("cannot open lock file {}: {e}", lock_path.display());
            std::process::exit(1);
        }
        Err(SingletonError::Lock(e)) => {
            eprintln!("cannot lock file {}: {e}", lock_path.display());
            std::process::exit(1);
        }
    };

    // 2. token:存在则读入,不存在则生成并写 0644
    let token = match fs::read_to_string(&token_path) {
        Ok(t) => {
            let t = t.trim().to_string();
            if t.is_empty() {
                eprintln!("token 文件 {} 为空,重新生成", token_path.display());
                let t = gen_token().unwrap();
                write_file_0644(&token_path, &t).unwrap();
                t
            } else {
                // 兼容旧版本写入的 0600:一次性归位 0644,失败可忽略(CLI 读不到才需处理)
                let _ = fs::set_permissions(&token_path, fs::Permissions::from_mode(0o644));
                t
            }
        }
        Err(_) => {
            let t = gen_token().unwrap();
            write_file_0644(&token_path, &t).unwrap();
            t
        }
    };

    // 3. bind 127.0.0.1:DEFAULT_API_PORT,被占用则 +1 重试(上限 +100)
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

    // 4. 注册表:仅启动加载,不写盘(import 等变更端点写盘)
    let state = AppState {
        config_dir: dir.clone(),
        api_port: port,
        registry: Arc::new(Mutex::new(registry::Registry::load(&dir))),
        events: Arc::new(events::Ring::new()),
        planets: Arc::new(Mutex::new(HashMap::new())),
        runtime_gate: Arc::new(tokio::sync::Mutex::new(())),
        dns_gate: Arc::new(tokio::sync::Mutex::new(())),
        net_cache: Arc::new(Mutex::new(HashMap::new())),
        shares: Arc::new(Mutex::new(HashMap::new())),
        token: Arc::new(token),
    };

    // 4.5 DNS 残留清理(kill -9 未走 stop 路径):恢复 planet 之前先把上次
    // 应用未还的系统 resolver 还原为自动获取。持 dns_gate:与后续 netconf /
    // leave / stop 的 DNS 路径同一把锁,此刻虽无并发方,语义保持统一
    {
        let _dns_gate = state.dns_gate.lock().await;
        let events = Arc::clone(&state.events);
        let dir_for_dns = dir.clone();
        tokio::task::spawn_blocking(move || dns::restore_leftover(&dir_for_dns, &events))
            .await
            .map_err(|e| eprintln!("[boot] DNS 残留清理任务失败: {e}"))
            .ok();
    }

    // 4.6 崩溃孤儿 feth 清扫(kill -9 未走 stop 路径):上次运行登记的
    // feth 设备对连同其上的路由会残留,本轮新建 pair 后路由因 EEXIST 装不上。
    // 在恢复 planet 之前销毁登记在册的孤儿;只动 registry 登记过的名字,
    // 绝不枚举系统 feth(官方 zerotier-one 的常驻 feth 不能误伤)
    {
        let st = state.clone();
        tokio::task::spawn_blocking(move || cleanup::destroy_leftover(&st))
            .await
            .map_err(|e| eprintln!("[boot] feth 孤儿清扫任务失败: {e}"))
            .ok();
    }

    // 5. 恢复:上次运行时标记 running=true 的逐个重启拉起;
    //    失败(start_planet 内部已登记 planetStartFailed + running=false)则继续
    let recover: Vec<registry::RegistryEntry> = state
        .registry
        .lock()
        .unwrap()
        .planets
        .iter()
        .filter(|e| e.running)
        .cloned()
        .collect();
    for e in recover {
        eprintln!("[boot] 恢复 planet {}(port {})", e.id, e.port);
        if let Err(m) = runtime::start_planet(&state, &e).await {
            eprintln!("[boot] planet {} 启动失败: {m}", e.id);
        }
    }

    // /share/<token> 在 auth 层之后挂载:分享链接本身即凭证,不走 X-ZT1-Auth
    let app = Router::new()
        .route("/version", get(version))
        .route("/status", get(status))
        .merge(api_planet::router())
        .merge(api_net::router())
        .merge(api_misc::router())
        .layer(middleware::from_fn_with_state(state.clone(), auth_mw))
        .route("/share/{token}", get(api_planet::share_download))
        .with_state(state.clone());
    println!(
        "planet-portd listening on http://127.0.0.1:{port} (config_dir: {})",
        dir.display()
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown(state, port_path))
        .await
        .unwrap();
    // 信号收尾完成后显式 exit 0:确保服务型运行(launchd)看到干净的退出码
    std::process::exit(0);
}
