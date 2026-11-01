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
#[cfg(windows)]
mod service_win;
mod state;
mod world;

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::net::Ipv4Addr;
#[cfg(unix)]
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

/// 取加密随机字节。Unix 走 /dev/urandom;Windows 走 BCryptGenRandom 的
/// 系统首选 RNG(免算法句柄生命周期管理)。调用方自行做 hex 编码。
#[cfg(unix)]
fn fill_random(buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(windows)]
fn fill_random(buf: &mut [u8]) -> std::io::Result<()> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "BCryptGenRandom 失败: NTSTATUS {status:#x}"
        )))
    }
}

/// 取 32 字节随机数,hex 编码为 64 字符 token(不引入 getrandom 依赖)
fn gen_token() -> std::io::Result<String> {
    let mut buf = [0u8; 32];
    fill_random(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// 写文件并设 0644。token/port 文件权限对齐官方 macOS zerotier authtoken.secret 的可读性:
/// CLI 以普通用户运行需能读到 token 才能调 API;API 仅绑 127.0.0.1,不暴露到网络。
/// Windows 无 POSIX mode 位,跳过(数据目录 ACL 由 install 阶段统一设置)
fn write_file_0644(path: &Path, data: &str) -> std::io::Result<()> {
    fs::write(path, data)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

/// 命令行参数。service 仅 Windows 有意义:SCM 以服务身份启动时由安装脚本
/// (Task 7 的 sc create)带上,是「服务上下文 vs 控制台直跑」的判定入口
struct Args {
    config_dir: Option<PathBuf>,
    #[cfg(windows)]
    service: bool,
}

fn parse_args() -> Args {
    let mut config_dir = None;
    #[cfg(windows)]
    let mut service = false;
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
            #[cfg(windows)]
            "--service" => service = true,
            other => {
                #[cfg(unix)]
                eprintln!("未知参数: {other}(仅支持 --config-dir <path>)");
                #[cfg(windows)]
                eprintln!("未知参数: {other}(仅支持 --config-dir <path> 与 --service)");
                std::process::exit(2);
            }
        }
    }
    Args {
        config_dir,
        #[cfg(windows)]
        service,
    }
}

/// acquire_singleton 的失败分类:「锁被其它实例持有」与「锁资源本身拿不到」
/// 是两种不同故障,排查方向不同,文案必须可区分
enum SingletonError {
    /// 锁资源创建/打开失败(权限、磁盘等):不是「已有实例」
    Open(std::io::Error),
    /// 锁被其它实例持有(Unix: flock EWOULDBLOCK;Windows: 互斥体已存在)
    Locked,
    /// flock 的其它错误(如 EBADF/EINVAL):按锁文件故障报告
    /// (Windows 的 CreateMutexW 只有「创建失败/已存在」两种结果,无此分类)
    #[cfg(unix)]
    Lock(std::io::Error),
}

/// 单例锁的持有物:Unix 是 daemon.lock 的文件句柄,Windows 是命名互斥体
/// 句柄。两者都必须存活至进程结束——锁随句柄/进程退出由内核释放
#[cfg(unix)]
type SingletonGuard = fs::File;

#[cfg(windows)]
struct SingletonGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SingletonGuard {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// 单实例守卫(Unix):对 `<config>/daemon.lock` 取 `flock(LOCK_EX | LOCK_NB)`。
/// 第二个实例加锁失败即退出,杜绝两个 daemon 共用同一 config 目录互相
/// 覆写 registry/token/port。锁文件不删除、内容不复用。
#[cfg(unix)]
fn acquire_singleton(dir: &Path) -> Result<SingletonGuard, SingletonError> {
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

/// 单实例守卫(Windows):命名互斥体 `Global\planetportd`。CreateMutexW
/// 返回已存在对象的句柄时 GetLastError 为 ERROR_ALREADY_EXISTS,语义对齐
/// flock 的「锁被其它实例持有」。互斥体不带初始所有权(FALSE)也不显式
/// 释放:句柄存活至进程结束,对象随最后一个句柄消失,进程崩溃时由内核
/// 回收——与 flock「锁随进程退出释放」一致。Global\ 前缀保证服务上下文
/// (Task 6 的 SCM,session 0)与控制台直跑(用户 session)互斥到同一把锁。
#[cfg(windows)]
fn acquire_singleton(_dir: &Path) -> Result<SingletonGuard, SingletonError> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Global\\planetportd\0".encode_utf16().collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(SingletonError::Open(std::io::Error::last_os_error()));
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return Err(SingletonError::Locked);
    }
    Ok(SingletonGuard(handle))
}

/// Windows 优雅退出的停止信号源:watch(bool) 静态通道。SCM 控制回调
/// (Task 6 的 service_win.rs)在任意线程调 `request_shutdown()` 置位,
/// `shutdown_signal()` 侧收到后走与 SIGTERM 等价的收尾路径。选 watch 而非
/// Notify:watch 保留最新值,回调先于监听方 await 触发也不会丢信号。
#[cfg(windows)]
static STOP_CHANNEL: std::sync::OnceLock<
    (tokio::sync::watch::Sender<bool>, tokio::sync::watch::Receiver<bool>),
> = std::sync::OnceLock::new();

#[cfg(windows)]
fn stop_channel()
-> &'static (tokio::sync::watch::Sender<bool>, tokio::sync::watch::Receiver<bool>) {
    STOP_CHANNEL.get_or_init(|| tokio::sync::watch::channel(false))
}

