//! SCM 服务管理(Windows 平台实现,接口与 macos.rs/linux.rs 对齐)。
//!
//! 系统落点(brief 定型的目录真值):
//!   C:\Program Files\planet-port\planet-portd.exe / planet-port.exe / wintun.dll
//!   C:\ProgramData\planet-port\                   ← 数据目录(--config-dir)
//!   服务名 planetportd(SCM 分发与服务名一致,见 daemon service_win.rs)
//!
//! 语义对照(与 macos.rs 注释互参):
//!   sc failure restart/5000        ↔ KeepAlive={SuccessfulExit=false}
//!     (进程异常死亡由 SCM 5s 后拉起;sc stop 是明确意图,不触发恢复)
//!   start 前先 stop 再 start       ↔ kickstart -k(幂等重启)
//!   sc stop + 轮询 STOPPED,超时 taskkill /F ↔ bootout 等退出
//!
//! 变更命令(install/uninstall/start/stop)走外部 `sc.exe`(与 macos.rs 走
//! launchctl 同构);状态查询也经 `sc query`/`sc qc` 文本解析——sc 的标签列在
//! 非英文系统上会本地化(如「状态」),故解析以 `: <数字>` 的取值为主、
//! 关键字匹配为辅,数字 STATE/START_TYPE 语义跨语言稳定。

use super::{json_done, probe_online, progress, wait_online};
use std::path::{Path, PathBuf};
use std::process::Command;

const SERVICE_NAME: &str = "planetportd";
const SERVICE_DISPLAY: &str = "Planet Port Daemon";
const BIN_DIR: &str = r"C:\Program Files\planet-port";
const DATA_DIR: &str = r"C:\ProgramData\planet-port";
pub(crate) const BIN_PATH: &str = r"C:\Program Files\planet-port\planet-portd.exe";
pub(crate) const CLI_BIN: &str = r"C:\Program Files\planet-port\planet-port.exe";
/// wintun.dll 与两枚 exe 同目录随装:tun-rs 建 wintun 适配器时从加载目录取
/// (Task 9 打包负责把目标架构的 dll 放进发布包,与本处约定配套)
const WINTUN_DLL: &str = r"C:\Program Files\planet-port\wintun.dll";

/// 本产品 NRPT 规则的归属前缀:daemon 以 `-Comment "planet-port:<nwid>"`
/// 建规则(daemon dns/win.rs 同一约定,勿单方改动),uninstall 清理按
/// Comment 前缀匹配——Namespace 是业务域名,不能用来识别归属
const NRPT_COMMENT_PREFIX: &str = "planet-port:";

/// SCM 查询 STATE 数值 → 展示名(仅 offline_diag 展示用)
fn state_name(code: u32) -> &'static str {
    match code {
        1 => "STOPPED",
        2 => "START_PENDING",
        3 => "STOP_PENDING",
        4 => "RUNNING",
        5 => "CONTINUE_PENDING",
        6 => "PAUSE_PENDING",
        7 => "PAUSED",
        _ => "?",
    }
}

/// SCM STATE 原始值(windows-sys 0.59 Services 模块常量同值):
/// 1=STOPPED 2=START_PENDING 3=STOP_PENDING 4=RUNNING
/// 5=CONTINUE_PENDING 6=PAUSE_PENDING 7=PAUSED
const SERVICE_STOPPED: u32 = 1;

/// START_TYPE 数值:2 = AUTO_START(is-enabled 的判定依据)
const START_TYPE_AUTO: u32 = 2;

