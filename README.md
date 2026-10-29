# planet-port-desktop

ZeroTier 多 Planet 管理桌面端(Rust)。架构与可行性结论见
docs/2026-10-28-desktop-port-feasibility.md,CLI/API 设计见
docs/superpowers/specs/2026-10-28-cli-api-design.md。

## 组件

| crate | 说明 |
|---|---|
| planet-port-api | REST 契约类型与端点构造(CLI 与 daemon 共用) |
| mockd | 契约 mock daemon(联调/契约验证,后续 GUI 开发复用) |
| planet-portd | 真实 daemon(内嵌 zt-core,管理多 planet 实例) |
| planet-port | CLI 薄客户端 |

## 开发联调

```bash
cargo run -p mockd &
export PLANET_PORT_URL=http://127.0.0.1:9998 PLANET_PORT_TOKEN=dev
cargo run -p planet-port -- info
```

- 鉴权走 `X-ZT1-Auth` 头:优先取 `PLANET_PORT_TOKEN` 环境变量,未设置时读取
  token 文件(`<config_dir>/planet-portd/authtoken.secret`)。mockd 接受任意非空 token,
  联调用 `PLANET_PORT_TOKEN=dev` 即可。
- 未设置 URL 时 CLI 先读 port 文件(`<config_dir>/planet-portd/port`,planet-portd
  启动写入):存在且内容为 1-65535 的合法端口 → `http://127.0.0.1:<该端口>`;
  读不到或非法 → 默认 `http://127.0.0.1:9998`。`--api-url` / `PLANET_PORT_URL`
  显式指定时完全跳过端口文件。注意:mockd **不写** port 文件,若机器上残留
  planet-portd 的 port 文件,联调 mockd 需显式设 URL(或删掉该文件)。

### 环境变量

| 变量 | 作用 | 优先级 |
|---|---|---|
| `PLANET_PORT_URL` | 覆盖 daemon API 地址 | 高于端口文件协商,低于 `--api-url` 显式传参 |
| `PLANET_PORT_TOKEN` | 鉴权 token(`X-ZT1-Auth` 头) | 高于 token 文件(`<config_dir>/planet-portd/authtoken.secret`) |
| `PLANET_PORT_CONFIG_DIR` | planet-portd 的配置目录 | daemon 与 CLI 共用同一解析规则,测试/多实例隔离用 |
| `PLANET_PORT_PLANET` | 默认 planet(定位链②) | 低于 `--planet` 显式传参,高于 daemon 侧自动定位(③④) |

CLI 是一次性子进程,`use` 命令即通过输出 `export PLANET_PORT_PLANET=<id>` 供 eval 写入该变量。

### 请求超时

常规请求全局 10s 超时,超时以错误退出(不误报 daemon 未在线);`events --follow`
的轮询请求无超时,长挂不受影响。

## planet 定位链

变更类命令(join / leave / set / orbit / deorbit / dump / moon import)必须落到**恰好一个**
planet 实例,按以下顺序解析:

| 优先级 | 来源 | 说明 |
|---|---|---|
| ① | `--planet <name\|id>` | 显式指定,优先级最高 |
| ② | 环境变量 `PLANET_PORT_PLANET` | 由 `eval $(planet-port use <x>)` 设置 |
| ③ | 唯一 running planet 自动选中 | 仅有一个激活 planet 时自动落到它 |
| ④ | 多个 running planet | **交互式 TTY**:弹 planet 选择列表(见下);非交互(管道/脚本/`-j`):报 `409` 并列出候选,退出码 2,不猜 |

查询类命令(info / list / listnetworks / listpeers / listmoons)**默认聚合展示**,
`--planet` 与环境变量在其上的语义是**过滤**,不受上述链约束。
**events 例外**:必须落到具体 planet,定位与 dump 同款——显式 `--planet` >
仅一个 running planet 时自动;多 running 未指定时交互式终端弹单选列表,
非交互报用法错误(退出码 2)。显式指定已停止的 planet 仍可查其历史事件。

### 交互式 planet 选择

交互式终端(stdin 为 TTY)且未显式指定 planet 时,多 planet 场景弹选择列表
代替 ④ 的 409 报错:

- **变更类命令**(join / leave / set / orbit / deorbit / dump / moon import)单选:
  候选为全部 running planet,↑↓ 移动,回车确认;
  **events** 同为单选,候选与 dump 一致(仅 running planet);
- **可批量查询命令**(info / list / listnetworks / listpeers / listmoons)多选:
  候选含「<全部>」首项,空格勾选,回车确认;勾选「全部」或空选 = 聚合全部,
  勾选子集则按选中集合过滤(多成员时逐 planet 请求合并);
- Esc / Ctrl+C 取消,**退出码 130**;
- 非 TTY(管道/脚本)或 `-j` 模式不弹列表,保持原聚合 / 409 行为。

`--planet` 同时接受别名(name)与 worldId;服务端解析,命中即返回 canonical id
(表格 PLANET 列显示的是 canonical id,如 company 显示 `2187945993`)。

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 运行错误(API 4xx/5xx、IO 错误) |
| 2 | 用法错误(非法参数、未知 set key)、planet 歧义(409 + 候选列表,仅非交互路径)、service 组需 root(install/uninstall/start/stop 未以 root 运行) |
| 3 | daemon 不在线,提示 `planet-portd is not running (try: planet-port service start)` |
| 130 | 交互式 planet 选择被取消(Esc / Ctrl+C) |

## 命令参考

全局 flag:`-j/--json`(全命令)、`--planet <name|id>`(global)、`--api-url <url>`(global)。

### 聚合查询

| 命令 | 说明 |
|---|---|
| `info` | 所有 planet 聚合状态表(PLANET/STATUS/ADDRESS/VERSION/LATENCY) |
| `list` | 按行星分组列出网络(listpeers 同款分组样式);`--planet` 只列指定行星 |
| `listnetworks` | 跨 planet 网络列表,带 PLANET 列;`--planet` 过滤 |
| `listpeers` | peers 按 planet 分组展示 |
| `listmoons` | moons 聚合表;`--planet` 过滤 |
| `events [--planet] [--limit N] [--follow]` | 事件日志表,定位与 dump 同款(**必须指定 planet**,单 running 自动;显式指定已停止 planet 可查历史);`--limit` 默认 40;`--follow` 每 2s 轮询按 ts 游标增量输出,Ctrl-C 退出;daemon 断开/不可达时命令报错退出(exit 3),不会静默重连,长时挂着需自行拉起与保活 |

### 变更类(按定位链落到单一 planet)

| 命令 | 说明 |
|---|---|
| `join <nwid>` | 加入网络 |
| `leave <nwid>` | 退出网络 |
| `set <nwid> <k>=<v>...` | 客户端本地配置:`routeViaZeroTier=0\|1`(非 0/1 退出码 2)、`dnsMode=none\|network\|custom`、`customDns=<逗号分隔>`;未知 key 或不带任何 k=v 退出码 2 |
| `orbit <moonid> <seed>` | orbit 一个 moon |
| `deorbit <moonid>` | deorbit |
| `moon import <file>` | 导入 .moon 文件,关联定位链 planet(成功为 201) |
| `dump` | **单一 planet** 状态 dump,不聚合;歧义时报候选列表 |

### planet 生命周期(顶层命令)

| 命令 | 说明 |
|---|---|
| `add <file\|URL> [--name <别名>] [--data-dir <绝对路径>] [--port <UDP端口>]` | 导入 planet 文件并立即上线;`--data-dir`/`--port` 为开发验证覆写(复用既有 dataDir/指定 UDP 端口),日常省略 |
| `remove <name\|id> [--yes]` | 删除登记;**自动批量 leave 全部已加入网络并停止实例**后再删除(单个 leave 失败记录事件不阻断);交互确认提示走 stderr(stdout 纯净),`--yes` 供脚本跳过;`-j` 下必须配 `--yes`,否则退出码 2 |
| `start <name\|id>` / `stop <name\|id>` | 上线 / 下线(保留登记);start 会按登记的本地网络配置自动 re-join |

### planet 管理

| 命令 | 说明 |
|---|---|
| `planet list` | 登记清单(ICON emoji / ID / STATUS / NETWORKS / PORT) |
| `planet show <name\|id>` | 详情(worldId、类型、roots、来源、大小、修订时间、端口等) |
| `planet rename <name\|id> <新名>` | 重命名别名 |
| `planet export <name\|id> [-o <file>]` | 导出 planet 二进制;缺省 `-o` 写 stdout |
| `planet share <name\|id> [--ttl <秒>]` | 生成临时下载链接并打印 URL 与过期时间 |

