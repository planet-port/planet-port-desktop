# wincli 设计（Windows 正式客户端，功能与完成度对齐 macOS）

## 背景与目标

Windows POC（windows-poc 分支，报告见 docs/2026-10-31-windows-poc-report.md）已在 Win11 实机验证三道 Gate 全 PASS：MSVC 构建链路、wintun L3 设备层、与 mac 节点组网互 ping。本设计把 POC 验证过的能力**正式化**：Windows 上获得与 macOS 现状完全对等的客户端——daemon 数据面、系统服务、CLI 全命令、DNS 接线、线上与离线安装。

**成功标准**：Win11 实机上完成与 Linux/macOS 同级的 e2e 验收（install→status→join→互 ping→stop/start→uninstall→重装，含离线安装路径）；mac/linux 现有功能零回归；与官方 zerotier-one 同机共存无干扰（POC 已有实测背书）。

### 关键决策（已拍板）

- **架构**：方案一——POC 架构 cfg 分叉回迁正式 crate，与仓库 `cfg(target_os)` 惯例一致，mac/linux 代码路径零改动
- **范围**：完整正式版（数据面+服务化+CLI 全命令+DNS+安装分发），本版本不做代码签名
- **分发**：线上 `install.ps1`（iex）+ 离线 zip，对齐 mac 的 install.sh + tar.gz 形态
- **实测基线**：Win11 build 26200；最低 Win10 纸面支持（wintun 要求 Win10+），实测标注
- **分支**：全部工作在 windows-poc 分支完成后再推

## 总体架构（mac → Windows 对应）

| 层 | macOS 现状 | Windows 对应（POC 已验证项标注 ✅） |
|---|---|---|
| 设备 | feth 对（L2，内核 ARP） | wintun（L3）+ 用户态 ARP 垫层 ✅ |
| 路由 | route_manager mac 后端 | route_manager Windows 后端（IP Helper） ✅ |
| node 驱动 | Unix pipe/poll 唤醒 | UDP 读线程 + mpsc 唤醒 ✅ |
| IPC | loopback TCP + authtoken | **原样复用，零改动** |
| 服务 | launchd Daemons（root） | Windows Service（LocalSystem） |
| DNS 覆写 | networksetup | NRPT（DnsClientNrptRule） |
| 安装 | install.sh + tar.gz | install.ps1（线上）+ zip（离线） |

多 planet、per-planet 身份、registry 数据结构、daemon API（api_* 模块）全部平台无关，不动。

## 数据面回迁（zt-core）

- `node.rs`：驱动线程按 `cfg(windows)` 分叉，架构取自 POC driver.rs——六回调 glue、UDP 独立读线程（读超时 + running 检查）、`AsRawSocket`、c_ulong ADI、关停硬顺序。**回补 POC 裁剪项**：leave/orbit/deorbit 命令、pathCheck/pathLookup 回调（POC 置 None）
- `device.rs`：DevicePlane 内部按平台分叉——mac feth L2 / windows wintun L3 + `ArpShim`；对外 `apply()`/teardown 接口签名不变，daemon 无感知
- `routes.rs`：RouteApplier 加 Windows 后端（POC routes_win.rs 语义：prefix==0 跳过、差分应用、失败告警继续）
- `sa.rs`/bindgen 布局：POC 已实证的 Windows 形态（AF_INET:u32、in6_addr.u.Byte、sin_family:ADDRESS_FAMILY、in_addr.S_un、ADI c_ulong）直接沿用
- **已知平台差异（如实文档化，不假装等价）**：Windows 设备面 IPv6 帧静默丢弃（计 v6drop，NDP 垫层不做）；L3 垫层引入的 ARP 首包代价（POC 实测 31/32 通）

## 服务化（cli/src/cmd/service/windows.rs）

