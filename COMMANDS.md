# planet-port 命令总览

`planet-port` 是 planet-portd 守护进程的 CLI。命令命名对齐 `zerotier-cli`
(info / listpeers / listnetworks / join / leave / set / dump / orbit / deorbit),
多 planet 管理命令(add / remove / start / stop 等)为顶层命令,不与 zerotier-cli
现有命令冲突。

## 全局选项

| 选项 | 说明 |
|---|---|
| `--planet <name\|id>` | 定位 planet:变更类命令的目标选择器,查询类命令的过滤器;省略时取环境变量 `PLANET_PORT_PLANET`,再省略由 daemon 按「单 running 自动选择 / 多 running 报歧义」处理 |
| `-j, --json` | JSON 输出(stdout 仅 JSON,人话提示走 stderr) |
| `--api-url <URL>` | 覆写 daemon API 地址;省略时读配置目录 port 文件,再回退 `127.0.0.1:9998`。也可用环境变量 `PLANET_PORT_URL` |

## 状态查询

| 命令 | 说明 |
|---|---|
| `info` | 所有 planet 的聚合状态 |
| `listnetworks [--planet]` | 跨 planet 网络列表(含 PLANET 列) |
| `listpeers [--planet]` | peer 列表,按 planet 分组 |
| `dump [--planet]` | 单一 planet 状态 dump(networks/peers/events);歧义时报候选 |
| `events [--limit N] [--follow] [--planet]` | 事件日志;默认取 40 条,`--follow` 增量轮询 |

## 网络操作(需 planet 定位)

| 命令 | 说明 |
|---|---|
| `join <nwid>` | 加入网络 |
| `leave <nwid>` | 退出网络 |
| `set <nwid> <k>=<v>...` | 客户端本地配置:`routeViaZeroTier=0\|1`、`dnsMode=none\|network\|custom`、`customDns=<逗号分隔>`;未知 key 退出码 2 |

## moon 操作

| 命令 | 说明 |
|---|---|
| `listmoons [--planet]` | moon 列表 |
| `orbit <moonid> <seed> [--planet]` | orbit 一个 moon |
| `deorbit <moonid> [--planet]` | deorbit |
| `moon import <file> [--planet]` | 导入 .moon 文件(成功 201) |

## planet 生命周期(顶层)

| 命令 | 说明 |
|---|---|
| `add <file> [--name <别名>] [--data-dir <绝对路径>] [--port <UDP端口>]` | 导入 planet 文件并立即上线;`--data-dir`/`--port` 为开发验证覆写(如复用既有 dataDir),日常省略 |
| `start <name\|id>` | 上线实例;按登记的本地网络配置自动 re-join |
| `stop <name\|id>` | 下线实例(feth 设备与路由清理),保留登记 |
| `remove <name\|id> [--yes]` | 删除登记;有活跃网络时拒绝(先 leave);交互确认走 stderr,`--yes` 供脚本跳过,`-j` 下必须配 `--yes` |

## planet 管理

| 命令 | 说明 |
|---|---|
| `planet list` | 登记清单(ICON / PLANET / ID / STATUS / NETWORKS / PORT) |
| `planet show <name\|id>` | 详情(worldId、类型、roots、来源、大小、修订时间等) |
| `planet rename <name\|id> <新名>` | 重命名别名 |
| `planet export <name\|id> [-o <file>]` | 导出 planet 二进制;缺省 `-o` 写 stdout |
| `planet share <name\|id> [--ttl <秒>]` | 生成一次性局域网下载链接(默认 300s) |

## shell 上下文与系统服务

| 命令 | 说明 |
|---|---|
| `use <name\|id>` | stdout 输出 `export PLANET_PORT_PLANET=<id>` 供 `eval $(planet-port use <name\|id>)` |
| `service install` | 需 root:安装 LaunchDaemon 并拉起(幂等,重装先卸载旧服务) |
| `service uninstall` | 需 root:卸载服务;数据目录保留 |
| `service start` / `service stop` | 需 root:拉起 / SIGTERM 优雅停止 |
| `service status` | 无需 root:运行状态(端口取自 port 文件) |

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | daemon 返回错误(4xx/5xx,stderr 带 error 码与候选列表) |
| 2 | CLI 用法错误(未知 key、参数缺失、`use` 目标不存在等) |

详细行为说明与验证清单见 [README.md](README.md) 。
