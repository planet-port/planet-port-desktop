# Windows 安装自动注册 PATH 与换行符约定 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `service install` 自动把安装目录注册进 Machine PATH（uninstall 对称移除），并给 repo 落定换行符约定。

**Architecture:** PATH 增删实现在 crates/cli/src/cmd/service/windows.rs（所有三条 Windows 安装路径最终都走 `service::install()`，一处覆盖全部）；注册表读写用已有的 windows-sys 0.59 加两个 feature，写后广播 `WM_SETTINGCHANGE`。换行符约定走根级 .gitattributes + renormalize。

**Tech Stack:** Rust（windows-sys 0.59，仅 `cfg(windows)` 代码路径）、git attributes、Markdown 文档。

**Spec:** docs/superpowers/specs/2026-11-03-path-registration-design.md

## Global Constraints

- 本项目不添加测试用例（用户规则 7.1），验证 = `cargo check --target x86_64-pc-windows-msvc -p planet-port` + clippy + Win11 实机 e2e。
- 任务过程中**不逐任务 commit**；全部完成并通过 e2e 后由主会话统一检查 diff 并提交（用户指示"做完再提交"）。
- 注释与文案全部中文，风格对齐 windows.rs 现有注释（讲"为什么"不讲"是什么"）。
- i18n 三份 locale 文件（en.yml / zh-Hans.yml / zh-Hant.yml）必须同步增改。
- PATH 注册失败只 warning 不阻断安装/卸载（spec 错误处理节）。
- 实机操作要点见 memory `windows-e2e-machine`：ssh 需 `-o UserKnownHostsFile=tmp/known_hosts -o PubkeyAuthentication=no`，密码 88888888，配合 sshpass 使用。

---

### Task 1: .gitattributes 换行符约定 + renormalize

**Files:**
- Create: `.gitattributes`（仓库根）
- 无源码改动，renormalize 只重写检出文件

**Interfaces:**
- Consumes: 无
- Produces: 仓库级换行符规则（后续任务不受影响，仅一次性归一）

- [ ] **Step 1: 写入 .gitattributes**

```gitattributes
# 文本文件仓库内统一 LF
* text=auto eol=lf

# Windows 脚本工作区强制 CRLF(cmd 批处理对 LF 敏感会吃行首字符)
*.bat text eol=crlf
*.cmd text eol=crlf
*.ps1 text eol=crlf

# 明确标记二进制,防误判
*.dll binary
*.zip binary
*.png binary
```

- [ ] **Step 2: renormalize 并核对影响面**

```bash
git add .gitattributes
git add --renormalize .
git status --short
```

Expected: `.gitattributes` 新增；被重写为 CRLF 的只有 `scripts/install.ps1`、`scripts/install-offline.ps1`（当前仅这两个文本文件工作区需变 CRLF，其余文件 repo 内本就是 LF）。若出现大量其他文件变动，停下来向主会话报告，不要继续。

- [ ] **Step 3: 确认 ps1 内容除换行外零变化**

```bash
git --no-pager diff --stat scripts/
git --no-pager diff scripts/ | head -20
```

Expected: diff 只有换行符差异（`^M` 行尾），无内容增删。

---

### Task 2: windows.rs 实现 Machine PATH 幂等增删 + 环境变更广播

**Files:**
- Modify: `crates/cli/Cargo.toml:25-30`（windows-sys features 增两项）
- Modify: `crates/cli/src/cmd/service/windows.rs`（文件尾部追加 PATH 节；`install()` 尾部与 `uninstall()` 各插一行调用）

**Interfaces:**
- Consumes: 既有常量 `BIN_DIR`（windows.rs:25）、`progress(json, msg)`（mod.rs）
- Produces: `fn machine_path_add(json: bool)` / `fn machine_path_remove(json: bool)`（仅本文件使用，无外部消费者）；i18n key `cli.service.pathAdded` / `cli.service.pathRemoved` / `cli.service.pathFailed`（Task 3 落文案，本任务的代码先引用）

