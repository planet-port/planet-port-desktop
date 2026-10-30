# Linux systemd 安装流程 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `planet-port install` / `service` 子命令在 Linux 上完整工作（systemd），发布收敛为 linux_amd64 + linux_arm64 两个包。

**Architecture:** 把 `crates/cli/src/cmd/service.rs`（现为 macOS/launchd 平铺实现）拆为 `mod.rs`（平台无关流程）+ `macos.rs` + `linux.rs` 三个模块，cfg 编译期分流、同名函数即接口；`install.rs` 中的落盘路径常量改为经平台模块获取。Linux 服务管理走 systemd，语义逐条对齐 launchd 现状。

**Tech Stack:** Rust（std::process 调 systemctl/journalctl，不引入新依赖）、bash（install.sh / release.sh）、cargo-zigbuild（Linux 交叉编译，glibc 2.31 基线）。

## Global Constraints

- **不新增 Rust 依赖**：systemctl/journalctl 经 `Command` 调用，与现有 launchctl 方式一致。
- **不写单元测试**（用户规则：未明确要求不自动加测试），验证 = 编译通过 + 两台真机端到端。
- **发布矩阵**：Linux 仅两个产物 `planet-port_<ver>_linux_amd64.tar.gz`、`planet-port_<ver>_linux_arm64.tar.gz`（glibc ≥ 2.31，覆盖 Ubuntu 20.04+ / Mint 21+；不做 .deb/.rpm）。
- **跨平台接口预留**：平台差异只允许出现在 `macos.rs` / `linux.rs` 内，`mod.rs` 与 `install.rs` 不允许出现 `cfg(target_os)` 或平台路径字面量；将来 Windows 加 `windows.rs` 即可。
- **提交信息**：Conventional Commits 中文（见仓库 CLAUDE.md/全局规则），无 Co-authored-by。
- 现有 macOS 行为**逐字节不变**（重构移动，不改逻辑）；i18n 三份 locale（zh-Hans / zh-Hant / en）同步。
- daemon 在两台验证机已后台运行（`/home/<user>/planet-port-test/planet-portd`，pid 由 root nohup 拉起），端到端任务会替换为 systemd 托管。

## 平台模块统一接口

`macos.rs` 与 `linux.rs` 各自导出（`pub(crate)`），`mod.rs` 内 `#[cfg(target_os = ...)] mod macos;` / `mod linux;` + `use` 别名 `platform`：

```rust
// 常量
pub(crate) const CLI_BIN: &str;   // 已装 CLI 落盘路径(install.rs 版本比对、uninstall 清理)
pub(crate) const LINK: &str;      // PATH 入口。macOS=软链路径;Linux=与 CLI_BIN 相同
pub(crate) const BIN_PATH: &str;  // daemon 落盘路径
pub(crate) const DATA_DIR: &str;  // 数据目录(uninstall 时提示保留)
// 函数
pub(crate) fn install(json: bool);          // 定位同目录 daemon(经 mod::locate_daemon_src)后完整安装
pub(crate) fn uninstall(json: bool);
pub(crate) fn start(json: bool);
pub(crate) fn stop(json: bool);
pub(crate) fn logs(lines: usize, follow: bool);
pub(crate) fn service_pid() -> Option<u32>;
pub(crate) fn install_cli(cli_src: &Path, daemon_src: &Path, json: bool); // install.rs 步骤 3:落盘 CLI/daemon + PATH 入口
pub(crate) fn offline_diag();               // status 不在线时的安装态诊断(内部 exit 3)
```

各平台取值：

| 项 | macOS | Linux |
|---|---|---|
| CLI_BIN | `/Library/Application Support/planet-portd/bin/planet-port` | `/usr/local/bin/planet-port` |
| LINK | `/usr/local/bin/planet-port`（软链→CLI_BIN） | `/usr/local/bin/planet-port`（即本体） |
| BIN_PATH | `/Library/Application Support/planet-portd/bin/planet-portd` | `/usr/local/bin/planet-portd` |
| DATA_DIR | `/Library/Application Support/planet-portd` | `/var/lib/planet-portd` |
| 服务定义 | `/Library/LaunchDaemons/cn.imgozi.d.planetport.daemon.plist` | `/etc/systemd/system/planet-portd.service` |
| 日志 | `/Library/Logs/planet-portd/daemon.log` + newsyslog | journald（`journalctl -u planet-portd`） |
| 服务名/label | `cn.imgozi.d.planetport.daemon` | `planet-portd` |

