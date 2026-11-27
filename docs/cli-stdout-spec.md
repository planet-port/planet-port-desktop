# planet-port CLI 标准输出规范

本文整理所有命令的 **stdout 标准输出**。口径:

- **stdout 只放数据**,人话提示/进度/确认全部走 stderr(管道安全);
- 文本模式给人看(表格/键值),`-j` 模式 stdout 只有纯 JSON(给机器/AI);
- `#` 开头的行是 stderr 提示,仅为展示对应关系,不属于 stdout。

本文为现行基准,随版本演进持续更新。
标注说明:❇ = 以下输出为实机采集(gozi25 + official 双登记环境);
▣ = 变更类命令未在实网执行,样例按代码行为给出;
◇ = v0.3/v0.4 新增样例,按 `crates/cli` 当前代码行为给出(未实机采集);
❇ 样例中 v0.4 补充的列/行(如 info 的 RELAY 列)同 ◇ 口径。

单行星命令在登记 ≥2 个行星时必须显式 `--planet`(或 `PLANET_PORT_PLANET`),
否则报错退出码 2,下文样例均带显式指定。

---

## 状态查询

### --version — 版本信息 ◇(本机实测)

```text
planet-port 0.5.1-dev (zerotier-core 1.16.2, built 2026-11-25 20:40:08)
planet-portd 0.5.1-dev
```

- 两行输出:第一行 cli 自身(name + 包版本 + zerotier-core 版本(FFI 取核心
  常量,三段式,与 `info` 的 VERSION 列同源)+ 构建时间(build.rs 注入 epoch,
  本地时区展示));第二行为 daemon 版本**运行时探测**(先找 cli 同目录的
  planet-portd,便携套装优先;再退已安装路径),探测不到(未安装或 daemon
  过旧无 `--version`)时为 `planet-portd (not installed)`,不猜版本;
- `-V` 仍为单行(仅第一行);
- e2e 脚本按 `grep -q <ver>` 子串匹配版本号,两行不影响既有断言;
- daemon 自身 `--version` 输出与第一行同构的单行
  `planet-portd X.Y.Z (zerotier-core M.m.r, built …)`,GUI 离线探测按
  「首行第二 token」解析;daemon 侧 `GET /version` 同源:
  `{"version","zerotierCore","built"}`。

### info — 聚合状态表 ❇(RELAY 列为 v0.4 补充,◇ 口径)

```
PLANET    ADDRESS     VERSION  STATUS  LATENCY  RELAY                NETWORKS
gozi25    92c680e88f  1.16.2   已连接  59ms     124.222.91.224:9443  2
official  —           —        未连接  —        —                    1
```

节点地址/版本是本地事实,实例运行即显示(连接中也显示,仅 LATENCY 为 —);
停止的行星两者为 —。NETWORKS 运行中取实时 join 数,停止回退 registry 登记数。
RELAY 显示 TCP 兜底 relay **配置态**(`planet set-relay` 所设完整地址,
未配置为 `—`);隧道实时状态看 dump / `planet show` 的 transport 行。

`-j`(完整 PlanetStatus 数组):

```json
{
  "planets": [
    { "id": "3530253632", "name": "gozi25", "status": "running", "online": true,
      "enabled": true,
      "nodeId": "92c680e88f", "coreVersion": "1.16.2", "latencyMs": 59,
      "networkCount": 2, "port": 9994, "type": "custom", "icon": "", "color": "",
      "tcpFallbackRelay": "124.222.91.224:9443",
      "transport": { "primaryPort": 9993, "secondaryPort": null,
                     "tcp": "established", "relay": "124.222.91.224:9443" } },
    { "id": "149604618", "name": "official", "status": "stopped", "online": false,
      "enabled": true,
      "nodeId": null, "coreVersion": null, "latencyMs": null,
      "networkCount": 1, "port": 9995, "type": "custom", "icon": "", "color": "",
      "tcpFallbackRelay": null, "transport": null }
  ]
}
```

### listnetworks — 按行星分组的网络列表 ❇

覆盖**全部登记行星**(不只在线的);多 managed IP 在 MANAGED-IPS 列内逐行堆叠;
NAME 列为融合显示名:**alias > 控制器名(netconf/缓存)> nwid**(见 alias 命令);
停止/禁用行星下的网络以 STOPPED 占位行展示(名称/MAC 等动态字段为 —);
已断开(leave)的网络以 **DISCONNECTED 占位行**保留可见(name 用断开
时快照,动态字段 `—`)——断开的网络是用户资产,不从列表消失;组内排序
ACTIVE → STOPPED → DISCONNECTED;`--planet`(别名:id/name/worldId)只看指定行星:

