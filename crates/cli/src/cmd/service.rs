//! LaunchDaemon 服务管理:`planet-port service <install|uninstall|start|stop|status>`。
//!
//! install 把当前 CLI 同目录的 daemon 二进制(current_exe 上溯定位)拷到
//! `/Library/Application Support/planet-portd/bin/planet-portd` 并写
//! `/Library/LaunchDaemons/com.planetport.daemon.plist`,之后经 launchctl
//! bootstrap 装载。plist 指向拷贝而非 target/debug 原路径:构建产物路径不稳定。
//! KeepAlive={SuccessfulExit=false}:崩溃/被强杀等异常退出由 launchd 自动
//! 重启,SIGTERM 优雅退出(exit 0)不重启。
//! install/uninstall/start/stop 需 root(euid≠0 → 中文提示 + 退出码 2);
//! status 无需特权。

use crate::client::{CliError, Client};
use crate::output::print_json;
use planet_port_api::endpoints;
use planet_port_api::port_file;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const LABEL: &str = "com.planetport.daemon";
const PLIST_PATH: &str = "/Library/LaunchDaemons/com.planetport.daemon.plist";
const APP_DIR: &str = "/Library/Application Support/planet-portd";
const BIN_PATH: &str = "/Library/Application Support/planet-portd/bin/planet-portd";

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
}

pub fn run(cmd: ServiceCmd, json: bool) -> Result<(), CliError> {
    match cmd {
        ServiceCmd::Install => {
            require_root();
            install();
        }
        ServiceCmd::Uninstall => {
            require_root();
            uninstall();
        }
        ServiceCmd::Start => {
            require_root();
            start();
        }
        ServiceCmd::Stop => {
            require_root();
            stop();
        }
        ServiceCmd::Status => status(json)?,
    }
    Ok(())
}

// ---- 子命令实现 ----

fn require_root() {
    // euid 判定须真实:root 判断不准会误导安装失败原因(libc 已是 workspace 依赖)
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("该操作需要 root 权限,请使用: sudo planet-port service <install|uninstall|start|stop>");
        std::process::exit(2);
    }
}

fn install() {
    // 1. 拷贝当前二进制到固定路径(plist 指向它,target/debug 路径不稳定)
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("无法定位当前可执行文件: {e}");
        std::process::exit(1);
    });
    // 拷贝源是 CLI 同目录的 daemon 二进制 planet-portd,不是 CLI 自己:
    // current_exe 直接拿会拷成 CLI,plist 以 --config-dir 调它,clap 解析
    // 失败 exit 2,KeepAlive 反复拉起反复崩。从 daemon 自身(同名)运行
    // install 时才用自身
    let src = if exe.file_name().and_then(|n| n.to_str()) == Some("planet-portd") {
        exe.clone()
    } else {
        let sibling = exe.parent().unwrap_or_else(|| Path::new(".")).join("planet-portd");
        if !sibling.exists() {
            eprintln!(
                "在 {} 旁未找到 daemon 二进制 planet-portd,请先构建 daemon",
                exe.parent().map(|p| p.display().to_string()).unwrap_or_default()
            );
            std::process::exit(1);
        }
        sibling
    };
    let bin_dir = Path::new(BIN_PATH).parent().unwrap().to_path_buf();
    if let Err(e) = fs::create_dir_all(&bin_dir) {
        eprintln!("无法创建目录 {}: {e}", bin_dir.display());
        std::process::exit(1);
    }
    // 从已安装路径运行 install 时跳过自拷贝(fs::copy 同路径会失败)
    let same = fs::canonicalize(&src).ok().zip(fs::canonicalize(BIN_PATH).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if !same {
        copy_exe(&src, BIN_PATH);
    }
    if let Err(e) = fs::set_permissions(BIN_PATH, fs::Permissions::from_mode(0o755)) {
        eprintln!("设置 {BIN_PATH} 权限失败: {e}");
        std::process::exit(1);
    }

    // 2. 写 LaunchDaemon plist
    let plist = plist_content(BIN_PATH);
    if let Err(e) = fs::write(PLIST_PATH, plist) {
        eprintln!("写入 {PLIST_PATH} 失败: {e}");
        std::process::exit(1);
    }
    if let Err(e) = fs::set_permissions(PLIST_PATH, fs::Permissions::from_mode(0o644)) {
        eprintln!("设置 {PLIST_PATH} 权限失败: {e}");
        std::process::exit(1);
    }

    // 3. 装载(RunAtLoad=true:bootstrap 装载即拉起一次,随开机自动启动;
    //    KeepAlive={SuccessfulExit=false}:仅异常退出(非零码/信号)时
    //    launchd 自动重启,SIGTERM 优雅退出(exit 0)不重启)。
    //    重复安装幂等:先 bootout 卸掉已加载的旧服务(未加载属预期),
    //    bootstrap 撞上「已装载」则提示用 start,不硬退。
    match launchctl_output(&["bootout", &format!("system/{LABEL}")]) {
        Ok(_) => println!("已卸载旧服务 {LABEL}(重装)"),
        Err(stderr) => {
            let s = stderr.to_lowercase();
            if s.contains("not loaded") || s.contains("no such process") || s.contains("could not find") {
                // 首次安装,正常
            } else {
                eprintln!("launchctl bootout 失败: {}", stderr.trim());
                std::process::exit(1);
            }
        }
    }
    if let Err(stderr) = launchctl_output(&["bootstrap", "system/", PLIST_PATH]) {
        if stderr.to_lowercase().contains("already bootstrapped") {
            println!("服务 {LABEL} 已被 launchd 装载,跳过 bootstrap");
            return;
        }
        eprintln!("launchctl bootstrap 失败: {}", stderr.trim());
        std::process::exit(1);
    }

    println!("已安装 LaunchDaemon {LABEL}");
    println!("  二进制: {BIN_PATH}");
    println!("  配置:   {PLIST_PATH}");
    println!("  数据:   {APP_DIR}");
}