### shell 上下文

| 命令 | 说明 |
|---|---|
| `use <name\|id>` | stdout 输出 `export PLANET_PORT_PLANET=<id>` 供 `eval`;人话提示走 stderr;planet 不存在时退出码 2 且 stdout 无输出 |

用法:`eval $(planet-port use company)`。CLI 是一次性子进程,无法修改父 shell 环境,
故必须 eval;状态存于环境变量,可见、可覆盖、脚本可自省。

### service 服务管理

| 命令 | 说明 |
|---|---|
| `service install` | 需 root:拷贝二进制到 `/Library/Application Support/planet-portd/bin/planet-portd`,写 `/Library/LaunchDaemons/com.planetport.daemon.plist` 并 `launchctl bootstrap system/`;RunAtLoad=true(装载即随开机启动)、KeepAlive={SuccessfulExit=false}(崩溃等异常退出自动拉起,SIGTERM 优雅退出不拉起) |
| `service uninstall` | 需 root:`launchctl bootout` + 删 plist 与拷贝的二进制;**数据目录保留**,提示手动 `rm -rf` |
| `service start` | 需 root:`launchctl kickstart -k system/com.planetport.daemon` |
| `service stop` | 需 root:`launchctl kill TERM …`,命中 daemon 的信号优雅退出路径(逐 planet 红线收尾) |
| `service status` | 无需 root:ping `/version` 成功 → `运行中(端口 N)`(N 取自 port 文件);失败时按 plist 是否存在区分「未安装」/「已安装未运行」并给出 launchctl 状态摘要 |

### 说明

- 单操作成功输出为简式,如 `200 join OK`;`moon import` 为 `201 moon import OK`。
- `add`(planet 导入)支持**本地文件路径与 URL**(`http(s)://`,整体超时 30s、
  响应体上限 8MB);`moon import` 当前仅支持本地文件路径;容器导入返回
  `501 notImplemented`。
- `set` 的 `routeViaZeroTier` 在下一次网络配置更新(控制器下发 netconf)时生效:
  false 时清空 managed routes 后再 apply,已装上的路由不会立即回撤。
- `set` 的校验比对 mockd 更严:`routeViaZeroTier` 仅接受 `0|1`(其它值返回 400,
  退出码 2),`dnsMode` 仅接受 `none|network|custom`。

## 真实 daemon(planet-portd)

### 构建

```bash
cargo build --workspace
```

### 前台调试运行(需 sudo:实例要创建 utun、装路由)

```bash
sudo ./target/debug/planet-portd --config-dir "$HOME/Library/Application Support/planet-portd"
```

- `--config-dir` 缺省时按三级解析:env `PLANET_PORT_CONFIG_DIR` →
  `/Library/Application Support/planet-portd`(已存在时,service 安装态与 CLI 同目录)→
  `~/Library/Application Support/planet-portd`。
- 启动即写 `authtoken.secret`(0644,缺失/为空时生成)与 `port` 文件(实际监听端口,
  9998 被占用时自动 +1 递增到 +100);**port 文件的存在即活性信号**,CLI 靠它协商地址,
  daemon 退出时最后删除它。同一 config 目录内另有单实例守卫 `<config>/daemon.lock`
  (flock):第二个实例立即报锁退出,锁文件不删除。
- 上次运行时标记 running 的 planet 会被自动拉起;daemon 在每次 start(含重启
  自动拉起)时按 registry 中该 planet 的 `net_local` 逐网络自动 re-join。Core 的
  `networks.d/<nwid>.conf` 只是 netconf 缓存,**不承载网络成员关系**,核心自身
  不会在重启后恢复 join,故 re-join 必须由 daemon 显式完成(个别网络失败仅记
  事件,不影响实例启动)。

### 服务方式运行(推荐)

```bash
sudo planet-port service install   # 拷贝二进制 + 写 LaunchDaemon + bootstrap
sudo planet-port service start     # 拉起
planet-port service status         # 无需 root 查看状态
sudo planet-port service stop      # SIGTERM,优雅退出(逐 planet 红线收尾)
```

