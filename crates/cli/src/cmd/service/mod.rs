//! `planet-port service <install|uninstall|start|stop|status|logs>` 服务管理。
//!
//! 平台服务管理(macOS launchd / Linux systemd)收敛在同名子模块 macos/linux
//! 中,经 cfg 编译期选择;本模块只放平台无关的命令分发与公共流程(HTTP 探测、
//! root 校验、原子拷贝、进度输出)。平台接口见 macos.rs/linux.rs 头部注释。

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "linux")]
use linux as platform;

// install.rs 依赖的平台落盘入口与 CLI 安装路径(其余常量仅平台内部使用)
pub(crate) use platform::{install_cli, CLI_BIN};

use crate::client::{CliError, Client};
use crate::output::print_json;
use planet_port_api::endpoints;
use planet_port_api::port_file;
use std::fs;
use std::path::Path;

#[derive(Clone, clap::Subcommand)]
pub enum ServiceCmd {
    /// 安装 LaunchDaemon(拷贝二进制、写 plist 并 bootstrap,需 root)
    Install,
    /// 卸载 LaunchDaemon(bootout 并删除二进制,数据目录保留,需 root)
    Uninstall,
    /// 启动 daemon(经 launchctl,需 root)
    Start,
    /// 停止 daemon(SIGTERM 走优雅退出,需 root)
    Stop,
    /// 查看 daemon 运行状态(无需 root)
    Status,
    /// 查看 daemon 日志(tail;--follow 持续跟踪)
    Logs {
        /// 显示末尾 N 行
        #[arg(long, default_value_t = 50)]
        lines: usize,
        /// 持续跟踪(类似 tail -f)
        #[arg(long)]
        follow: bool,
    },
}

pub fn run(cmd: ServiceCmd, json: bool) -> Result<(), CliError> {
    match cmd {
        ServiceCmd::Install => {
            require_root();
            platform::install(json);
        }
        ServiceCmd::Uninstall => {
            require_root();
            platform::uninstall(json);
        }
        ServiceCmd::Start => {
            require_root();
            platform::start(json);
        }
        ServiceCmd::Stop => {
            require_root();
            platform::stop(json);
        }
        ServiceCmd::Status => status(json)?,
        ServiceCmd::Logs { lines, follow } => platform::logs(lines, follow),
    }
    Ok(())
}

/// 变更进度回执:文本模式走 stdout;`-j` 模式 stdout 让位给结果 JSON,人话转 stderr
pub(crate) fn progress(json: bool, msg: String) {
    if json {
        eprintln!("{msg}");
    } else {
        println!("{msg}");
    }
}

/// `-j` 模式的机器回执:stdout 唯一一条 JSON;文本模式无输出
pub(crate) fn json_done(json: bool, v: serde_json::Value) {
    if json {
        print_json(&v);
    }
}

// ---- 公共流程 ----

pub(crate) fn require_root() {
    // euid 判定须真实:root 判断不准会误导安装失败原因(libc 已是 workspace 依赖)
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("{}", rust_i18n::t!("cli.service.needRoot"));
        std::process::exit(2);
    }
}

/// install.rs 步骤 4 转发:服务安装(装载/拉起)交给平台实现
pub(crate) fn install(json: bool) {
    platform::install(json);
}

/// 二进制更新用「临时文件 + rename」原子替换:直接 fs::copy 覆盖正在运行的
/// 二进制会因文本文件忙(macOS ETXTBSY)失败,rename 换 inode 则不受影响。
pub(crate) fn copy_exe(src: &Path, dst: &str) {
    let tmp = Path::new(dst).with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    if let Err(e) = fs::copy(src, &tmp) {
        eprintln!("{}", rust_i18n::t!("cli.service.copyFailed", src = src.display(), dst = tmp.display(), e = e));
        std::process::exit(1);
    }
    if let Err(e) = fs::rename(&tmp, dst) {
        let _ = fs::remove_file(&tmp);
        eprintln!("{}", rust_i18n::t!("cli.service.renameFailed", path = dst, e = e));
        std::process::exit(1);
    }
}

/// 定位与当前可执行文件同目录的 daemon 二进制:CLI 运行时取兄弟
/// planet-portd,从 daemon 自身运行时用自身;找不到则报错退出 1
pub(crate) fn locate_daemon_src() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("{}", rust_i18n::t!("cli.service.exeNotFound", e = e));
        std::process::exit(1);
    });
    if exe.file_name().and_then(|n| n.to_str()) == Some("planet-portd") {
        return exe;
    }
    let sibling = exe.parent().unwrap_or_else(|| std::path::Path::new(".")).join("planet-portd");
    if !sibling.exists() {
        eprintln!(
            "{}",
            rust_i18n::t!(
                "cli.service.daemonMissing",
                dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_default()
            )
        );
        std::process::exit(1);
    }
    sibling
}

