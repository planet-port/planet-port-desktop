//! systemd 服务管理(Linux 平台实现,接口与 macos.rs 对齐)。
//!
//! 系统落点:
//!   /usr/local/bin/planet-port / planet-portd   ← 二进制直接落 PATH 规范位
//!   /etc/systemd/system/planet-portd.service    ← unit
//!   /var/lib/planet-portd                       ← 数据目录(api crate 同源)
//!   日志走 journald:journalctl -u planet-portd,无需轮转配置
//!
//! 语义对照(与 macos.rs 注释互参):
//!   Restart=on-failure ↔ KeepAlive={SuccessfulExit=false}
//!     (异常退出自动拉起;SIGTERM 优雅退出 exit 0 / systemctl stop 不拉起)
//!   systemctl restart ↔ kickstart -k(幂等重启)
//!   systemctl stop 自带等退出,无需手动轮询 pid;stop 后 systemd 保证不拉回

use super::{json_done, locate_daemon_src, probe_online, progress, wait_online};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const UNIT: &str = "planet-portd";
const UNIT_PATH: &str = "/etc/systemd/system/planet-portd.service";
/// 数据目录:install 命令落盘 CLI/daemon 的根;--config-dir 亦指向此处
const DATA_DIR: &str = "/var/lib/planet-portd";
/// CLI 落点:直接落 PATH 规范位,无软链层(mod.rs 经此对外提供已装路径)
pub(crate) const CLI_BIN: &str = "/usr/local/bin/planet-port";
pub(crate) const BIN_PATH: &str = "/usr/local/bin/planet-portd";