systemd ↔ launchd 语义对照（linux.rs 注释里也要写明）：

| launchd 现状 | systemd 等价 |
|---|---|
| `RunAtLoad` + bootstrap 即拉起 | `WantedBy=multi-user.target` + `systemctl enable` + install 时 `restart` |
| `KeepAlive={SuccessfulExit=false}`（异常退出拉起、SIGTERM exit 0 不拉） | `Restart=on-failure`（systemctl stop 后不会 restart） |
| `kickstart -k`（幂等重启） | `systemctl restart` |
| `launchctl kill TERM` + 等 pid 消失 | `systemctl stop`（自带等退出） |
| `launchctl print` 取 pid/state | `systemctl show -p MainPID --value` / `systemctl is-active` |

---

### Task 1: service.rs 拆分为 mod.rs + macos.rs（纯移动，macOS 行为不变）

**Files:**
- Create: `crates/cli/src/cmd/service/mod.rs`
- Create: `crates/cli/src/cmd/service/macos.rs`
- Delete: `crates/cli/src/cmd/service.rs`
- Modify: 无（`cmd/mod.rs` 里 `mod service;` 对目录模块同样成立）

**Interfaces:**
- Produces: 上表统一接口的 macOS 实现；`mod.rs` 导出 `require_root` / `progress` / `json_done` / `copy_exe` / `probe_online` / `wait_online` / `locate_daemon_src`（从 install.rs 抽不出、给平台 install 用）与 `run()` / `ServiceCmd`。`install.rs` 通过 `super::service` 用到的项（`require_root`、`copy_exe`、`progress`、`install`、`wait_online`）签名不变，本任务先在 `mod.rs` 里 `pub(crate) use platform::*` 再转调，Task 3 才改 install.rs 本体。

**Steps:**

- [ ] **Step 1: 创建 `service/mod.rs`**

把现 `service.rs` 中的**平台无关部分**移入，其余原样保留在头部：

```rust
//! `planet-port service <install|uninstall|start|stop|status|logs>` 服务管理。
//!
//! 平台服务管理(macOS launchd / Linux systemd)收敛在同名子模块 macos/linux
//! 中,经 cfg 编译期选择;本模块只放平台无关的命令分发与公共流程(HTTP 探测、
//! root 校验、原子拷贝、进度输出)。平台接口见 macos.rs/linux.rs 头部注释。

mod macros_placeholder_none; // (占位行不存在——此行不写入,见下)
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "linux")]
use linux as platform;

pub(crate) use platform::{
    install as platform_install, install_cli, uninstall as platform_uninstall,
    BIN_PATH, CLI_BIN, DATA_DIR, LINK,
};
```

（注：`mod` 声明与 `use` 之外，上面 `mod macros_placeholder_none` 一行**不要写入**——计划文档笔误示例。）

`mod.rs` 保留并 re-export 的公共项（内容从现 service.rs **原样剪切**，含注释）：
`ServiceCmd`、`run()`、`progress()`、`json_done()`、`require_root()`、`copy_exe()`、`probe_online()`、`wait_online()`、`read_port_file()`、`port_of()`、`status()`、`kv()`（仅 macos 用则随迁 macos.rs，见 Step 2）。

`run()` 中 install/uninstall/start/stop/logs 的调用改为 `platform::install(...)` 等转发；`status()` 尾部"不在线"分支（现 443-465 行的 plist 检查 + launchctl print 诊断）整体替换为 `platform::offline_diag(); std::process::exit(3);`（macos.rs 内实现同样输出）。

新增公共函数（从现 install.rs / service.rs 重复逻辑合并，两平台共用）：

```rust
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
```

- [ ] **Step 2: 创建 `service/macos.rs`**

