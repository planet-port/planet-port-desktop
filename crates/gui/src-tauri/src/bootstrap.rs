use serde::Deserialize;
use serde_json::Value;
use tauri::Manager;

/// 内嵌资源清单(manifest.json 由 Task 4 组包脚本写入 resources/<triple>/)
#[derive(Debug, PartialEq, Deserialize)]
struct Manifest {
    version: String,
    target: String,
}

/// 资源目录定位:bundle 内为 resources/<triple>,dev 下无该目录 → None(前端降级手动引导)
fn resource_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let triple = current_triple();
    app.path()
        .resource_dir()
        .ok()
        .map(|d| d.join("resources").join(&triple))
        .filter(|d| d.join("manifest.json").exists())
}

/// 当前平台 triple(与 Task 4 组包脚本写入的目录名一致)
fn current_triple() -> String {
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
    let os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(target_os = "windows") {
        "pc-windows-msvc"
    } else {
        "unknown-linux-gnu"
    };
    format!("{arch}-{os}")
}

/// manifest.json 文本 → 结构;缺文件/坏 JSON 一律 None(按"无内嵌资源"处理)
fn parse_manifest(s: &str) -> Option<Manifest> {
    serde_json::from_str(s).ok()
}

/// 提权执行体:拼装平台提权包装命令(纯函数可单测;实际 spawn 在 spawn_elevated_wait)
/// mac → osascript 参数向量;linux → ["pkexec", exe, ...args];
/// windows 不存在该函数 —— ShellExecuteW 的 runas 是动词,不走 argv 构造
#[cfg(target_os = "macos")]
fn elevated_cmd(exe: &std::path::Path, args: &[&str]) -> Vec<std::ffi::OsString> {
    // osascript -e 'do shell script "<cmd>" with administrator privileges'
    // staging 路径含空格(mac 为 Application Support),exe 也须走 shell 引号
    let mut script = shell_quote(&exe.to_string_lossy());
    for a in args {
        script.push(' ');
        script.push_str(&shell_quote(a));
    }
    let line =
        format!("do shell script \"{}\" with administrator privileges", escape_double(&script));
    vec![std::ffi::OsString::from("-e"), std::ffi::OsString::from(line)]
}

/// linux 提权:pkexec 直接接 argv,无需引号转义(pkexec 自身会拆参转发)
#[cfg(target_os = "linux")]
fn elevated_cmd(exe: &std::path::Path, args: &[&str]) -> Vec<std::ffi::OsString> {
    let mut argv = vec![std::ffi::OsString::from("pkexec"), exe.as_os_str().to_os_string()];
    argv.extend(args.iter().map(std::ffi::OsString::from));
    argv
}

/// POSIX sh 单引号包裹:内嵌单引号以 '\'' (关引号-转义-开引号)表达
#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// AppleScript 字符串字面量转义:仅 " 与 \ 需反斜杠(sh 层引号已由 shell_quote 保证)
#[cfg(target_os = "macos")]
fn escape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 资源内的 CLI 可执行文件名(Windows 带 .exe 后缀)
fn exe_file_name(base: &str) -> String {
    if cfg!(target_os = "windows") { format!("{base}.exe") } else { base.to_string() }
}

/// 单次 TCP 探测:连上即视为端口有服务(仅需端口信号,语义校验由心跳的 HTTP 探测负责)
fn tcp_probe(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_ok()
}

/// 递归拷贝目录内容:src 下条目平铺进 dst(staging 只取 resources/<triple> 层内文件)
fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 轮询等待 daemon API 端口可连(每 400ms;每次重读端口,daemon 可能换端口写新 port 文件)
fn wait_port(secs: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if tcp_probe(crate::api_client::api_port()) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
}

/// 安装退出后等待 daemon 上线的时长:mac/linux 拿得到退出码,15s 与 CLI install 等待一致;
/// windows 只知"已提交拉起",须额外覆盖 UAC 确认与安装耗时
#[cfg(target_os = "windows")]
const WAIT_ONLINE_SECS: u64 = 30;
#[cfg(not(target_os = "windows"))]
const WAIT_ONLINE_SECS: u64 = 15;

/// 安装 in-flight 位:同一时刻只允许一路提权安装(防双击/多窗口并发弹 UAC/密码框)
static INSTALL_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// tauri command:释放内嵌资源到 staging 并提权执行 `planet-port install`。
/// 成败不经返回值:提权子进程耗时由用户驱动,invoke 立即返回 Ok(()) 仅表示流程已启动,
/// 结果走 install-done / install-failed(String) 事件,中间态走 install-progress(String)
#[tauri::command]
pub fn start_install(app: tauri::AppHandle) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    // 原子 swap 抢占占位:并发触发直接拒绝(前端已有按钮禁用作主防线,此处为纵深防御)
    if INSTALL_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return Err("install already in progress".to_string());
    }
    let started = stage_and_spawn(&app);
    // 启动期失败(缺资源/拷贝 staging)不会产生后台线程,占位须就地释放
    if started.is_err() {
        INSTALL_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
    started
}