pub(crate) fn install(json: bool) {
    // 1. 拷 daemon 到固定路径(unit 指向它,发布包解压路径不稳定)
    let src = locate_daemon_src();
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
    // 2. 数据目录先行创建:unit 显式 --config-dir,但目录须存在 daemon 才能写
    if let Err(e) = fs::create_dir_all(DATA_DIR) {
        eprintln!("{}", rust_i18n::t!("cli.service.mkdirFailed", dir = DATA_DIR, e = e));
        std::process::exit(1);
    }
    // 3. 写 unit(ExecStart 显式 --config-dir,与 macOS plist 同构,
    //    不依赖 config_dir() 的"目录已存在"隐式回退)
    if let Err(e) = fs::write(UNIT_PATH, unit_content(BIN_PATH, DATA_DIR)) {
        eprintln!("{}", rust_i18n::t!("cli.service.writeUnitFailed", path = UNIT_PATH, e = e));
        std::process::exit(1);
    }
    // 4. 装载:daemon-reload 认新 unit;enable 开机自启;restart 幂等拉起
    //    (升级场景二进制已被 install 命令原子替换,restart 即换新进程)
    systemctl(&["daemon-reload"]);
    systemctl(&["enable", UNIT]);
    progress(json, rust_i18n::t!("cli.service.unitEnabled").to_string());
    systemctl(&["restart", UNIT]);
    // 5. 等应答确认真的起来了(与 macos bootstrap 后验在线一致)
    if wait_online(std::time::Duration::from_secs(15)) {
        let port = probe_online().unwrap_or(0);
        progress(json, rust_i18n::t!("cli.service.started", port = port).to_string());
    } else {
        eprintln!("{}", rust_i18n::t!("cli.service.startTimeout", log = "journalctl -u planet-portd"));
    }
    progress(json, rust_i18n::t!("cli.service.installedSvc", unit = UNIT).to_string());
    progress(json, rust_i18n::t!("cli.service.installedBin", path = BIN_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.installedUnit", path = UNIT_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.installedData", dir = DATA_DIR).to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

/// install.rs 步骤 3:CLI/daemon 落盘。
/// installed 参数:从已安装路径运行时跳过自拷(rename 同路径会自删);
/// json 参数与 macos 签名对齐(预留结果回执,Linux 无软链层暂无输出)
pub(crate) fn install_cli(cli_src: &Path, daemon_src: &Path, installed: bool, _json: bool) {
    // Linux 无软链层:CLI/daemon 直接落 /usr/local/bin(PATH 规范位,
    // zerotier-one 落 /usr/sbin 同理)
    if !installed {
        super::copy_exe(cli_src, CLI_BIN);
        super::copy_exe(daemon_src, BIN_PATH);
    }
}

pub(crate) fn uninstall(json: bool) {
    // 未加载/已卸载属预期,不算失败(systemctl 对不存在的 unit 报错 → 静默)
    match systemctl_output(&["stop", UNIT]) {
        Ok(_) => progress(json, rust_i18n::t!("cli.service.stopped").to_string()),
        Err(_) => progress(json, rust_i18n::t!("cli.service.notRunningSkip").to_string()),
    }
    let _ = systemctl_output(&["disable", UNIT]);
    let _ = fs::remove_file(UNIT_PATH);
    let _ = systemctl_output(&["daemon-reload"]);
    let _ = systemctl_output(&["reset-failed", UNIT]);
    // install 命令落盘的 CLI 与 daemon 一并清;数据目录保留
    let _ = fs::remove_file(CLI_BIN);
    let _ = fs::remove_file(BIN_PATH);
    progress(json, rust_i18n::t!("cli.service.removed", plist = UNIT_PATH, bin = BIN_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.dataKept", dir = DATA_DIR).to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

pub(crate) fn start(json: bool) {
    // restart = 幂等重启(已在运行则换新拉起),等价 kickstart -k
    systemctl(&["restart", UNIT]);
    match wait_online(std::time::Duration::from_secs(15)) {
        true => {
            let port = probe_online().unwrap_or(0);
            progress(json, rust_i18n::t!("cli.service.started", port = port).to_string());
            json_done(json, serde_json::json!({"result": "ok", "port": port}));
        }
        false => {
            progress(json, rust_i18n::t!("cli.service.startTimeout", log = "journalctl -u planet-portd").to_string());
            json_done(json, serde_json::json!({"result": "error"}));
        }
    }
}

pub(crate) fn stop(json: bool) {
    // systemctl stop 发 SIGTERM 并等优雅退出后置 inactive,不拉回
    // (Restart=on-failure 只管异常退出;stop 是明确意图)
    systemctl(&["stop", UNIT]);
    progress(json, rust_i18n::t!("cli.service.stopped").to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

/// 读取 systemd 记录的服务 pid(pid = 0 = 未在运行)
/// 仅保留备用:Linux 侧 stop 靠 systemd 自身等退出、start 靠 HTTP 探测,
/// 均不需要轮询 pid;后续需精细化时再接入
#[allow(dead_code)]
pub(crate) fn service_pid() -> Option<u32> {
    systemctl_output(&["show", UNIT, "-p", "MainPID", "--value"])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|p| *p != 0)
}

/// daemon 不在线时的平台诊断:unit 缺失 = 未安装,否则给 systemd 状态;
/// 与 macos.rs 一致,内部自行 exit 3
pub(crate) fn offline_diag() {
    if !Path::new(UNIT_PATH).exists() {
        println!("{}", rust_i18n::t!("cli.service.notInstalled", path = UNIT_PATH));
        std::process::exit(3);
    }
    println!("{}", rust_i18n::t!("cli.service.installedNotRunning"));
    if let Ok(state) = systemctl_output(&["is-active", UNIT]) {
        println!("{}", rust_i18n::t!("cli.service.systemdState", state = state.trim()));
    }
    std::process::exit(3);
}

/// 查看 daemon 日志:journalctl 末尾 N 行,--follow 时持续跟踪。
/// journald 无"文件缺失"概念;unit 不存在时 journalctl 自行报错,原样透传
pub(crate) fn logs(lines: usize, follow: bool) {
    let mut args: Vec<String> = vec!["--no-pager".into(), "-n".into(), lines.to_string()];
    if follow {
        args.push("-f".into());
    }
    args.push("-u".into());
    args.push(UNIT.into());
    match Command::new("journalctl").args(&args).status() {
        Ok(s) if s.success() => {}
        // journalctl 被信号杀死时 code() 为 None;Ctrl+C 的 130 由 SIGINT 命中
        // CLI 自身产生,这里只兜非信号的异常退出
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("{}", rust_i18n::t!("cli.service.journalctlExecFailed", e = e));
            std::process::exit(1);
        }
    }
}

fn unit_content(bin: &str, data_dir: &str) -> String {
    format!(
        r#"[Unit]
Description=planet-port daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={bin} --config-dir {data_dir}
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target
"#
    )
}

/// 执行 systemctl,失败打印 stderr 并退出 1(命令拼装错误/权限不足都是硬失败)
fn systemctl(args: &[&str]) {
    match systemctl_output(args) {
        Ok(_) => {}
        Err(stderr) => {
            eprintln!("{}", rust_i18n::t!("cli.service.systemctlFailed", args = args.join(" "), e = stderr.trim()));
            std::process::exit(1);
        }
    }
}

fn systemctl_output(args: &[&str]) -> Result<String, String> {
    match Command::new("systemctl").args(args).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(e) => Err(rust_i18n::t!("cli.service.systemctlExecFailed", e = e).to_string()),
    }
}