现 service.rs 中以下内容**原样移入**（保持注释与逻辑逐字节不变，只改可见性为 `pub(crate)`）：
常量 `LABEL` / `PLIST_PATH` / `LEGACY_LABEL` / `LEGACY_PLIST_PATH` / `APP_DIR` / `BIN_PATH` / `LOG_DIR` / `LOG_PATH` / `NEWSYSLOG_CONF`（`APP_DIR` 更名导出为 `DATA_DIR`：`pub(crate) const DATA_DIR: &str = APP_DIR;`）；函数 `install()` / `uninstall()` / `service_pid()` / `start()` / `stop()` / `logs()` / `wait_unloaded()` / `pid_alive()` / `plist_content()` / `launchctl()` / `launchctl_output()` / `launchctl_output_quiet()` / `kv()`；新增 `offline_diag()`（内容即 status() 被抽走的那段）与 `install_cli()`（内容即 install.rs 步骤 3 的拷贝+软链段，见 Task 3 的最终形态，本任务先建同签名空实现并 `todo!()`？**不**——本任务即完整实现，代码如下）：

```rust
/// install.rs 步骤 3:CLI/daemon 落盘 + PATH 软链。
/// installed 参数:从已安装路径运行时跳过自拷(rename 同路径会自删)
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
    super::progress(json, rust_i18n::t!("cli.install.linked", link = LINK, bin = CLI_BIN).to_string());
}

fn replace_symlink(target: &str, link: &str) -> std::io::Result<()> {
    // 原样从 install.rs 移入(replace_symlink 函数体不变)
    match std::fs::symlink_metadata(link) {
        Ok(_) => std::fs::remove_file(link)?,
        Err(_) => {}
    }
    if let Some(dir) = std::path::Path::new(link).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::os::unix::fs::symlink(target, link)
}
```

`install()` / `uninstall()` 开头的"定位 daemon 源"段替换为 `let src = super::locate_daemon_src();`；`uninstall()` 里本地的 `CLI_BIN` / `LINK` 常量声明删除（改用模块级）。

- [ ] **Step 3: 删除 `service.rs`，编译验证**

```bash
rm crates/cli/src/cmd/service.rs   # 内容已全部迁出
cargo check -p planet-port 2>&1 | tail -5
```
Expected: 无 error（warning 也不应有：`install_cli` 暂未被调用会有 dead_code 警告，加 `#[allow(dead_code)]` 到 install_cli 上，Task 3 移除）。

注意：`install.rs` 现有 `use super::service` 引用的 `service::install` / `service::copy_exe` / `service::progress` / `service::wait_online` / `service::BIN_PATH` 必须仍可用——`mod.rs` 加 `pub(crate) use platform::install;` 等转发（或 `install.rs` 暂不改，mod.rs 提供 `pub(crate) fn install(json: bool) { platform::install(json) }` 包装）。选包装函数方式，Task 3 一并清理。

- [ ] **Step 4: 提交**

```bash
git add crates/cli/src/cmd/service.rs crates/cli/src/cmd/service/
git commit -m "refactor(cli): service 拆分平台模块,launchd 实现移入 macos.rs

body: 为 Linux systemd 实现铺路。mod.rs 收敛平台无关流程(命令分发/
HTTP 探测/root 校验/原子拷贝),macos.rs 保留 launchd 全部现有逻辑,
行为逐字节不变;平台经 cfg 编译期选择,后续 Windows 增 windows.rs 即可。"
```

---

### Task 2: linux.rs systemd 实现 + i18n 词条

**Files:**
- Create: `crates/cli/src/cmd/service/linux.rs`
- Modify: `crates/cli/locales/zh-Hans.yml`、`zh-Hant.yml`、`en.yml`

**Interfaces:**
- Consumes: Task 1 的 `super::locate_daemon_src` / `super::copy_exe` / `super::progress` / `super::json_done` / `super::probe_online` / `super::wait_online` / `super::read_port_file`。
- Produces: 统一接口的 Linux 实现（常量值见前表）。

- [ ] **Step 1: 写 i18n 词条（三份 locale）**

zh-Hans.yml 追加（zh-Hant / en 同 key 同参数翻译，en 用英文）：

