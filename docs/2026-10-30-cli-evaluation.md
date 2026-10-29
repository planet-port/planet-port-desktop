# CLI 多视角功能校验报告(2026-10-30)

四个独立评估者以不同使用者身份,各自对独享 mockd 实例(端口 9981–9984)实测
`planet-port` CLI v0.1.0,按五个维度评分:易用性 / zerotier-cli 平替 / 反馈信息 /
精简度 / 日常覆盖。

| 使用者视角 | 总评 | 一句话画像 |
|---|---|---|
| 家庭用户(几台设备互通) | 3.0 | 主路径顺,但「最后一公里」人话反馈没做完 |
| 运维脚本作者(自动化/CI) | 3.3 | 退出码与 env 链扎实,`-j` 输出契约没被当真 |
| zerotier-cli 老用户(迁移) | 3.5 | 查询与 moon 链路可无缝平替,门槛在 `set` 与 `-j` |
| 网络管理员(多网络排障) | 3.6 | 命令面成熟、反馈层粗糙,排障闭环缺失 |

**共识优点**(四方一致):命令命名对齐 zerotier-cli、定位链
「显式 > env > 单 running 自动 > 歧义不猜」、退出码体系(0/1/2/3/130)严格执行、
stderr/stdout 纪律好、`events --follow` 与多 planet 生命周期管理是 zerotier-cli
没有的净增量、daemon 不在线的提示(`try: planet-port service start`,exit 3)
是全 CLI 错误反馈的标杆。

---

## 一、按严重度归并的问题清单

### P0:正确性 / 安全性(脚本会误判,规范与实现矛盾)

| # | 问题 | 提出者 | 详情 |
|---|---|---|---|
| 1 | **`-j` 变更类命令输出 `null`** | 家庭/运维/老用户 | `join/leave/orbit/deorbit/remove -j` 在 daemon 200 空响应体下 stdout 打 `null`,`jq -e` 假失败;对比 `set -j`/`add -j` 返回完整对象,同类操作不一致 |
| 2 | **`remove` 确认拒绝 exit 0** | 运维/老用户/管理员 | `echo n \| remove x` → `aborted` + exit 0,批量脚本把「没删」误判为「删了」;与选择器取消的 130 语义不一致 |
| 3 | **重名别名变更静默落点** | 管理员 | mockd 允许 `add --name company` 两次;之后 `join --planet company` 静默落到第一个,无歧义警告——多 planet 公司环境的事故源 |
| 4 | **`service status` 异常状态 exit 0 且忽略 `PLANET_PORT_URL`** | 家庭/运维 | env 指向 9981 时仍探测真实 daemon 的 9998,token 校验失败却返回成功码,排障误导 |

### P1:契约 / 口径不统一

| # | 问题 | 提出者 | 详情 |
|---|---|---|---|
| 5 | **`dump -j` 是 JSON 字符串而非对象** | 运维/老用户 | 人话文本被 quote 一层,`jq keys` 直接报错,与其它命令的 `-j` 破例 |
| 6 | **歧义报错两套话术** | 家庭/老用户/管理员 | `join/leave/dump` 报 `error 409: ... specify ?planet=`(HTTP 行话,源头 `daemon/src/api_net.rs:69,88`);`events` 报中文且无候选列表。CLI 层应统一改写为 `--planet` + 候选 |
| 7 | **COMMANDS.md 与 `--ui` 实现漂移** | 家庭/运维/管理员 | 文档承诺「TTY 自动弹选择列表」,实现要求显式 `--ui`(main.rs「仅显式传 --ui 且 stdin 为 TTY 时弹选择列表」)。二选一改掉 |
| 8 | **成功回执不带 planet 落点** | 家庭/管理员 | 定位链②③生效时 `200 join OK` 不说是哪个 planet,多 planet 下必须再查一次 |
| 9 | **时间戳全是原始 epoch** | 家庭/管理员 | `events` TS 毫秒 epoch、`planet show` revision、`share` expires_at 均不可判读;文本模式应本地时间化(`-j` 保留原值) |
| 10 | **`planet share` 输出 `http://127.0.0.1:...`** | 家庭/老用户/管理员 | 回环地址发给同事打不开,mockd 下还写死 9998;需可配置 base 或跨机场景明确告警 |
| 11 | **`join`/`leave` 非幂等** | 老用户/运维 | 重复 join 从 zerotier 的静默 200 变 `409` exit 1,老脚本与批量重跑场景断;建议幂等标志(`--if-not-joined` 类) |