/// 请求优雅退出(Windows):从任意线程(含 SCM 的 extern "system" 回调)
/// 同步调用,非 async。service_win.rs 的控制回调(STOP/SHUTDOWN 控制码)
/// 经此触发收尾;控制台直跑时不经此路径。
#[cfg(windows)]
fn request_shutdown() {
    let _ = stop_channel().0.send(true);
}

/// 等待首个优雅退出信号,返回信号名供收尾日志。平台分叉:
/// - Unix:SIGTERM/SIGINT(mac/linux 行为零改动)
/// - Windows:控制台直跑 Ctrl-C;服务上下文 STOP 通道(Task 6 置位)
#[cfg(unix)]
async fn shutdown_signal() -> &'static str {
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
        _ = &mut term_fut => "SIGTERM",
        _ = &mut int_fut => "SIGINT",
    }
}

#[cfg(windows)]
async fn shutdown_signal() -> &'static str {
    let mut stop_rx = stop_channel().1.clone();
    // 服务停止可能先于监听方注册(理论上不会,启动远早于 STOP;防御性兜底)
    if *stop_rx.borrow_and_update() {
        return "SERVICE_STOP";
    }
    let stopped = async move {
        while stop_rx.changed().await.is_ok() {
            if *stop_rx.borrow_and_update() {
                break;
            }
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => "CTRL_C",
        _ = stopped => "SERVICE_STOP",
    }
}

/// 优雅退出:SIGINT/SIGTERM → 逐 planet 走 stop_planet 红线收尾 → registry
/// 兜底落盘 → 最后删 port 文件(存在即活性信号,CLI 据此快速判断)。
/// 作为 `with_graceful_shutdown` 的 future 传入:收尾在服务停止前完成,
/// 在途请求不会被打断,也不被收尾无限拖住(收尾本身有界)。
async fn shutdown(state: AppState, port_path: PathBuf) {
    let sig = shutdown_signal().await;
    eprintln!("收到 {sig},正在停止服务");
    // 待停清单先克隆(短锁):id 取 registry 中标记 running 的(期望运行,含
    // 上次启动失败的残留),再并入运行时 map 中可能多出的 id;name 一并带出
    // 供日志展示,运行时残留无登记名时以 id 兜底
    let mut items: Vec<(String, String)> = state
        .registry
        .lock()
        .unwrap()
        .planets
        .iter()
        .filter(|e| e.running)
        .map(|e| (e.id.clone(), e.name.clone().unwrap_or_else(|| e.id.clone())))
        .collect();
    for id in state.planets.lock().unwrap().keys().cloned().collect::<Vec<_>>() {
        if !items.iter().any(|(i, _)| i == &id) {
            items.push((id.clone(), id.clone()));
        }
    }
    for (id, name) in &items {
        eprintln!("正在停止行星 {name}({id})");
        // ShutdownKeep:保留 registry 期望状态,daemon 下次启动自动恢复运行
        runtime::stop_planet(&state, id, runtime::StopMark::ShutdownKeep).await;
    }
    // registry 在每次变更/停止时均已落盘,此处兜底再存一次
    if let Err(e) = state.registry.lock().unwrap().save(&state.config_dir) {
        eprintln!("[shutdown] registry 兜底写盘失败: {e}");
    }
    if let Err(e) = fs::remove_file(&port_path) {
        if e.kind() != ErrorKind::NotFound {
            eprintln!("删除 port 文件失败: {e}");
        }
    }
    eprintln!("服务已停止");
}

