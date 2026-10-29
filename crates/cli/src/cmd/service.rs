//! LaunchDaemon 服务管理:`planet-port service <install|uninstall|start|stop|status>`。
//!
//! install 把当前 CLI 同目录的 daemon 二进制(current_exe 上溯定位)拷到
//! `/Library/Application Support/planet-portd/bin/planet-portd` 并写
//! `/Library/LaunchDaemons/cn.imgozi.d.planetport.daemon.plist`,之后经 launchctl
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

// 服务标识对齐 iOS 反向域名命名规则(cn.imgozi.d 前缀);plist 文件名与
// label 保持一致。旧版曾用 com.planetport.daemon,install/uninstall 时
// 一并清理,避免开机时旧服务被重复拉起
const LABEL: &str = "cn.imgozi.d.planetport.daemon";
const PLIST_PATH: &str = "/Library/LaunchDaemons/cn.imgozi.d.planetport.daemon.plist";
const LEGACY_LABEL: &str = "com.planetport.daemon";
const LEGACY_PLIST_PATH: &str = "/Library/LaunchDaemons/com.planetport.daemon.plist";
const APP_DIR: &str = "/Library/Application Support/planet-portd";
const BIN_PATH: &str = "/Library/Application Support/planet-portd/bin/planet-portd";
// 日志放 /Library/Logs:路径无空格(newsyslog.conf 不支持带空格的文件名),
// 且是 macOS 约定的系统级日志位置,Console.app 可直接浏览
const LOG_DIR: &str = "/Library/Logs/planet-portd";
const LOG_PATH: &str = "/Library/Logs/planet-portd/daemon.log";
const NEWSYSLOG_CONF: &str = "/etc/newsyslog.d/planet-portd.conf";

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
        eprintln!("{}", rust_i18n::t!("cli.service.needRoot"));
        std::process::exit(2);
    }
}

fn install() {
    // 1. 拷贝当前二进制到固定路径(plist 指向它,target/debug 路径不稳定)
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("{}", rust_i18n::t!("cli.service.exeNotFound", e = e));
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
                "{}",
                rust_i18n::t!(
                    "cli.service.daemonMissing",
                    dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_default()
                )
            );
            std::process::exit(1);
        }
        sibling
    };
    let bin_dir = Path::new(BIN_PATH).parent().unwrap().to_path_buf();
    if let Err(e) = fs::create_dir_all(&bin_dir) {
        eprintln!("{}", rust_i18n::t!("cli.service.mkdirFailed", dir = bin_dir.display(), e = e));
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
        eprintln!("{}", rust_i18n::t!("cli.service.chmodFailed", path = BIN_PATH, e = e));
        std::process::exit(1);
    }

    // 2. 写 LaunchDaemon plist
    let plist = plist_content(BIN_PATH);
    if let Err(e) = fs::write(PLIST_PATH, plist) {
        eprintln!("{}", rust_i18n::t!("cli.service.writePlistFailed", path = PLIST_PATH, e = e));
        std::process::exit(1);
    }
    if let Err(e) = fs::set_permissions(PLIST_PATH, fs::Permissions::from_mode(0o644)) {
        eprintln!("{}", rust_i18n::t!("cli.service.chmodFailed", path = PLIST_PATH, e = e));
        std::process::exit(1);
    }

    // 2.5 日志目录 + newsyslog 轮转:launchd 不轮转 StandardErrorPath,
    // 无界增长;注册系统级 newsyslog(超 5MB 压缩滚动,保留 3 份)。
    // 失败只警告不阻断——轮转缺失不该挡住安装
    if let Err(e) = fs::create_dir_all(LOG_DIR) {
        eprintln!("{}", rust_i18n::t!("cli.service.logdirFailed", dir = LOG_DIR, e = e));
    }
    let rotation = format!(
        "# logfilename          owner:group mode count size when  flags\n\
         {LOG_PATH}           root:admin  644  3     5120 *     J\n"
    );
    if let Err(e) = fs::write(NEWSYSLOG_CONF, rotation) {
        eprintln!("{}", rust_i18n::t!("cli.service.newsyslogFailed", path = NEWSYSLOG_CONF, e = e));
    }
    // 旧版日志曾落数据目录,搬过来保留历史
    let legacy_log = format!("{APP_DIR}/daemon.log");
    if Path::new(&legacy_log).exists() && !Path::new(LOG_PATH).exists() {
        let _ = fs::rename(&legacy_log, LOG_PATH);
    }

    // 3. 清理旧版标识(com.planetport.daemon):bootout + 删 plist,
    //    残留的旧 plist 会在开机时把旧服务再拉起来,与新版并存
    if Path::new(LEGACY_PLIST_PATH).exists() {
        let _ = launchctl_output(&["bootout", &format!("system/{LEGACY_LABEL}")]);
        let _ = fs::remove_file(LEGACY_PLIST_PATH);
        println!("{}", rust_i18n::t!("cli.service.legacyCleaned", label = LEGACY_LABEL));
    }

    // 4. 装载(RunAtLoad=true:bootstrap 装载即拉起一次,随开机自动启动;
    //    KeepAlive={SuccessfulExit=false}:仅异常退出(非零码/信号)时
    //    launchd 自动重启,SIGTERM 优雅退出(exit 0)不重启)。
    //    重复安装幂等:先 bootout 卸掉已加载的旧服务(未加载属预期),
    //    bootstrap 撞上「已装载」则提示用 start,不硬退。
    match launchctl_output(&["bootout", &format!("system/{LABEL}")]) {
        Ok(_) => println!("{}", rust_i18n::t!("cli.service.bootedOutForReinstall", label = LABEL)),
        Err(stderr) => {
            let s = stderr.to_lowercase();
            if s.contains("not loaded") || s.contains("no such process") || s.contains("could not find") {
                // 首次安装,正常
            } else {
                eprintln!("{}", rust_i18n::t!("cli.service.bootoutFailed", e = stderr.trim()));
                std::process::exit(1);
            }
        }
    }
    if let Err(stderr) = launchctl_output(&["bootstrap", "system/", PLIST_PATH]) {
        if stderr.to_lowercase().contains("already bootstrapped") {
            println!("{}", rust_i18n::t!("cli.service.alreadyBootstrapped", label = LABEL));
            return;
        }
        eprintln!("{}", rust_i18n::t!("cli.service.bootstrapFailed", e = stderr.trim()));
        std::process::exit(1);
    }

    println!("{}", rust_i18n::t!("cli.service.installed", label = LABEL));
    println!("{}", rust_i18n::t!("cli.service.installedBin", path = BIN_PATH));
    println!("{}", rust_i18n::t!("cli.service.installedPlist", path = PLIST_PATH));
    println!("{}", rust_i18n::t!("cli.service.installedData", dir = APP_DIR));
}

