# Planet Port 桌面端(macOS / Windows / Linux)迁移可行性调研 —— Rust 重构方案

> 日期:2026-10-28
> 性质:可行性调研报告(非实施设计)。回答四个问题:
> ① 迁移桌面是否可行、代价多大;② 桌面 UI 如何适配、哪些移动端配置可移除;
> ③ 多 Planet 同时开启是否可行;④ 能否拆分为 CLI 核心 + 纯 UI 交互层。
> 注:本文档定稿为 **Rust 重构单一方案,不依赖 Docker**。Kotlin/JVM 备选路线经评估后排除(见 §5 收敛记录,评估细节见本文件 git 历史)。
> 结论置信度标注:【已验证】官方源码/文档直接证实;【社区经验】多方社区资料交叉印证;【推测】基于事实的推断。

## 0. 总结论(先说重点)

| 问题 | 结论 |
|---|---|
| 整体可行性 | **可行**。Rust 重构方式:store/planet/moon 的模型与校验算法从 Android 版**语义平移**(约 1.2k 行等价逻辑,现有实现作黄金向量);用户态数据面**不移植**(交给内核,§2.3);全新实现的只有 FFI 封装层、每网络设备 + 配置应用器、daemon/CLI/GUI |
| 多 Planet 同时开启 | **可行**。ZeroTier 核心一个 node 同一时刻只有一份 planet(`Topology::_planet` 单成员),多 planet 并存 = 多 node 实例(每 planet 一个实例、独立 dataStore + 独立 UDP 端口)。官方明文支持同机多实例。Android 端"按 planet 分目录隔离"的设计(`files/zt/<planetId>/`)正是这个方向的地基 |
| CLI/UI 拆分 | **可行且是业界标准形态**。特权 daemon 持核心与设备,本地 HTTP/JSON API(ZeroTier 官方 `127.0.0.1:<port>` + authtoken 模式、Tailscale LocalAPI 模式双先例),CLI 与 GUI 都是薄客户端。与"多 planet = 多实例"天然契合:daemon 内多 Node 实例,每个实例挂各自的 planet |
| 技术栈(定稿) | **Rust daemon(FFI 直链 libzerotiercore 1.16.2)+ Rust CLI + Tauri 2 GUI**;本地 REST API 对齐 zerotier-one 语义 |

**一句话**:桌面端不是"把 App 移植过去",而是借这次机会把架构升级为
**「planet-portd(daemon)+ planet-port(CLI)+ Planet Port(GUI)」**——iOS 的 ZTBridge/PacketTunnelProvider、Android 的 ZeroTierVpnService 在桌面上的对应物是一个 root daemon。

## 1. 现状盘点:迁移的本钱是什么

依据本仓库(Android 版)代码实测(grep `import android|androidx` 逐文件分类)。Rust 方案下复用方式是**语义与算法平移**,非代码复用:

### 1.1 可平移资产(语义与算法)

| 资产 | 来源 | 平移方式 |
|---|---|---|
| 四份 JSON 数据契约(planets/networks/moons/prefs 字段语义、Mutex + tmp/rename 原子写、损坏隔离不重建) | `store/` | Rust(serde)模型照抄字段与语义,保持三端文件结构互认 |
| planet 校验算法(0x01 头 + worldId 大端解析、非官方 planet ID) | `PlanetImporter.kt` | 直接翻译,现有实现作黄金向量 |
| moon 文件解析 | `MoonImporter.kt` | 同上 |
| NodeRuntime dataStore hook 语义(planet 重定向、moon/peer/netconf 按 planet 分目录、identity 跨 planet 共享) | `NodeRuntime.kt:236-242` | FFI 封装层照此语义实现——**多 planet 的核心机制** |
| QRTransfer 二维码传输协议 | `qrtransfer/` | 纯算法;桌面可作二维码展示端,让手机扫桌面 |