- [ ] **Step 1: Cargo.toml 加 feature**

`crates/cli/Cargo.toml` 的 `[target.'cfg(windows)'.dependencies]` 段，windows-sys features 列表追加两项（注释同步更新）：

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_System_Threading",
    # Machine PATH 注册(service/windows.rs):注册表读写 + WM_SETTINGCHANGE 广播
    "Win32_System_Registry",
    "Win32_UI_WindowsAndMessaging",
] }
```

- [ ] **Step 2: windows.rs 尾部追加 PATH 注册实现**

在文件末尾（`cleanup_nrpt_rules` 之后）追加：

```rust
// ===== Machine PATH 注册 =====
//
// install 把安装目录幂等追加进 HKLM Session Manager\Environment 的 Path,
// uninstall 对称移除;写后广播 WM_SETTINGCHANGE,新开终端无需注销即可拿到
// 新值(cmd/PowerShell 均生效,注册表层与环境变量读取方无关)。
// PATH 属便利性而非安装本体,任何失败只打 warning,不阻断不回滚——
// 服务装好而 PATH 没写,远好于反过来。
//
// 细节:注册表 Path 值类型 REG_EXPAND_SZ / REG_SZ 均可能(系统镜像默认
// 前者),写回必须保留原类型,否则 %SystemRoot% 等展开引用会被冻结成字面量;
// 条目比较忽略大小写(Windows 路径不区分大小写);追加用纯字符串拼接,
// 不触碰任何既有条目。

/// 系统环境变量注册表子键(Machine 级 PATH 的唯一真源)
const ENV_SUBKEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// UTF-8 → NUL 结尾的 UTF-16(注册表 API 的统一入参形态)
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 读 Machine Path 值。Ok(None) = 值不存在(全新系统状态,按需新建);
/// Err = 注册表操作失败(键被组策略锁定等)
fn read_machine_path(key: HKEY) -> Result<Option<(String, REG_VALUE_TYPE)>, String> {
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;

    let name = wide("Path");
    let mut vtype: REG_VALUE_TYPE = 0;
    let mut len: u32 = 0;
    // 两段式:先探长度(缓冲 null)再取值——单次调用无法预知长度
    let rc = unsafe {
        RegQueryValueExW(
            key, name.as_ptr(), std::ptr::null(),
            &mut vtype, std::ptr::null_mut(), &mut len,
        )
    };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if rc != 0 {
        return Err(format!("RegQueryValueExW 探长失败 rc={rc}"));
    }
    let mut buf = vec![0u8; len as usize];
    let rc = unsafe {
        RegQueryValueExW(
            key, name.as_ptr(), std::ptr::null(),
            &mut vtype, buf.as_mut_ptr(), &mut len,
        )
    };
    if rc != 0 {
        return Err(format!("RegQueryValueExW 读取失败 rc={rc}"));
    }
    // 字节流按本机序还原为 UTF-16 再转字符串(REG_*_SZ 系列即 UTF-16LE)
    let units: Vec<u16> = buf[..len as usize]
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect();
    let value = String::from_utf16_lossy(units.trim_end())
        .trim_end_matches('\0')
        .to_string();
    Ok(Some((value, vtype)))
}

/// 原类型写回并广播环境变更。REG_EXPAND_SZ 写入须显式带 NUL 终止,
/// 长度按字节数(UTF-16 单元数 × 2)
fn write_machine_path(key: HKEY, value: &str, vtype: REG_VALUE_TYPE) -> Result<(), String> {
    let units = wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len() * 2)
    };
    let name = wide("Path");
    let rc = unsafe {
        RegSetValueExW(key, name.as_ptr(), 0, vtype, bytes.as_ptr(), bytes.len() as u32)
    };
    if rc != 0 {
        return Err(format!("RegSetValueExW 失败 rc={rc}"));
    }
    broadcast_env_change();
    Ok(())
}