/// 二进制更新用「临时文件 + rename」原子替换:直接 fs::copy 覆盖正在运行的
/// 二进制会因文本文件忙(macOS ETXTBSY)失败,rename 换 inode 则不受影响。
fn copy_exe(src: &Path, dst: &str) {
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

fn uninstall() {
    // 未加载/已卸载属预期,不算失败
    match launchctl_output(&["bootout", &format!("system/{LABEL}")]) {
        Ok(_) => println!("{}", rust_i18n::t!("cli.service.unloaded", label = LABEL)),
        Err(stderr) => {
            let s = stderr.to_lowercase();
            if s.contains("not loaded") || s.contains("no such process") || s.contains("could not find") {
                println!("{}", rust_i18n::t!("cli.service.notRunningSkip"));
            } else {
                eprintln!("{}", rust_i18n::t!("cli.service.bootoutFailed", e = stderr.trim()));
            }
        }
    }
    let _ = fs::remove_file(PLIST_PATH);
    // 旧版标识一并清理(可能同时存在)
    if Path::new(LEGACY_PLIST_PATH).exists() {
        let _ = launchctl_output(&["bootout", &format!("system/{LEGACY_LABEL}")]);
        let _ = fs::remove_file(LEGACY_PLIST_PATH);
        println!("{}", rust_i18n::t!("cli.service.legacyCleaned", label = LEGACY_LABEL));
    }
    let _ = fs::remove_file(BIN_PATH);
    // bin 目录若已空则一并移除(数据目录保留)
    if let Some(dir) = Path::new(BIN_PATH).parent() {
        let _ = fs::remove_dir(dir);
    }
    // 轮转配置与日志目录一并清理;日志文件留给用户查阅,不替用户删
    let _ = fs::remove_file(NEWSYSLOG_CONF);
    if let Ok(mut entries) = fs::read_dir(LOG_DIR) {
        if entries.next().is_none() {
            let _ = fs::remove_dir(LOG_DIR);
        }
    }
    println!("{}", rust_i18n::t!("cli.service.removed", plist = PLIST_PATH, bin = BIN_PATH));
    println!("{}", rust_i18n::t!("cli.service.dataKept", dir = APP_DIR));
}

/// 探测 daemon 是否在线:HTTP 有应答(含 401)即在线,返回端口
fn probe_online() -> Option<u16> {
    let client = Client::new_lenient(None);
    match client.get_json(endpoints::version()) {
        Ok(_) | Err(CliError::Api(..)) => {
            Some(read_port_file().unwrap_or_else(|| port_of(client.base())))
        }
        Err(_) => None,
    }
}

/// 读取 launchd 记录的服务 pid(pid = 0 或无此键 = 未在运行)
fn service_pid() -> Option<u32> {
    let out = launchctl_output_quiet(&["print", &format!("system/{LABEL}")]).ok()?;
    kv(&out, "pid").and_then(|p| p.parse().ok()).filter(|p| *p != 0)
}

/// 轮询等待 daemon HTTP 接口应答(在线),达成返回 true,超时返回 false。
/// 每次探测都按新 port 文件构造:启动可能绑定新端口,旧值会失效
fn wait_online(timeout: std::time::Duration) -> bool {
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

fn start() {
    // -k:已在运行则先杀再拉(幂等重启);KeepAlive={SuccessfulExit=false}
    // 只管异常退出后的自动重启,手动 stop(exit 0)不会被拉回
    let old_pid = service_pid();
    launchctl(&["kickstart", "-k", &format!("system/{LABEL}")]);
    // 重启场景先等旧 pid 消失再等新实例应答:kickstart 返回时旧实例可能
    // 仍在优雅退出并应答 HTTP,首轮探测会命中它误报成功(端口还可能是
    // 旧值,新实例若在崩溃循环则完全未被验证)
    if old_pid.is_some() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while service_pid() == old_pid && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    match wait_online(std::time::Duration::from_secs(15)) {
        true => {
            let port = probe_online().unwrap_or(0);
            println!("{}", rust_i18n::t!("cli.service.started", port = port));
        }
        false => println!("{}", rust_i18n::t!("cli.service.startTimeout", log = LOG_PATH)),
    }
}

fn stop() {
    // TERM 命中 daemon 的信号优雅退出路径:逐 planet 红线收尾后删 port 文件;
    // 退出码 0,KeepAlive 不会把它重新拉起
    launchctl(&["kill", "TERM", &format!("system/{LABEL}")]);
    // 以 launchd 的 pid 消失为准:HTTP 探测在 daemon 收尾卡住(假死)时
    // 会把「超时无应答」误判为已停止;pid 是进程存活的事实
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while service_pid().is_some() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if service_pid().is_some() {
        println!("{}", rust_i18n::t!("cli.service.stopTimeout", log = LOG_PATH));
        return;
    }
    // pid 已消失:优雅退出本不会被 KeepAlive 拉回;若短暂窗口内又上线,
    // 说明它以非零码退出触发了自动重启,如实报告
    std::thread::sleep(std::time::Duration::from_secs(1));
    if probe_online().is_some() {
        println!("{}", rust_i18n::t!("cli.service.keepaliveRelaunched"));
    } else {
        println!("{}", rust_i18n::t!("cli.service.stopped"));
    }
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
            return Ok(());
        }
        // daemon 应答了非 401 的错误:进程在线,原样透传状态
        Err(e @ CliError::Api(..)) => return Err(e),
        Err(CliError::Offline) | Err(CliError::Io(_)) | Err(CliError::Usage(_)) => {}
    }

    // 不在线:看 plist 是否存在判断「未安装」vs「已安装未运行」
    if !Path::new(PLIST_PATH).exists() {
        println!("{}", rust_i18n::t!("cli.service.notInstalled", path = PLIST_PATH));
        return Ok(());
    }
    println!("{}", rust_i18n::t!("cli.service.installedNotRunning"));
    match launchctl_output_quiet(&["print", &format!("system/{LABEL}")]) {
        Ok(out) => {
            let state = kv(&out, "state").unwrap_or_else(|| rust_i18n::t!("common.unknown").to_string());
            let pid = kv(&out, "pid");
            let last = kv(&out, "last exit code");
            print!("{}", rust_i18n::t!("cli.service.launchctlState", state = state));
            if let Some(p) = pid {
                print!("{}", rust_i18n::t!("cli.service.pidFragment", pid = p));
            }
            if let Some(c) = &last {
                print!("{}", rust_i18n::t!("cli.service.exitCodeFragment", code = c));
            }
            println!();
        }
        Err(_) => println!("{}", rust_i18n::t!("cli.service.notLoaded")),
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
    <string>{LOG_PATH}</string>
    <key>StandardErrorPath</key>
    <string>{LOG_PATH}</string>
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
            eprintln!("{}", rust_i18n::t!("cli.service.launchctlFailed", args = args.join(" "), e = stderr.trim()));
            std::process::exit(1);
        }
    }
}

fn launchctl_output(args: &[&str]) -> Result<String, String> {
    match Command::new("launchctl").args(args).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(e) => Err(rust_i18n::t!("cli.service.launchctlExecFailed", e = e).to_string()),
    }
}

fn launchctl_output_quiet(args: &[&str]) -> Result<String, String> {
    match Command::new("launchctl").args(args).output() {
        Ok(o) => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Err(e) => Err(rust_i18n::t!("cli.service.launchctlExecFailed", e = e).to_string()),
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