/// 清旧 staging → 拷贝内嵌资源 → 起后台线程提权安装;Ok 仅代表流程已启动
fn stage_and_spawn(app: &tauri::AppHandle) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    use tauri::Emitter;
    let dir = resource_dir(app).ok_or("embedded resources missing")?;
    let staging = app.path().app_data_dir().map_err(|e| e.to_string())?.join("staging");
    // 清旧 staging 后整目录拷贝资源(staging 是我们自己的领地,递归拷贝安全)
    let _ = std::fs::remove_dir_all(&staging);
    copy_dir(&dir, &staging).map_err(|e| format!("copy staging: {e}"))?;
    let cli = staging.join(exe_file_name("planet-port"));
    let app = app.clone();
    std::thread::spawn(move || {
        // 先宣告进入提权阶段:mac 密码框/Windows UAC 可能长时间挂起,前端须即刻切安装态
        let _ = app.emit("install-progress", "elevating");
        let ok = spawn_elevated_wait(&cli, &["install", "--yes", "--json"]);
        let _ = app.emit("install-progress", "waiting-daemon");
        let online = wait_port(WAIT_ONLINE_SECS);
        if ok && online {
            let _ = app.emit("install-done", ());
        } else {
            // 失败原因走短枚举串,前端映射文案(全局约束:错误详情 = 退出码 + 日志指引)
            let reason = if !ok { "install-exit-nonzero" } else { "daemon-timeout" };
            let _ = app.emit("install-failed", reason);
        }
        // 结果落地后释放占位,允许下一次安装/升级
        INSTALL_IN_FLIGHT.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// 提权拉起安装子进程并等待退出;返回 true = 退出码 0。
/// mac:osascript 包装(AppleScript 提权),阻塞到子进程退出;
/// linux:pkexec 直接执行,阻塞到子进程退出;
/// windows:ShellExecuteW("runas") 拿不到进程句柄,返回 true 仅表示已成功提交执行
#[cfg(target_os = "macos")]
fn spawn_elevated_wait(cli: &std::path::Path, args: &[&str]) -> bool {
    std::process::Command::new("osascript")
        .args(elevated_cmd(cli, args))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn spawn_elevated_wait(cli: &std::path::Path, args: &[&str]) -> bool {
    let argv = elevated_cmd(cli, args);
    let Some(pgm) = argv.first() else { return false };
    std::process::Command::new(pgm).args(&argv[1..]).status().map(|s| s.success()).unwrap_or(false)
}

/// windows:UAC 经 "runas" 动词触发;ShellExecuteW 返回值 >32 视为已启动,
/// ≤32(如用户取消 UAC 的 SE_ERR_*)视为未启动,成败只看 wait_port 轮询(无句柄可等)
#[cfg(target_os = "windows")]
fn spawn_elevated_wait(cli: &std::path::Path, args: &[&str]) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;
    let verb = to_wide("runas");
    let file = to_wide(&cli.to_string_lossy());
    // 参数均为固定字面量("install --yes --json"),无需二次引号
    let params = to_wide(&args.join(" "));
    let ret = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(), // HWND:无父窗口
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(), // 工作目录:默认
            SW_SHOW,
        )
    };
    (ret as isize) > 32
}

