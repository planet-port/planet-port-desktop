# planet-port CLI 规范

`planet-port` 是 planet-portd 守护进程的 CLI。命令命名对齐 `zerotier-cli`
(info / listpeers / listnetworks / join / leave / set / dump / orbit / deorbit),
多 planet 管理命令(add / remove / start / stop 等)为顶层命令,不与 zerotier-cli
现有命令冲突。

## 全局选项

| 选项 | 说明 |
|---|---|
| `--planet <name\|id>` | 定位 planet:变更类命令的目标选择器,查询类命令的过滤器;省略时取环境变量 `PLANET_PORT_PLANET`,再省略走交互选择(见下)或由 daemon 按「单 running 自动选择 / 多 running 报歧义」处理 |
| `-j, --json` | JSON 输出(stdout 仅 JSON,人话提示走 stderr) |
| `--api-url <URL>` | 覆写 daemon API 地址;省略时读配置目录 port 文件,再回退 `127.0.0.1:9998`。也可用环境变量 `PLANET_PORT_URL` |

## API 地址与鉴权协商

- 鉴权走 `X-ZT1-Auth` 头:优先取 `PLANET_PORT_TOKEN` 环境变量,未设置时读取
  token 文件(`<config_dir>/planet-portd/authtoken.secret`)。
- 未设置 URL 时 CLI 先读 port 文件(`<config_dir>/planet-portd/port`,planet-portd
  启动写入):存在且内容为 1-65535 的合法端口 → `http://127.0.0.1:<该端口>`;
  读不到或非法 → 默认 `http://127.0.0.1:9998`。`--api-url` / `PLANET_PORT_URL`
  显式指定时完全跳过端口文件。

## 环境变量

| 变量 | 作用 | 优先级 |
|---|---|---|
| `PLANET_PORT_URL` | 覆盖 daemon API 地址 | 高于端口文件协商,低于 `--api-url` 显式传参 |
| `PLANET_PORT_TOKEN` | 鉴权 token(`X-ZT1-Auth` 头) | 高于 token 文件(`<config_dir>/planet-portd/authtoken.secret`) |
| `PLANET_PORT_CONFIG_DIR` | 配置目录 | daemon 与 CLI 共用同一解析规则,测试/多实例隔离用 |
| `PLANET_PORT_PLANET` | 默认 planet(定位链②) | 低于 `--planet` 显式传参,高于 daemon 侧自动定位(③④) |

## 多语言

CLI 文案支持简体中文(zh-Hans,源语言)/ 繁体中文(zh-Hant)/ 英语(en),**跟随系统语言自动选择,无需配置**:按 `LC_ALL` > `LC_MESSAGES` > `LANG` > 系统语言探测,均无法判定时回退简体中文。表格表头与协议级字段(如 `ONLINE` / `status`)保持英文,对齐 zerotier-cli 惯例;daemon 事件消息属 API 数据,暂不本地化。

新增语言:在 `crates/cli/locales/` 下新建 `<locale>.yml`(键结构参照现有文件)并补充 `main.rs` 中 `map_locale` 的映射即可,无需改动其它代码。

## planet 定位链

变更类命令(join / leave / set / orbit / deorbit / dump / moon import)必须落到
**恰好一个** planet 实例,按以下顺序解析:

| 优先级 | 来源 | 说明 |
|---|---|---|
| ① | `--planet <name\|id>` | 显式指定,优先级最高 |
| ② | 环境变量 `PLANET_PORT_PLANET` | 由 `eval $(planet-port use <x>)` 设置 |
| ③ | 唯一**登记**行星自动选中 | 计数只看登记,不看连接状态;登记 1 个(即使未启用)自动落到它,未启用的报 `409 planetNotRunning` |
| ④ | 登记 ≥2 个行星 | 即使只有 1 个在运行也报 `409` 列出全部候选(标注 running/stopped),退出码 2,不猜 |

