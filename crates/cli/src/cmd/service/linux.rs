//! systemd 服务管理(Linux 平台实现,接口与 macos.rs 对齐)。
//!
//! 系统落点(与 mac 的 App Support/bin + PATH 软链同构):
//!   /usr/local/lib/planet-port/bin/{planet-port,planet-portd} ← 二进制真身
//!   /usr/local/bin/{planet-port,planet-portd}                 ← PATH 软链
//!   /etc/systemd/system/planet-portd.service    ← unit
//!   /var/lib/planet-portd                       ← 数据目录(api crate 同源)
//!   日志走 journald:journalctl -u planet-portd,无需轮转配置
//!
//! 语义对照(与 macos.rs 注释互参):
//!   Restart=on-failure ↔ KeepAlive={SuccessfulExit=false}
//!     (异常退出自动拉起;SIGTERM 优雅退出 exit 0 / systemctl stop 不拉起)
//!   systemctl restart ↔ kickstart -k(幂等重启)
//!   systemctl stop 自带等退出,无需手动轮询 pid;stop 后 systemd 保证不拉回

use super::{json_done, locate_daemon_src, plan_layout, probe_online, progress, wait_online, LayoutPlan};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const UNIT: &str = "planet-portd";
const UNIT_PATH: &str = "/etc/systemd/system/planet-portd.service";
/// 数据目录:install 命令落盘 CLI/daemon 的根;--config-dir 亦指向此处
const DATA_DIR: &str = "/var/lib/planet-portd";
/// 统一二进制目录(与 mac 的 App Support/bin 对齐):升级只切软链,卸载删目录
pub(crate) const BIN_DIR: &str = "/usr/local/lib/planet-port/bin";
/// PATH 入口软链:指向 BIN_DIR 真身(mod.rs 经 CLI_BIN 对外提供已装路径,
/// install.rs 的 canonicalize 比对会解析软链命中真身,无需改动)
pub(crate) const CLI_BIN: &str = "/usr/local/bin/planet-port";
pub(crate) const BIN_PATH: &str = "/usr/local/bin/planet-portd";

