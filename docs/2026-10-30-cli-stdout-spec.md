# planet-port CLI 标准输出规范

本文整理所有命令的 **stdout 标准输出**。口径:

- **stdout 只放数据**,人话提示/进度/确认全部走 stderr(管道安全);
- 文本模式给人看(表格/键值),`-j` 模式 stdout 只有纯 JSON(给机器/AI);
- `#` 开头的行是 stderr 提示,仅为展示对应关系,不属于 stdout。

标注说明:❇ = 以下输出为 2026-10-30 实机采集(gozi25 + official 双登记环境);
▣ = 变更类命令未在实网执行,样例按代码行为给出。

单行星命令在登记 ≥2 个行星时必须显式 `--planet`(或 `PLANET_PORT_PLANET`),
否则报错退出码 2,下文样例均带显式指定。

---

## 状态查询

### info — 聚合状态表 ❇

```
PLANET    STATUS  ADDRESS     VERSION  LATENCY
gozi25    已连接  92c680e88f  1.16.2   59ms
official  未连接  —           —        —
```

`-j`(完整 PlanetStatus 数组):

```json
{
  "planets": [
    { "id": "3530253632", "name": "gozi25", "status": "running", "online": true,
      "nodeId": "92c680e88f", "coreVersion": "1.16.2", "latencyMs": 59,
      "networkCount": 2, "port": 9994, "type": "custom", "icon": "", "color": "" },
    { "id": "149604618", "name": "official", "status": "stopped", "online": false,
      "nodeId": null, "coreVersion": null, "latencyMs": null,
      "networkCount": 1, "port": 9995, "type": "custom", "icon": "", "color": "" }
  ]
}
```

### list — 按行星分组的网络列表 ❇

覆盖**全部登记行星**(不只在线的);多 managed IP 在 MANAGED-IPS 列内逐行堆叠:

```
planet: 3530253632 (gozi25) [已连接]
NETWORK           NAME                    MAC                STATUS                    TYPE     DEV    MANAGED-IPS
5cf54f181554090f  Planet Port Desktop 开  0e:9b:92:95:f0:c0  OK                        PRIVATE  feth2  10.121.16.1/24
1234567890abcdef  —                       ee:5f:6d:10:90:d9  REQUESTING_CONFIGURATION  PRIVATE  feth0


planet: 149604618 (official) [未连接]
  (无网络)

```

`-j`(分组结构,平铺数组是 listnetworks 的职责):

```json
{
  "planets": [
    { "id": "3530253632", "name": "gozi25", "status": "running", "online": true,
      "networks": [ { "...NetworkEntry 全字段": "见 network show -j" } ] }
  ]
}
```

### listnetworks — 跨行星平铺网络表 ❇

```
PLANET               NETWORK           NAME                    MAC                STATUS                    TYPE     DEV    MANAGED-IPS
3530253632 (gozi25)  5cf54f181554090f  Planet Port Desktop 开  0e:9b:92:95:f0:c0  OK                        PRIVATE  feth2  10.121.16.1/24
3530253632 (gozi25)  1234567890abcdef  —                       ee:5f:6d:10:90:d9  REQUESTING_CONFIGURATION  PRIVATE  feth0
```

`-j`:`{ "networks": [ NetworkEntry, ... ] }`(平铺,含 planet 字段)。

### listpeers — 按行星分组 ❇

```
planet: 3530253632 (gozi25)
ROLE    ADDRESS     LATENCY  VERSION  PATH
LEAF    264cab0ce3  —        1.16.2   (relay)
PLANET  5cf54f1815  59ms     1.14.2   udp/124.222.91.224:9993
LEAF    618857da15  37ms     1.16.2   udp/122.193.195.143:10003

```

`-j`:`{ "peers": [ PeerEntry, ... ] }`。

### listmoons — 聚合 moon 表 ❇

```
PLANET  MOONID  SEED  SOURCE  CACHED
```

`-j`:`{ "moons": [] }`(字段:planet / moonId / seed / fromFile / cached)。

### dump — 单行星状态汇总(zerotier-cli dump 对位)❇