查询类命令(info / list / listnetworks / listpeers / listmoons)**默认聚合展示**,
`--planet` 与环境变量在其上的语义是**过滤**,不受上述链约束。
**events 例外**:必须落到具体 planet,定位与 dump 同款——显式 `--planet` >
仅登记 1 个行星时自动;登记 ≥2 未指定时报用法错误(退出码 2)。
显式指定已停止的 planet 仍可查其历史事件。

`--planet` 同时接受别名(name)与 worldId,**别名不区分大小写**;服务端解析,命中即返回 canonical id
(表格 PLANET 列显示的是 canonical id,如 company 显示 `2187945993`)。
网络定位同款:`leave` / `set` / `get` / `network show` 的 `<nwid>` 参数也接受
已加入网络的**名字**(不区分大小写;含空格/中文的名字直接引号包裹即可),同名多条报
409 列候选;`join` 只认 nwid——未加入的网络名字无从得知。

## 状态查询

| 命令 | 说明 |
|---|---|
| `info` | 所有 planet 聚合状态表(PLANET/STATUS/ADDRESS/VERSION/LATENCY) |
| `list [--planet]` | 按**全部登记行星**分组列出网络(不只在线的),行星头标注三态 `[已连接/连接中/未连接]`,无网络行星显示 `(无网络)`;指定 planet 时只列该行星;多 managed IP 在 MANAGED-IPS 列内逐行堆叠(全量展示,不截断) |
| `listnetworks [--planet]` | 跨 planet 网络列表(含 PLANET 列);`--planet` 过滤;多 managed IP 堆叠展示同 `list` |
| `listpeers [--planet]` | peers 按 planet 分组展示 |
| `listmoons [--planet]` | moons 聚合表;`--planet` 过滤 |
| `dump [--planet]` | **单一 planet** 状态 dump(networks/peers/events),不聚合;歧义时报候选列表 |
| `events [--planet] [--limit N] [--follow]` | 事件日志表,定位与 dump 同款(**必须指定 planet**,单 running 自动;显式指定已停止 planet 可查历史);`--limit` 默认 40;`--follow` 每 2s 轮询按 ts 游标增量输出,Ctrl-C 退出;daemon 断开/不可达时命令报错退出(exit 3),不会静默重连,长时挂着需自行拉起与保活 |

## 网络操作(按定位链落到单一 planet)

| 命令 | 说明 |
|---|---|
| `join <nwid>` | 加入网络;**幂等**:对已加入网络重复 join 静默成功(`{"result":"ok","idempotent":true}`),批量脚本可重入 |
| `leave <nwid>` | 退出网络;**幂等**:未加入时重复 leave 静默成功;**同时清除本地配置记录**(net_local),重启/重新上线不会被 re-join 复活 |
| `set <nwid> <k>=<v>...` | 客户端本地配置:`routeViaZeroTier=0\|1`(非 0/1 退出码 2)、`dnsMode=none\|network\|custom`、`customDns=<逗号分隔>`;未知 key 或不带任何 k=v 退出码 2;zerotier-cli 旧 key(`allowDefault`/`allowManaged`/`allowDNS`)报错时提示等价 key 映射 |
| `get <nwid> <key>` | 读单个网络配置值(脚本友好):key 取 `routeViaZeroTier`/`dnsMode`/`customDns`/`assignedAddrs`/`managedRoutes` 等字段;字符串裸输出,数组/对象紧凑 JSON;未知 key 退出码 2(同样带旧 key 映射提示) |
| `network show <nwid>` | 单网络详情视图:planet 落点、状态、MAC、设备名、mtu、assigned IP、managed routes、DNS 三件套、routeViaZeroTier 一屏可见;`-j` 输出完整 NetworkEntry |

## moon 操作(按定位链落到单一 planet)

| 命令 | 说明 |
|---|---|
| `orbit <moonid> <seed>` | orbit 一个 moon |
| `deorbit <moonid>` | deorbit |
| `moon import <file>` | 导入本地 `.moon` 文件(成功 201);当前仅支持本地文件路径 |

