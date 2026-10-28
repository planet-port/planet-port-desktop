# planet-port CLI 与本地 REST API 设计

> 日期:2026-10-28
> 前置文档: docs/2026-10-28-desktop-port-feasibility.md (架构定稿:planet-portd + planet-port CLI + Tauri GUI)
> 本文档范围:**CLI 命令面 + 本地 REST API 契约**。GUI(Tauri)交互设计另出文档。
> 已明确排除:TUI(ratatui 方案取消,本次不做)。
> 修订:2026-10-28 并入 iOS 功能交叉评估结果(planet 元数据/重命名/分享通道、客户端侧网络设置模型、events 端点),见 §7。

## 0. 设计决策摘要(已与用户逐项确认)

| 决策点 | 结论 |
|---|---|
| 多 planet 交互模型 | **无状态聚合**:查询类默认聚合展示,`--planet` 过滤/定位;无隐藏配置态 |
| 二进制形态 | `planet-portd`(特权 daemon)+ `planet-port`(CLI)两个二进制,对齐 Tailscale 模式;不采用官方 argv[0] 多人格 |
| 命令命名 | zerotier-cli 已有命令**原名原参数保留**;新增 `planet`/`use`/`service` 子命令 |
| 变更类定位 | 优先级链(见 §2),单 planet 自动、多 planet 报错列候选,绝不猜 |
| shell 上下文 | `use` 命令输出 export 行配合 eval,本质是 `PLANET_PORT_PLANET` 环境变量 |
| planet import 语义 | import 即上线(登记 + 创建 Node 实例 + 立即运行);`stop` 降级为仅登记 |
| dump 语义 | **不聚合**:按定位链落到单一 planet,一次只 dump 一个实例 |
| 网络设置模型 | **仅客户端本地配置**:routeViaZeroTier、dnsMode(none/network/custom)、customDns;不提供控制器侧授权项(见 §7) |
| planet 元数据 | icon(animal key,三端 planets.json 互认)+ color(#RRGGBB);CLI 渲染 emoji,GUI 渲染自有图标,同一字段两种呈现并存 |
| 全局偏好端点 | 不做,disableIPv6 等跟随系统默认,后期有需要再加 |
| 事件日志 | 提供 `/events` 端点与 `events` 命令(冲突记录、状态变更、identity collision) |

## 1. 架构位置

```
planet-portd(特权 daemon,单进程多 Node:每 planet 一个实例)
    │ REST/JSON @ 127.0.0.1:<port>,authtoken 文件鉴权(官方 zerotier-one 同款)
    ├── planet-port(CLI,薄客户端,零业务逻辑)
    └── Planet Port GUI(Tauri,纯交互层)——复用同一 API
```

原则:**CLI 不含任何业务规则**。planet 定位、聚合、冲突校验全部在 daemon 端实现,CLI/GUI 只是同一语义的两个视图。这样两个客户端不会出现行为分叉。

## 2. planet 定位优先级链(核心语义)

变更类命令(join/leave/set/orbit/deorbit/dump)必须落到**恰好一个** planet 实例。解析顺序:

```
① --planet <name|id> 显式指定
② 环境变量 PLANET_PORT_PLANET(由 eval $(planet-port use <x>) 设置)
③ 恰好只有一个激活(running)planet → 自动选它
④ 多个激活 planet → 报错并列出候选,退出码 2,不猜
```

- 查询类命令(listnetworks/listpeers/info/listmoons)不受此链约束:**永远聚合展示**,`--planet`/环境变量在查询类上的语义是**过滤**(全部/单个)。
- `--planet` 同时接受 planet 的用户别名(name)与 worldId,daemon 端解析,歧义(多个同名)时报错。
- daemon 侧 REST API 的 `?planet=` 参数执行同一套逻辑:省略时若单激活 planet 自动选,多个则 `409 Conflict` + 候选列表。

## 3. CLI 命令面

### 3.1 对齐 zerotier-cli(原名原参数)

| 命令 | 多 planet 行为 |
|---|---|
| `info` | 聚合:每 planet 一行(PLANET/STATUS/ADDRESS/VERSION/LATENCY) |
| `listnetworks` | 聚合,输出加 PLANET 列 |
| `listpeers` | 聚合,按 planet 分组 |
| `join <nwid>` | 变更类,按定位链 |
| `leave <nwid>` | 变更类,按定位链 |
| `set <nwid> <k>=<v>...` | 变更类,按定位链;key 仅客户端本地配置:`routeViaZeroTier=0\|1`、`dnsMode=none\|network\|custom`、`customDns=<v4,v4,v6,v6>`(逗号分隔,空槽省略) |
| `listmoons` | 聚合,可过滤 |
| `orbit <moonid> <seed>` | 变更类,按定位链 |
| `deorbit <moonid>` | 变更类,按定位链 |
| `dump` | **单一 planet**(定位链),不聚合;多 planet 未指定时报错列候选 |

全局 flag:`-j/--json`(全命令)、`--planet <name|id>`、`--api-url <url>`(调试用,覆盖默认端点)。

### 3.2 新增子命令组

**planet 组**(zerotier-cli 没有,多 planet 管理核心):

```
planet-port planet list                    # 登记清单 + 运行状态 + 网络数 + UDP 端口(图标列显示 emoji)
planet-port planet import <file|URL> [--name <别名>]
                                           # CLI 仅支持文件与 URL 两种来源
                                           # 校验(0x01 头 + worldId 解析,语义平移 Android PlanetImporter)
                                           # → 登记 → 创建 Node 实例 → 立即上线
planet-port planet show <name|id>          # 详情:World ID、类型(official/custom)、roots、文件大小、
                                           #   修订时间、lastNodeId、来源、网络数、端口、状态
planet-port planet rename <name|id> <新名>
planet-port planet start <name|id>         # stopped → 上线
planet-port planet stop <name|id>          # 下线,保留登记;该 planet 下网络全部离线
planet-port planet rm <name|id> [--yes]    # 删除登记;有活跃网络时拒绝(先 leave);交互确认,--yes 供脚本跳过
planet-port planet export <name|id> [-o <file>]
                                           # 导出 planet 二进制;缺省 -o 时写 stdout(供重定向)
planet-port planet share <name|id> [--ttl <秒>]
                                           # 开局域网临时下载链接(一次性 token,默认 300s),打印 URL
```

**planet 元数据**:每个 planet 携带 `icon`(animal key,与移动端 planets.json 字段互认)+ `color`(#RRGGBB)。CLI 在 list/show 中将 icon 经映射表渲染为 **emoji**(无映射回退 🪐);GUI 渲染自有图标体系——同一字段、两种呈现并存,元数据由 daemon 统一存储。

**moon 导入**(Android MoonImporter 对应物):

```
planet-port moon import <file|URL>         # moon 文件登记,关联当前定位链 planet
```

**shell 上下文**:

```
planet-port use <name|id>
# 输出(供 eval): export PLANET_PORT_PLANET=<id>
# stderr 打印人话提示;校验 planet 存在,不存在则报错不输出
# 用法: eval $(planet-port use company)
```

约束说明:CLI 是一次性子进程,无法修改父 shell 环境,故必须 eval。状态存于 shell 环境变量,可见、可覆盖、脚本可自省(`echo $PLANET_PORT_PLANET`),不引入隐藏配置态。

**service 组**(daemon 生命周期,提权收敛点):

```
planet-port service install                # 注册系统服务(launchd/systemd/Windows Service),需提权,一次性
planet-port service uninstall
planet-port service start / stop           # 操作系统服务
planet-port service status
```

**事件查询**:

```
planet-port events [--planet <name|id>] [--limit N] [--follow]
                                           # 事件日志:状态变更、冲突(address/subnet/timeout)、
                                           #   identity collision、配置应用决策;--limit 默认 40,--follow 持续输出
```

### 3.3 输出与退出码

- 人类可读输出对齐 zerotier-cli 风格:单操作 `200 join OK` 简式;列表类表格(聚合时带 PLANET 列)
- `-j`:聚合类返回嵌套结构 `{"planets": [...]}`;单 planet 上下文的输出结构对齐官方 zerotier-one API 字段
- 退出码:`0` 成功 / `1` 运行错误 / `2` 用法错误与 planet 歧义 / `3` daemon 不在线
- daemon 不在线时提示:`planet-portd is not running (try: planet-port service start)`

## 4. REST API 契约

对齐官方 zerotier-one API 风格(REST + JSON,监听 127.0.0.1,authtoken.secret 鉴权,token 文件 0600;Windows 走 loopback + token)。所有变更端点的 `?planet=` 遵循 §2 定位链。

```
GET    /version
GET    /status                          # 聚合:各 planet 实例状态数组(含 FATAL/冲突状态)

GET    /planets                         # 登记清单(含运行状态、icon、color)
POST   /planets                         # 导入,body 二选一:
                                        #   {source: <path|url>, name?, icon?, color?}
                                        #   {container: <ZTP 容器 base64>}   # 二维码导入:
                                        #     GUI 采集帧重组后提交,daemon 解析容器
                                        #     (planet 二进制 + 元数据),登记 + 立即上线
PATCH  /planets/{id}                    # {name?, icon?, color?} 重命名/改图标/改底色
GET    /planets/{id}                    # 详情:worldId、type、roots、fileSize、revisionTime、
                                        #   lastNodeId、source、networkCount、port、status
POST   /planets/{id}/start
POST   /planets/{id}/stop
DELETE /planets/{id}                    # 有活跃网络时 409
GET    /planets/{id}/export             # planet 二进制(文件下载 / GUI 二维码展示的数据源)
POST   /planets/{id}/share              # {ttlSeconds?} → {url, expiresAt} 局域网临时下载链接
                                        #   一次性 token,默认 300s,仅服务该 planet 二进制

GET    /networks?planet=<id>            # 省略 planet = 聚合
                                        # 返回体:状态机码、冲突码(address/subnet/timeout)、
                                        #   assignedAddrs、managedRoutes(含 via)、mac、mtu、type、
                                        #   name、下发 DNS 服务器、routeViaZeroTier、dnsMode、
                                        #   customDns、routeViaLocked(同行星多网络启用时锁定)
POST   /networks/{nwid}/join?planet=<id>
POST   /networks/{nwid}/leave?planet=<id>
POST   /networks/{nwid}                 # set,body 仅客户端本地配置:
                                        #   {routeViaZeroTier?, dnsMode?, customDns?}

GET    /peers?planet=<id>
GET    /moons?planet=<id>
POST   /moons?planet=<id>               # moon 导入 {source},planet 遵循定位链
POST   /moons/{moonid}/orbit?planet=<id>   # body: {seed}
DELETE /moons/{moonid}?planet=<id>         # deorbit(真实调用核心,非仅删本地记录)

GET    /events?planet=<id>&limit=       # 事件日志:状态变更、冲突、identity collision、
                                        #   配置应用决策;默认 limit 40
GET    /dump?planet=<id>                # 单一 planet,定位链语义
```

错误体统一格式:`{"error": "<code>", "message": "...", "candidates": [...]}`(candidates 仅 planet 歧义时出现)。

## 5. 技术选型

| 项 | 选择 | 理由 |
|---|---|---|
| CLI 框架 | `clap` 4 (derive) | Rust 事实标准,MIT/Apache,子命令组/flag 模型匹配 |
| HTTP 客户端 | `ureq`(同步) | 一次性进程不值得 tokio+reqwest;启动快依赖小 |
| 表格输出 | `comfy-table` | 轻量,MIT/Apache;聚合表数量少 |

daemon 侧 API 实现(tokio 路由、authtoken、事件轮询/SSE)属于 daemon 设计范畴,本文档只定契约。

## 6. 明确不做

- TUI(ratatui)——本轮取消,CLI + GUI 已覆盖
- `/prefs` 全局偏好端点——disableIPv6 等先跟随系统默认,后期有需要再加
- 控制器侧授权项管理(allowManaged/allowEthernetBridging 等)——本产品是客户端,只管理客户端本地配置(与 iOS 端模型一致)
- `zerotier-idtool` 的密钥工具功能(generate/validate)——非 zerotier-cli 职责,需要时另行评估
- identity 管理/迁移命令——identity 由 daemon 持有共享语义(跨 planet 共享,见可行性报告 §3.2),不暴露给 CLI
- 远程管理 / ZeroTier Central 集成——纯本地

## 7. iOS 功能交叉评估对齐(2026-10-28 修订依据)

对 planet-port-ios 全量功能面盘点后逐项对照 API,结论:除以下通道矩阵与有意差异外,iOS 全部用户可见功能均可由本 API 满足;语言/主题/背景图等纯 GUI 本地设置、蜂窝权限检测(iOS 专属)、旧版迁移(fresh 平台)不需要 API。

### 7.1 planet 跨端传输通道矩阵

| 通道 | CLI | GUI | 说明 |
|---|---|---|---|
| 本地文件导入 | ✓ | ✓ | `planet import <file>` / POST source=path |
| URL 导入 | ✓ | ✓ | `planet import <url>` / POST source=url |
| 动态二维码扫入 | ✗ | 仅当有摄像头 | GUI 调系统摄像头,QRTransfer 帧协议(语义平移)重组 → ZTP 容器 → POST container;无摄像头则无此功能 |
| 文件导出 | ✓ | ✓ | `planet export` / GET export |
| 局域网临时链接 | ✓(打印 URL) | ✓ | POST share → 一次性 token + TTL,供手机/他机浏览器或客户端直接下载 |
| 动态二维码展示 | ✗ | ✓ | GUI 由 export 二进制编码 ZTP 容器(planet + 名称 + 图标 + 底色)生成动态码,手机端 PlanetPort 扫入 |

### 7.2 与 iOS 的有意差异

| 差异 | 方向 | 说明 |
|---|---|---|
| moon deorbit | 桌面反超 | 真实调用核心 deorbit;iOS 仅删本地记录 |
| 网段冲突检测 | 桌面反超 | 升级为全局跨实例视角(可行性报告 §3.2) |
| maxNetworks(2-6 软上限) | 桌面移除 | 核心层无上限,仅保留冲突校验 |
| connectOnDemand | 桌面替换 | daemon 常驻 + 开机自启(service install),无 app 层按需模拟 |
| allowGlobal/allowDefault/allowDNS | 桌面不提供 | iOS 客户端同样没有;客户端本地只有 routeViaZeroTier + dnsMode 两个覆盖维度 |
| 全局 prefs | 桌面暂缓 | 跟随系统默认,后期再议 |