pub(crate) fn install(json: bool) {
    ensure_dirs();
    // 升级路径:旧服务在跑时其 exe 映像锁着文件,先停/删才拷得进去
    reinstall_preclean(json);

    // 1. 二进制套装落盘:发布包/构建产物目录 → 固定安装路径(sc binPath 指向
    //    拷贝而非原路径,原路径不稳定,同 macos/linux);从安装位置自跑时跳过
    //    自拷贝(copy 同路径会失败)
    let src = locate_sibling("planet-portd.exe")
        .unwrap_or_else(|| missing_daemon_exit());
    if !same_file(&src, Path::new(BIN_PATH)) {
        super::copy_exe(&src, BIN_PATH);
    }
    let cli_src = locate_sibling("planet-port.exe")
        .unwrap_or_else(|| missing_bin_exit("planet-port.exe"));
    if !same_file(&cli_src, Path::new(CLI_BIN)) {
        super::copy_exe(&cli_src, CLI_BIN);
    }
    ensure_wintun(json);

    // 2. 数据目录 ACL 收敛:默认 ProgramData ACL 对 Users 发放写位,而数据
    //    目录里存 authtoken.secret 与 planet 登记,应只有服务(LocalSystem)
    //    与管理员可写;/inheritance:r 切断父目录继承,/grant:r 整表重授。
    //    失败硬退——权限不齐就不该继续把 token 写进一个 Users 可读的目录
    match Command::new("icacls")
        .args([
            DATA_DIR,
            "/inheritance:r",
            "/grant:r", "SYSTEM:(OI)(CI)F",
            "/grant:r", "Administrators:(OI)(CI)F",
        ])
        .output()
    {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "{}",
                rust_i18n::t!(
                    "cli.service.aclFailed",
                    dir = DATA_DIR,
                    e = String::from_utf8_lossy(&o.stderr).trim()
                )
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{}", rust_i18n::t!("cli.service.aclFailed", dir = DATA_DIR, e = e));
            std::process::exit(1);
        }
    }

    // 3. sc create:start= auto 即开机自启(RunAtLoad 语义);binPath 内嵌
    //    引号——路径带空格,--service/--config-dir 必须作为 exe 参数传进去。
    //    `key= value` 的空格是 sc.exe 的固定语法(= 后必须有空格)
    let bin_path_value =
        format!("\"{BIN_PATH}\" --service --config-dir \"{DATA_DIR}\"");
    sc(&[
        "create", SERVICE_NAME,
        "binPath=", &bin_path_value,
        "start=", "auto",
        "DisplayName=", SERVICE_DISPLAY,
    ]);

    // 4. 失败自拉起(KeepAlive 语义):进程异常死亡 5s 后由 SCM 重启;
    //    reset= 86400 = 失败计数按天清零
    sc(&["failure", SERVICE_NAME, "reset=", "86400", "actions=", "restart/5000"]);

    // 5. 拉起并等应答(bootstrap 后验在线,同 macos/linux)
    sc(&["start", SERVICE_NAME]);
    if wait_online(std::time::Duration::from_secs(15)) {
        let port = probe_online().unwrap_or(0);
        progress(json, rust_i18n::t!("cli.service.started", port = port).to_string());
    } else {
        // 服务本身已创建并置 START,daemon 没应答只是「还没就绪」,不算安装失败
        progress(json, rust_i18n::t!("cli.service.startTimeoutWin", name = SERVICE_NAME).to_string());
    }

    progress(json, rust_i18n::t!("cli.service.winInstalled", name = SERVICE_NAME).to_string());
    progress(json, rust_i18n::t!("cli.service.installedBin", path = BIN_PATH).to_string());
    progress(json, rust_i18n::t!("cli.service.installedData", dir = DATA_DIR).to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

/// install.rs 步骤 3:CLI/daemon/wintun 三件套落盘。
/// installed 参数:从已安装路径运行时跳过自拷(copy 同路径会自删)
pub(crate) fn install_cli(cli_src: &Path, daemon_src: &Path, installed: bool, json: bool) {
    ensure_dirs();
    // 先于拷贝做重装清理:运行中的 daemon 锁着待覆盖的 exe 映像(unix 靠
    // rename 换 inode 规避,Windows 无此通道)
    reinstall_preclean(json);
    if !installed {
        super::copy_exe(cli_src, CLI_BIN);
        super::copy_exe(daemon_src, BIN_PATH);
    }
    ensure_wintun(json);
}

pub(crate) fn uninstall(json: bool) {
    if service_exists() {
        stop_service(json);
        if let Err(e) = sc_output(&["delete", SERVICE_NAME]) {
            if !err_is_not_exist(&e) {
                eprintln!("{}", rust_i18n::t!("cli.service.scFailed", args = "delete planetportd", e = e.trim()));
            }
        }
    } else {
        progress(json, rust_i18n::t!("cli.service.notRunningSkip").to_string());
    }

    // 程序目录清空(数据目录保留,对齐 mac/linux 的「数据留」红线)
    let _ = std::fs::remove_file(BIN_PATH);
    let _ = std::fs::remove_file(CLI_BIN);
    let _ = std::fs::remove_file(WINTUN_DLL);
    let _ = std::fs::remove_dir(BIN_DIR);

    // NRPT 清理(Task 8 落地前前缀未定,函数内直接跳过)
    cleanup_nrpt_rules(json);

    progress(json, rust_i18n::t!("cli.service.winRemoved", name = SERVICE_NAME, dir = BIN_DIR).to_string());
    progress(json, rust_i18n::t!("cli.service.dataKept", dir = DATA_DIR).to_string());
    json_done(json, serde_json::json!({"result": "ok"}));
}

pub(crate) fn start(json: bool) {
    if !service_exists() {
        eprintln!("{}", rust_i18n::t!("cli.service.notInstalled", path = BIN_PATH));
        std::process::exit(1);
    }
    // 幂等重启(kickstart -k 语义):运行中先优雅停(SCM STOP 走 daemon
    // 逐 planet 收尾)再拉,否则 sc start 对 RUNNING 服务报 1056
    if query_state() == Some(4) {
        stop_service(json);
    }
    sc(&["start", SERVICE_NAME]);
    match wait_online(std::time::Duration::from_secs(15)) {
        true => {
            let port = probe_online().unwrap_or(0);
            progress(json, rust_i18n::t!("cli.service.started", port = port).to_string());
            json_done(json, serde_json::json!({"result": "ok", "port": port}));
        }
        false => {
            progress(json, rust_i18n::t!("cli.service.startTimeoutWin", name = SERVICE_NAME).to_string());
            json_done(json, serde_json::json!({"result": "error"}));
        }
    }
}

pub(crate) fn stop(json: bool) {
    if !service_exists() {
        eprintln!("{}", rust_i18n::t!("cli.service.notInstalled", path = BIN_PATH));
        std::process::exit(1);
    }
    stop_service(json);
    // 优雅 stop 不触发失败恢复;若进程是被强杀收场,failure restart/5000 会在
    // 5s 后把服务拉回——等满这个窗口再下结论(提前变 RUNNING 立即跳出),
    // 如实报告(对齐 macos 的 keepaliveRelaunched 分支)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    while query_state() != Some(4) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if query_state() == Some(4) {
        progress(json, rust_i18n::t!("cli.service.failureRestarted").to_string());
        json_done(json, serde_json::json!({"result": "warning"}));
    } else {
        progress(json, rust_i18n::t!("cli.service.stopped").to_string());
        json_done(json, serde_json::json!({"result": "ok"}));
    }
}

