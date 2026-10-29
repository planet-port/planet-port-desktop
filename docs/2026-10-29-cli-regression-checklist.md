# CLI 手工回归清单

对 mockd 与 planet-portd 均可执行(两者暴露同一套 REST API;对 planet-portd 执行时
请清掉或尊重 port 文件,CLI 会自动指向真实 daemon)。

## 与 mockd 的行为差异(对真实 daemon 验证时注意)

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

## 清单(mockd 种子:official running / company running / home-lab stopped)

以下为对 mockd 逐条实测结果,作为回归基准;对 planet-portd 执行时命令一致,
注意上节行为差异:

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
