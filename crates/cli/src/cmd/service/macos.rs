//! LaunchDaemon 平台实现:`install|uninstall|start|stop|logs|offline_diag`。
//!
//! install 把 daemon 二进制拷到
//! `/Library/Application Support/planet-portd/bin/planet-portd` 并写
//! `/Library/LaunchDaemons/cn.imgozi.d.planetport.daemon.plist`,之后经 launchctl
//! bootstrap 装载。plist 指向拷贝而非 target/debug 原路径:构建产物路径不稳定。
//! KeepAlive={SuccessfulExit=false}:崩溃/被强杀等异常退出由 launchd 自动
//! 重启,SIGTERM 优雅退出(exit 0)不重启。
//! install/uninstall/start/stop 需 root(euid≠0 → 中文提示 + 退出码 2);
//! status 无需特权。

use super::{json_done, locate_daemon_src, probe_online, progress, wait_online};
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
pub(crate) const BIN_PATH: &str = "/Library/Application Support/planet-portd/bin/planet-portd";
// install.rs 步骤 3 落盘的 CLI 与 PATH 软链(uninstall 一并清理)
pub(crate) const CLI_BIN: &str = "/Library/Application Support/planet-portd/bin/planet-port";
pub(crate) const LINK: &str = "/usr/local/bin/planet-port";
/// 数据目录(install 命令落盘 CLI/daemon 的根;--config-dir 亦指向此处)
/// Task 3 接线 install.rs 后经由 super 供外部使用,先临时放行 dead_code
#[allow(dead_code)]
pub(crate) const DATA_DIR: &str = APP_DIR;
// 日志放 /Library/Logs:路径无空格(newsyslog.conf 不支持带空格的文件名),
// 且是 macOS 约定的系统级日志位置,Console.app 可直接浏览
const LOG_DIR: &str = "/Library/Logs/planet-portd";
const LOG_PATH: &str = "/Library/Logs/planet-portd/daemon.log";
const NEWSYSLOG_CONF: &str = "/etc/newsyslog.d/planet-portd.conf";