/// daemon 不在线时的平台诊断:查无此服务 = 未安装,否则给 SCM 状态与 pid;
/// 与 macos.rs 一致,内部自行 exit 3(mod.rs 调用方随后也 exit 3)
pub(crate) fn offline_diag() {
    if !service_exists() {
        println!("{}", rust_i18n::t!("cli.service.notInstalled", path = BIN_PATH));
        std::process::exit(3);
    }
    println!("{}", rust_i18n::t!("cli.service.installedNotRunning"));
    let (state, pid) = query_state_pid();
    let state = state.map(|c| state_name(c).to_string())
        .unwrap_or_else(|| rust_i18n::t!("common.unknown").to_string());
    print!("{}", rust_i18n::t!("cli.service.winState", state = state));
    if let Some(p) = pid {
        print!("{}", rust_i18n::t!("cli.service.pidFragment", pid = p));
    }
    println!();
    std::process::exit(3);
}

/// daemon 日志:Windows 服务身份的 stdout/stderr 不落盘(SCM 无重定向,
/// Task 6 也未引入文件日志),没有可 tail 的对象——明确告知而非假装成功
pub(crate) fn logs(_lines: usize, _follow: bool) {
    eprintln!("{}", rust_i18n::t!("cli.service.logsUnsupported"));
    std::process::exit(1);
}