/// 广播 WM_SETTINGCHANGE:explorer 收到后刷新其环境副本,此后新开的
/// 终端直接拿到新 PATH;已开终端读的是启动时快照,Windows 机制无热刷新
fn broadcast_env_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    // lParam 指向 "Environment" 宽字符串,告诉接收方刷新的是环境变量
    let param = wide("Environment");
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            2000,
            std::ptr::null_mut(),
        );
    }
}

/// 打开环境键执行编辑,确保句柄在所有路径上都被关闭。
/// edit 返回 Some(新值) 才写回(None 表示无需变更,跳过写入)
fn edit_machine_path(edit: impl FnOnce(String) -> Option<String>) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE,
    };

    let subkey = wide(ENV_SUBKEY);
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        )
    };
    if rc != 0 {
        return Err(format!("RegOpenKeyExW 失败 rc={rc}"));
    }
    struct CloseKeyOnDrop(HKEY);
    impl Drop for CloseKeyOnDrop {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }
    let _guard = CloseKeyOnDrop(key);

    // 值不存在时按 (空串, REG_EXPAND_SZ) 新建——系统默认类型,与镜像行为一致
    let (old, vtype) = read_machine_path(key)?.unwrap_or((String::new(), REG_EXPAND_SZ));
    if let Some(new) = edit(old) {
        write_machine_path(key, &new, vtype)?;
    }
    Ok(())
}

/// install 尾部调用:安装目录幂等追加进 Machine PATH
fn machine_path_add(json: bool) {
    let result = edit_machine_path(|old| {
        let exists = old
            .split(';')
            .any(|e| e.trim_end_matches('\0').eq_ignore_ascii_case(BIN_DIR));
        if exists {
            return None; // 幂等:重装/升级不产生重复条目
        }
        let joined = if old.is_empty() {
            BIN_DIR.to_string()
        } else {
            format!("{old};{BIN_DIR}")
        };
        Some(joined)
    });
    match result {
        Ok(()) => progress(json, rust_i18n::t!("cli.service.pathAdded", dir = BIN_DIR).to_string()),
        Err(e) => progress(json, rust_i18n::t!("cli.service.pathFailed", action = "add", e = e).to_string()),
    }
}

/// uninstall 调用:从 Machine PATH 移除安装目录(同样幂等)
fn machine_path_remove(json: bool) {
    let result = edit_machine_path(|old| {
        let kept: Vec<&str> = old
            .split(';')
            .filter(|e| !e.trim_end_matches('\0').eq_ignore_ascii_case(BIN_DIR))
            .collect();
        if kept.len() == old.split(';').count() {
            return None; // 没有条目可移,无需写回
        }
        Some(kept.join(";"))
    });
    match result {
        Ok(()) => progress(json, rust_i18n::t!("cli.service.pathRemoved", dir = BIN_DIR).to_string()),
        Err(e) => progress(json, rust_i18n::t!("cli.service.pathFailed", action = "remove", e = e).to_string()),
    }
}
```

同时在文件头部（`use std::process::Command;` 之后）追加模块级 import（`read_machine_path` 等函数签名用到）：

```rust
use windows_sys::Win32::System::Registry::{
    RegQueryValueExW, RegSetValueExW, HKEY, REG_EXPAND_SZ, REG_VALUE_TYPE,
};
```

注：`HKEY` 在 windows-sys 0.59 为 `*mut core::ffi::c_void`，`REG_VALUE_TYPE` 为 `u32`。若 `cargo check` 报 API 签名不符（版本细节差异），以编译器报错为准微调参数形态（如 `LPARAM` 传法），不改设计。

- [ ] **Step 3: install() / uninstall() 插入调用点**

`install()` 中，在 `progress(json, ... winInstalled ...)` 那行**之前**插入：

```rust
    // 6. Machine PATH 注册:新开终端可直接敲 planet-port(失败仅警告)
    machine_path_add(json);