### 典型流程

```bash
planet-port add ./planet.mine --name mine             # 导入 planet 文件并立即上线
eval $(planet-port use mine)                          # 设默认 planet(本 shell)
planet-port join 5cf54f181554090f                     # 加入网络
planet-port listnetworks                              # 复查(等控制器授权后状态 RUNNING)
planet-port info                                      # 聚合状态
```

### 与 mockd 的行为差异(验证时注意)

- planet 数据为实网:planet 文件解析自真实 world,实例连真实根服务器;
- `add` 的 URL 源可用(下载后走与本地文件同一套校验),容器导入 `501 notImplemented`;
- `dns` 模式(`set` 的 `dnsMode`/`customDns`)为 `network`/`custom` 时,netconf 应用
  阶段会用 `networksetup` 把 DNS 写进主网络服务,leave 网络 / 停止或删除 planet 时
  还原为自动获取;失败只记事件,不影响网络数据面;
- `set` 校验更严(见上「说明」);`routeViaZeroTier` 于下次 netconf 生效。

下文回归清单对 mockd 与 planet-portd 均可执行(两者暴露同一套 REST API;
对 planet-portd 执行时请清掉或尊重 port 文件,CLI 会自动指向真实 daemon)。

## 手工验证清单(对 mockd 实测,planet-portd 可按同一清单回归)

以下为对 mockd(种子:official running / company running / home-lab stopped)逐条实测结果,
作为回归基准;对 planet-portd 执行时命令一致,注意上节行为差异(planet 数据为实网、
URL 源可用、container 501):

| 命令 | 预期 | 实测 | 结论 |
|---|---|---|---|
| `info` / `info -j` | 3 planet 聚合表 / JSON | 表格三行;`-j` 返回 `{"planets":[...]}` | 通过 |
| `listnetworks` / `--planet company` | 聚合 + PLANET 列 / 过滤 | 聚合两行;过滤后仅 company 网络,PLANET 列显示 `2187945993` | 通过 |
| `listpeers` / `listmoons` | 分组表 / moon 表 | 按 planet 分组;moons 表含 SEED/SOURCE/CACHED 列 | 通过 |
| `join`(不带 planet) | exit=2,候选列表 | `error 409: multiple running planets` + official/company 候选,exit=2 | 通过 |
| `join --planet company` | 200 join OK | 首次 join 新 nwid 为 `200 join OK`;对已加入网络返回 409(退出码 1) | 通过 |
| `set ... routeViaZeroTier=1 dnsMode=custom customDns=...` | 200 set OK;`-j` 复查生效 | `200 set OK`;复查 `routeViaZeroTier:true`、`dnsMode:"custom"` | 通过 |
| `set ... bogus=1` | exit=2 | `error: unknown key "bogus"; valid: ...`,exit=2 | 通过 |
| `orbit` / `deorbit` / `moon import` | 200/201 OK;listmoons 复查 | orbit/deorbit `200 ... OK`,moon import `201`;listmoons 增删一致 | 通过 |
| `dump --planet official` | 单 planet 文本 | 输出 networks/peers/events 概要;不带 `--planet` 时报候选 | 通过 |
| `planet list/show/rename/export/share` + 顶层 `add/remove/start/stop` | 各自预期 | 全部通过;交互确认拒绝时 exit=0;export stdout/`-o` 均可(mockd 的 remove 仍为旧语义 409 planetRunning,真实现已改为自动 leave + 删除) | 通过 |
| `use company` + eval 后 `join` | 定位链② 生效 | `PLANET_PORT_PLANET=company join` 直接落到 company,无歧义报错 | 通过 |
| `use 不存在` | exit=2,stdout 无输出 | `error: planet "nosuchplanet" not found`,stdout 0 字节,exit=2 | 通过 |
| `events [--planet] [--follow]` | 表格/增量输出 | 过滤生效;`--follow` 每 2s 增量输出新事件 | 通过 |
| 停掉 official 后 `join`(不带 planet) | 定位链③ 自动选 | 仅 company running 时自动落到 company,`200 join OK` | 通过 |
| daemon 未启动 `info` | exit=3 + 提示 | `planet-portd is not running (try: planet-port service start)`,exit=3 | 通过 |