pub(crate) fn install(json: bool) {
    // 0. 布局收敛:旧直落形态(真实文件在 /usr/local/bin)一次性迁入 BIN_DIR,
    //    避免软链与旧真实文件互相覆盖;BIN_DIR 统一先行创建(三分支都可能往
    //    里落盘,InPlace 下软链在而目录被删的残局也能自愈)
    let _ = fs::create_dir_all(BIN_DIR);
    let plan = plan_layout(
        fs::symlink_metadata(CLI_BIN).map(|m| !m.file_type().is_symlink()).unwrap_or(false),
        fs::read_link(CLI_BIN).is_ok(),
    );
    match plan {
        LayoutPlan::Migrate => {
            for p in [CLI_BIN, BIN_PATH] {
                let name = Path::new(p).file_name().unwrap().to_str().unwrap();
                // 只有 planet-port 没有 planet-portd 的半态:旧 daemon 缺失属
                // 预期,稍后由 copy_exe 补齐,静默跳过
                if !Path::new(p).exists() {
                    continue;
                }
                // 仍在使用的真实文件 rename 进 BIN_DIR(原子,不触发 ETXTBSY);
                // 失败不静默:真身留在 /usr/local/bin 会被 ensure_links 消费成
                // 悬空链,CLI 直接打断,故报错退出(重跑即恢复)
                let dst = format!("{BIN_DIR}/{name}");
                if let Err(e) = fs::rename(p, &dst) {
                    eprintln!("{}", rust_i18n::t!("cli.service.migrateFailed", path = p, dst = dst, e = e));
                    std::process::exit(1);
                }
            }
        }
        LayoutPlan::Fresh | LayoutPlan::InPlace => {}
    }
    // 1. 拷 daemon 到 BIN_DIR 真实路径(unit 指向它,发布包解压路径不稳定)
    let real_daemon = format!("{BIN_DIR}/planet-portd");
    let src = locate_daemon_src();
    let same = fs::canonicalize(&src).ok().zip(fs::canonicalize(&real_daemon).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if !same {
        super::copy_exe(&src, &real_daemon);
    }
    if let Err(e) = fs::set_permissions(&real_daemon, fs::Permissions::from_mode(0o755)) {
        eprintln!("{}", rust_i18n::t!("cli.service.chmodFailed", path = real_daemon, e = e));
        std::process::exit(1);
    }
    // 2. 数据目录先行创建:unit 显式 --config-dir,但目录须存在 daemon 才能写
    if let Err(e) = fs::create_dir_all(DATA_DIR) {
        eprintln!("{}", rust_i18n::t!("cli.service.mkdirFailed", dir = DATA_DIR, e = e));
        std::process::exit(1);
    }
    // 3. 写 unit(ExecStart 显式 --config-dir,与 macOS plist 同构,
    //    不依赖 config_dir() 的"目录已存在"隐式回退)
    if let Err(e) = fs::write(UNIT_PATH, unit_content(&real_daemon, DATA_DIR)) {
        eprintln!("{}", rust_i18n::t!("cli.service.writeUnitFailed", path = UNIT_PATH, e = e));
        std::process::exit(1);
    }
    // 3.5 PATH 软链(指向 BIN_DIR 真身;已有软链/普通文件一律替换,幂等重建)
    ensure_links();
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
    progress(json, rust_i18n::t!("cli.service.installedBin", path = real_daemon).to_string());
    progress(json, rust_i18n::t!("cli.service.installedUnit", path = UNIT_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.installedData", dir = DATA_DIR).to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

/// install.rs 步骤 3:CLI/daemon 落盘 + PATH 软链。
/// installed 参数:从已安装路径运行时跳过自拷(rename 同路径会自删);
/// json 参数与 macos 签名对齐(软链重建静默完成,无输出)
pub(crate) fn install_cli(cli_src: &Path, daemon_src: &Path, installed: bool, _json: bool) {
    // BIN_DIR 先行:copy_exe 的临时文件落在该目录下
    let _ = fs::create_dir_all(BIN_DIR);
    if !installed {
        super::copy_exe(cli_src, &format!("{BIN_DIR}/planet-port"));
        super::copy_exe(daemon_src, &format!("{BIN_DIR}/planet-portd"));
    } else if !Path::new(&format!("{BIN_DIR}/planet-port")).exists() {
        // installed=true 只蕴含 /usr/local/bin/planet-port 与 cli_src 同一真身,
        // 不蕴含 BIN_DIR 里有 CLI(旧直落形态的真实文件同样 canonicalize 判等):
        // 不补拷则 ensure_links 会把该真实文件消费成悬空链,CLI 打断
        super::copy_exe(cli_src, &format!("{BIN_DIR}/planet-port"));
    }
    ensure_links();
}

/// PATH 软链建立:/usr/local/bin/{planet-port,planet-portd} → BIN_DIR 真身。
/// 目标位置已有任何东西(旧软链/普通文件)先移除再建,重复安装幂等。
/// 真身缺失时只告警并跳过该链,不退出:装出悬空链只会让 CLI 打断;正常
/// 安装流(install() 拷 daemon、install_cli 补拷 CLI)都已保证真身在,
/// 此处兜「真身被外部删除」的残局,完整 install 重跑即自愈
fn ensure_links() {
    for (name, link) in [("planet-port", CLI_BIN), ("planet-portd", BIN_PATH)] {
        let real = format!("{BIN_DIR}/{name}");
        if !Path::new(&real).exists() {
            eprintln!("{}", rust_i18n::t!("cli.service.linkRealMissing", link = link, real = real));
            continue;
        }
        let _ = fs::remove_file(link);
        if let Some(dir) = Path::new(link).parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Err(e) = std::os::unix::fs::symlink(&real, link) {
            eprintln!("{}", rust_i18n::t!("cli.service.linkFailed", path = link, e = e));
            std::process::exit(1);
        }
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
    // install 命令落盘的 PATH 软链与 BIN_DIR 真身一并清(旧直落形态残留的
    // 真实文件也是这两条路径,同样覆盖);数据目录保留
    let _ = fs::remove_file(CLI_BIN);
    let _ = fs::remove_file(BIN_PATH);
    let _ = fs::remove_dir_all(BIN_DIR);
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
    let state = systemctl_output_quiet(&["is-active", UNIT])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| rust_i18n::t!("common.unknown").to_string());
    println!("{}", rust_i18n::t!("cli.service.systemdState", state = state));
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

/// 只取 stdout 不问退出码:is-active 对 inactive/failed/activating 一律非零
/// 退出,状态却写在 stdout——offline_diag 恰只在「已安装未运行」时用它,
/// 按退出码判定会永远丢状态行(仿 macos.rs 的 launchctl_output_quiet)。
/// systemctl 本身执行失败(不存在等)仍 Err
fn systemctl_output_quiet(args: &[&str]) -> Result<String, String> {
    match Command::new("systemctl").args(args).output() {
        Ok(o) => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Err(e) => Err(rust_i18n::t!("cli.service.systemctlExecFailed", e = e).to_string()),
    }
}