### P2:信息缺口(功能增强方向)

| # | 问题 | 提出者 | 详情 |
|---|---|---|---|
| 12 | **网络成员在线列表缺失** | 家庭 | 组网后第一问「NAS 在不在、IP 多少」无命令能答,只能开 ZeroTier Central;`listpeers` 列的是根/中继,不是成员 |
| 13 | **单网络详情视图缺失** | 家庭/管理员 | `set dnsMode=custom` 后文本输出无变化,只有 `-j` 可见;缺 `network show <nwid>`(IP/DNS/routeViaZeroTier/managedRoutes 一体) |
| 14 | **排障闭环缺失** | 管理员 | 无 `service logs`;`dump` 只有计数无明细;授权等待(AUTH_REQUESTING)状态无行动提示 |
| 15 | **`set` key 不兼容 + 无 `get`** | 老用户 | `allowManaged/allowDNS/allowDefault` 肌肉记忆全断,报错不提示映射;缺 `get <nwid> <key>` |
| 16 | **`listpeers` 单路径** | 运维/老用户 | `path` 是单字符串,zerotier 是 `paths[]` 数组且带 lastTX/lastRX,多路径信息有损 |
| 17 | **`list`/`listnetworks` `-j` 完全同源** | 运维/老用户/管理员 | 文本模式分组 vs 平铺有差异,JSON 模式是同一数据的两个入口 |
| 18 | **批量语义缺失** | 管理员/运维 | 同一 nwid 加两个 planet 只能两条命令;无 `--planet all`、无 `--wait`(join→授权→OK 的收敛等待) |
| 19 | **`info` STATUS 列语义混叠** | 管理员 | 「实例运行态」(stopped)与「节点在线态」(ONLINE/OFFLINE)混在一列,`info` 与 `planet list`/`list` 口径一时对不上 |
| 20 | **mockd 回归盲区** | 老用户 | mockd 的 `remove` 还是旧语义(409 要求先 leave)、`add`/`moon import` 不校验输入;真实行为无法在 mockd 回归 |

### 已知取舍(评估者认可、无需改)

- `-j` 下错误走人话 stderr + 退出码(退出码承载语义);
- `events --follow` 断连不自动重连(exit 3);
- `leave` 未加入时报 404(比 zerotier 严但诚实);
- 表格表头与协议字段(`ONLINE`/`status`)保持英文,对齐 zerotier-cli 惯例。

---

## 二、四方 Top 改进诉求的交集

按「被几方独立提出 × 修复成本」排序,修复优先级建议:

1. **`-j` 契约统一**(问题 1、5):变更类空响应输出真实对象、`dump -j` 结构化——
   三方点名,自动化用户的信任基础,改动集中在 CLI 层。
2. **退出码修正**(问题 2、4):`remove` 拒绝非零、`service status` 异常非零——
   小改动,消除脚本误判。
3. **报错口径与文档对齐**(问题 6、7):`?planet=` → `--planet` + 候选;COMMANDS.md
   的交互选择章节与实现对齐。
4. **成功回执带落点 + 时间本地化**(问题 8、9):`company(2187945993) 200 join OK`
   样式;文本模式 epoch → 本地时间。
5. **重名别名设防**(问题 3):`add` 拒绝重名,或变更类遇重名报候选。

功能增强(问题 12–18)建议按使用量排期:网络成员列表 → `network show` →
`service logs` → `set` 兼容别名 + `get`。

## 三、原始报告

四份完整报告存于会话记录,本文件为归并后的结论版;如需原文可从评估 agent
输出归档(`docs/` 下不再重复存放)。
