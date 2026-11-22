# CLI 手工回归清单

对 planet-portd(真实 daemon)执行;CLI 经 port 文件自动定位 daemon,
显式指定时用 `PLANET_PORT_URL`。变更类命令请用测试 planet / 测试网络,避免污染实网。

## 对真实 daemon 验证时注意

- planet 数据为实网:planet 文件解析自真实 world,实例连真实根服务器;
- `planet add` 的 URL 源可用(下载后走与本地文件同一套校验),容器导入 `501 notImplemented`;
- `dns` 模式(`set` 的 `dnsMode`/`customDns`)为 `network`/`custom` 且算出的
  servers 非空时,netconf 应用阶段会用 `networksetup` 把 DNS 写进主网络服务,
  servers 为空(不再托管)或 leave 网络 / 停止或删除 planet 时还原为自动获取;
  失败只记事件,不影响网络数据面;
- `set` 校验更严:`routeViaZeroTier` 仅接受 `0|1`(其它值返回 400,退出码 2),
  `dnsMode` 仅接受 `none|network|custom`;`routeViaZeroTier` 于下次 netconf
  (控制器下发网络配置)时生效,false 时清空 managed routes 后再 apply,
  已装上的路由不会立即回撤。

## 清单

以下为逐条实测结果,作为现行回归基准,随版本演进持续更新;执行时注意上节真实 daemon 的行为特性:

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
| `planet list/show/rename/export/share` + `planet add/remove`（顶层 add/remove 已迁入 planet 子命令;start/stop 已移除,现并入 planet enable/disable） | 各自预期 | 全部通过;交互确认拒绝时 exit=130(2026-10-30 修复,原为 0);export stdout/`-o` 均可(planet remove 为自动 leave + 删除语义) | 通过 |
| `use company` + eval 后 `join` | 定位链② 生效 | `PLANET_PORT_PLANET=company join` 直接落到 company,无歧义报错 | 通过 |
| `use 不存在` | exit=2,stdout 无输出 | `error: planet "nosuchplanet" not found`,stdout 0 字节,exit=2 | 通过 |
| `events [--planet] [--follow]` | 表格/增量输出 | 过滤生效;`--follow` 每 2s 增量输出新事件 | 通过 |
| 停掉 official 后 `join`(不带 planet) | 定位链③ 自动选 | 仅 company running 时自动落到 company,`200 join OK` | 通过 |
| daemon 未启动 `info` | exit=3 + 提示 | `planet-portd is not running (try: planet-port service start)`,exit=3 | 通过 |

## 2026-10-30 修复批次(评估报告驱动)

| 命令 | 预期 | 实测 | 结论 |
|---|---|---|---|
| `join/leave/orbit -j` | 真实响应体 | `{"result":"ok"}`(幂等时附 `"idempotent":true`);planet remove/deorbit 代 `{"result":"ok"}` | 通过 |
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
| `planet remove` 确认拒绝 | exit=130 | `已取消` + 130 | 通过 |
| join/leave 幂等(daemon 侧) | 重复操作静默成功 | 新二进制安装后实测:重复 join 与 leave 未加入网络均 `200 ... OK` + `"idempotent":true`,exit=0 | 通过 |

## 2026-10-30 追加修复(现场问题驱动)

| 命令 | 预期 | 实测 | 结论 |
|---|---|---|---|
| `list` / `listnetworks` 多 managed IP | 列内逐行堆叠全量展示 | 3 IP(含超长畸形 CIDR)实测:续行对齐 MANAGED-IPS 列起点,无截断 | 通过 |
| 全命令 `-j` 覆盖 | stdout 纯 JSON,机器可解析 | 14 条命令(info/list/dump/events/use/planet 组/service status/get/network show 等)连发实测全部 exit 0 且 JSON 合法;`use` 与 service 四动作补齐 `-j`(service 动作需 root,代码路径回归) | 通过 |

## v0.4 命令面(v0.3/v0.4 新增回归点)

以下为 v0.3(identity)/ v0.4(set-relay、enable/disable、info RELAY 列)新增
命令面的回归点,**尚未逐条实测**,回填实测结果后并入上方基准表;
命令口径与 [COMMANDS.md](../COMMANDS.md) / `crates/cli` 当前实现对齐:

| 命令 | 预期 |
|---|---|
| `identity`(缺省)/ `identity list` | 三段 stdout:身份模式行(`身份模式: 共享(全部 planet 复用全局身份)`)、全局身份行(未建立时提示 `可 identity import 导入官方身份`)、每 planet 一行 `名称 / node id / running\|stopped`;`-j` 输出完整 IdentityOverview |
| `identity import --path <identity.secret>` | 先打只读预检,通过后正式导入;stdout 回执依次为:`已导入全局身份: <node>` → 官方客户端当前 planet 的登记/缺失指引 → 模式切换回执 →(independent 且换用官方身份时)`planet <名> 已换用官方身份` →(有被取代 id 时)控制器侧成员记录清理指引 |
| `identity import`(旧 daemon,无 preflight 路由) | 版本错配拦截:`error: daemon 版本过旧不支持导入预检,请先升级 planet-portd 再执行 identity import`,exit=2,不触发正式导入 |
| `identity use shared` ↔ `identity use independent` | stderr 警告 `…将变更节点地址,成员资格需重新授权`,stdout `身份模式已切换为 …,planet 下次运行时生效`;切换被 409 拒绝时如实报错,不做任何变更且不打印警告 |
| `planet set-relay <p> --relay :9443` / `--relay 9443` / `--relay <ip>:9443` | 三种形态回执均为 `<label> 200 set-relay OK`;`:端口` 与裸端口展开为首根 IP+所给端口,完整形原样;回显链:`info` RELAY 列 = `planet show` 的 transport/relay 行 = 所设完整地址;再执行不带 `--relay` 清除,两处回显回到 `—` |
| `planet enable <p>` / `planet disable <p>` | disable 后 `planet list` STATUS 翻为 `disabled`、`info` 状态翻为 `未连接`,在跑实例立即停;enable(有本地网络)按派生规则自动拉起回到 `running`/`已连接`;daemon 重启后按派生规则(enabled ∧ net_local 非空)恢复,disabled 的不拉起 |
| TCP 兜底实测路径(家宽 UDP 被滤环境) | 配置 relay 且 relay 可达时:`planet show` transport 行 `tcp=established(<relay地址>)`,`listpeers` 根条目 PATH 显示 `(relay)`,连接实际走 TCP 隧道;UDP 恢复后回落 UDP 主通道 |
| `events --planet <p> --limit N` | 输出条数按 N 截取(缺省 40);`--follow` 每 2s 增量,Ctrl-C 退出 |
| `service logs --lines N --follow` | tail 末 N 行(缺省 50)/持续跟踪;日志文件不存在 exit=1 |
| `planet remove <p> -j`(无 `--yes`) | 拒绝执行:`error: --json requires --yes for remove (confirmation prompt would corrupt stdout)`,exit=2(交互确认拒绝才是 exit=130) |
| `install -j`(版本差异且无 `--yes`) | 同款约束:`error: --json requires --yes for install when version differs …`,exit=2 |

## 网络启用禁用(2026-11-22 命令面;统一调试阶段执行)

- [ ] disable → listnetworks 出 DISABLED 行(name 用快照,动态字段 —)→ enable 恢复原配置(routeViaZeroTier/dnsMode 不变)
- [ ] disable 后 net_local 清空 → planet idle 停机;enable 自动拉起并 re-join
- [ ] 重复 disable 幂等成功;未登记网络 disable/enable 404;已加入态 enable 幂等成功
- [ ] join 命中禁用网络 → 自动恢复(等价 enable,不报错)
- [ ] STOPPED planet:disable 仅记录迁移(按名定位退化为仅 nwid);enable 按禁用区 name 快照定位
- [ ] leave 清两区(net_local 与 net_disabled),不强制 planet running
- [ ] planet disabled 下 enable/disable 报 409 planetDisabled
- [ ] `planet add`/`planet remove` 新路径;旧顶层入口报未知命令
- [ ] share:终端二维码(扫出 16hex 纯文本)+ 网络 ID 已复制;无显示环境降级提示不报错
- [ ] 网络命令定位规范:单登记自动选中;≥2 登记无 --planet/use 报 409 列候选