/// 二进制更新用「临时文件 + rename」原子替换:直接 fs::copy 覆盖正在运行的
/// 二进制会因文本文件忙(macOS ETXTBSY)失败,rename 换 inode 则不受影响。
fn copy_exe(src: &Path, dst: &str) {
    let tmp = Path::new(dst).with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    if let Err(e) = fs::copy(src, &tmp) {
        eprintln!("拷贝二进制 {} → {} 失败: {e}", src.display(), tmp.display());
        std::process::exit(1);
    }
    if let Err(e) = fs::rename(&tmp, dst) {
        let _ = fs::remove_file(&tmp);
        eprintln!("替换二进制 {} 失败: {e}", dst);
        std::process::exit(1);
    }
}

fn uninstall() {
    // 未加载/已卸载属预期,不算失败
    match launchctl_output(&["bootout", &format!("system/{LABEL}")]) {
        Ok(_) => println!("已卸载运行中的服务 {LABEL}"),
        Err(stderr) => {
            let s = stderr.to_lowercase();
            if s.contains("not loaded") || s.contains("no such process") || s.contains("could not find") {
                println!("服务未在运行,跳过卸载步骤");
            } else {
                eprintln!("launchctl bootout 失败: {}", stderr.trim());
            }
        }
    }
    let _ = fs::remove_file(PLIST_PATH);
    let _ = fs::remove_file(BIN_PATH);
    // bin 目录若已空则一并移除(数据目录保留)
    if let Some(dir) = Path::new(BIN_PATH).parent() {
        let _ = fs::remove_dir(dir);
    }
    println!("已删除 {PLIST_PATH} 与 {BIN_PATH}");
    println!("数据目录已保留:{APP_DIR}");
}

fn start() {
    // -k:已在运行则先杀再拉(幂等重启);KeepAlive={SuccessfulExit=false}
    // 只管异常退出后的自动重启,手动 stop(exit 0)不会被拉回
    launchctl(&["kickstart", "-k", &format!("system/{LABEL}")]);
    println!("已请求启动 {LABEL}");
}

fn stop() {
    // TERM 命中 daemon 的信号优雅退出路径:逐 planet 红线收尾后删 port 文件;
    // 退出码 0,KeepAlive 不会把它重新拉起
    launchctl(&["kill", "TERM", &format!("system/{LABEL}")]);
    println!("已发送 SIGTERM,{LABEL} 将优雅退出");
}

