# CLI 手工回归清单

对 planet-portd(真实 daemon)执行;CLI 经 port 文件自动定位 daemon,
显式指定时用 `PLANET_PORT_URL`。变更类命令请用测试 planet / 测试网络,避免污染实网。

## 对真实 daemon 验证时注意

- planet 数据为实网:planet 文件解析自真实 world,实例连真实根服务器;
- `add` 的 URL 源可用(下载后走与本地文件同一套校验),容器导入 `501 notImplemented`;
- `dns` 模式(`set` 的 `dnsMode`/`customDns`)为 `network`/`custom` 且算出的
  servers 非空时,netconf 应用阶段会用 `networksetup` 把 DNS 写进主网络服务,
  servers 为空(不再托管)或 leave 网络 / 停止或删除 planet 时还原为自动获取;
  失败只记事件,不影响网络数据面;
- `set` 校验更严:`routeViaZeroTier` 仅接受 `0|1`(其它值返回 400,退出码 2),
  `dnsMode` 仅接受 `none|network|custom`;`routeViaZeroTier` 于下次 netconf
  (控制器下发网络配置)时生效,false 时清空 managed routes 后再 apply,
  已装上的路由不会立即回撤。

## 清单

以下为逐条实测结果,作为回归基准;执行时注意上节真实 daemon 的行为特性:

| 命令 | 预期 | 实测 | 结论 |
|---|---|---|---|
| `info` / `info -j` | 3 planet 聚合表 / JSON | 表格三行;`-j` 返回 `{"planets":[...]}` | 通过 |
| `listnetworks` / `--planet company` | 聚合 + PLANET 列 / 过滤 | 聚合两行;过滤后仅 company 网络,PLANET 列显示 `2187945993` | 通过 |
| `listpeers` / `listmoons` | 分组表 / moon 表 | 按 planet 分组;moons 表含 SEED/SOURCE/CACHED 列 | 通过 |
| `join`(不带 planet) | exit=2,候选列表 | `error 409: multiple running planets` + official/company 候选,exit=2 | 通过 |
| `join --planet company` | 200 join OK | 首次 join 新 nwid 为 `200 join OK`;重复 join 幂等成功(`{"result":"ok","idempotent":true}`) | 通过(daemon 侧需新版二进制重启后回归) |
| `set ... routeViaZeroTier=1 dnsMode=custom customDns=...` | 200 set OK;`-j` 复查生效 | `200 set OK`;复查 `routeViaZeroTier:true`、`dnsMode:"custom"` | 通过 |
| `set ... bogus=1` | exit=2 | `error: unknown key "bogus"; valid: ...`,exit=2 | 通过 |
| `orbit` / `deorbit` / `moon import` | 200/201 OK;listmoons 复查 | orbit/deorbit `200 ... OK`,moon import `201`;listmoons 增删一致 | 通过 |
| `dump --planet official` | 单 planet 文本 | 输出 networks/peers/events 概要;不带 `--planet` 时报候选 | 通过 |
| `planet list/show/rename/export/share` + 顶层 `add/remove/start/stop` | 各自预期 | 全部通过;交互确认拒绝时 exit=130(2026-10-30 修复,原为 0);export stdout/`-o` 均可(remove 为自动 leave + 删除语义) | 通过 |
| `use company` + eval 后 `join` | 定位链② 生效 | `PLANET_PORT_PLANET=company join` 直接落到 company,无歧义报错 | 通过 |
| `use 不存在` | exit=2,stdout 无输出 | `error: planet "nosuchplanet" not found`,stdout 0 字节,exit=2 | 通过 |
| `events [--planet] [--follow]` | 表格/增量输出 | 过滤生效;`--follow` 每 2s 增量输出新事件 | 通过 |
| 停掉 official 后 `join`(不带 planet) | 定位链③ 自动选 | 仅 company running 时自动落到 company,`200 join OK` | 通过 |
| daemon 未启动 `info` | exit=3 + 提示 | `planet-portd is not running (try: planet-port service start)`,exit=3 | 通过 |

## 2026-10-30 修复批次(评估报告驱动)

| 命令 | 预期 | 实测 | 结论 |
|---|---|---|---|
| `join/leave/orbit -j` | 真实响应体 | `{"result":"ok"}`(幂等时附 `"idempotent":true`);remove/deorbit 代 `{"result":"ok"}` | 通过 |
| `dump -j` | 结构化对象 | `planet/networks/peers/events` 四键,`jq keys` 可用 | 通过 |
| 双 running 下 `join`(无 --planet) | 人话歧义报错 | 本地化 `--planet` 指引 + 候选列表,exit=2;en/zh-Hant 同款 | 通过 |
| 变更回执 | 带 planet 落点 | `gozi25(3530253632) 200 join OK` 样式 | 通过 |
| `events` / `planet show` / `planet share` | 时间本地化 | TS/revision/expires_at 均本地时间,`-j` 保留 epoch | 通过 |
| `planet share` | URL 改写 | URL 用 CLI base;回环地址时 stderr 提示 | 通过 |
| `service status` | 退出码 | 在线 0 / token 失败 1 / 不在线 3;尊重 `PLANET_PORT_URL` | 通过 |
| `service logs` | tail 日志 | `--lines`/`--follow` 可用,缺文件 exit=1 | 通过 |
| `get <nwid> <key>` | 单值读取 | 字符串裸输出;旧 key(allowDNS 等)提示映射 | 通过 |
| `network show <nwid>` | 详情视图 | planet/status/IP/routes/DNS/routeVia 一屏 | 通过 |
| `list -j` | 分组结构 | `planets[]` 各带 `networks[]`;listnetworks -j 保持平铺 | 通过 |
| `info` STATUS | 三态对齐 list | 已连接/连接中/未连接(与 list 同口径) | 通过 |
| `remove` 确认拒绝 | exit=130 | `已取消` + 130 | 通过 |
| join/leave 幂等(daemon 侧) | 重复操作静默成功 | 新二进制安装后实测:重复 join 与 leave 未加入网络均 `200 ... OK` + `"idempotent":true`,exit=0 | 通过 |

## 2026-10-30 追加修复(现场问题驱动)

| 命令 | 预期 | 实测 | 结论 |
|---|---|---|---|
| `list` / `listnetworks` 多 managed IP | 列内逐行堆叠全量展示 | 3 IP(含超长畸形 CIDR)实测:续行对齐 MANAGED-IPS 列起点,无截断 | 通过 |