```yaml
  cli.service.systemctlFailed: "systemctl %{args} 失败: %{e}"
  cli.service.systemctlExecFailed: "无法执行 systemctl: %{e}"
  cli.service.journalctlExecFailed: "无法执行 journalctl: %{e}"
  cli.service.writeUnitFailed: "写入 %{path} 失败: %{e}"
  cli.service.installedUnit: "  配置:   %{path}"
  cli.service.systemdState: "  systemd 状态: %{state}"
  cli.service.unitEnabled: "已设置开机自启"
```

复用既有 key：`installed`(改用通用文案见 Step 2)、`installedBin`、`installedData`、`notInstalled`、`installedNotRunning`、`notRunningSkip`、`removed`、`dataKept`、`started`、`startTimeout`、`stopped`、`copyFailed`、`renameFailed`、`mkdirFailed`、`chmodFailed`、`needRoot`、`exeNotFound`、`daemonMissing`。
注意 `cli.service.installed`（"已安装 LaunchDaemon %{label}"）是 launchd 专属文案，linux 不用它，新增 `cli.service.installedSvc: "已安装 systemd 服务 %{unit}"`。

- [ ] **Step 2: 写 `linux.rs` 完整实现**

```rust
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

use crate::client::{CliError, Client};
use planet_port_api::endpoints;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const UNIT: &str = "planet-portd";
const UNIT_PATH: &str = "/etc/systemd/system/planet-portd.service";
const DATA_DIR: &str = "/var/lib/planet-portd";
pub(crate) const CLI_BIN: &str = "/usr/local/bin/planet-port";
pub(crate) const LINK: &str = CLI_BIN; // Linux 直接落 PATH,无软链层
pub(crate) const BIN_PATH: &str = "/usr/local/bin/planet-portd";

pub(crate) fn install(json: bool) {
    // 1. 拷 daemon 到固定路径(unit 指向它,发布包解压路径不稳定)
    let src = super::locate_daemon_src();
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
    super::progress(json, rust_i18n::t!("cli.service.unitEnabled").to_string());
    systemctl(&["restart", UNIT]);
    // 5. 等应答确认真的起来了(与 macos bootstrap 后验在线一致)
    if super::wait_online(std::time::Duration::from_secs(15)) {
        let port = super::probe_online().unwrap_or(0);
        super::progress(json, rust_i18n::t!("cli.service.started", port = port).to_string());
    } else {
        eprintln!("{}", rust_i18n::t!("cli.service.startTimeout", log = "journalctl -u planet-portd"));
    }
    super::progress(json, rust_i18n::t!("cli.service.installedSvc", unit = UNIT).to_string());
    super::progress(json, rust_i18n::t!("cli.service.installedBin", path = BIN_PATH).to_string());
    super::progress(json, rust_i18n::t!("cli.service.installedUnit", path = UNIT_PATH).to_string());
    super::progress(json, rust_i18n::t!("cli.service.installedData", dir = DATA_DIR).to_string());
    super::json_done(json, serde_json::json!({"result": "ok"}));
}

pub(crate) fn install_cli(cli_src: &Path, daemon_src: &Path, installed: bool, json: bool) {
    // Linux 无软链层:CLI/daemon 直接落 /usr/local/bin(PATH 规范位,
    // zerotier-one 落 /usr/sbin 同理);installed 时跳过自拷
    if !installed {
        super::copy_exe(cli_src, CLI_BIN);
        super::copy_exe(daemon_src, BIN_PATH);
    }
}

pub(crate) fn uninstall(json: bool) {
    // 未加载/已卸载属预期,不算失败(systemctl 对不存在的 unit 报错 → 静默)
    match systemctl_output(&["stop", UNIT]) {
        Ok(_) => super::progress(json, rust_i18n::t!("cli.service.stopped").to_string()),
        Err(_) => super::progress(json, rust_i18n::t!("cli.service.notRunningSkip").to_string()),
    }
    let _ = systemctl_output(&["disable", UNIT]);
    let _ = fs::remove_file(UNIT_PATH);
    let _ = systemctl_output(&["daemon-reload"]);
    let _ = systemctl_output(&["reset-failed", UNIT]);
    // install 命令落盘的 CLI 与 daemon 一并清;数据目录保留
    let _ = fs::remove_file(CLI_BIN);
    let _ = fs::remove_file(BIN_PATH);
    super::progress(json, rust_i18n::t!("cli.service.removed", plist = UNIT_PATH, bin = BIN_PATH).to_string());
    super::progress(json, rust_i18n::t!("cli.service.dataKept", dir = DATA_DIR).to_string());
    super::json_done(json, serde_json::json!({"result": "ok"}));
}

pub(crate) fn start(json: bool) {
    // restart = 幂等重启(已在运行则换新拉起),等价 kickstart -k
    systemctl(&["restart", UNIT]);
    match super::wait_online(std::time::Duration::from_secs(15)) {
        true => {
            let port = super::probe_online().unwrap_or(0);
            super::progress(json, rust_i18n::t!("cli.service.started", port = port).to_string());
            super::json_done(json, serde_json::json!({"result": "ok", "port": port}));
        }
        false => {
            super::progress(json, rust_i18n::t!("cli.service.startTimeout", log = "journalctl -u planet-portd").to_string());
            super::json_done(json, serde_json::json!({"result": "error"}));
        }
    }
}

pub(crate) fn stop(json: bool) {
    // systemctl stop 发 SIGTERM 并等优雅退出后置 inactive,不拉回
    // (Restart=on-failure 只管异常退出;stop 是明确意图)
    systemctl(&["stop", UNIT]);
    super::progress(json, rust_i18n::t!("cli.service.stopped").to_string());
    super::json_done(json, serde_json::json!({"result": "ok"}));
}

pub(crate) fn logs(lines: usize, follow: bool) {
    // journald 无"文件缺失"概念;unit 不存在时 journalctl 自行报错,原样透传
    let mut args: Vec<String> = vec!["--no-pager".into(), "-n".into(), lines.to_string()];
    if follow {
        args.push("-f".into());
    }
    args.push("-u".into());
    args.push(UNIT.into());
    match Command::new("journalctl").args(&args).status() {
        Ok(s) if s.success() => {}
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("{}", rust_i18n::t!("cli.service.journalctlExecFailed", e = e));
            std::process::exit(1);
        }
    }
}

pub(crate) fn service_pid() -> Option<u32> {
    systemctl_output(&["show", UNIT, "-p", "MainPID", "--value"])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|p| *p != 0)
}

/// status 不在线时的安装态诊断:unit 缺失 = 未安装,否则给 systemd 状态
pub(crate) fn offline_diag() {
    if !Path::new(UNIT_PATH).exists() {
        println!("{}", rust_i18n::t!("cli.service.notInstalled", path = UNIT_PATH));
        std::process::exit(3);
    }
    println!("{}", rust_i18n::t!("cli.service.installedNotRunning"));
    if let Ok(state) = systemctl_output(&["is-active", UNIT]) {
        println!("{}", rust_i18n::t!("cli.service.systemdState", state = state.trim()));
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

/// 执行 systemctl,失败打印 stderr 并退出 1
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
```