```
planet: 3530253632 (gozi25) [已连接]
NETWORK           NAME                    MAC                STATUS                    TYPE     DEV    MANAGED-IPS
5cf54f181554090f  Planet Port Desktop 开  0e:9b:92:95:f0:c0  OK                        PRIVATE  feth2  10.121.16.1/24
1234567890abcdef  —                       ee:5f:6d:10:90:d9  REQUESTING_CONFIGURATION  PRIVATE  feth0
aabbccddeeff0011  家庭网络                —                  DISCONNECTED              —        —


planet: 149604618 (official) [未连接]
NETWORK           NAME  MAC  STATUS   TYPE  DEV  MANAGED-IPS
633e31d8a29d9c97  —     —    STOPPED  —     —

```

`-j`(分组结构,与文本模式同口径):

```json
{
  "planets": [
    { "id": "3530253632", "name": "gozi25", "status": "running", "online": true,
      "networks": [ { "...NetworkEntry 全字段": "planet/nwid/name/status/mac/..." } ] }
  ]
}
```

### listpeers — 按行星分组 ❇

分组覆盖全部登记行星,停止/离线的行星占位 `(无 peer)`:

```
planet: 3530253632 (gozi25) [已连接]
ROLE    ADDRESS     LATENCY  VERSION  PATH
LEAF    264cab0ce3  —        1.16.2   (relay)
PLANET  5cf54f1815  59ms     1.14.2   udp/124.222.91.224:9993
LEAF    618857da15  37ms     1.16.2   udp/122.193.195.143:10003


planet: 149604618 (official) [未连接]
  (无 peer)

```

`-j`:分组结构 `{ "planets": [ { "id", "name", "status", "online", "peers": [ PeerEntry ] } ] }`。

### listmoons — 按行星分组 ❇

```
planet: 3530253632 (gozi25) [已连接]
MOONID  SEED  SOURCE  CACHED

planet: 149604618 (official) [未连接]
  (无 moon)

```

`-j`:分组结构 `{ "planets": [ { ..., "moons": [ MoonEntry ] } ] }`
(字段:planet / moonId / seed / fromFile / cached)。

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

### leave / remove — 断开 / 彻底删除 ◇

文本模式单行回执,与变更类同款带行星落点:

```
smoke(3530253632) 200 leave OK
smoke(3530253632) 200 remove OK
```

`-j`:

```json
{ "result": "ok" }
```

口径:`leave` = 断开(设备面拆除 + DNS 还原,id / 本地配置 / 名字快照迁入断开
区,net_local 清空时 idle 停机);`remove` = 彻底删除(在网先脱离,net_local +
net_detached 全清,列表不再显示)。幂等:重复 leave / 无记录 remove(含未登
记 nwid)均成功。错误口径分列:`leave` 未登记网络 404 `networkNotFound`,
planet 禁用时 409 `planetDisabled`;`remove` 仅按**名字**定位未命中时 404
`networkNotFound`,409 `planetDisabled` 仅在在网或 net_local 有记录需收尾
脱离时触发(disabled planet 上仅存断开区记录时 remove 直接成功)。
`join` 命中断开区自动恢复配置。

### share — 网络分享(终端二维码 + 剪贴板)◇

stdout 依次为:网络名(无名字退化 nwid)、空行、二维码(unicode 半块字符)、
网络 ID;剪贴板结果提示走 **stderr**(stdout 保持数据纯净):

```
Planet Port Desktop 开

▀▀▀▀▀▀▀▄▄▄▄▄▄█▀▀ ...
(半块字符渲染的二维码,内容 = 网络 ID 16 位 hex 纯文本)

5cf54f181554090f
# 已复制网络 ID 到剪贴板                        ← stderr
# 剪贴板不可用,请手动复制上面的网络 ID          ← stderr(无显示环境降级提示,不报错)
```

二维码规范对齐 iOS 版:内容为网络 ID 纯文本(与 App 内扫码填 ID 互逆,非
URL)、纠错级别 M。任何网络状态(ACTIVE / STOPPED / DISCONNECTED)可分享,名称
用快照或 nwid。`-j`:stdout 仅 NetworkEntry(二维码/剪贴板属展示层,人话
照走 stderr)。

### set — 本地网络配置 ▣(格式同上)

```
gozi25(3530253632) 200 set OK
```

`-j`:`{ "result": "ok" }`。生效时机为下一轮控制器 netconf 下发。对断开
(DISCONNECTED)网络报 404 `networkNotFound`(一期不做「断开期间预改配置」,
先 join 再改)。

### alias — 网络显示别名 ▣(格式同上)

```
gozi25(3530253632) 200 alias OK
```

`alias <nwid|name> [name]`:设/清网络显示别名,与 `set <nwid> alias=...`
同一通道。别名即时生效,展示优先级 **alias > 控制器名(netconf/缓存)>
nwid**,各端(卡片/托盘/CLI 按名定位)一致;控制器名不受影响,清除即回落。
缺省或空名 = 清除。生命周期随网络本地配置:leave/join 保留,remove 清除。
`-j`:同 set 真实响应体。