```

`uninstall()` 中，在 `cleanup_nrpt_rules(json);` 之后、`winRemoved` progress 之前插入：

```rust
    // Machine PATH 对称移除(失败仅警告)
    machine_path_remove(json);
```

- [ ] **Step 4: 编译验证（本机交叉检查，无需 Windows）**

```bash
cargo check --target x86_64-pc-windows-msvc -p planet-port
cargo clippy --target x86_64-pc-windows-msvc -p planet-port 2>&1 | tail -5
```

Expected: check 通过（i18n key 未落地不影响编译——rust_i18n 宏对缺失 key 编译期不报错，运行时回退 key 名）；clippy 无新告警。

---

### Task 3: i18n 文案 + ps1 提示语 + install.md 文档

**Files:**
- Modify: `crates/cli/locales/zh-Hans.yml`（`cli.service.winRemoved` 附近，约 95 行）
- Modify: `crates/cli/locales/en.yml`（约 90 行）
- Modify: `crates/cli/locales/zh-Hant.yml`（约 90 行）
- Modify: `scripts/install.ps1:82`（完成提示）
- Modify: `scripts/install-offline.ps1:48`（完成提示）
- Modify: `docs/install.md:99-134`（cmd 特例 + PATH 差异节）

**Interfaces:**
- Consumes: Task 2 引用的 i18n key（本任务落地文案）
- Produces: 无代码接口

- [ ] **Step 1: 三份 locale 追加三个 key**

zh-Hans.yml（`cli.service.winRemoved` 行后追加）：

```yaml
  cli.service.pathAdded: "已加入 PATH: %{dir}(重开终端后 planet-port 可直接使用)"
  cli.service.pathRemoved: "已从 PATH 移除: %{dir}"
  cli.service.pathFailed: "PATH %{action} 失败(不影响安装/卸载): %{e}"
```

en.yml：

```yaml
  cli.service.pathAdded: "Added to PATH: %{dir} (reopen terminal to use planet-port directly)"
  cli.service.pathRemoved: "Removed from PATH: %{dir}"
  cli.service.pathFailed: "PATH %{action} failed (install/uninstall unaffected): %{e}"
```

zh-Hant.yml：

```yaml
  cli.service.pathAdded: "已加入 PATH: %{dir}(重開終端後 planet-port 可直接使用)"
  cli.service.pathRemoved: "已從 PATH 移除: %{dir}"
  cli.service.pathFailed: "PATH %{action} 失敗(不影響安裝/解除安裝): %{e}"
```

- [ ] **Step 2: 两个 ps1 完成提示语更新**

install.ps1:82 与 install-offline.ps1:48，把

```
Write-Host "  CLI: C:\Program Files\planet-port\planet-port.exe(建议加入 PATH)"
```

改为（离线脚本同步）：

```
Write-Host "  CLI: C:\Program Files\planet-port\planet-port.exe(已加入 PATH,重开终端后 planet-port 可直接使用)"
```

- [ ] **Step 3: docs/install.md 两处补充**

(a) 在线安装节（105-107 代码块后的"脚本行为"段落之前）追加一段：

```markdown
在 cmd(旧命令行)中同样可装:cmd 不识别 .ps1,但上面的
`powershell -ExecutionPolicy Bypass -File ...` 命令在 cmd 里原样可用,
无需切换到 PowerShell 窗口(管理员 cmd:开始菜单搜 cmd → 右键 → 以管理员身份运行)。
```

(b) 离线安装节代码块（119-123）后补同一句 cmd 特例说明（措辞可精简为一行引用上文）。

(c) "与 macOS/Linux 的差异"节的 PATH 条目（133-134）改写为：

```markdown
- **PATH 自动注册(Machine 级)**:安装时自动把 `C:\Program Files\planet-port`
  写入系统 PATH 并广播环境变更,重开终端后 `planet-port` 可直接使用;卸载时
  自动移除。已开着的终端读的是启动时环境快照,需重开窗口。