/// 服务已安装且 START_TYPE 为自动 = 开机自启(对齐 launchd is-enabled 语义:
/// 装了且自启才为 true)。mod.rs 暂无 is-enabled 子命令,供打包/doctor 后续
/// 接入(仿 linux.rs 的 service_pid 保留位)
#[allow(dead_code)]
pub(crate) fn is_enabled() -> bool {
    service_exists() && query_start_type() == Some(START_TYPE_AUTO)
}

/// mod.rs require_root() 的 Windows 等价物:管理员令牌校验,语义对齐
/// (非管理员 → 中文提示 + 退出码 2)
pub(crate) fn require_admin() {
    if !is_elevated() {
        eprintln!("{}", rust_i18n::t!("cli.service.needAdmin"));
        std::process::exit(2);
    }
}

/// 管理员令牌判定:OpenProcessToken + GetTokenInformation(TokenElevation)。
/// 不解析 `whoami /groups` 文本(输出本地化且依赖 find 的匹配行为)
fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    // OpenProcessToken 归属 Threading(windows-sys 0.59 的模块划分)
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

// ---- 服务生命周期(共用) ----

/// 停服务并等真正 STOPPED:sc stop 后 SCM 给 daemon 30s(WaitHint)收尾,
/// 轮询照此放宽;超时 taskkill /F 强杀(SCM 视为异常死亡,failure 策略会把
/// 它拉回来——stop 命令侧负责把这一态如实报告)
fn stop_service(json: bool) {
    match sc_output(&["stop", SERVICE_NAME]) {
        Ok(_) => {}
        Err(e) => {
            // 1062 = 服务未启动;1060 = 服务不存在。二者都是「已停」语义
            let s = e.to_lowercase();
            if s.contains("1062") || s.contains("1060") || s.contains("not started") || s.contains("has not started") {
                progress(json, rust_i18n::t!("cli.service.notRunningSkip").to_string());
                return;
            }
            eprintln!(
                "{}",
                rust_i18n::t!("cli.service.scFailed", args = format!("stop {SERVICE_NAME}"), e = e.trim())
            );
            std::process::exit(1);
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while query_state() != Some(SERVICE_STOPPED) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if query_state() != Some(SERVICE_STOPPED) {
        // 强杀按进程映像名(服务句柄随之失效);/T 连带子进程
        let _ = Command::new("taskkill").args(["/F", "/T", "/IM", "planet-portd.exe"]).output();
        progress(json, rust_i18n::t!("cli.service.forceKilled").to_string());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while query_state() != Some(SERVICE_STOPPED) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    if query_state() != Some(SERVICE_STOPPED) {
        progress(json, rust_i18n::t!("cli.service.stopTimeoutWin", name = SERVICE_NAME).to_string());
        std::process::exit(1);
    }
}

// ---- sc.exe 输出解析 ----

/// 服务是否已注册(sc query 对不存在的服务报 1060 非零退出)
fn service_exists() -> bool {
    matches!(sc_output(&["query", SERVICE_NAME]), Ok(_))
}

/// 当前 STATE 数值(解析失败 = 未知,返回 None)
fn query_state() -> Option<u32> {
    query_state_pid().0
}

/// (STATE, PID) 一次解析;PID 仅展示用,解析不到不给
fn query_state_pid() -> (Option<u32>, Option<String>) {
    let out = match sc_output(&["query", SERVICE_NAME]) {
        Ok(o) => o,
        Err(_) => return (None, None),
    };
    let mut state = None;
    let mut pid = None;
    let mut state_fallback = None;
    for line in out.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let key = k.trim().to_lowercase();
        let value = v.trim();
        let num = value.split_whitespace().next().and_then(|t| t.parse::<u32>().ok());
        // 确认路径:关键字命中(英文 STATE / 中文「状态」)且取值列以数字开头。
        // 数字兜底:输出中首个取值在 1..=7 且非 TYPE/PID 行——sc query 的输出
        // 顺序固定为 TYPE/STATE/退出码/PID,满足该特征的只有 STATE(TYPE 取值
        // 是 10/16/20 等十六进制写法,WIN32_EXIT_CODE 等取值为 0 或大数,PID 在
        // STATE 之后且显式排除)。兜底覆盖 GBK 乱码/本地化标签变体(TYPE→TYP、
        // STATUS 不含 "state" 等)下关键字不命中的场景,否则 stop 空转 30s 后
        // 硬杀、start 幂等分支误判吃 1056
        if (key.contains("state") || key.contains("状态")) && num.is_some() {
            state = num;
        } else if state_fallback.is_none() && !key.contains("type") && !key.contains("pid") {
            if let Some(n @ 1..=7) = num {
                state_fallback = Some(n);
            }
        }
        if state.is_some() && (key.contains("pid") || key.contains("进程")) {
            pid = value.split_whitespace().next().map(String::from);
        }
    }
    (state.or(state_fallback), pid)
}

/// qc 的 START_TYPE 数值(2 = AUTO_START);标签本地化时按「取值 2/3 的首个
/// 数字行」兜底(qc 输出里这类行只有 START_TYPE)
fn query_start_type() -> Option<u32> {
    let out = sc_output(&["qc", SERVICE_NAME]).ok()?;
    let mut fallback = None;
    for line in out.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let key = k.trim().to_lowercase();
        let num = v.trim().split_whitespace().next().and_then(|t| t.parse::<u32>().ok());
        if key.contains("start_type") || key.contains("start type") {
            return num;
        }
        if fallback.is_none() && matches!(num, Some(2) | Some(3)) {
            fallback = num;
        }
    }
    fallback
}

/// 重装预清理(对齐 macos 的 bootout-for-reinstall):已存在的同名服务会让
/// sc create 失败,且运行中的 daemon 锁着待覆盖的 exe 映像——stop → delete
/// → 等「标记待删」真正落定
fn reinstall_preclean(json: bool) {
    if !service_exists() {
        return;
    }
    progress(json, rust_i18n::t!("cli.service.stoppedForReinstall", name = SERVICE_NAME).to_string());
    stop_service(json);
    let _ = sc_output(&["delete", SERVICE_NAME]);
    wait_deleted(std::time::Duration::from_secs(10));
}

fn wait_deleted(timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while service_exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if service_exists() {
        // 「标记待删」未落定(通常有句柄未释放):继续 create 必失败,如实报错
        eprintln!("{}", rust_i18n::t!("cli.service.deletePending", name = SERVICE_NAME));
        std::process::exit(1);
    }
}

/// 执行 sc.exe,非零退出算失败(错误文本可能走 stdout,一并带上)
fn sc_output(args: &[&str]) -> Result<String, String> {
    match Command::new("sc.exe").args(args).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => {
            let mut err = String::from_utf8_lossy(&o.stderr).into_owned();
            if err.trim().is_empty() {
                err = String::from_utf8_lossy(&o.stdout).into_owned();
            }
            Err(err)
        }
        Err(e) => Err(rust_i18n::t!("cli.service.scExecFailed", e = e).to_string()),
    }
}