### get — 读单个配置值(脚本友好)❇

字符串裸输出,数组/对象紧凑 JSON:

```
none
[{"target":"10.121.16.0/24","via":null}]
```

`-j`:该值的 JSON 字面量(如 `"none"`)。未知 key 退出码 2 并提示合法 key
(ZeroTier 旧 key `allowDefault` 等附等价映射提示)。

### networkinfo — 单网络详情视图 ❇

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

`-j`(完整 NetworkEntry)。原 `network show` 子命令改为顶层 `networkinfo <nwid>`。

行星未运行时**不拒绝**:定位不校验 running,展示 STOPPED 占位详情
(仅 registry 登记配置,name/mac/device/mtu 等动态字段为 —),
并以 stderr 反馈原因:

```
# 提示: 所属行星未运行(已禁用或空闲停止),以下仅为登记配置; ...   ← stderr
network   633e31d8a29d9c97
planet    official(149604618)
name      —
status    STOPPED
...
```

网络本身已禁用(status=DISABLED)时**不拒绝**:展示禁用前登记配置并以
stderr 反馈恢复方式(mtu 等动态字段为 —,与 STOPPED 同款占位形态):

```
# 提示: 该网络已禁用(脱离但保留本地配置),以下仅为禁用前登记配置; ...   ← stderr
network   aabbccddeeff0011
planet    smoke(3530253632)
name      家庭网络
status    DISABLED
mtu       —
...
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

## planet 生命周期(行星命令)

### planet add — 导入登记 ▣

```
added planet "gozi25" (id 3530253632) → stopped
```

`-j`:PlanetDetail(同 `planet show -j` 内层对象)。容器导入返回 501。
导入只登记不拉起(派生运行规则:enabled ∧ 有网络才运行),首次 join 时自动上线。

### planet enable / disable ▣

```
gozi25(3530253632) 200 enable OK
gozi25(3530253632) 200 disable OK
```

`-j`:`{ "planet": PlanetDetail }`。enable/disable 是唯一的启停语义:
disable 无条件停且 join/boot 不再运行;enable 对有网络的 planet 按派生
规则自动拉起。原顶层 `start`/`stop` 命令已移除;原顶层 `add`/`remove` 同步
迁入 `planet` 子命令(不留兼容别名,旧顶层入口报未知命令)。注意:此处是
**行星级**持久开关,与网络级连接/断开(`leave` / `join`,作用于单个
网络,回执 `{ "result": "ok" }`)对象不同,见上文「网络操作」。

### planet remove — 删除登记 ▣

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

STATUS 为整值 `running` / `disabled` / `stopped`(v0.4 起):底层 status 为
running 时显示 `running`,否则按持久开关显示 `disabled`(enabled=false)或
`stopped`——禁用与运行状态正交,避免「stopped 但其实被禁用」的误读。

`-j`:`{ "planets": [ PlanetStatus, ... ] }`(同 info -j,含 `enabled` 字段)。

### planet show — 详情 ❇(transport/relay 两行为 v0.4 补充,◇ 口径)

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
transport udp/9993 tcp=established(124.222.91.224:9443)
relay     124.222.91.224:9443
```

transport 行格式:`udp/<主口>`(启用副口时追加 `+<副口>`)+ ` tcp=<disabled|idle|connecting|established>`(配置 relay 且隧道有目标时追加 `(<relay地址>)`);relay 行为 TCP 兜底 relay 配置态,未配置为 `—`。实例未运行时 transport 为 `—`。

`-j`:`{ "planet": { PlanetDetail } }`,修订时间保留 epoch。

### planet rename ▣(格式实测)

```
gozi25(3530253632) 200 rename OK
```

`planet alias` 为等价别名入口(行星 name 即别名语义)。

### planet set-relay — 设置/清除 TCP 兜底 relay ◇

文本模式单行回执,设置与清除同格式(落点标签规则同变更类通用口径):

```
gozi25(3530253632) 200 set-relay OK
```

- 参数形态:`--relay <ip:port>`(完整形,原样保存)/ `--relay :<端口>` 与
  `--relay <裸端口>`(精简形,CLI 层展开为 planet **首根 IP**+所给端口)/
  仅 `--relay` 不带值(端口取缺省 9443)/ 不带 `--relay` 标志(清除,存 null);
- 回显链:设置后 `info` 的 RELAY 列与 `planet show` 的 transport/relay 行
  均回显所设**完整地址**;清除后两处回到 `—`;
- 地址合法性由服务端校验:非法值 `error 400: tcpFallbackRelay must be
  "ip:port", got: ...`(exit=1);精简形展开失败(planet 无根端点/端口非法)
  为 CLI 用法错 `error: relay 端口非法: <raw>` 等(exit=2)。