```

同时把第 96 行 zip 内容清单、第 125-129 行"两条路径最终都走 `service install`"段落中补 PATH 一句（"+ Machine PATH 注册"）。

- [ ] **Step 4: 快速回归编译**

```bash
cargo check -p planet-port && cargo check --target x86_64-pc-windows-msvc -p planet-port
```

Expected: 双目标通过。

---

### Task 4: 构建 Windows 产物 + Win11 实机 e2e 验收

**Files:**
- 无源码修改（验证任务）

**Interfaces:**
- Consumes: Task 1-3 全部改动
- Produces: e2e 验收结论（给主会话决定是否提交）

- [ ] **Step 1: 构建 Windows 产物**

```bash
(export PATH="/opt/homebrew/opt/llvm/bin:$PATH"; \
    cargo xwin build --release --target x86_64-pc-windows-msvc -p planet-port -p daemon)
```

Expected: target/x86_64-pc-windows-msvc/release/ 下产出 planet-port.exe / planet-portd.exe。

- [ ] **Step 2: 部署实机并重装**

```bash
scp -o UserKnownHostsFile=tmp/known_hosts -o PubkeyAuthentication=no \
    target/x86_64-pc-windows-msvc/release/planet-port.exe \
    target/x86_64-pc-windows-msvc/release/planet-portd.exe \
    zivwin@10.152.124.122:C:/Users/zivwin/pp-e2e/   # 目录不存在则先 ssh mkdir
```

ssh 密码 88888888（用 sshpass）。执行（实机 ssh 会话即 High integrity，无需再提权）：

```cmd
C:\Users\zivwin\pp-e2e\planet-port.exe service uninstall
C:\Users\zivwin\pp-e2e\planet-port.exe service install
```

- [ ] **Step 3: 验收断言（全部满足才算过）**

```powershell
# 1. Machine PATH 含安装目录,且既有条目数量未变、类型仍为 REG_EXPAND_SZ
$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SYSTEM\CurrentControlSet\Control\Session Manager\Environment')
$v=$k.GetValue('Path',$null,'DoNotExpandEnvironmentNames'); ($v -split ';') -contains 'C:\Program Files\planet-port'
$k.GetValueKind('Path')
# 2. 重复 install 一遍,PATH 条目数不增(幂等)
# 3. 新开 cmd(ssh 开新会话即新进程环境): where planet-port 命中安装目录、planet-port info 直接可跑
# 4. service uninstall 后: PATH 不再含该目录
```

e2e 时顺手清理：User 级 PATH 里此前手动加的 `C:\Program Files\planet-port` 条目（排查残留，见 spec 验收第 4 条）。

- [ ] **Step 4: 恢复实机安装态**

e2e 结束后重跑 `service install`，确认服务 planetportd RUNNING、`planet-port info` 正常（memory：网络恢复有 1-3 分钟等待期，ping 超时勿误判）。

---

### Task 5: 汇总检查 + 统一提交（主会话执行）

- [ ] **Step 1: `git --no-pager diff` 全量过一遍**——无调试残留、无无关文件、ps1 仅换行符差异 + 提示语一行。
- [ ] **Step 2: 单次提交**

```bash
git add .gitattributes Cargo.lock crates/cli scripts/install.ps1 scripts/install-offline.ps1 docs/install.md docs/superpowers
git commit -m "feat(cli): Windows 安装自动注册 Machine PATH 并约定换行符

- service install/uninstall 幂等增删 HKLM Machine PATH,广播 WM_SETTINGCHANGE
- PATH 注册失败仅警告不阻断安装/卸载;三语文案与 ps1 提示语同步
- 新增 .gitattributes:文本 LF、bat/cmd/ps1 CRLF、二进制显式标记,renormalize 归一
- install.md 补 cmd 运行 ps1 特例说明"
```

（Cargo.lock 若无变化则从列表去掉；commit message 按实际 diff 校准。）