- [ ] **Step 3: 编译验证（Linux 目标 + macOS 目标互不破坏）**

```bash
cargo zigbuild --target x86_64-unknown-linux-gnu.2.31 -p planet-port 2>&1 | tail -3
cargo check -p planet-port 2>&1 | tail -3
```
Expected: 两条都无 error。

- [ ] **Step 4: 提交**

```bash
git add crates/cli/src/cmd/service/linux.rs crates/cli/locales/
git commit -m "feat(cli): service 新增 Linux systemd 平台实现

body: 与 macos.rs 同接口:unit 落 /etc/systemd/system,二进制直接落
/usr/local/bin,数据目录 /var/lib/planet-portd,日志走 journald 免轮转。
Restart=on-failure 对齐 KeepAlive.SuccessfulExit 语义,systemctl
restart/stop 对齐 kickstart -k/launchctl kill TERM。"
```

---

### Task 3: install.rs 适配平台钩子

**Files:**
- Modify: `crates/cli/src/cmd/install.rs`

**Interfaces:**
- Consumes: `service::{CLI_BIN, install_cli, install, wait_online, require_root, progress}`（Task 1/2 产出）。
- Produces: `install` 命令双平台工作；删除本地 `CLI_BIN`/`LINK` 常量与 `replace_symlink`（已移入 macos.rs）。