/// 探测 daemon 是否在线:HTTP 有应答(含 401)即在线,返回端口。
/// 尊重 PLANET_PORT_URL:env 指向哪就探哪(旧版恒探真实 daemon,排障误导)
pub(crate) fn probe_online() -> Option<u16> {
    let client = Client::new_lenient(std::env::var("PLANET_PORT_URL").ok());
    match client.get_json(endpoints::version()) {
        Ok(_) | Err(CliError::Api(..)) => {
            Some(read_port_file().unwrap_or_else(|| port_of(client.base())))
        }
        Err(_) => None,
    }
}

/// 轮询等待 daemon HTTP 接口应答(在线),达成返回 true,超时返回 false。
/// 每次探测都按新 port 文件构造:启动可能绑定新端口,旧值会失效
pub(crate) fn wait_online(timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if probe_online().is_some() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
}

// ---- 工具 ----

/// 读 port 文件(daemon 启动写入;存在即活性信号)
pub(crate) fn read_port_file() -> Option<u16> {
    fs::read_to_string(port_file()).ok()?.trim().parse().ok().filter(|p| *p != 0)
}

/// 从 base URL 提取端口(兜底展示用)
pub(crate) fn port_of(base: &str) -> u16 {
    base.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0)
}

// ---- 子命令实现 ----

fn status(json: bool) -> Result<(), CliError> {
    // 宽松构造:token 缺失不报错;连接成功即在线(401 等任何 HTTP 响应都算,
    // 探测超时/连接失败才算不在线)。URL 尊重 PLANET_PORT_URL
    let client = Client::new_lenient(std::env::var("PLANET_PORT_URL").ok());
    match client.get_json(endpoints::version()) {
        Ok(v) => {
            let port = read_port_file().unwrap_or_else(|| port_of(client.base()));
            if json {
                let planets = client
                    .get_json(endpoints::status())
                    .ok()
                    .and_then(|s| s.get("planets").cloned())
                    .unwrap_or_else(|| serde_json::json!([]));
                print_json(&serde_json::json!({
                    "running": true, "port": port, "version": v, "planets": planets
                }));
            } else {
                println!("{}", rust_i18n::t!("cli.service.running", port = port));
                // planet 概览:名称 + 在线状态;daemon 在但无 planet 也一眼可见
                match client.get_json(endpoints::status()) {
                    Ok(s) => {
                        if let Some(planets) = s.get("planets").and_then(|p| p.as_array()) {
                            if planets.is_empty() {
                                println!("{}", rust_i18n::t!("cli.service.noPlanets"));
                            }
                            for p in planets {
                                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                                let state = if p.get("online").and_then(|v| v.as_bool()).unwrap_or(false) {
                                    // API 序列化为 camelCase(latencyMs/networkCount)
                                    let ms = p.get("latencyMs").and_then(|v| v.as_u64());
                                    match ms {
                                        Some(ms) => format!("ONLINE {ms}ms"),
                                        None => "ONLINE".into(),
                                    }
                                } else if p.get("status").and_then(|v| v.as_str()) == Some("running") {
                                    rust_i18n::t!("status.connecting").to_string()
                                } else {
                                    "stopped".into()
                                };
                                let nets = p.get("networkCount").and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("{}", rust_i18n::t!("cli.service.planetRow", name = name, state = state, nets = nets));
                            }
                        }
                    }
                    Err(CliError::Api(401, _)) => {}
                    Err(_) => println!("{}", rust_i18n::t!("cli.service.planetsUnavailable")),
                }
            }
            return Ok(());
        }
        Err(CliError::Api(401, _)) => {
            let port = read_port_file().unwrap_or_else(|| port_of(client.base()));
            println!("{}", rust_i18n::t!("cli.service.tokenFailed", port = port));
            // 异常态非零退出(1):「进程在但鉴权失败」不该被脚本读作健康
            std::process::exit(1);
        }
        // daemon 应答了非 401 的错误:进程在线,原样透传状态
        Err(e @ CliError::Api(..)) => return Err(e),
        Err(CliError::Offline) | Err(CliError::Io(_)) | Err(CliError::Usage(_)) => {}
    }

    // 不在线诊断留给平台实现,不在线 = 异常态,退出码 3(与 CLI 的 Offline 语义一致)
    platform::offline_diag();
    std::process::exit(3);
}