fn status(json: bool) -> Result<(), CliError> {
    // 宽松构造:token 缺失不报错;连接成功即在线(401 等任何 HTTP 响应都算,
    // 探测超时/连接失败才算不在线)
    let client = Client::new_lenient(None);
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
                println!("运行中(端口 {port})");
                // planet 概览:名称 + 在线状态;daemon 在但无 planet 也一眼可见
                match client.get_json(endpoints::status()) {
                    Ok(s) => {
                        if let Some(planets) = s.get("planets").and_then(|p| p.as_array()) {
                            if planets.is_empty() {
                                println!("  尚未登记任何 planet");
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
                                    "连接中".into()
                                } else {
                                    "stopped".into()
                                };
                                let nets = p.get("networkCount").and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("  {name}: {state},网络 {nets} 个");
                            }
                        }
                    }
                    Err(CliError::Api(401, _)) => {}
                    Err(_) => println!("  (planet 列表获取失败)"),
                }
            }
            return Ok(());
        }
        Err(CliError::Api(401, _)) => {
            let port = read_port_file().unwrap_or_else(|| port_of(client.base()));
            println!("运行中(端口 {port}),但 token 校验失败:请确认 authtoken.secret 与 daemon 配置目录一致");
            return Ok(());
        }
        // daemon 应答了非 401 的错误:进程在线,原样透传状态
        Err(e @ CliError::Api(..)) => return Err(e),
        Err(CliError::Offline) | Err(CliError::Io(_)) | Err(CliError::Usage(_)) => {}
    }

    // 不在线:看 plist 是否存在判断「未安装」vs「已安装未运行」
    if !Path::new(PLIST_PATH).exists() {
        println!("未安装(找不到 {PLIST_PATH})");
        return Ok(());
    }
    println!("已安装但未在运行");
    match launchctl_output_quiet(&["print", &format!("system/{LABEL}")]) {
        Ok(out) => {
            let state = kv(&out, "state").unwrap_or_else(|| "未知".into());
            let pid = kv(&out, "pid");
            let last = kv(&out, "last exit code");
            print!("  launchctl 状态: {state}");
            if let Some(p) = pid {
                print!(",pid {p}");
            }
            if let Some(c) = &last {
                print!(",上次退出码 {c}");
            }
            println!();
        }
        Err(_) => println!("  未被 launchd 加载"),
    }
    Ok(())
}

// ---- 工具 ----

fn plist_content(bin: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>--config-dir</string>
        <string>{APP_DIR}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>{APP_DIR}/daemon.log</string>
    <key>StandardErrorPath</key>
    <string>{APP_DIR}/daemon.log</string>
</dict>
</plist>
"#
    )
}

/// 执行 launchctl,失败打印 stderr 并退出 1(命令拼装错误/权限不足都是硬失败)
fn launchctl(args: &[&str]) {
    match launchctl_output(args) {
        Ok(_) => {}
        Err(stderr) => {
            eprintln!("launchctl {} 失败: {}", args.join(" "), stderr.trim());
            std::process::exit(1);
        }
    }
}

fn launchctl_output(args: &[&str]) -> Result<String, String> {
    match Command::new("launchctl").args(args).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(e) => Err(format!("无法执行 launchctl: {e}")),
    }
}

fn launchctl_output_quiet(args: &[&str]) -> Result<String, String> {
    match Command::new("launchctl").args(args).output() {
        Ok(o) => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Err(e) => Err(format!("无法执行 launchctl: {e}")),
    }
}

/// 读 port 文件(daemon 启动写入;存在即活性信号)
fn read_port_file() -> Option<u16> {
    fs::read_to_string(port_file()).ok()?.trim().parse().ok().filter(|p| *p != 0)
}

/// 从 base URL 提取端口(兜底展示用)
fn port_of(base: &str) -> u16 {
    base.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0)
}

/// 从 `launchctl print` 输出提取 `key = value` 行的 value
fn kv(out: &str, key: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        if k.trim() == key {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}