```
planet gozi25 (world 3530253632) status=running port=9994
networks:
  5cf54f181554090f OK assigned=[10.121.16.1/24] routeViaZeroTier=true
  1234567890abcdef REQUESTING_CONFIGURATION assigned=[] routeViaZeroTier=true
peers:
  264cab0ce3 LEAF latency=- path=relay
  5cf54f1815 PLANET latency=59ms path=udp/124.222.91.224:9993
  618857da15 LEAF latency=37ms path=udp/122.193.195.143:10003
net_local:
  1234567890abcdef routeViaZeroTier=true dnsMode=none customDns=[]
  5cf54f181554090f routeViaZeroTier=true dnsMode=none customDns=[]
events (last 10):
  1793340930652 netconf 网络 5cf54f181554090f status=OK rev=6 assigned=[10.121.16.1/24]
  ...
```

`-j`:`{ "planet": ..., "networks": [...], "peers": [...], "events": [...] }`
结构化对象(events 为最近 10 条)。

### events — 事件日志表 ❇

```
TS                       PLANET      KIND     MESSAGE
2026-10-30 14:15:30.652  3530253632  netconf  网络 5cf54f181554090f status=OK rev=6 assigned=[10.121.16.1/24]
```

- 文本模式 TS 为本地时间;`-j` 保留 `tsMs` epoch(游标/脚本语义);
- `--follow` 每 2s 增量输出,daemon 断开时退出码 3,不静默重连;
- 定位必须唯一:登记 ≥2 未指定时报错退出码 2;显式指定已停止行星可查历史。

`-j`:

```json
{ "events": [ { "planet": "3530253632", "tsMs": 1793341003072,
    "kind": "shareCreated", "message": "..." } ] }
```

---

## 网络操作

`<nwid>` 均接受网络 ID(3-16 位 hex,容错 0x 前缀)或**已加入网络的名字**
(不区分大小写,含空格/中文直接引号包裹);`join` 只认 nwid。

### join / leave — 幂等变更 ❇

文本模式单行回执,带行星落点:

```
gozi25(3530253632) 200 join OK
gozi25(3530253632) 200 leave OK
```

`-j`(幂等重入附 idempotent):

```json
{ "idempotent": true, "result": "ok" }
```

### set — 本地网络配置 ▣(格式同上)

```
gozi25(3530253632) 200 set OK
```

`-j`:`{ "result": "ok" }`。生效时机为下一轮控制器 netconf 下发。

### get — 读单个配置值(脚本友好)❇

字符串裸输出,数组/对象紧凑 JSON:

```
none
[{"target":"10.121.16.0/24","via":null}]
```

`-j`:该值的 JSON 字面量(如 `"none"`)。未知 key 退出码 2 并提示合法 key
(ZeroTier 旧 key `allowDefault` 等附等价映射提示)。

### network show — 单网络详情视图 ❇

```
network   5cf54f181554090f
planet    gozi25(3530253632)
name      Planet Port Desktop 开
status    OK
type      PRIVATE
mac       0e:9b:92:95:f0:c0
device    feth2
mtu       2800
assigned  10.121.16.1/24
routes    10.121.16.0/24
dns       mode=none servers=[] custom=[]
routeVia  true
```

`-j`(完整 NetworkEntry):

```json
{
  "planet": "3530253632", "nwid": "5cf54f181554090f", "name": "Planet Port Desktop 开",
  "status": "OK", "type": "PRIVATE", "mac": "0e:9b:92:95:f0:c0", "interface": "feth2",
  "mtu": 2800, "assignedAddrs": ["10.121.16.1/24"],
  "managedRoutes": [ { "target": "10.121.16.0/24", "via": null } ],
  "dnsServers": [], "routeViaZeroTier": true, "routeViaLocked": false,
  "dnsMode": "none", "customDns": [], "conflict": null
}
```

---

## moon 操作 ▣

```
gozi25(3530253632) 200 orbit OK
gozi25(3530253632) 200 deorbit OK      # 204 空体,-j 代以 {"result":"ok"}
gozi25(3530253632) 201 moon import OK
```

`moon import` 当前仅支持本地 `.moon` 文件路径。

---

## planet 生命周期

### add — 导入并上线 ▣

```
added planet "gozi25" (id 3530253632) → running
```

`-j`:PlanetDetail(同 `planet show -j` 内层对象)。容器导入返回 501。

### start / stop ▣

```
gozi25(3530253632) 200 start OK
gozi25(3530253632) 200 stop OK
```

`-j`:`{ "result": "ok" }`。stop 保留登记与期望状态,service start 后自动恢复;
显式 `planet-port stop` 后需手动 start。

### remove — 删除登记 ▣

确认提示走 **stderr**(stdout 保持纯净),拒绝时退出码 130:

```
# planet "gozi25" 将自动退出其全部网络、停止并删除登记,确认? [y/N]
gozi25(3530253632) 200 remove OK
```