- [ ] **Step 1: 改 install.rs**

变更点（其余流程——版本确认、升级兜底、running_planets——不动）：
1. 删除 `const CLI_BIN` / `const LINK` 与 `replace_symlink()`；`installed_version()` 用 `service::CLI_BIN`。
2. 步骤 3 整段（落盘+软链，现 87-97 行）替换为：

```rust
    // 3. 落盘 + PATH 入口(macOS:App Support+软链;Linux:直接 /usr/local/bin)
    service::install_cli(&cli_src, &daemon_src, installed, json);
```

3. 头部模块注释的平台专属描述改为双平台（"装载 launchd 服务" → "装载系统服务(launchd/systemd)"），路径示例删去或改为引用 service 模块。

- [ ] **Step 2: 编译验证**

```bash
cargo check -p planet-port && cargo zigbuild --target aarch64-unknown-linux-gnu.2.31 -p planet-port
```
Expected: 无 error、无 dead_code 警告（Task 1 的 `#[allow(dead_code)]` 移除）。

- [ ] **Step 3: macOS 真机回归（本机）**

```bash
sudo ./target/debug/planet-port install && planet-port service status
```
Expected: 输出与重构前一致（已装版本相同 → 幂等重装不提示）。

- [ ] **Step 4: 提交**

```bash
git add crates/cli/src/cmd/install.rs
git commit -m "refactor(cli): install 落盘路径收敛到平台模块

body: CLI_BIN/LINK 常量与软链逻辑下沉 macos.rs/linux.rs 的
install_cli,Linux 无软链层直接落 /usr/local/bin;主流程(版本
确认/升级兜底)保持平台无关。"
```

---

### Task 4: install.sh 支持 Linux

**Files:**
- Modify: `scripts/install.sh`

- [ ] **Step 1: 修改 install.sh**

变更点：
1. 架构标记（38-41 行）：

```bash
case "$(uname -m)" in
    arm64|aarch64) ARCH_TAG="$( [[ "$PLATFORM" == "linux" ]] && echo arm64 || echo aarch64 )" ;;
    x86_64|amd64)  ARCH_TAG="$( [[ "$PLATFORM" == "linux" ]] && echo amd64 || echo x86_64 )" ;;
    *) ARCH_TAG="" ;;
esac
```

2. 平台闸门（43-45 行）改为：

```bash
if [[ "$PLATFORM" == "unsupported" ]]; then
    die "当前平台(${OS} $(uname -m))的版本尚未发布,已支持: macOS(arm64/x86_64)、Linux(amd64/arm64,glibc≥2.31);Windows 在发布计划中"
fi
```

3. 路径常量（51-52 行）平台分流：

```bash
if [[ "$PLATFORM" == "macos" ]]; then
    APP_BIN="/Library/Application Support/planet-portd/bin"
else
    APP_BIN="/usr/local/bin"
fi
LINK="/usr/local/bin/planet-port"
```

4. `--no-service` 分支（99-108 行）：macOS 保持现逻辑；Linux 直接 `install -m 755` 落 `/usr/local/bin/planet-port{,d}`（无软链），输出文案共用。实现为在现 for 循环外按平台包一层软链步骤：仅 macOS 执行 `ln -sfn`。
5. 头部注释（8-15 行）与尾装完成输出：补 Linux 落点说明（`/usr/local/bin/`、`/etc/systemd/system/planet-portd.service`）。

- [ ] **Step 2: 本地验证（shell 语法 + PLANET_PORT_TARBALL 干跑不执行）**

```bash
bash -n scripts/install.sh && echo syntax-ok
```

- [ ] **Step 3: 提交**