## planet 生命周期(顶层命令)

| 命令 | 说明 |
|---|---|
| `add <file\|URL> [--name <别名>] [--data-dir <绝对路径>] [--port <UDP端口>]` | 导入 planet 文件并立即上线;支持本地路径与 `http(s)://` URL(URL 下载后走同一套校验,超时 30s、响应体上限 8MB);容器导入返回 `501 notImplemented`;`--data-dir`/`--port` 为开发验证覆写(如复用既有 dataDir),日常省略 |
| `start <name\|id>` | 上线实例;按登记的本地网络配置自动 re-join |
| `stop <name\|id>` | 下线实例(feth 设备与路由清理),保留登记 |
| `remove <name\|id> [--yes]` | 删除登记;**自动批量 leave 全部已加入网络并停止实例**后再删除(单个 leave 失败记录事件不阻断);交互确认提示走 stderr(stdout 纯净),**确认拒绝退出码 130**(与选择器取消一致,脚本不会误判已删),`--yes` 供脚本跳过;`-j` 下必须配 `--yes`,否则退出码 2 |

## planet 管理

| 命令 | 说明 |
|---|---|
| `planet list` | 登记清单(ICON emoji / ID / STATUS / NETWORKS / PORT) |
| `planet show <name\|id>` | 详情(worldId、类型、roots、来源、大小、修订时间、端口等);修订时间文本模式为本地时间,`-j` 保留 epoch |
| `planet rename <name\|id> <新名>` | 重命名别名 |
| `planet export <name\|id> [-o <file>]` | 导出 planet 二进制;缺省 `-o` 写 stdout |
| `planet share <name\|id> [--ttl <秒>]` | 生成临时下载链接并打印 URL 与过期时间(默认 300s);URL 按 CLI 实际访问的 base 改写(经 `PLANET_PORT_URL` 指到远端时对他人可达),回环地址时 stderr 提示换局域网 IP;过期时间文本模式为本地时间,`-j` 保留 epoch |

## shell 上下文

| 命令 | 说明 |
|---|---|
| `use <name\|id>` | stdout 输出 `export PLANET_PORT_PLANET=<id>` 供 `eval`;人话提示走 stderr;planet 不存在时退出码 2 且 stdout 无输出;`-j` 输出 `{"planet":"<id>","name":"<name>"}` |

用法:`eval $(planet-port use company)`。CLI 是一次性子进程,无法修改父 shell 环境,
故必须 eval;状态存于环境变量,可见、可覆盖、脚本可自省。

## 安装

| 命令 | 说明 |
|---|---|
| `install` | 需 root:一键自安装(在线/离线共用)。把 CLI 同目录的二进制套装(planet-port + planet-portd)拷到 `/Library/Application Support/planet-portd/bin/`,建 `/usr/local/bin/planet-port` 软链,并完成 `service install`(plist + bootstrap + 拉起);升级场景自动记录 running planet 并在装完后拉回(旧版 daemon 升级时 SIGTERM 落 stopped 的兜底,新版幂等);`-j` 回执同 service install |

在线安装(`curl … install.sh | sudo bash`)与离线安装(解压发布包后 `sudo ./planet-port install`)最终都走本命令,详见 [docs/install.md](docs/install.md) 。

## service 服务管理