- 命令面对齐 macos.rs：`install / uninstall / start / stop / status / is-enabled`
- `install`：拷贝二进制到服务目录、`sc.exe create` 注册 LocalSystem 自启动服务、写服务定义；`uninstall` 停服务删注册（数据保留，对齐 mac）
- daemon 程序加 Windows Service 入口：手写 SCM 接口（StartServiceCtrlDispatcher + 服务状态回调），不引第三方 service 框架——依赖面小，且 daemon 主逻辑（tokio runtime）与 SCM 启动信号的胶合层很薄
- cli 普通命令（planet/network/peer 等）走既有 loopback API，天然跨平台零改动
- 权限：服务以 LocalSystem 运行；`service install/uninstall` 需管理员（PowerShell `-Verb RunAs` 提升或文档说明），普通命令无需提权

## DNS NRPT

- `dnsMode: zerotier`：为网络托管域加 NRPT 规则，命名服务器指向 daemon 本地 DNS（loopback）
- `none / custom` 语义同 mac；leave/uninstall 时清理全部 NRPT 规则（按命名规则匹配本产品条目，不误删他人规则）
- DNS 服务端在 daemon 内（平台无关），只动系统接线层（daemon/dns.rs 加 windows 分支）

## 安装与分发

- **产物**：zip 内含 `planet-port.exe`（CLI）与 `planet-portd.exe`（daemon，对齐 mac/linux 的双 binary 资产形态：`-p planet-port -p daemon`）、`wintun.dll`（0.14.1，随包分发，放 exe 同目录）、`install-offline.ps1`；命名 `planet-port_<ver>_windows_x86_64.zip` 对齐既有 `tar.gz` 规范
- **线上**：install.ps1 托管 GitHub release，`iwr <url> | iex` 一条命令：选版本→下载 zip→解压 `C:\Program Files\planet-port\`→注册服务→提示 PATH
- **离线**：解压 zip 后管理员运行 install-offline.ps1，逻辑同线上（跳过下载）
- **release.sh**：加 `x86_64-pc-windows-msvc` 目标（cargo-xwin 交叉构建）。风险：POC 只验证到 check，交叉**链接**未实测——计划首任务实测，失败回退"实机构建产物上传"，流程不阻塞
- **不签名**：README/安装文档注明 SmartScreen 拦截属预期（"更多信息→仍要运行"）
- **数据目录**：`C:\ProgramData\planet-port\`（对齐 mac 的 `/Library/Application Support/planet-portd`），ACL 收敛 SYSTEM/Administrators（POC 的 0600 收敛留白在此补齐）

## 错误处理与边界

- 设备/路由操作失败：告警继续（POC 语义），状态面（cli info/status）如实反映
- 服务启动失败：SCM 报错码透传到 cli 输出
- 与官方 zerotier-one 共存：POC 实证无干扰（独立适配器/端口/数据目录）；wincli 服务名、适配器前缀（`planet-port`）与官方（`ZeroTier One`）天然不冲突
- 出口 UDP 过滤环境（家宽对根新建流丢弃）：不做产品级规避，文档 FAQ 说明排查思路（POC §3.6 记录）

## 测试与验收

1. **平台回归**：mac/linux `cargo build --workspace` 双 profile 通过；cfg 隔离零改动验证（diff 既有平台文件）
2. **Win11 实机 e2e**（对齐 Linux 验收清单）：线上安装→status/info→planet 导入→join→与 mac 节点互 ping→stop/start→is-enabled→uninstall（unit 删+数据留）→重装运行；外加离线 zip 安装路径
3. **共存验证**：官方 zerotier-one 同机运行全程无干扰
4. compatibility.md 补 Win11 安装级行（POC 级行保留，如实区分）

## 风险表

| 风险 | 等级 | 缓解 |
|---|---|---|
| cargo-xwin 交叉链接失败（zt-sys 全量 .obj） | 中 | 计划首任务实测；回退实机构建上传，产物形态不变 |
| SCM 手写胶合层边角（停止超时/崩溃恢复） | 中 | 对齐 mac 硬顺序；实机 e2e 覆盖 stop/start/崩溃重启 |
| NRPT 在无管理员上下文行为差异 | 低 | 服务以 LocalSystem 运行，天然有权限；实测确认 |
| L3+ARP 垫层的长尾（非常规流量） | 低 | POC 31/32 实测；v6drop/帧计数可观测性已在 |
| Win10 下限未实测 | 低 | 纸面支持标注；后续找 Win10 实机补安装级验证 |