### 1.2 不移植清单(理由见 §2.3 数据面原则)

数据面全家:`Dataplane`、NAT44、`ArpTable`/`NdpTable`、`FrameConverter`、`RoutePlanner`、`PendingPacketQueue`、`UdpCom` 的 `protect()`(`UdpCom.kt:13-17`)、`TunnelManager` 的 `VpnService.Builder.establish()`(`TunnelManager.kt:377-399`)、`core/jni/ZT_tunfd.cpp`(Android OEM 修正,直接丢弃)。

### 1.3 全新实现清单

FFI 封装层(-sys + safe wrapper)、每网络设备创建 + managed IP/route/DNS 配置应用器、macOS/Windows 的 ARP/NDP 用户态垫层(§2.3③)、daemon + REST API + CLI、Tauri GUI、三平台特权安装与签名分发。

## 2. 专题一:三平台 TUN 通道与特权模型

### 2.1 TUN API(多设备并发三平台均可行)

| 平台 | 通道 | 要点 | 置信度 |
|---|---|---|---|
| macOS | `UTUN_CONTROL` 内核控制套接字 → utun(L3) | **root daemon 直接建 utun,不需要 System Extension**;sysext 只是上架/沙盒分发路径(官方 GUI 走 sysext,Homebrew CLI 版至今走 root daemon + 特权 helper MacEthernetTapAgent)。系统自身就并发多个 utun | 已验证 |
| Linux | `/dev/net/tun` + `ioctl TUNSETIFF` | 需 `CAP_NET_ADMIN`(文件权限不够);非 root 可 `setcap cap_net_admin+ep`。TUN(L3)/TAP(L2)均可,可开任意多个 | 已验证 |
| Windows | **wintun**(L3,推荐)或 tap-windows6(L2) | 官方 ZeroTier 用改版 tap-windows6(L2,内核处理 ARP,但要装 NDIS 驱动);**wintun 是单个签名 DLL、免驱动安装**,代价是 L3 设备需 ARP/NDP 用户态垫层(§2.3③,同 macOS utun 处理方式);多适配器共存是 API 明确支持的语义 | 已验证 |

多设备并发 = 多实例、多网络设备的前提,三平台均无硬性限制【社区经验】。

来源:wintun.net 及 wintun.h API;ZeroTierOne `osdep/MacEthernetTapAgent`、`ext/bin/tap-windows-ndis6/x64/zttap300.inf`;ZeroTier macOS 官方 wiki。

### 2.2 特权模型

通行模式(ZeroTier / Tailscale 一致):**特权 daemon + 非特权 GUI/CLI + 本地 IPC**,提权收敛为一次性安装动作,运行期 GUI 永不提权:

- macOS:LaunchDaemon(root),现代注册方式 SMAppService(macOS 13+);或 sysext 路线(需特权 entitlement + Developer ID 公证,仅当要上沙盒分发时考虑)
- Windows:Windows Service(SYSTEM)持 TUN,GUI 普通用户进程
- Linux:systemd service(root)或 setcap 非特权

来源:zerotier-one.8.md、zerotier-cli.1.md、docs.zerotier.com、tailscale.com/docs/concepts/macos-variants。

### 2.3 数据面原则:复用系统网络栈,不移植用户态数据面

移动端的整套用户态数据面(NAT44、按网 ARP/NDP 表、FrameConverter、RoutePlanner、PendingPacketQueue)是被平台约束逼出来的:Android VpnService 只给**一根 L3 TUN fd**,没有以太网头、多网络共享一根设备,只能自己模拟 ARP、改写源地址、做选路。桌面没有这些约束,**照官方 zerotier-one 桌面版的模型走:每网络一张虚拟网卡,数据路径全部交给内核**:

| 移动端被迫自建 | 桌面系统原生替代 |
|---|---|
| 每网 ARP/NDP 用户态表 | 内核处理 ARP(L2 设备;L3 设备则仅在垫层应答,见下) |
| NAT44 源地址改写 + LPM 选网 | 内核路由表 + 源地址选择 |
| 单 TUN 多网络复用 | 多设备并发(§2.1) |
| `protect()` 防路由环回 | 只下发 ZT 管理网段路由、不劫持默认路由,物理口天然不走虚拟网卡 |
| 三模式 DNS 手工管理 | macOS scoped resolver / Windows NRPT / systemd-resolved 按网络分流 |
| supervisorLoop 进程内自愈 | launchd/systemd/Windows 服务 KeepAlive |

桌面端真正要写的网络层只剩三件:**① 每网络设备创建**(tun-rs / wintun / utun);**② 配置应用器**——把核心 network config 下发的 managed IP / managed route / DNS 写进系统和设备;**③ L3 设备的 ARP/NDP 用户态垫层**(macOS utun 与 Windows wintun 都是 L3,官方 MacEthernetTap 同样在用户态处理 ARP,模式现成;Linux 可直接用 TAP 拿 L2 免垫层)。

收益:工作量比"移植数据面"砍一半以上;内核数据路径性能优于任何用户态转发;与官方客户端行为一致(用户已有的心智模型)。代价:桌面与移动端数据面**架构性分叉**——但两套各自被平台最优解驱动,比"用移动端的镣铐版架构跑桌面"更合理。

## 3. 专题二:多 Planet 同时开启

### 3.1 核心机制约束(决定一切)

- ZeroTier 核心的 `Topology` 持有**单个** `World _planet` 成员【已验证,源码 `node/Topology.hpp`】——一个 node 同一时刻只有一份 planet,"多 planet 共存于一个实例"在核心层不成立。
- 官方换 planet 的做法 = 用新 planet 文件覆盖数据目录 `planet` 后重启服务【已验证,docs.zerotier.com/roots】。本仓库 Android 端"planet 作为 dataStore hook 重定向输入"与官方移动端 1.16.0+ 的 Add Planet File 机制本质相同。
- **结论:多 planet 同时在线的唯一正解是多个 node 实例**,每实例独立 dataStore(独立 planet)+ 独立 UDP 端口。

### 3.2 多实例的两种落地形态