| 命令 | 说明 |
|---|---|
| `service install` | 需 root:拷贝二进制到 `/Library/Application Support/planet-portd/bin/planet-portd`,写 `/Library/LaunchDaemons/cn.imgozi.d.planetport.daemon.plist` 并 `launchctl bootstrap system/`;RunAtLoad=true(装载即随开机启动)、KeepAlive={SuccessfulExit=false}(崩溃等异常退出自动拉起,SIGTERM 优雅退出不拉起);幂等,重装先卸载旧服务;`-j` 时进度人话走 stderr、stdout 仅 `{"result":"ok"}`(已装载重入附 `"idempotent":true`) |
| `service uninstall` | 需 root:`launchctl bootout` + 删 plist、daemon/CLI 二进制与 PATH 软链(软链仅当指向本产品时删);**数据目录保留**,提示手动 `rm -rf`;`-j` 回执同 install |
| `service start` | 需 root:`launchctl kickstart -k …`;daemon 启动时按 registry 期望状态自动恢复 planet;`-j` 成功回执 `{"result":"ok","port":N}`,超时 `{"result":"error"}` |
| `service stop` | 需 root:`launchctl kill TERM …`,命中 daemon 的信号优雅退出路径(逐 planet 红线收尾;**保留期望状态**,start 后自动恢复;显式 `planet-port stop` 才不恢复);`-j` 回执 ok / `{"result":"error"}`(超时)/ `{"result":"warning"}`(KeepAlive 拉起) |
| `service logs [--lines N] [--follow]` | 无需 root:tail daemon 日志(`/Library/Logs/planet-portd/daemon.log`,默认末 50 行);`--follow` 持续跟踪,Ctrl+C 退出;日志文件不存在时退出码 1 |
| `service status` | 无需 root:尊重 `PLANET_PORT_URL`;ping `/version` 成功 → `运行中(端口 N)`(N 取自 port 文件,**退出码 0**);token 校验失败 → 退出码 1;不在线时按 plist 是否存在区分「未安装」/「已安装未运行」并给出 launchctl 状态摘要,**退出码 3** |

## 行为说明

- 单操作成功输出为简式并带 planet 落点,如 `gozi25(3530253632) 200 join OK`(定位链②③生效时用户看不到目标,回执自报家门;落点拿不到时退化为 `200 join OK`);`moon import` 为 `201 moon import OK`。
- 变更类 `-j` 输出真实响应体:join/leave/orbit 为 `{"result":"ok"}`(幂等重入时附 `"idempotent":true`),remove/deorbit(204 空体)代以 `{"result":"ok"}`,不再是 `null`。
- 文本模式时间戳本地化(`events` TS、`planet show` 修订时间、`share` 过期时间);`-j` 一律保留原始 epoch(游标/脚本语义不变)。
- `dump -j` 为结构化对象(`planet`/`networks`/`peers`/`events` 各取自对应 JSON 端点),不再是被 quote 一层的 JSON 字符串;`list -j` 为分组结构 `planets[]` 各带 `networks[]`(平铺数组是 `listnetworks -j` 的职责)。
- 定位类 409(`planetAmbiguous`/`noPlanetRunning`)在人话层改写为 `--planet` 指引 + 候选列表(跟随 CLI 语言);其余 API 错误原样透传 `error <code>: <message>`。
- 常规请求全局 10s 超时,超时以错误退出(不误报 daemon 未在线);`events --follow`
  的轮询请求无超时,长挂不受影响。
- `set` 的 `routeViaZeroTier` 在下一次网络配置更新(控制器下发 netconf)时生效:
  false 时清空 managed routes 后再 apply,已装上的路由不会立即回撤。
- `set` 的校验:`routeViaZeroTier` 仅接受 `0|1`(其它值返回 400,退出码 2),
  `dnsMode` 仅接受 `none|network|custom`。
- `dnsMode`/`customDns` 算出的 servers 非空时(仅真实 daemon)经 `networksetup`
  写入主网络服务;servers 为空(不再托管)或 leave 网络 / 停止或删除 planet 时
  还原为自动获取;失败只记事件,不影响网络数据面。

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 运行错误(API 4xx/5xx、IO 错误) |
| 2 | 用法错误(非法参数、未知 set key)、planet 歧义(409 + 候选列表)、service 组需 root(install/uninstall/start/stop 未以 root 运行) |
| 3 | daemon 不在线,提示 `planet-portd is not running (try: planet-port service start)` |
| 130 | 用户中止:`remove` 确认拒绝 |

---

详细行为背景见 [README.md](README.md) ,手工回归清单见 [docs/2026-10-29-cli-regression-checklist.md](docs/2026-10-29-cli-regression-checklist.md) 。