pub(crate) fn install(json: bool) {
    // 1. 拷贝当前二进制到固定路径(plist 指向它,target/debug 路径不稳定)
    let src = locate_daemon_src();
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
        super::copy_exe(&src, BIN_PATH);
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
        progress(json, rust_i18n::t!("cli.service.legacyCleaned", label = LEGACY_LABEL).to_string());
    }

    // 4. 装载(RunAtLoad=true:bootstrap 装载即拉起一次,随开机自动启动;
    //    KeepAlive={SuccessfulExit=false}:仅异常退出(非零码/信号)时
    //    launchd 自动重启,SIGTERM 优雅退出(exit 0)不重启)。
    //    重复安装幂等:先 bootout 卸掉已加载的旧服务(未加载属预期),
    //    bootstrap 撞上「已装载」则提示用 start,不硬退。
    // pid 须在 bootout 前取:拆除中的 job 查不到 pid
    let old_pid = service_pid();
    match launchctl_output(&["bootout", &format!("system/{LABEL}")]) {
        Ok(_) => {
            progress(json, rust_i18n::t!("cli.service.bootedOutForReinstall", label = LABEL).to_string());
            // bootout 返回 ≠ 旧实例已退:daemon 收 SIGTERM 后要逐 planet 红线
            // 收尾(拆网卡/路由/DNS),期间立即 bootstrap 会被 launchd 以
            // Input/output error 5 拒绝。等旧 job/进程真正消失再装载
            wait_unloaded(old_pid, std::time::Duration::from_secs(10));
        }
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
            progress(json, rust_i18n::t!("cli.service.alreadyBootstrapped", label = LABEL).to_string());
            json_done(json, serde_json::json!({"result": "ok", "idempotent": true}));
            return;
        }
        eprintln!("{}", rust_i18n::t!("cli.service.bootstrapFailed", e = stderr.trim()));
        std::process::exit(1);
    }

    progress(json, rust_i18n::t!("cli.service.installed", label = LABEL).to_string());
    progress(json, rust_i18n::t!("cli.service.installedBin", path = BIN_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.installedPlist", path = PLIST_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.installedData", dir = APP_DIR).to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

/// install.rs 步骤 3:CLI/daemon 落盘 + PATH 软链。
/// installed 参数:从已安装路径运行时跳过自拷(rename 同路径会自删)
#[allow(dead_code)]
pub(crate) fn install_cli(cli_src: &Path, daemon_src: &Path, installed: bool, json: bool) {
    if !installed {
        super::copy_exe(cli_src, CLI_BIN);
        super::copy_exe(daemon_src, BIN_PATH);
    }
    // PATH 软链:已存在(软链或普通文件)一律替换;指回我们自己的 CLI_BIN
    if let Err(e) = replace_symlink(CLI_BIN, LINK) {
        eprintln!("{}", rust_i18n::t!("cli.install.linkFailed", link = LINK, e = e));
        std::process::exit(1);
    }
    progress(json, rust_i18n::t!("cli.install.linked", link = LINK, bin = CLI_BIN).to_string());
}

/// 建软链,目标位置已有任何东西(旧软链/普通文件)先移除
fn replace_symlink(target: &str, link: &str) -> std::io::Result<()> {
    match std::fs::symlink_metadata(link) {
        Ok(_) => std::fs::remove_file(link)?,
        Err(_) => {}
    }
    if let Some(dir) = std::path::Path::new(link).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::os::unix::fs::symlink(target, link)
}

pub(crate) fn uninstall(json: bool) {
    // 未加载/已卸载属预期,不算失败
    match launchctl_output(&["bootout", &format!("system/{LABEL}")]) {
        Ok(_) => progress(json, rust_i18n::t!("cli.service.unloaded", label = LABEL).to_string()),
        Err(stderr) => {
            let s = stderr.to_lowercase();
            if s.contains("not loaded") || s.contains("no such process") || s.contains("could not find") {
                progress(json, rust_i18n::t!("cli.service.notRunningSkip").to_string());
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
        progress(json, rust_i18n::t!("cli.service.legacyCleaned", label = LEGACY_LABEL).to_string());
    }
    let _ = fs::remove_file(BIN_PATH);
    // install 命令落盘的 CLI 与 PATH 软链一并清(仅当软链确实指向我们的 CLI)
    let _ = fs::remove_file(CLI_BIN);
    if let Ok(target) = fs::read_link(LINK) {
        if target == Path::new(CLI_BIN) {
            let _ = fs::remove_file(LINK);
        }
    }
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
    progress(json, rust_i18n::t!("cli.service.removed", plist = PLIST_PATH, bin = BIN_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.dataKept", dir = APP_DIR).to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

/// 读取 launchd 记录的服务 pid(pid = 0 或无此键 = 未在运行)
fn service_pid() -> Option<u32> {
    let out = launchctl_output_quiet(&["print", &format!("system/{LABEL}")]).ok()?;
    kv(&out, "pid").and_then(|p| p.parse().ok()).filter(|p| *p != 0)
}

pub(crate) fn start(json: bool) {
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
            progress(json, rust_i18n::t!("cli.service.started", port = port).to_string());
            json_done(json, serde_json::json!({"result": "ok", "port": port}));
        }
        false => {
            progress(json, rust_i18n::t!("cli.service.startTimeout", log = LOG_PATH).to_string());
            json_done(json, serde_json::json!({"result": "error"}));
        }
    }
}

pub(crate) fn stop(json: bool) {
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
        progress(json, rust_i18n::t!("cli.service.stopTimeout", log = LOG_PATH).to_string());
        json_done(json, serde_json::json!({"result": "error"}));
        return;
    }
    // pid 已消失:优雅退出本不会被 KeepAlive 拉回;若短暂窗口内又上线,
    // 说明它以非零码退出触发了自动重启,如实报告
    std::thread::sleep(std::time::Duration::from_secs(1));
    if probe_online().is_some() {
        progress(json, rust_i18n::t!("cli.service.keepaliveRelaunched").to_string());
        json_done(json, serde_json::json!({"result": "warning"}));
    } else {
        progress(json, rust_i18n::t!("cli.service.stopped").to_string());
        json_done(json, serde_json::json!({"result": "ok"}));
    }
}

/// daemon 不在线时的平台诊断:看 plist 是否存在判断「未安装」vs「已安装未
/// 运行」,并透出 launchctl 的 job 状态;内部自行 exit 3
pub(crate) fn offline_diag() {
    if !Path::new(PLIST_PATH).exists() {
        println!("{}", rust_i18n::t!("cli.service.notInstalled", path = PLIST_PATH));
        std::process::exit(3);
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
    std::process::exit(3);
}

/// 查看 daemon 日志:tail 末尾 N 行,--follow 时持续跟踪。
/// 文件缺失(daemon 从未运行/未安装)是明确错误而非空输出
pub(crate) fn logs(lines: usize, follow: bool) {
    if !Path::new(LOG_PATH).exists() {
        eprintln!("{}", rust_i18n::t!("cli.service.logMissing", path = LOG_PATH));
        std::process::exit(1);
    }
    let mut args: Vec<String> = vec!["-n".into(), lines.to_string()];
    if follow {
        args.push("-f".into());
    }
    args.push(LOG_PATH.into());
    match Command::new("tail").args(&args).status() {
        Ok(s) if s.success() => {}
        // tail 被信号杀死时 code() 为 None;Ctrl+C 的 130 由 SIGINT 命中 CLI
        // 自身产生,这里只兜非信号的异常退出
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("{}", rust_i18n::t!("cli.service.tailExecFailed", e = e));
            std::process::exit(1);
        }
    }
}

/// 重装路径:等待旧服务真正消失——launchd 域里查无此 job(print 失败)且
/// 已知旧 pid 已退出。超时兜底放行(照旧尝试 bootstrap,由其报错)
fn wait_unloaded(old_pid: Option<u32>, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let job_gone =
            launchctl_output_quiet(&["print", &format!("system/{LABEL}")]).is_err();
        let proc_gone = old_pid.map(|p| !pid_alive(p)).unwrap_or(true);
        if job_gone && proc_gone {
            return;
        }
        if std::time::Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// 信号 0 探测进程存活(zombie 也算存活,launchd 收尸后才会消失,恰好是我们要的语义)
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
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
