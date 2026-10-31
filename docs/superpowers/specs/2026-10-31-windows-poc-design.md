# Windows 支持评估 POC 设计（最低 Win10 验证）

日期：2026-10-31
状态：已批准

## 背景与目标

当前平台矩阵：macOS 已发布可用，Linux 数据面已通且有三台实测记录，Windows 处于"📐 规划中"，代码中无任何 `cfg(windows)` 分支。本次评估的目标是回答：**最低 Win10 上，Windows 支持的三条硬骨头（构建链路、设备层、组网互通）各自行不行**。

评估为 POC 级验证，不做服务化、DNS、CLI 安装等非关键路径。

### 关键决策（已拍板）

| 决策点 | 结论 |
|---|---|
| 测试环境 | 实体 Win10 机器（版本/架构待查 `winver`） |
| 评估深度 | POC 级：构建 + 设备 + 组网三道 Gate |
| 设备层路线 | **wintun（L3）**，目标架构，免驱动；ARP/NDP 垫层为新增代价。现有代码全部按 L2 以太网帧写死，POC 不改造它 |
| 构建方式 | Win10 实机本地构建（MSVC + LLVM + Rust），mac 上交叉编译（cargo-xwin）仅作 Gate 0 后的可选实验，不阻塞主线 |
| 开发方式 | mac 编码 + ssh 远程驱动 Win10 实机构建运行，全程不在两台电脑间人工切换 |

### 平台代码隔离原则

Windows 代码全部进 `cfg(windows)` 隔离的新文件，mac/linux 二进制不编译这些代码，现有实现零影响。验证方式：mac 上全量 `cargo build`，现有产物不受影响。

## Gate 划分与通过标准

任一 Gate 失败即停、定位归因，不做补丁式绕过。

### Gate 0：构建链路

- 内容：在 Win10 实机编译 `zt-sys`——cc 驱动 cl.exe 编译 ZeroTier 1.16.2 C++ 核心子集（C++17），bindgen 解析 `Core/include/ZeroTierOne.h` 生成绑定。
- 通过标准：`cargo build -p planet-port-zt-sys` 成功。
- 主要风险：bindgen 对 MSVC 环境下 libclang 的版本匹配。
- 可选附加：cargo-xwin 交叉编译实验（mac 上出 exe），成功则后续开发提效，失败不影响主线。

### Gate 1：设备层

- 内容：tun-rs 创建 wintun L3 适配器（需 wintun.dll 随 exe 分发）、读写数据包、netconfig 配置 IP、route_manager 下发路由。
- 通过标准：适配器在网络面板可见、IP/路由生效（`route print` 验证、ping 自身地址）、能从设备读到本机产生的 ICMP 包。
- 前提：wintun 要求管理员权限运行。

### Gate 2：组网互通

- 内容：
  - Windows 版 node 驱动：UDP socket + wintun 读线程 + 线程唤醒机制（替代 Unix 的 pipe/poll 主循环）；
  - **IPv4 ARP 垫层**：应答远端 peer 的 ARP request（IP → 本机虚拟 MAC），出向 L3 包补 14 字节以太网头喂给 ZeroTier 核心（核心工作在 L2 虚拟交换语义）；
  - sockaddr 布局的 winsock 转换在 POC 内自带（不动 zt-core 的 sa.rs）；
  - 管理 IP/路由按 planet 配置应用。
- 通过标准：加入现有测试 planet，Win10 ↔ mac/linux 节点互 ping IPv4 成功。

## 代码落位

- 新增 `crates/pocs/src/bin/poc4_windows.rs` 及其 Windows 专属模块。
- POC 只依赖 `zt-sys` 的 FFI 绑定 + `tun-rs` + `route_manager`，**完全不碰 zt-core**（现有 fd/poll 代码在 Windows 编译不过，POC 自写精简版 node 驱动，参考 `crates/zt-core/src/node.rs` 逻辑）。
- 所有 Windows 代码包 `#[cfg(windows)]`；Windows 专属依赖放 pocs 的 `[target.'cfg(windows)'.dependencies]`。
- 评估结束、路线确认后，POC 产出再转化为 zt-core 的正式 Windows 后端（另行设计）。

## 环境准备（Win10 实机，一次性）

1. 查明版本与架构（`winver` / `systeminfo`），记录到实测文档；
2. 安装 VS Build Tools（C++ 工作负载）、LLVM、Rust（MSVC 工具链），ssh 静默安装；
3. 开启内置 OpenSSH Server（可选功能）；
4. 网络注意：GitHub 直连受限——rustup/crate 走 rsproxy 镜像，wintun.dll 从 wintun.net 获取；
5. 代码同步：mac 为主仓，ssh 推送（scp/git 按网络实测选）。

## 评估产出

- `docs/compatibility.md` 增加 Win10 实测记录（版本、架构、各 Gate 结果）；
- wintun 路线可行性结论（确认或否决），ARP 垫层实测代价；
- 后续正式 Windows 支持的工作量清单更新（基于实测，非纸面推断）。

## 风险表

| 风险 | 应对 |
|---|---|
| 实机开启 HVCI/严格驱动签名策略，wintun 被拦 | 实测记录；wintun.dll 为 WHQL 签名，默认策略下 Win10 全系理论可用 |
| bindgen 与 MSVC 环境 libclang 版本不匹配 | Gate 0 专门暴露，必要时钉 LLVM 版本 |
| 早期 Win10（1507~1607）装不上最新 VS Build Tools | 降级 VS 2019 工具链 |
| IPv6 NDP 垫层工作量远大于 ARP | POC 只验 IPv4，NDP 留待正式实现阶段 |
| 实机版本过新（非"最低 Win10"） | 结论按实际版本如实记录，标注适用范围 |
