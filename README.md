# planet-port-desktop

ZeroTier 多 Planet 管理桌面端(Rust)。架构与可行性结论见
docs/2026-10-28-desktop-port-feasibility.md,CLI/API 设计见
docs/superpowers/specs/2026-10-28-cli-api-design.md。

## 组件

| crate | 说明 |
|---|---|
| planet-port-api | REST 契约类型与端点构造(CLI 与 daemon 共用) |
| mockd | 契约 mock daemon(联调/契约验证,后续 GUI 开发复用) |
| planet-port | CLI 薄客户端 |

## 开发联调

```bash
cargo run -p mockd &
export PLANET_PORT_URL=http://127.0.0.1:9998 PLANET_PORT_TOKEN=dev
cargo run -p planet-port -- info
```

- 默认 API 端点 `http://127.0.0.1:9998`,可用 `--api-url` 或 `PLANET_PORT_URL` 覆盖(调试用)。
- 鉴权走 `X-ZT1-Auth` 头:优先取 `PLANET_PORT_TOKEN` 环境变量,未设置时读取
  token 文件(`<config_dir>/planet-portd/authtoken.secret`)。mockd 接受任意非空 token,
  联调用 `PLANET_PORT_TOKEN=dev` 即可。
- 未设置 URL 时 CLI 使用内置默认端口 9998;真实 `planet-portd` 的端口文件协商(CLI 读取)尚未实现。

## planet 定位链

变更类命令(join / leave / set / orbit / deorbit / dump / moon import)必须落到**恰好一个**
planet 实例,按以下顺序解析:

| 优先级 | 来源 | 说明 |
|---|---|---|
| ① | `--planet <name\|id>` | 显式指定,优先级最高 |
| ② | 环境变量 `PLANET_PORT_PLANET` | 由 `eval $(planet-port use <x>)` 设置 |
| ③ | 唯一 running planet 自动选中 | 仅有一个激活 planet 时自动落到它 |
| ④ | 多个 running planet | 报 `409` 并列出候选,**退出码 2,不猜** |

查询类命令(info / listnetworks / listpeers / listmoons / events)**永远聚合展示**,
`--planet` 与环境变量在其上的语义是**过滤**,不受上述链约束。

`--planet` 同时接受别名(name)与 worldId;服务端解析,命中即返回 canonical id
(表格 PLANET 列显示的是 canonical id,如 company 显示 `2187945993`)。

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 运行错误(API 4xx/5xx、IO 错误) |
| 2 | 用法错误(非法参数、未知 set key)与 planet 歧义(409 + 候选列表) |
| 3 | daemon 不在线,提示 `planet-portd is not running (try: planet-port service start)` |

## 命令参考

全局 flag:`-j/--json`(全命令)、`--planet <name|id>`(global)、`--api-url <url>`(global)。

### 聚合查询

| 命令 | 说明 |
|---|---|
| `info` | 所有 planet 聚合状态表(PLANET/STATUS/ADDRESS/VERSION/LATENCY) |
| `listnetworks` | 跨 planet 网络列表,带 PLANET 列;`--planet` 过滤 |
| `listpeers` | peers 按 planet 分组展示 |
| `listmoons` | moons 聚合表;`--planet` 过滤 |
| `events [--limit N] [--follow]` | 事件日志表,`--limit` 默认 40;`--follow` 每 2s 轮询按 ts 游标增量输出,Ctrl-C 退出 |

### 变更类(按定位链落到单一 planet)

| 命令 | 说明 |
|---|---|
| `join <nwid>` | 加入网络 |
| `leave <nwid>` | 退出网络 |
| `set <nwid> <k>=<v>...` | 客户端本地配置:`routeViaZeroTier=0\|1`、`dnsMode=none\|network\|custom`、`customDns=<逗号分隔>`;未知 key 退出码 2 |
| `orbit <moonid> <seed>` | orbit 一个 moon |
| `deorbit <moonid>` | deorbit |
| `moon import <file>` | 导入 .moon 文件,关联定位链 planet(成功为 201) |
| `dump` | **单一 planet** 状态 dump,不聚合;歧义时报候选列表 |

### planet 管理

| 命令 | 说明 |
|---|---|
| `planet list` | 登记清单(ICON emoji / ID / STATUS / NETWORKS / PORT) |
| `planet import <file\|URL> [--name <别名>]` | 导入 planet 文件并立即上线 |
| `planet show <name\|id>` | 详情(worldId、类型、roots、来源、大小、修订时间、端口等) |
| `planet rename <name\|id> <新名>` | 重命名别名 |
| `planet start <name\|id>` / `planet stop <name\|id>` | 上线 / 下线(保留登记) |
| `planet rm <name\|id> [--yes]` | 删除登记;有活跃网络时拒绝;交互确认,`--yes` 供脚本跳过 |
| `planet export <name\|id> [-o <file>]` | 导出 planet 二进制;缺省 `-o` 写 stdout |
| `planet share <name\|id> [--ttl <秒>]` | 生成临时下载链接并打印 URL 与过期时间 |

### shell 上下文

| 命令 | 说明 |
|---|---|
| `use <name\|id>` | stdout 输出 `export PLANET_PORT_PLANET=<id>` 供 `eval`;人话提示走 stderr;planet 不存在时退出码 2 且 stdout 无输出 |

用法:`eval $(planet-port use company)`。CLI 是一次性子进程,无法修改父 shell 环境,
故必须 eval;状态存于环境变量,可见、可覆盖、脚本可自省。

### 说明

- `service` 子命令组(spec §3.2)尚未实现,daemon 生命周期管理在后续任务中落地。
- 单操作成功输出为简式,如 `200 join OK`;`moon import` 为 `201 moon import OK`。

## 手工验证清单(对 mockd 实测)

以下为对 mockd(种子:official running / company running / home-lab stopped)逐条实测结果,
作为后续对接真实 daemon 时的回归基准:

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
| `planet list/import/show/rename/start/stop/rm/export/share` | 各自预期 | 全部通过;rm 有活跃网络时 409 拒绝(exit=1),交互确认拒绝时 exit=0;export stdout/`-o` 均可 | 通过 |
| `use company` + eval 后 `join` | 定位链② 生效 | `PLANET_PORT_PLANET=company join` 直接落到 company,无歧义报错 | 通过 |
| `use 不存在` | exit=2,stdout 无输出 | `error: planet "nosuchplanet" not found`,stdout 0 字节,exit=2 | 通过 |
| `events [--planet] [--follow]` | 表格/增量输出 | 过滤生效;`--follow` 每 2s 增量输出新事件 | 通过 |
| 停掉 official 后 `join`(不带 planet) | 定位链③ 自动选 | 仅 company running 时自动落到 company,`200 join OK` | 通过 |
| daemon 未启动 `info` | exit=3 + 提示 | `planet-portd is not running (try: planet-port service start)`,exit=3 | 通过 |
