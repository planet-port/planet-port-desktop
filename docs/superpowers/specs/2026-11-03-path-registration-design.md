# Windows 安装自动注册 PATH 与仓库换行符约定 设计

日期：2026-11-03
状态：已确认（用户拍板）

## 背景

安装脚本（install.ps1 / install-offline.ps1）把 CLI 落在 `C:\Program Files\planet-port\`，但不写
PATH，用户装完后直接敲 `planet-port` 报"不是内部或外部命令"。现有提示语只写"建议加入 PATH"，
把负担留给用户。另：repo 无 .gitattributes，换行符无约定，Windows 脚本存在 LF/CRLF 隐患
（cmd 批处理对 LF 敏感会吃行首字符）。

## 决策记录

- 实现位置：二进制 `service install` / `service uninstall`（crates/cli/src/cmd/service/windows.rs），
  非 ps1 脚本 —— 三条安装路径（install.ps1、install-offline.ps1、顶层 `planet-port install`）
  全覆盖，卸载自然清理，符合"脚本只调起、落盘归二进制"的既有职责切分。
- PATH 层级：Machine（HKLM），全机安装语义，与 Program Files 落点一致；安装本就要求管理员。
- cmd 入口：**不做** .bat 转发壳，在 docs/install.md 列特例说明（cmd 调 powershell 执行同一行命令）。
- 换行符：新增 .gitattributes 约定。

## 设计一：安装自动注册 PATH

### install（windows.rs `install()` 尾部）

1. 读 HKLM `SYSTEM\CurrentControlSet\Control\Session Manager\Environment` 的 `Path` 值。
2. 幂等判断：按分号拆条目，忽略大小写比较，已含 `C:\Program Files\planet-port` 则跳过写入。
3. 追加该目录到末尾（前缀分号），**保留原注册表值类型**（REG_EXPAND_SZ / REG_SZ 原样写回），
   不改动任何既有条目（含 `%SystemRoot%` 之类的展开引用，原样字符串拼接）。
4. 写入成功后 `SendMessageTimeout(HWND_BROADCAST, WM_SETTINGCHANGE, 0, "Environment", ...)`
   广播，新开终端立即拿到新 PATH，无需注销。
5. Path 值不存在时按 REG_EXPAND_SZ 新建。

### uninstall（windows.rs `uninstall()`）

对称移除该条目（拆分 → 过滤 → 重新 join → 原类型写回 → 广播），同样幂等。

### 错误处理

PATH 属便利性，不属安装本体：注册表读/写失败（如被组策略锁定）仅打印 warning 继续流程，
不回滚服务安装/卸载。

### 依赖

crates/cli Cargo.toml 的 windows-sys 0.59 增加 features：`Win32_System_Registry`、
`Win32_UI_WindowsAndMessaging`。不加新 crate（MIT，满足选型规则）。

### 生效范围

Machine PATH 在注册表层，cmd / PowerShell / 新进程统一读取，天然双支持。
限制：已开着的终端读的是启动时的环境快照，Windows 机制无法热刷新，提示语写明"重开终端生效"。

### 文案

- rust_i18n：install 完成提示由"建议加入 PATH"改为"已加入 PATH，重开终端后 planet-port 可直接使用"；
  新增一条注册失败 warning 文案；uninstall 提示如涉及同步调整。
- scripts/install.ps1、scripts/install-offline.ps1 尾部提示语同步（"已加入 PATH，重开终端生效"）。

## 设计二：.gitattributes 换行符约定

新增仓库根 .gitattributes：

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

- 落地时 `git add --renormalize .` 一次性归一（现存两个 ps1 为 LF，检出重写为 CRLF，属预期）。
- `eol=lf` 仅影响检出行为，已有 mac/Linux 工作区无感。
- repo 当前无 .bat/.cmd 文件，规则为未来预留。

## 设计三：cmd 安装特例记入文档

docs/install.md Windows 安装节（在线安装与离线安装两处）补一行说明：

> 在 cmd（旧命令行）中同样可装：cmd 不识别 .ps1，但上面的 `powershell -ExecutionPolicy
> Bypass -File ...` 命令在 cmd 里原样可用，无需切换到 PowerShell 窗口。

## 验收

1. 实机 Win11：`service install` 后新开 cmd/PowerShell，`planet-port status` 直接可用；
   Machine PATH 含该目录、原条目与类型不变。
2. 重复安装幂等：PATH 不出现重复条目。
3. `service uninstall` 后 PATH 条目被移除。
4. 现存 User PATH 残留（本次排查手动加的）由 e2e 顺手清理，不属代码职责。
5. `git add --renormalize .` 后 `git status` 仅显示预期文件（.gitattributes 新增 + 必要的重写）。