```bash
git add scripts/install.sh
git commit -m "feat(scripts): install.sh 支持 Linux amd64/arm64 在线安装

body: 平台识别放行 Linux,产物命名 linux_amd64/linux_arm64 对齐
发布矩阵;--no-service 落盘路径按平台分流,Linux 直接落
/usr/local/bin 无软链层;安装主体仍由 planet-port install 承担。"
```

---

### Task 5: release.sh 增加 Linux 双目标

**Files:**
- Modify: `scripts/release.sh`

- [ ] **Step 1: 修改 release.sh**

变更点：
1. TARGETS 与构建命令分平台（Linux 用 zigbuild + glibc 2.31 基线，macOS 用 cargo build）：

```bash
# 目标三元组;linux 目标带 .2.31 后缀(cargo-zigbuild 的 glibc 基线标记)
TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin" \
         "x86_64-unknown-linux-gnu.2.31" "aarch64-unknown-linux-gnu.2.31")
arch_tag() {
    case "$1" in
        aarch64-apple-darwin)            echo "macos_aarch64" ;;
        x86_64-apple-darwin)              echo "macos_x86_64" ;;
        x86_64-unknown-linux-gnu.2.31)    echo "linux_amd64" ;;
        aarch64-unknown-linux-gnu.2.31)   echo "linux_arm64" ;;
        *) echo "" ;;
    esac
}
build_target() { # $1=target
    case "$1" in
        *-linux-*) cargo zigbuild --release --target "$1" -p planet-port -p daemon ;;
        *)         cargo build --release --target "$1" -p planet-port -p daemon ;;
    esac
}
```

（注意 zigbuild 产物目录是 `target/<三元组去后缀>/release`，循环里 `BIN_DIR="target/${target%.*}/release"` 处理。）

2. 循环内 `cargo build` 行替换为 `build_target "$target"`，`BIN_DIR` 改 `"target/${target%.*}/release"`。
3. NOTES 产物表追加两行：`| planet-port_${VERSION}_linux_amd64.tar.gz | Linux x86_64 (glibc≥2.31) |`、`| planet-port_${VERSION}_linux_arm64.tar.gz | Linux arm64 (glibc≥2.31) |`。
4. 头部注释"双架构构建"改为"四目标构建(macOS×2 + Linux×2)"。

- [ ] **Step 2: 验证**

```bash
bash -n scripts/release.sh && echo syntax-ok
```

- [ ] **Step 3: 提交**

```bash
git add scripts/release.sh
git commit -m "feat(scripts): release.sh 增加 linux_amd64/linux_arm64 产物

body: 发布矩阵收敛为 macOS 双架构 + Linux amd64/arm64 共四目标;
Linux 走 cargo-zigbuild 交叉编译,glibc 2.31 基线覆盖 Ubuntu 20.04+,
不做按发行版拆包。"
```

---

### Task 6: 文档更新

**Files:**
- Modify: `docs/install.md`
- Modify: `COMMANDS.md`（service/install 章节如有平台专属表述）

- [ ] **Step 1: 更新 docs/install.md**

- 顶部"适用平台"改为：macOS(Apple Silicon/Intel)、Linux(amd64/arm64,glibc≥2.31,systemd);Windows 在发布计划中。
- 系统落点表加 Linux 列（或并列第二张表）：`/usr/local/bin/planet-port{,d}`、`/etc/systemd/system/planet-portd.service`、`/var/lib/planet-portd`、日志 `journalctl -u planet-portd`。
- "install 命令细节"一节补充 Linux 差异说明（无软链层、journald 免轮转、Restart=on-failure 语义）。
- 离线安装示例的 tar.gz 名换成 `planet-port_0.1.0_linux_amd64.tar.gz` 或加双平台示例。

- [ ] **Step 2: 更新 COMMANDS.md**

检查 `service install/uninstall/start/stop/status/logs` 与顶层 `install` 的输出示例,launchd 字样改为"系统服务(launchd/systemd)",Linux 下 logs 为 journalctl、start 超时提示指向 journalctl。

- [ ] **Step 3: 提交**

```bash
git add docs/install.md COMMANDS.md
git commit -m "docs: 安装指南与命令规范补 Linux 平台说明

body: 适用平台、系统落点、日志与轮转差异、发布矩阵更新为
四目标;离线安装示例补 linux_amd64 命名。"
```