/// 执行 sc.exe,失败打印 stderr 并退出 1(命令拼装错误/权限不足都是硬失败)
fn sc(args: &[&str]) {
    if let Err(e) = sc_output(args) {
        eprintln!(
            "{}",
            rust_i18n::t!("cli.service.scFailed", args = args.join(" "), e = e.trim())
        );
        std::process::exit(1);
    }
}

/// sc 的失败文本含退出码数字;1060(服务不存在)在卸载路径上属预期
fn err_is_not_exist(e: &str) -> bool {
    e.contains("1060")
}

// ---- 落盘与来源定位 ----

fn ensure_dirs() {
    for dir in [BIN_DIR, DATA_DIR] {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("{}", rust_i18n::t!("cli.service.mkdirFailed", dir = dir, e = e));
            std::process::exit(1);
        }
    }
}

/// wintun.dll 随装:exe 同目录找到就拷(缺位则报错引导),装好/同路径自跑
/// 时跳过
fn ensure_wintun(json: bool) {
    let dst = Path::new(WINTUN_DLL);
    let src = match std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        Some(dir) => dir.join("wintun.dll"),
        None => return,
    };
    if src.is_file() {
        if !same_file(&src, dst) {
            super::copy_exe(&src, WINTUN_DLL);
            progress(json, rust_i18n::t!("cli.service.wintunCopied", path = WINTUN_DLL).to_string());
        }
    } else if !dst.exists() {
        eprintln!(
            "{}",
            rust_i18n::t!(
                "cli.service.wintunMissing",
                dir = src.parent().map(|p| p.display().to_string()).unwrap_or_default()
            )
        );
        std::process::exit(1);
    }
}

