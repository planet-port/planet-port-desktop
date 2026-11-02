# 身份管理层设计(全局身份 / 每 planet 独立身份 / 官方身份迁移)

日期:2026-11-02
状态:已与用户逐节确认

## 背景与动机

当前 daemon 阶段为每 planet 独立身份(身份落在各 planet dataDir,由 ZeroTier 核心经 state 回调惰性生成),daemon/API/CLI 三层均无任何身份读取、导入、导出或切换能力。本设计新增统一身份管理层,覆盖三个需求:

1. **官方身份迁移**:把 ZeroTier 官方软件的既有身份导入本软件,免重新入网授权,支持设备整体搬家。
2. **全局身份 / 每 planet 独立身份双模式**:每个 planet 可独立绑定"全局身份"或"自己的身份"。
3. **`identity` CLI 指令**:列出当前身份模式与身份列表,支持切换绑定、导入官方身份。

> 本设计推翻早前"身份管理/迁移命令不暴露给 CLI"的约束( docs/superpowers/specs/2026-10-28-cli-api-design.md:199 ),以用户 2026-11-02 决定为准。本设计仅覆盖桌面端。

## 用户拍板的关键决策

- 导入动机:免重新入网授权 + 设备整体搬家,两者都要。
- 导入入口:CLI 子命令(不与 planet 创建绑死)。
- 身份来源:按平台自动探测官方安装目录,探测不到回退显式 `--path`。
- 导入成功后自动 **stop + disable** 官方服务(不卸载),消除重启后同身份双开互踢(官方 issue #2433)。
- 模式粒度:**每 planet 绑定**(非全局模式位、非切换时迁移存量)。
- 导入的身份默认落**全局身份**(config 根)。
- **独立身份与全局身份互不继承**:planet 切到 own 绑定后用新生成的身份(若盘上留有旧 own 身份则复用,不重新生成);原全局身份不带过来。

## 数据模型与存储布局

config 根新增全局身份文件(单一事实源,均 0600):

```
<config_dir>/
├── identity.public / identity.secret      # 新增:全局身份
├── planets/<worldId>.bin
├── registry.json                          # 每 planet 条目新增 identity 字段
└── data/<worldId>/                        # planet dataDir(现状不变)
    ├── identity.public / identity.secret  # 仅 own 绑定的 planet 存在
    └── ...
```

registry.json 每 planet 条目新增字段:`"identity": "own" | "global"`。

- 存量 planet 无此字段 → 视为 `own`(完全兼容现状,无迁移)。
- 新建 planet 默认绑定:存在全局身份 → `global`,否则 → `own`。
- 全局身份文件不存在时,首个 `global` 绑定的 planet 启动时由核心惰性生成并经 state_put 落到 config 根(复用现有路径与 0600 语义)。
- own 身份完全由 planet 自己管理:生成、装载、保存在其 dataDir 内;切到 global 后旧文件原样留盘(可切回复用);仅 `DELETE /planets/{id}` 删 dataDir 时消失。
- `DELETE /planets/{id}` 的 `remove_dir_all` 不需改动:全局身份在 config 根,天然删不到。

## zt-core 改动(方案 A:路径映射注入)

- `NodeParams`( crates/zt-core/src/config.rs:15-21 )新增 `identity_dir: Option<PathBuf>`。
- `state.rs` 的 `file_for`( crates/zt-core/src/state.rs:32-43 ):`IdentityPublic/IdentitySecret` 在 `identity_dir` 为 Some 时映射到该目录下,其余 state 对象映射不变。
- zt-core 不感知"模式"概念,只认路径;daemon 依据 registry 绑定决定注入与否。
- 核心只在启动(`ZT_Node_new` + 首次 processBackgroundTasks)时读/生成身份,因此切换绑定 = planet stop → 改 registry → start 即生效。

## REST API(均走现有 X-ZT1-Auth 鉴权)

| 端点 | 作用 |
|---|---|
| `GET /identity` | 全局身份(是否存在、node id)+ 各 planet 绑定与 node id |
| `POST /identity/import` | body `{ path?: string, force?: bool }`;path 缺省自动探测 |
| `PUT /planets/{id}/identity` | body `{ identity: "global" \| "own" }`;planet 须 stopped,否则 409 |

## 导入处理链

1. 定位身份文件:path 显式给出则用之;否则按平台探测:
   - macOS:`/Library/Application Support/ZeroTier/`
   - Linux:`/var/lib/zerotier-one/`
   - Windows:`C:\ProgramData\ZeroTier\One\`
   - 探测不到 → 报错要求显式 path。
2. 校验:`identity.secret` 与 `identity.public` 都须存在;解析 secret 首字段(冒号前 10 位 hex)得节点地址(参考 crates/pocs/src/bin/poc3_multi.rs:325-329 的 `identity_addr` ),与 public 中地址一致才接受。
3. 已存在全局身份且未 `force` → 拒绝(覆盖会改变所有 global 绑定 planet 的节点地址)。
4. **停 + 禁官方服务**(见下节);停止失败 → **中止导入**,报错让用户手动处理。
5. 写 `<config>/identity.*`(0600),不触碰任何 planet。
6. 处置结果随响应返回。

## 官方服务停用(daemon 新增 service_control 模块)

trait + 平台实现,职责:检测官方服务是否存在/运行、stop、disable 自启,不卸载。

| 平台 | stop + disable |
|---|---|
| macOS | `launchctl bootout system/com.zerotier.one` + `launchctl disable system/com.zerotier.one` |
| Linux | `systemctl stop zerotier-one && systemctl disable zerotier-one` |
| Windows | `sc stop ZeroTierOneService` + `sc config ZeroTierOneService start=disabled` |

结果三态:`stopped_and_disabled` / `not_installed` / `failed`(附原因)。官方未安装时跳过停用、正常导入(身份文件可能来自其它机器)。

## CLI

```
planet-port identity                  # 缺省 list:全局身份 node id + 各 planet 绑定/node id/运行态
planet-port identity import [--path <identity.secret 路径>] [--force]
planet-port identity use <planet名> global|own   # planet 运行中则报错提示先 stop
```

- `identity use` 切到 own 时输出警告:节点地址将变化、该 planet 网络成员资格失效需重新授权。
- own 绑定启动时若 dataDir 已留有旧 own 身份则直接复用,不重新生成。

## 切换语义

- `own → global`:须 stopped;dataDir 旧 `identity.*` 留盘不删;节点地址变为全局身份地址。
- `global → own`:须 stopped;启动时核心发现 dataDir 无身份则新生成,有则复用留盘旧身份;独立身份不继承全局身份。

## 边界处理

- planet 运行中切换绑定 → 409,CLI 提示先 `planet stop`。
- 探测不到官方目录 → 报错要求 `--path`。
- 已有全局身份且未 `--force` → 拒绝覆盖。
- secret/public 缺失、解析失败、地址不一致 → 拒绝导入。
- 身份冲突兜底:现有 `IdentityCollision` 事件通道( crates/daemon/src/runtime.rs:77 )不变。

## 验证方式

不写测试用例。`cargo build` + `cargo clippy` 全平台通过(含 zigbuild 交叉编译);macOS 本机对官方客户端做一次真机 e2e:导入 → 官方服务已停+禁自启 → planet 启动沿用官方节点地址 → `identity use` 往返切换身份不丢。