---

### Task 7: 两台真机端到端验证

**Files:** 无代码变更（验证任务）。

验证机（sudo 需密码,交互命令经用户执行）：
- gozimba@10.152.124.24（Linux Mint 21.3,x86_64,已跑官方 zerotier-one）
- ztl@10.121.15.88（Ubuntu 20.04,aarch64,已跑官方 zerotier-one）

- [ ] **Step 1: 本机重新交叉编译两目标（debug 即可）**

```bash
cargo zigbuild --target x86_64-unknown-linux-gnu.2.31 -p planet-port -p daemon
cargo zigbuild --target aarch64-unknown-linux-gnu.2.31 -p planet-port -p daemon
```

- [ ] **Step 2: 上传并清理手动测试残留**

```bash
scp target/x86_64-unknown-linux-gnu/debug/planet-port target/x86_64-unknown-linux-gnu/debug/planet-portd \
    gozimba@10.152.124.24:~/planet-port-test/
scp target/aarch64-unknown-linux-gnu/debug/planet-port target/aarch64-unknown-linux-gnu/debug/planet-portd \
    ztl@10.121.15.88:~/planet-port-test/
```
残留 daemon（root nohup 的手动实例）需先杀：含在 Step 3 脚本里 `pkill -f planet-portd`。

- [ ] **Step 3: 端到端脚本（用户经 `! ssh -t` 执行,每机一条）**

脚本 `e2e.sh`（随二进制一起 scp）：

```bash
#!/bin/bash
set -e
cd ~/planet-port-test
echo "== 0. 清理手动测试实例 =="
pkill -f planet-portd 2>/dev/null || true; sleep 1
echo "== 1. install =="
sudo ./planet-port install
echo "== 2. status =="
planet-port service status
echo "== 3. info =="
planet-port info
echo "== 4. logs(journald) =="
journalctl -u planet-portd -n 5 --no-pager
echo "== 5. service stop/start =="
sudo planet-port service stop
planet-port service status || echo "  (退出码 $? = 已装未运行,预期 3)"
sudo planet-port service start
planet-port service status
echo "== 6. 开机自启 =="
systemctl is-enabled planet-portd
echo "== 7. 重复 install 幂等 =="
sudo ./planet-port install --yes
echo "== 8. uninstall =="
sudo planet-port service uninstall
ls /etc/systemd/system/planet-portd.service 2>&1 || echo "  unit 已删(预期)"
ls /var/lib/planet-portd >/dev/null && echo "  数据目录保留(预期)"
echo "== 9. 重装(终态:留在机器上运行) =="
sudo ./planet-port install
planet-port service status
```

用户执行：
```
! ssh -t gozimba@10.152.124.24 'bash ~/planet-port-test/e2e.sh'
! ssh -t ztl@10.121.15.88 'bash ~/planet-port-test/e2e.sh'
```
（sudo 密码在各 sudo 调用处输入;或先 `sudo -v` 缓存凭证——Step 3 前加 `sudo -v` 一行。）

Expected: 全部步骤按标注预期通过；官方 zerotier-one 接口全程不受影响（`ip link` 里 zt* 接口不变）。

- [ ] **Step 4: 结果回填与收尾**

验证输出贴回会话;若有失败项,按 superpowers:systematic-debugging 排查修复后重跑。通过后：

```bash
git --no-pager log --oneline -8   # 确认提交序列
```

---

## Self-Review 记录

- 覆盖检查：用户三项要求——手动验证(Task 前置已完成)、完善安装流程(Task 1-3)、发布收敛 amd/arm64(Task 4-5)——均有对应任务；跨平台接口预留落在 Task 1 的模块结构。
- 类型一致性：`install_cli(cli_src, daemon_src, installed, json)` 四参签名在 Task 1/2/3 三处一致;`DATA_DIR` 在 macos.rs 为 `APP_DIR` 别名、linux.rs 独立常量,`mod.rs` re-export 统一。
- 占位符扫描：Task 1 Step 1 中已自纠一处笔误示例（mod 占位行,文中已标注不写入）;linux.rs 为完整可编译代码。