`-j`:`{ "result": "ok" }`(必须配 `--yes`,否则退出码 2)。

---

## planet 管理

### planet list — 登记清单 ❇

```
ICON  PLANET    ID          STATUS   NETWORKS  PORT
🪐    gozi25    3530253632  running  2         9994
🪐    official  149604618   stopped  1         9995
```

`-j`:`{ "planets": [ PlanetStatus, ... ] }`(同 info -j)。

### planet show — 详情 ❇

```
planet    🪐 gozi25
id        3530253632
worldId   3530253632
type      custom
status    running (online: true)
roots     124.222.91.224:9993
source    file
size      257 bytes
revision  2025-07-26 23:13:26.865
lastNodeId 92c680e88f
networks  2
port      9994
```

`-j`:`{ "planet": { PlanetDetail } }`,修订时间保留 epoch。

### planet rename ▣(格式实测)

```
gozi25(3530253632) 200 rename OK
```

### planet export — 导出二进制 ❇

`-o` 缺省时 **stdout 输出 planet 二进制原始字节**(管道重定向友好,勿直接
打印到终端);`-o <file>` 时 stdout 无输出。`-j` 同样输出二进制(export 无 JSON 形态)。

### planet share — 临时下载链接 ❇

```
http://127.0.0.1:9998/share/0cc0366d8ad27afd9d2f9dda
# expires_at: 1793341008 (2026-10-30 14:16:48)        ← stderr
# 注意: 链接为回环地址,仅本机可打开;发给他人请改用本机的局域网 IP   ← stderr
```

`-j`:`{ "url": "...", "expiresAt": 1793341008 }`(epoch)。

---

## shell 上下文

### use — 设置默认行星 ❇

别名不区分大小写;stdout 只有 export 行供 eval:

```
export PLANET_PORT_PLANET=3530253632
# default planet set to "gozi25" (this shell only)   ← stderr
# usage: eval $(planet-port use gozi25)              ← stderr
```

`-j`:

```json
{ "planet": "3530253632", "name": "gozi25" }
```

行星不存在时退出码 2,stdout 无输出。

---

## service 服务管理

install/uninstall/start/stop 需 root;进度提示走 stderr,`-j` 时 stdout 仅回执 JSON。

| 命令 | stdout(text) | stdout(-j) |
|---|---|---|
| `service install` ▣ | 安装进度各步(stderr),无数据行 | `{"result":"ok"}`(重入附 `"idempotent":true`) |
| `service uninstall` ▣ | 同上 | `{"result":"ok"}` |
| `service start` ▣ | 同上 | `{"result":"ok","port":9998}` / 超时 `{"result":"error"}` |
| `service stop` ▣ | 同上 | `{"result":"ok"}` / `{"result":"error"}` / KeepAlive 拉起 `{"result":"warning"}` |
| `service logs` ❇ | daemon 日志原文(默认末 50 行) | —(无 -j) |
| `service status` ❇ | 见下 | `{"running":true,"port":9998,"version":{...},"planets":[...]}` |

`service status` 文本模式(不在线时按 plist 区分「未安装/已安装未运行」,退出码 3):

```
运行中(端口 9998)
  gozi25: ONLINE 59ms,网络 2 个
  official: stopped,网络 1 个
```

---

## 错误输出(全部走 stderr,stdout 为空)

| 场景 | stderr 样例 | 退出码 |
|---|---|---|
| 登记 ≥2 未指定行星 | `error: 登记了多个 planet,请用 --planet 指定目标:` + 候选 `gozi25 (3530253632, running)` / `official (149604618, stopped)` | 2 |
| 非法 nwid | `error 400: invalid network id "xyz" (expect 3-16 hex digits)` | 1 |
| 未知 set key | `error: unknown key "allowDefault"; ZeroTier's allowDefault ≈ "routeViaZeroTier" here; valid: ...` | 2 |
| daemon 不在线 | `planet-portd is not running (try: planet-port service start)` | 3 |
| remove 确认拒绝 | `已取消` | 130 |

通用 API 错误格式:`error <code>: <message>`(409 定位类由 CLI 改写为上表指引样式,
跟随 CLI 语言)。

## 退出码总表

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 运行错误(API 4xx/5xx、IO) |
| 2 | 用法错误、planet 歧义、service 组未以 root 运行 |
| 3 | daemon 不在线 |
| 130 | 用户中止(remove 确认拒绝) |