/// daemon 主逻辑(既有 main 主体,服务/控制台双入口复用):config 解析到
/// graceful shutdown 收尾一整段。正常路径以 return 结束(退出码由入口的
/// ExitCode 给出);所有故障路径仍走 std::process::exit 非 0 退出。
/// 正常路径不再用 std::process::exit(0) 收尾:服务工作线程需要先 return
/// 让 service_proc 报 STOPPED,再由进程正常退出给 SCM 干净的 0
async fn daemon_main(args: Args) {
    // 1. config dir:--config-dir 覆盖 env/系统目录/用户目录三级解析
    let dir = args.config_dir.unwrap_or_else(planet_port_api::config_dir);
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
    // 占用才是「已有实例」;锁资源拿不到是环境故障(权限/磁盘),文案须可区分
    #[cfg(unix)]
    let lock_path = dir.join("daemon.lock");
    let _singleton = match acquire_singleton(&dir) {
        Ok(f) => f,
        Err(SingletonError::Locked) => {
            #[cfg(unix)]
            eprintln!(
                "another planet-portd instance is running (lock: {})",
                lock_path.display()
            );
            #[cfg(windows)]
            eprintln!("another planet-portd instance is running (mutex: Global\\planetportd)");
            std::process::exit(1);
        }
        Err(SingletonError::Open(e)) => {
            #[cfg(unix)]
            eprintln!("cannot open lock file {}: {e}", lock_path.display());
            #[cfg(windows)]
            eprintln!("cannot create singleton mutex Global\\planetportd: {e}");
            std::process::exit(1);
        }
        #[cfg(unix)]
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
                #[cfg(unix)]
                {
                    let _ = fs::set_permissions(&token_path, fs::Permissions::from_mode(0o644));
                }
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

    // 5. 恢复:registry 中 running=true 即期望运行的 planet(含优雅退出保留的
    //    状态),逐个拉起;失败(start_planet 内部已登记 planetStartFailed +
    //    running=false)则继续
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
    // 信号收尾完成即 return:退出码由入口函数的 ExitCode 统一给出(控制台
    // 直跑与服务身份均为 0;服务工作线程 return 后 service_proc 先报
    // STOPPED,进程再正常退出,SCM 看到干净的生命周期)
}

/// 构建 tokio runtime 并跑 daemon 主逻辑。服务/控制台两条入口共用:服务身份
/// 由 service_proc 的工作线程调,控制台直跑由 main 主线程调。多线程 +
/// enable_all 与原 #[tokio::main] 的默认展开等价,mac/linux 行为零改动。
fn run_daemon(args: Args) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("构建 tokio runtime 失败");
    rt.block_on(daemon_main(args));
}

/// 进程入口。main 保持同步:StartServiceCtrlDispatcherW 必须由进程主线程
/// 调用与 SCM 建立控制通道,不能在 tokio runtime 的工作线程里分发。
/// - Windows + --service:服务身份,阻塞至服务停止(Task 7 的 sc create)
/// - 其余(含 mac/linux、Windows 控制台直跑):构建 runtime 跑主逻辑
fn main() -> std::process::ExitCode {
    let args = parse_args();
    #[cfg(windows)]
    if args.service {
        let code = service_win::run_service(move || run_daemon(args));
        return if code == 0 {
            std::process::ExitCode::SUCCESS
        } else {
            eprintln!("Windows 服务分发失败: Win32 错误码 {code}");
            std::process::ExitCode::from(u8::try_from(code).unwrap_or(1))
        };
    }
    run_daemon(args);
    std::process::ExitCode::SUCCESS
}