/// exe 同目录兄弟定位:Windows 二进制带 .exe 后缀,mod.rs 的
/// locate_daemon_src 按无后缀名比对在 Windows 命不中,故本地实现同构逻辑
/// (自身名字命中 → 自身;否则同目录找)
fn locate_sibling(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let self_name = exe.file_name()?.to_str()?;
    let stem = self_name.strip_suffix(".exe").unwrap_or(self_name);
    let want = name.strip_suffix(".exe").unwrap_or(name);
    if stem == want {
        return Some(exe);
    }
    let sibling = exe.parent()?.join(name);
    sibling.is_file().then_some(sibling)
}

fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok())
        .map(|(x, y)| x == y)
        .unwrap_or(false)
}

fn missing_daemon_exit() -> ! {
    let dir = std::env::current_exe().ok()
        .and_then(|e| e.parent().map(|p| p.display().to_string()))
        .unwrap_or_default();
    eprintln!("{}", rust_i18n::t!("cli.service.daemonMissing", dir = dir, name = "planet-portd.exe"));
    std::process::exit(1);
}

fn missing_bin_exit(name: &str) -> ! {
    let dir = std::env::current_exe().ok()
        .and_then(|e| e.parent().map(|p| p.display().to_string()))
        .unwrap_or_default();
    eprintln!("{}", rust_i18n::t!("cli.service.winBinaryMissing", name = name, dir = dir));
    std::process::exit(1);
}

/// NRPT 残留清理(uninstall):直连 PowerShell 的 NRPT cmdlet 按「Comment
/// 前缀属于本产品」清理,不走 daemon 的 DNS 清理接口(服务此刻已停,接口
/// 不可达也不可靠)。前缀匹配保证绝不误删他产品的 NRPT 规则
fn cleanup_nrpt_rules(json: bool) {
    let ps = format!(
        "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -like '{NRPT_COMMENT_PREFIX}*' }} | Remove-DnsClientNrptRule -Force"
    );
    match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .output()
    {
        Ok(o) if o.status.success() => {
            progress(json, rust_i18n::t!("cli.service.nrptCleaned", prefix = NRPT_COMMENT_PREFIX).to_string());
        }
        Ok(o) => {
            let err = if o.stderr.is_empty() { &o.stdout } else { &o.stderr };
            eprintln!(
                "{}",
                rust_i18n::t!("cli.service.nrptCleanFailed", e = String::from_utf8_lossy(err).trim())
            );
        }
        Err(e) => {
            eprintln!("{}", rust_i18n::t!("cli.service.nrptCleanFailed", e = e));
        }
    }
}