`-j`:`{ "planet": { PlanetDetail } }`(与 rename 同款 PATCH 响应体)。

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

## 身份管理(identity) ◇

身份模式是 daemon 级全局开关:`shared`(默认)= 全部 planet 复用全局身份;
`independent` = 各 planet 保持自有身份。

### identity(缺省)/ identity list — 身份模式与清单

stdout 依次为模式行、全局身份行、每 planet 一行(planet 行为
`名称␣␣node id␣␣状态` 双空格直拼,不做列对齐;无身份的 planet node id 为 `-`):

```
身份模式: 共享(全部 planet 复用全局身份)
全局身份: 92c680e88f
gozi25  92c680e88f  running
official  -  stopped
```

全局身份未建立时第二行为:

```
全局身份: (未建立,可 identity import 导入官方身份)
```

`-j`:完整 IdentityOverview(`{ "mode": "shared", "global": { "exists": true,
"nodeId": "92c680e88f" }, "planets": [ { "id", "name", "nodeId", "running" } ] }`)。

### identity import — 导入官方身份

执行前先打**只读预检**:旧版 daemon 无预检路由(404)或响应缺 preflight
标记时,stderr 报 `error: daemon 版本过旧不支持导入预检,请先升级
planet-portd 再执行 identity import`(exit=2),不触发正式导入。

已有全局身份且未 `--force`:

- 非交互(含 `-j`):stderr `error: 已有全局身份 <old>,覆盖导入需加 --force
  (或交互终端下确认)`,exit=2;
- 交互终端:确认提问经 **stdout** 打印(`将覆盖现有全局身份 <old>,继续?
  [y/N]`,注意:该提问不走 stderr,与本文件全局口径不同,仅在交互终端出现),
  非 y/N → stdout `已取消导入`,exit=0。

`--mode` 未指定时非交互直接报用法错 `error: 非交互环境需显式指定
--mode shared|independent`(exit=2);交互终端 stdout 二选一提问。

导入成功回执为 **stdout 序列**,各段按条件依次出现:

```
已导入全局身份: 92c680e88f
官方客户端当前 planet 已导入为 gozi25;原已加入的网络直接 join 即可恢复,无需重新授权
身份模式已切换为 共享(全部 planet 复用全局身份),planet 下次运行时生效
planet gozi25 已换用官方身份,下次运行时生效
被替换的节点地址 <id1>, <id2> 在各网络的成员记录不会自动删除,请到控制器(如 ztnet / ZeroTier Central)手动清理
```

- 第 1 行:官方服务存在并被停用时为 `已导入全局身份: <node>(官方服务已停止
  并禁用自启)`;本机未检测到官方客户端时追加一行 `本机未检测到官方
  ZeroTier 客户端(身份文件可能来自其它机器)`;
- 第 2 行:官方客户端当前 planet 已登记时如上;未登记时改为一行指引
  `官方客户端当前 自定义 planet(worldId <worldId>)尚未导入;恢复原网络请先执行: planet-port planet add <planet文件路径>`;
- 第 3 行:仅模式实际发生变化时出现;
- 第 4 行:仅 `--mode independent` 且同源 planet 换用官方身份时出现
  (下次运行生效);
- 第 5 行:仅存在被新身份取代的 node id 时出现。

`-j`:stdout 仅导入响应体的 pretty JSON(`nodeId`/`serviceAction`/
`officialPlanet`/`modeChanged`/`identityMode`/`planetIdentityApplied`/
`supersededNodeIds` 等),无上述人话序列。

### identity use <shared|independent> — 切换身份模式

成功后先 stderr 警告再 stdout 回执(管道安全:数据行仍在 stdout):

```
身份模式已切换为 独立(每 planet 各自身份),planet 下次运行时生效
# 注意: 切换身份模式后部分或全部 planet 将变更节点地址,受影响网络的成员资格需重新授权   ← stderr
```

切换被 409 拒绝时如实透传 API 错误(`error 409: <message>` 走 stderr,
stdout 为空),不做任何变更且**不打印**上述警告(警告只在成功后输出)。

`-j`:切换响应体 pretty JSON;警告照走 stderr。

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
| `-j` 无 `--yes` 执行 remove / install ◇ | `error: --json requires --yes for remove (confirmation prompt would corrupt stdout)` / `error: --json requires --yes for install when version differs (<old> -> <new>)` | 2 |

通用 API 错误格式:`error <code>: <message>`(409 定位类由 CLI 改写为上表指引样式,
跟随 CLI 语言)。

## 退出码总表

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 运行错误(API 4xx/5xx、IO) |
| 2 | 用法错误、planet 歧义、service 组未以 root 运行 |
| 3 | daemon 不在线 |
| 130 | 用户中止(remove / install 交互确认拒绝) |