/// UTF-16 + NUL 结尾(windows-sys 的 PCWSTR 形参所需)
#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// tauri command:内嵌资源清单(前端据此决定自动安装或手动引导;dev 下为 None)
#[tauri::command]
pub fn embedded_manifest(app: tauri::AppHandle) -> Option<Value> {
    let m = parse_manifest(
        &std::fs::read_to_string(resource_dir(&app)?.join("manifest.json")).ok()?,
    )?;
    Some(serde_json::json!({ "version": m.version, "target": m.target }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本机 triple 必须落在三平台命名之内(与 Task 4 组包目录名对齐)
    #[test]
    fn current_triple_is_valid_platform_triple() {
        let t = current_triple();
        assert!(t.contains('-'), "triple 需含架构-系统分隔符: {t}");
        assert!(
            t.ends_with("darwin") || t.ends_with("msvc") || t.contains("linux"),
            "triple 需为 darwin/msvc/linux 平台: {t}"
        );
    }

    /// manifest.json 结构契约:仅 version/target 两字段
    #[test]
    fn parse_manifest_parses_version_and_target() {
        let m = parse_manifest(r#"{"version":"0.4.1","target":"x86_64-unknown-linux-gnu"}"#)
            .expect("合法 manifest 应可解析");
        assert_eq!(m.version, "0.4.1");
        assert_eq!(m.target, "x86_64-unknown-linux-gnu");
    }

    /// 坏 JSON 按无资源处理,不让首启向导拿到半截数据
    #[test]
    fn parse_manifest_returns_none_on_garbage() {
        assert_eq!(parse_manifest("not json"), None);
    }

    /// mac staging 落 ~/Library/Application Support/…(含空格),提权脚本必须引住路径,
    /// 且参数按原样出现在 shell 命令里
    #[cfg(target_os = "macos")]
    #[test]
    fn elevated_cmd_builds_osascript_with_quoted_path_and_args() {
        let argv = elevated_cmd(
            std::path::Path::new("/Users/u/Library/Application Support/x/staging/planet-port"),
            &["install", "--yes", "--json"],
        );
        assert_eq!(argv.len(), 2);
        assert_eq!(argv[0], "-e");
        let line = argv[1].to_string_lossy();
        assert!(line.starts_with("do shell script \""), "应为 osascript 表达式: {line}");
        assert!(line.ends_with("\" with administrator privileges"), "须带提权后缀: {line}");
        assert!(
            line.contains("'/Users/u/Library/Application Support/x/staging/planet-port'"),
            "含空格路径须被单引号包裹: {line}"
        );
        // 参数统一走单引号字面量(sh 语义与裸写等价)
        assert!(
            line.contains("'install' '--yes' '--json'"),
            "install/--yes/--json 须在命令内: {line}"
        );
    }

    /// sh 单引号转义:参数里的单引号用 '\'' 关闭-转义-重开;AppleScript 字符串层再把 \ 双写为 \\
    #[cfg(target_os = "macos")]
    #[test]
    fn elevated_cmd_escapes_single_quotes_and_backslashes() {
        let argv = elevated_cmd(std::path::Path::new("/tmp/o'hara/planet-port"), &["install"]);
        let line = argv[1].to_string_lossy();
        // osascript 实际收到的字面串:'\\'' 反解为 '\'',sh 最终看到 '/tmp/o'\''hara/planet-port'
        assert!(
            line.contains("'\\\\''"),
            "AppleScript 字面量须把 sh 的 '\\'' 转义为 '\\\\'' : {line}"
        );
        assert!(line.ends_with("\" with administrator privileges"), "须带提权后缀: {line}");
    }

    /// 可执行文件名:Windows 带 .exe,其余平台裸名(与 CLI 双 bin 命名一致)
    #[test]
    fn exe_file_name_appends_exe_on_windows_only() {
        let want = if cfg!(target_os = "windows") { "planet-port.exe" } else { "planet-port" };
        assert_eq!(exe_file_name("planet-port"), want);
    }

    /// 测试临时目录(env::temp_dir + 进程号隔离,用毕自清)
    fn temp_dir(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("planet-port-gui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// 拷贝语义:src 的内容平铺进 dst(不新增 <triple> 层),子目录与文件都到位
    #[test]
    fn copy_dir_flattens_source_contents() {
        let src = temp_dir("copy-src");
        let dst = temp_dir("copy-dst");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("planet-port"), b"cli").unwrap();
        std::fs::write(src.join("sub").join("planet-portd"), b"daemon").unwrap();

        copy_dir(&src, &dst).unwrap();

        assert_eq!(std::fs::read(dst.join("planet-port")).unwrap(), b"cli");
        assert_eq!(std::fs::read(dst.join("sub").join("planet-portd")).unwrap(), b"daemon");
        // dst 不应出现 src 目录自身嵌套(dst/ copy-src/ 之类)
        assert!(!dst.join("copy-src").exists());

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    /// 端口探测:监听中的端口可连,未监听端口拒绝连接
    #[test]
    fn tcp_probe_matches_listening_port_only() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_probe(port), "已监听端口应探测成功");
        assert!(!tcp_probe(1), "保留端口 1 未监听,应探测失败");
    }

    /// in-flight 位互斥语义:首次 swap 抢占成功(旧值 false),二次 swap 必得 true(被拒);
    /// 本组测试是唯一触碰该位的用例,进程内初始值恒为 false
    #[test]
    fn install_in_flight_swap_is_exclusive() {
        use std::sync::atomic::Ordering;
        assert!(!INSTALL_IN_FLIGHT.swap(true, Ordering::SeqCst), "首次抢占应成功");
        assert!(INSTALL_IN_FLIGHT.swap(false, Ordering::SeqCst), "二次 swap 应读到占用中");
        assert!(!INSTALL_IN_FLIGHT.load(Ordering::SeqCst), "释放后应回到空闲");
    }
}