| 形态 | 做法 | 依据与风险 |
|---|---|---|
| A. 单进程多 Node(推荐首选) | daemon 内每 planet 一个 Node 实例(libzerotiercore 的 C API 是实例化的:`ZT_Node_new` 返回独立句柄、回调随实例走,无全局单例),各绑 9994、9995…端口;网络设备按 §2.3 每网络一张 | API 设计允许【已验证】;但**无官方书面承诺单进程多 Node**,且官方 libzt(SDK 层)明文"每进程一个 node"(其限制来自 SDK 层全局状态,我们不经 libzt)——**需 POC 实测验证**。收益:一套进程、共享 store/配置、实例间路由冲突检测容易做 |
| B. 每 planet 一个 daemon 进程 | 对齐官方 zerotier-one 多实例条款:不同工作目录 + 不同 primary 端口即可,官方 man page 明文支持 | 官方路线内做法【已验证】;代价:进程管理复杂、配置要跨进程聚合、CLI/UI 要发现多实例。Windows 服务包装器对自定义 home/port 支持差(官方 issue #1019),Windows 上更建议形态 A |

**数据面继承**:Android 端已经按 planet 隔离核心状态(`files/zt/<planetId>/`,identity 共享)。多实例化后 identity 策略可选:沿用"跨 planet 共享 identity"(同一设备身份,现 Android/iOS 语义)或每 planet 独立 identity(对齐官方多实例语义)。建议保持共享,与移动端行为一致、用户好理解。

**identity 与冲突边界**:ZeroTier 节点地址由 identity 密钥对派生,共享 identity 跨 planet **不构成冲突**——每个 planet 是独立的根体系,各自的根/控制器只见到本体系内的一个实例,互不知晓。真正的冲突场景是**同一根体系内出现重复身份**(如两台设备共用一份 identity,官方 issue #2433:Home Assistant 迁移后备份带上了旧 identity,新旧两机在同一网络内互斥打架)。桌面端须防两件事:① 同一 planet 不允许开两个实例(daemon 按 planetId 一实例构造性杜绝);② 跨设备配置迁移(QRTransfer/备份)只传 planet/网络配置,**绝不传 identity 文件**。

**设备粒度:与官方客户端一致**——官方 zerotier-one 桌面版就是**每 network 一张虚拟网卡**(Linux `zt<网络ID>` 接口、Windows 适配器、macOS utun,内核看到 N 张独立网卡)。桌面版沿用该模型(§2.3):多 planet = 多实例 × 每实例每网络一张网卡。移动端的"单 TUN 用户态复用"是 VpnService 单 fd 约束的产物,不带到桌面。

**新增工程问题**(Android 单 planet 时代不存在,桌面多 planet 必须解决):
1. **网段冲突跨实例检测**:两个 planet 下的网络若下发同一 managed route,系统路由表会打架。现有 `TunnelManager` 的冲突检测语义要升级为全局跨实例视角。
2. **内存**:单 node 空闲 RSS 量级几十 MB【社区经验,无官方数字】,桌面开 3~5 个 planet 可接受,但应在 POC 中实测自家 1.16.2 core 的基线。
3. **UDP 端口规划**:每实例错开端口;planet 根服务器侧不要求客户端用 9993(出口端口任意),无互通问题。

### 3.3 maxNetworks 等移动端限制的移除(桌面)

- `maxNetworks`(2..6 软上限,`Models.kt:101-102`)是移动端人为限制;ZeroTier **核心层 join 网络数无任何上限**(`Node::join()` 无数量检查,源码级验证;网传"16 个上限"查无出处)。桌面端直接移除该设置项,仅保留网段重叠校验。
- 同理可移除/降级:`connectOnDemand` 在桌面改为 daemon 常驻 + 开机自启(系统级,不再需要 app 层模拟);扫码(CameraX)在桌面保留"扫二维码图片导入"即可,摄像头扫码可选。

## 4. 专题三:CLI + UI 拆分架构

### 4.1 业界范式(两大家一致,直接照抄)

```
┌─────────────────────────────────────────────┐
│  planet-portd(特权 daemon,Rust)           │
│  ├─ ZT Node 实例 × N(每 planet 一个,FFI)│
│  ├─ 每网络设备 + 配置应用器(§2.3)        │
│  ├─ store 层(四份 JSON,语义平移)         │
│  └─ 本地 HTTP API 127.0.0.1:<port>          │
│     (authtoken.secret 鉴权,对齐官方语义)  │
└──────────┬───────────────────┬──────────────┘
           │ REST/JSON         │ REST/JSON
   ┌───────┴───────┐   ┌───────┴────────────┐
   │ planet-port   │   │ Planet Port GUI    │
   │ (CLI,薄客户端)│   │ (Tauri 2,纯交互   │
   │               │   │  层,零特权)      │
   └───────────────┘   └────────────────────┘
```

- ZeroTier 官方:zerotier-one(daemon,UDP 端口兼作 `127.0.0.1` loopback HTTP/JSON API,`authtoken.secret` 鉴权,端口写 `zerotier-one.port` 文件)+ zerotier-cli(同二进制薄 personality)+ DesktopUI(托盘 GUI,纯 API 客户端,250ms 轮询)。API 有 OpenAPI 规范仓库 `zerotier/zerotier-one-api-spec`【已验证】。
- Tailscale:tailscaled(SYSTEM/root)+ tailscale CLI(Unix socket/命名管道 LocalAPI)+ 各平台 GUI(纯 LocalAPI 客户端)【已验证】。
- 借鉴要点:① CLI 与 GUI 共用同一 API,GUI 不含任何特权/网络代码,GUI 框架可随时更换;② 提权收敛到安装期;③ 配置命令幂等(`planet-port set ...` 全量陈述状态),便于脚本化与调试。

### 4.2 与用户问题的对应

- "核心管理功能都在 CLI" → 成立:daemon + CLI 覆盖全部功能(planet 导入/切换/多开、网络 join/leave、moon、状态查询、路由/DNS 模式)。GUI 挂了/不开,隧道照跑。
- "UI 只是纯交互层" → 成立:GUI 只做 API 消费 + 展示,可整体替换而不动核心。

### 4.3 API 形态建议

对齐官方 zerotier-one API 风格(REST + JSON + authtoken 文件)而非发明新协议:
- `GET /status`、`GET /planets`、`POST /planets`、`POST /planets/{id}/activate`、`DELETE /planets/{id}`
- `GET /networks?planet=<id>`、`POST /networks/{nwid}/join`、`POST /.../leave`
- `GET /peers`、`GET /moons`、`POST /moons`、`GET /version`
- 事件推送:状态变更可用长轮询(官方 GUI 即轮询)或 SSE,避免引入 WebSocket 复杂度。
- 安全:仅监听 loopback + token 文件权限 0600(官方同款);Windows 用命名管道或 loopback + token。

### 4.4 GUI 选型

| 方案 | 结论 |
|---|---|
| **Tauri 2(定稿)** | 托盘、autostart、updater 均为官方内置/官方 plugin,对"控制面板 + 托盘"场景一次配齐;内存约为 Electron 的一半(社区基准 ~172MB vs ~409MB);包体 ~10-20MB。代价:引入 Web 前端栈,Linux 依赖 webkitgtk 分发 |
| 纯 Rust GUI(slint / iced) | 无 webview、资源最省(slint 面板类应用 <50MB 量级),但托盘/自启/更新需自行拼装。GUI 是薄客户端,未来可随时整体替换,不影响 daemon |

### 4.5 桌面 UI 适配要点(差异于移动端)

- 形态:主窗口 + 系统托盘常驻(启停/状态一眼可见);关闭窗口 = 隐藏到托盘,退出 = 停 daemon 或仅退 GUI(daemon 独立存活)
- 多 planet 并存的主界面表达:planet 列表从"单选切换"升级为"多实例卡片墙"(每 planet 一张卡:在线网络数、节点状态、独立启停开关)——这是桌面版相对 iOS/Android 的功能增量,`AppPrefs.activePlanetId` 单指针模型需扩展为激活集合
- 移除:maxNetworks 上限、connectOnDemand(app 层模拟)→ 改为"daemon 开机自启"系统设置
- 保留并增强:planet/moon 导入(文件 + URL + 二维码图片解析)、路由/DNS 三模式、peer 列表、主题系统(桌面无背景图视差,可降级)
- QRTransfer(设备间互传配置)在桌面价值下降但可保留:桌面可作为二维码展示端,让手机扫桌面

## 5. 路线收敛记录与风险

### 5.1 已排除路线(一句话级,评估过程见 git 历史)

| 路线 | 排除原因 |
|---|---|
| Kotlin/JVM(复用 Android 代码) | daemon + GUI 常驻内存达数百 MB 量级(社区锚点),对开机自启 + 托盘常驻型应用不可接受;桌面 JVM 加载 JNI core 无公开使用先例;Kotlin 生态无"去 JVM"顺路走法(Compose 桌面绑死 JVM) |
| 包装官方 zerotier-one 服务 | 丢掉内嵌核心控制权(数据面/DNS 模式/冲突检测);多 planet 要管多份官方服务实例(Windows 服务包装器对自定义 home/port 支持差);与 iOS/Android 功能不对齐,QRTransfer 等特色失效 |

### 5.2 风险清单(按优先级)

1. **单进程多 Node 无官方背书**:POC 实测两实例并发 + 互扰检查;失败退路 = 每 planet 一个 daemon 子进程(形态 B,官方多实例条款明文支持)。
2. **macOS 路由/DNS 语义坑**(官方与 Tailscale 都踩过,如 `-ifscope`、路由回环):"只下发管理网段路由、不劫持默认路由"的设计要 POC 先行验证三平台行为。
3. **L3 设备 ARP/NDP 垫层正确性**(mac utun / win wintun):参照官方 MacEthernetTap 的用户态 ARP 模式,需用现网设备实测。
4. **Windows 签名**:wintun DLL 已签名免驱动安装,但安装包与守护服务需代码签名证书,否则 SmartScreen 拦截。
5. **分发/更新工具链**:cargo-dist 维护主体前景社区存疑、cross 正式版停在 0.2.5(2023)——需备选 Conveyor / GitHub Actions 自建矩阵。
6. **无先例可抄**:Rust 调 libzerotiercore 无公开项目,FFI unsafe 层与平台细节全自验证。

### 5.3 POC 顺序(每步都可独立止步决策)

1. **FFI POC**:bindgen 生成 -sys,单 Node 起来、join 网络(仅控制面)
2. **设备 + 配置应用 POC**:tun-rs 每网络设备 + managed IP/route 应用,单网络 ping 通(含 mac ARP 垫层)
3. **多 Node POC**:同进程两 Node(不同 planet + 端口),验证互不干扰 + 双设备
4. daemon(tokio)+ REST API + CLI 骨架(状态/启停/切换命令跑通)
5. Tauri GUI 壳 + 托盘 + autostart
6. Windows(wintun + 服务)/ Linux(tun + systemd)平台适配 + 签名公证

## 6. Rust 实现要点

### 6.1 FFI:没有现成绑定,但没有障碍

crates.io 全部 19 个 zerotier 相关 crate 逐一核查(2026-10-28,crates.io API):**没有任何维护中的 libzerotiercore FFI 绑定**——现存的是 libzt 死绑定(0.1.2,2021)、官方 Central/One HTTP API 客户端、占位 crate【已验证】。但 libzerotiercore 暴露的是 `ZT_Node_new` 回调式纯 C API,Rust FFI 是一等公民:bindgen 生成 -sys crate + unsafe 封装层约 1-2 天起步,封装逻辑与 Android 端 JNI 封装层(NodeRuntime 的 hook 语义)同构——且**不再需要官方那 2,826 行 JNI 绑定**(本地实测行数)。"Rust 版 ZeroTier 协议客户端"无先例(唯一尝试 tetanus 已死),但我们不重写协议,走 FFI 内嵌,先例缺失不阻塞。

注意:官方 2026 年发布的 ZeroTier Quantum 是 Rust 从零构建的**独立产品**(后量子安全 VPN),不是 libzerotiercore 的 Rust 化,无可复用绑定【已验证】。

### 6.2 生态五件套(全部活跃,2026-10 核查)

| 需求 | 选型 | 现状 |
|---|---|---|
| TUN 设备 | `tun-rs` 2.8.15 | Linux TUN/TAP + offload、Windows wintun、macOS utun 全覆盖;tokio 异步官方支持;Apache-2.0;90 天 26.9 万下载 |
| 路由管理 | `route_manager`(tun-rs 同组织) | 跨平台路由增删,配置应用器直接用 |
| 托盘 | `tray-icon` 0.22 | tauri-apps 官方维护,90 天 441 万下载 |
| 开机自启 | `auto-launch` 0.6 | 90 天 220 万下载 |
| 自更新/分发 | `self_update` 0.44 / Tauri updater plugin;`cargo-dist` / Conveyor | 走 GitHub Releases;打包 + 签名公证三平台 |

### 6.3 先例

Rust mesh VPN daemon 有 EasyTier(万级 star,LGPL-3.0,持续活跃)、innernet、Firezone(生产规模,sans-IO 架构)、boringtun(Cloudflare)——"Rust 写网络 daemon"是成熟实践;缺的只是"Rust 调 ZT core"这一个组合【已验证】。

### 6.4 代价:诚实清单

1. **与移动端零代码共享**:store/导入/FFI 层全部 Rust 重写(约 1.2k 行等价逻辑 + FFI 封装),靠语义平移与黄金向量控风险。
2. **三语维护税**:此后每个 iOS 功能对齐都是三语移植(Swift→Kotlin→Rust);好在数据面不移植(§2.3),税主要落在 store 与网络配置层,涉数据面的功能在桌面天然走内核路径、不产生移植。
3. **资源量级**(锚点,非精确数字):daemon 空闲 RSS 数十 MB 量级(zerotier-one C++ 官方称典型 <16MB,Rust 同量级)【推测 + 官方锚点】;Tauri GUI ~172MB(社区基准)或换纯 Rust GUI <50MB。

## 7. 来源索引

**ZeroTier 官方**
- 服务架构/本地 API/多实例条款:github.com/zerotier/ZeroTierOne/blob/master/doc/zerotier-one.8.md 、doc/zerotier-cli.1.md
- API 规范:github.com/zerotier/zerotier-one-api-spec
- planet/world 与 air-gap:docs.zerotier.com/roots
- Topology 单 planet:github.com/zerotier/ZeroTierOne/blob/master/node/Topology.hpp
- macOS 实现:github.com/zerotier/ZeroTierOne/blob/master/osdep/MacEthernetTapAgent.h 、zerotier.atlassian.net/wiki/spaces/SD/pages/70254878/Macintosh
- Windows TAP:github.com/zerotier/ZeroTierOne/blob/master/ext/bin/tap-windows-ndis6/x64/zttap300.inf
- Node join 无上限:github.com/zerotier/ZeroTierOne/blob/master/node/Node.cpp 、docs.zerotier.com/start
- DesktopUI:github.com/zerotier/DesktopUI
- ZeroTier Quantum 官方 Rust 重写公告(独立产品,非 libzerotiercore):zerotier.com/blog/zerotier-rust-memory-safety

**TUN / 特权**
- wintun:wintun.net 、git.zx2c4.com/wintun/plain/api/wintun.h
- Linux tun 需 CAP_NET_ADMIN:lists.debian.org(debian-user 2013/08/msg00021)
- SMAppService/launchd、tailscale macOS 形态:tailscale.com/docs/concepts/macos-variants 、tailscale.com/blog/macos-binary-size
- tailscaled/LocalAPI 架构:github.com/tailscale/tailscale(issue #1843、#5761、#177)

**Rust 生态(§6)**
- crates.io zerotier 相关 19 crate 全量核查(无 libzerotiercore FFI 绑定):crates.io/api/v1/crates?q=zerotier
- tun-rs(平台矩阵/tokio/route_manager):github.com/tun-rs/tun-rs
- wintun-bindings:crates.io/crates/wintun-bindings
- tray-icon / auto-launch / self_update:crates.io(tauri-apps 等维护方)
- Tauri 2 托盘/autostart/updater:v2.tauri.app/learn/system-tray
- 先例:github.com/EasyTier/EasyTier 、tonarino/innernet 、firezone.dev/blog/sans-io 、boringtun
- 内存锚点:docs.zerotier.com/compatibility(zerotier-one <16MB)
- tetanus(已死 WIP):github.com/nickolastone/tetanus

**社区经验(多实例/内存)**
- ZeroTierOne issues #2433(两机共用同一 identity 在同网络内冲突——多 planet 共享 identity 不受影响,见 §3.2)、#1019(Windows 服务自定义 home)、#149(内存)
- OpenWrt 论坛/Reddit 内存量级讨论(仅作量级参考,无官方数字)
