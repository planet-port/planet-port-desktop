# planet-portd 一期(daemon 本体 + CLI 对接)实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用真实 zt-core 实现替换 mockd 语义,交付可日常使用的 `planet-portd` daemon 与 CLI `service` 组命令、端口文件协商。

**Architecture:** 单进程多 Node:每 planet 一个 `NodeHandle`(独立驱动线程)+ 一个 `DevicePlane`(多网络设备表),axum+tokio REST 层经快照/命令通道访问实例,永不触碰 core 锁。存储集中在 `config_dir()`(registry.json + planets/ + data/ + token/port 文件)。

**Tech Stack:** 既有 zt-sys/zt-core/api/cli crate;新增 `crates/daemon`(bin 名 planet-portd,axum 0.8 栈与 mockd 对齐);CLI 侧 clap service 子命令 + launchctl。

**权威文档(实现时必读):**
- 契约:`docs/superpowers/specs/2026-10-28-cli-api-design.md` §4(REST 端点全集)
- 本期设计:`docs/superpowers/specs/2026-10-28-daemon-one.md`
- 行为基准:mockd 源码 `crates/mockd/src/main.rs`(错误码/错误体/定位链/字段名以它为准,CLI 已按它联调)

## Global Constraints

- **不修改 mockd**;`crates/api` 仅允许改 `config_dir()` 解析(Task 2)与新增类型字段(禁止改既有字段名/语义,CLI/GUI 契约已冻结)
- zt-core 架构红线(违反 = 审查直接打回):回调 glue 只做拷贝+channel send;所有 sink 调用发生在驱动线程、不取用户锁长持;FrameOut(核心→设备)与 FrameIn(设备→核心)方向不可混;shutdown 顺序 = 设备 teardown(join reader)→ running=false → wake → Cmd::Stop → join 驱动线程 → ZT_Node_delete
- FFI 边界禁 panic/unwrap 穿越;`ZT_Node_peers`/`networks` 返回的查询结果必须 `ZT_Node_freeQueryResult` 且拷贝后立即释放
- 存储/端口/token 路径一律经 `planet_port_api::{config_dir, port_file, token_file}`;不引入测试框架;workspace `cargo build` + `cargo clippy --workspace` 零警告
- git 一律 `--no-pager`;提交信息 Conventional Commits 中文 + Body,禁止任何 Co-authored-by/AI 署名
- 事件 ring 容量 1024;错误体 `{"error","message","candidates?"}` 与 mockd 逐字段一致
- macOS 专属命令(ifconfig/route/launchctl)按 macOS 语法写;sudo 操作只出现在 service 组与实测步骤,daemon 本体不做提权

---

### Task 1: zt-core 加固与 peers/orbit 扩展

**Files:**
- Modify: `crates/zt-core/src/node.rs`(peers/orbit/deorbit、catch_unwind、POLLERR/POLLNVAL)
- Modify: `crates/zt-core/src/device.rs`(Drop 兜底、reader 退避重试、多网络设备表)
- Modify: `crates/zt-core/src/routes.rs`(一律安装策略)
- Modify: `crates/zt-core/src/config.rs`(NetConfig 增 is_private)、`crates/zt-core/src/lib.rs`
- Create: `crates/zt-core/LICENSE-MPL.txt`;Modify: `scripts/bootstrap.sh`(增拷 `LICENSE-MPL.txt` 入 Core/)

**Interfaces(后续任务依赖,签名精确):**
- `pub struct PeerInfo { pub address: u64, pub role: PeerRole, pub version: Option<(i32, i32, i32)>, pub latency_ms: Option<i64>, pub paths: Vec<PeerPath> }`
- `pub struct PeerPath { pub addr: std::net::SocketAddr, pub active: bool, pub trusted: bool }`(active 判定 = `expired == 0 && lastReceive != 0`)
- `pub enum PeerRole { Leaf, Moon, Planet }`
- `impl NodeHandle { pub fn peers(&self) -> Vec<PeerInfo>; pub fn orbit(&self, moon_world_id: u64, seed: u64) -> Result<(), NodeError>; pub fn deorbit(&self, moon_world_id: u64) -> Result<(), NodeError> }`(orbit/deorbit 经驱动线程 Cmd 通道,与 join 同模式)
- `NetConfig` 增 `pub is_private: bool`(from_zt 按 `Core/include/ZeroTierOne.h` 的 `ZT_VirtualNetworkType` 枚举映射,实现时查头文件确认取值)

- [ ] **Step 1: peers/orbit/deorbit 封装**

peers() 照 networks() 模式:`ZT_Node_peers` → 逐条拷贝(paths 数组按 `ZT_MAX_PEER_NETWORK_PATHS=64` 遍历,`lastSend==0 && lastReceive==0` 的空槽跳过)→ `ZT_Node_freeQueryResult`。version 三元组任一 <0 即 None;latency <0 即 None。orbit/deorbit:Cmd 枚举加 `Orbit(u64,u64)`/`Deorbit(u64)`,驱动线程调 `ZT_Node_orbit`/`ZT_Node_deorbit`(头文件 1950/1960 行),rc != OK 时如何上报:Cmd 改带 `Sender<Result<(),NodeError>>` 应答(Join/Leave 一并统一为带应答,调用方才能返回真实错误)。

- [ ] **Step 2: 加固四项**

1. `run_driver` 内全部 sink 调用(sink.on_event/on_net_config/on_frame/local_ipv4s)包 `std::panic::catch_unwind(AssertUnwindSafe(...))`,Err → 推 `NodeEvent::Fatal("sink panic: ...")` + 置 running=false 退出驱动线程(不杀进程);
2. reader 线程:读错误不再永久退出,改为 1s 退避重试(设备 fd 失效错误如 EBADF/ENXIO 连续 3 次才放弃并打印);`spawn_reader` 的 JoinHandle 错误不再 `.ok()` 吞,记日志;
3. 驱动线程 poll 返回后检查 `POLLERR|POLLNVAL`(UDP fd 或 wake fd 异常 → 打日志 + 事件,防忙转);
4. `DevicePlane` 落 `impl Drop`(未 teardown 时尽力:停 reader、删路由、销毁设备);`NodeHandle` 落 `impl Drop`(running 仍 true 时 debug_assert + 尽力 shutdown)。

- [ ] **Step 3: 路由策略改「一律安装」**

routes.rs 现有「目标被其它接口占用则跳过」逻辑删除:managed routes 一律安装(默认路由 0.0.0.0/0、::/0 仍跳过)。`RouteApplier::apply` 返回本次新安装的 `Vec<ManagedRoute>`(供 daemon 记事件,共存检测在 daemon 层做全局扫描,zt-core 不做跨实例判断)。

- [ ] **Step 4: 多网络设备表**

DevicePlane 从单设备改为 `devices: HashMap<u64 /*nwid*/, NetDevice>`(每网络一张 feth + 各自 reader 线程 + applied 状态);`apply(cfg)` 按路由 `cfg.nwid` 找到/建设备;`send_frame(nwid, ...)` 按 nwid 发;`teardown(nwid)` 拆单网络设备、`teardown_all()` 全拆(Drop 用)。POC 二进制(poc2/poc3)编译适配即可,行为不变(单网络场景等价)。

- [ ] **Step 5: MPL 占位与红线文档**

- `scripts/bootstrap.sh` 裁剪清单增 `cp "$SRC/LICENSE.txt" "$CORE_DIR/"`(上游 MPL-2.0 全文即随 Core/ 分发的占位);`crates/zt-core/LICENSE-MPL.txt` 写一行占位说明 + 指向 Core/LICENSE.txt(MPL 源码使用声明,分发阶段补完整 NOTICE)
- zt-core 各模块 doc 注释补「架构红线」小节:回调 glue 只拷贝+send、sink 必须在驱动线程执行、FrameIn/FrameOut 方向、shutdown 硬顺序、peers/networks 查询结果必须 freeQueryResult

- [ ] **Step 6: 验证 + 提交**

```bash
cargo build --workspace && cargo clippy --workspace 2>&1 | tail -5   # 零警告
cargo run -p pocs --bin poc1_control -- --duration 10               # 行为不变,干净退出
git --no-pager add -A && git commit -m "feat(core): peers/orbit 封装与 POC 加固清单落地
..."
```

---

### Task 2: planet-portd 骨架(config_dir 解析、registry、token/port、axum+auth、/version)

**Files:**
- Create: `crates/daemon/Cargo.toml`、`crates/daemon/src/main.rs`、`crates/daemon/src/registry.rs`、`crates/daemon/src/events.rs`
- Modify: `Cargo.toml`(members 加 `"crates/daemon"`)、`crates/api/src/lib.rs`(config_dir 解析)

**Interfaces:**
- `api::config_dir()` 新解析:①env `PLANET_PORT_CONFIG_DIR` ②`/Library/Application Support/planet-portd` 已存在则用之 ③`dirs::config_dir()/planet-portd`(回退,开发期)。**原因:service 模式以 root 跑,HOME=/var/root,必须与用户 CLI 解析到同一目录**
- `pub struct RegistryEntry { pub id: String /*worldId 十进制*/, pub world_id: u64, pub name: Option<String>, pub icon: Option<String>, pub color: Option<String>, pub source: String, pub file_size: u64, pub revision_time_ms: u64, pub imported_at_ms: u64, pub last_node_id: Option<String>, pub data_dir: PathBuf /*绝对*/, pub port: u16, pub running: bool, pub net_local: HashMap<String /*nwid hex*/, NetLocal>, pub moons: Vec<MoonRecord> }`
- `pub struct NetLocal { pub route_via_zero_tier: bool /*默认 true*/, pub dns_mode: String /*"none"|"network"|"custom"*/, pub custom_dns: Vec<String> }`
- `pub struct MoonRecord { pub moon_id: String, pub seed: Option<u64>, pub source: String, pub cached_at_ms: u64 }`
- `pub struct Registry { pub planets: Vec<RegistryEntry> }` + `load(dir)/save(&self, dir)`(serde_json,tmp→rename 原子写,同 StateStore 模式)
- `pub struct Ring`(events.rs):`Mutex<VecDeque<EventEntry>>` 容量 1024,`push(planet,&str,&str)`、`tail(Option<&str> planet_filter, after: Option<u64>, limit: usize) -> Vec<EventEntry>`(语义同 mockd events handler:ts 降序取 limit)
- daemon `AppState { registry: Arc<Mutex<Registry>>, events: Arc<Ring>, planets: Arc<Mutex<HashMap<String, PlanetRuntime>>>>`(PlanetRuntime Task 4 定义,本任务先用空 map)

- [ ] **Step 1: crate 与 config_dir**

daemon Cargo.toml:axum/tokio/serde/serde_json/planet_port_api/zt-core(zt-core Task 4 才用,先引入无妨)。api lib.rs 改 config_dir()(见 Interfaces,含注释说明三级解析理由)。

- [ ] **Step 2: token/port 生成与 axum 骨架**

main.rs 启动序:
1. `--config-dir <path>` 可选参数(clap 或手解析均可,覆盖 config_dir);
2. 建目录;token 文件不存在 → 生成 32 字节随机(URL-safe base64 或 hex,`getrandom` 或读 `/dev/urandom`)写 token_file,权限 **0644**(对齐官方 macOS authtoken.secret 可读性:CLI 以普通用户运行需可读;API 仅绑 127.0.0.1,写入说明注释);存在则读入;
3. bind `127.0.0.1:9998`(DEFAULT_API_PORT),`AddrInUse` 则 +1 递增重试(上限 +100),实际端口写 port_file(0644),stdout 打印实际端口与 config_dir;
4. axum Router:**auth 中间件比照 mockd auth_mw,但校验 token 常量时间比较**(subtle 或手写 `constant_time_eq`,避免引入新依赖手写 6 行);`/version` 返回 `{"version": "0.1.0"}`;`/status`、`/planets` 先返回 registry 静态投影(无运行时信息:online=false、node_id=None)。

- [ ] **Step 3: 验证 + 提交**

```bash
cargo run -p daemon &  # 端口 9998(或递增);curl -s -H "X-ZT1-Auth: $(cat <config>/authtoken.secret)" http://127.0.0.1:<port>/version
# 预期 {"version":"0.1.0"};错 token → 401;杀进程重跑 → token/port 复用(不重新生成)
```

提交 `feat(daemon): planet-portd 骨架...`。

---

### Task 3: planet 生命周期(world 解析、import/CRUD/start-stop 占位、export/share)

**Files:**
- Create: `crates/daemon/src/world.rs`
- Modify: `crates/daemon/src/main.rs`(路由与 handlers,可拆 `src/api_planet.rs`)

**Interfaces:**
- `pub struct WorldInfo { pub world_id: u64, pub revision_ms: u64, pub kind: WorldKind /*Planet|Moon*/, pub roots: Vec<String> /*"ip:port"*/ }`
- `pub fn parse_world(bytes: &[u8]) -> Result<WorldInfo, String>`(上限 8MB)
- Planet 定位 `resolve_planet_id(registry, input) -> Option<String>`:依次尝试 id 精确、name 精确、按 u64 解析(十进制或 0x/纯十六进制)匹配 world_id

- [ ] **Step 1: world.rs 解析器**

World 二进制格式以 `Core/node/World.cpp` 的 `World::deserialize` 为准(实现时对照 C++ 源码逐字段平移):第 0 字节类型(1=planet 2=moon)、worldId 大端 u64、revision 时间戳大端 u64、随后根节点列表(每个根 = identity 字节串 + 稳定端点列表)。**只需要 roots 的端点地址**(identity 段长度按 World.cpp 的 Identity::deserialize 消费字节数跳过)。校验:类型合法、id/revision 非零、根数 ≥1、端点 sockaddr family 合法、总长不越界——错误返回中文明描述。解析通过 = 合法 planet 文件(import 的全部校验)。

- [ ] **Step 2: import/show/patch/delete/export/share**

全部比照 mockd 同名 handler 的错误码与响应结构(`{"planet": PlanetDetail}`、201 等):
- `POST /planets`:body `{source, name?, icon?, color?, dataDir?, port?}`(dataDir/port 为开发覆写,验证用;container 字段出现 → 501 `"notImplemented"`)。source 为本地路径或 URL(URL 用 `reqwest` blocking?**不引新依赖**:一期仅支持本地路径,URL 返回 400 `"unsupportedSource"`,README 注明)。流程:读文件(≤8MB)→ parse_world → worldId 已存在 → 409 `"planetExists"`;name 与现存重复 → 409 `"nameTaken"`;拷贝到 `<config>/planets/<worldId>.bin`;UDP 端口分配:body 指定则用之(校验未与其它 planet 冲突),否则从 9994 起找 registry 未占用且本机可 bind 的端口(临时 bind 探测后释放);写 registry;事件 `planetImported`;返回 PlanetDetail(roots 来自解析结果,status="stopped")。
- `GET /planets/{id}`(PlanetDetail)、`PATCH`(name/icon/color;name 撞车 409)、`DELETE`(running=true → 409 `"planetRunning"` 先 stop;data_dir 整目录删除前确认前缀在 config_dir 下,覆写的 dataDir 不删只摘记录)、`GET /planets/{id}/export`(原样回传 bin 文件)、`POST /planets/{id}/share`(token=随机 24 hex,内存表 `{token → (planet_id, expires_unix)}`,TTL body.ttlSeconds 默认 300;返回 ShareLink url=`http://127.0.0.1:<port>/share/<token>`)+ `GET /share/<token>`(一次性:命中且未过期回传 bin 并删除记录,否则 404;该路由不挂 auth 中间件——分享链接本身是凭证)。
- `start/stop` handler 本任务占位(start: 若无运行时则改 registry.running=true 并返回 501 `"runtimeNotReady"`;Task 4 换真实现)——**提交前确保占位注释清晰标注 TODO(task4)**。

- [ ] **Step 3: 验证 + 提交**

```bash
# 起 daemon 后:planet-port 的 planet import 命令(或 curl)导入 pocs/fixtures/planet.Gozi25
# 预期:201,worldId=3530253632,roots 含 124.222.91.224;重复导入 409 planetExists
# planets/planet.Gozi25 与源文件 shasum 一致;export 回传字节一致;share 链接 curl 下载成功且二次 404
```

提交 `feat(daemon): planet 导入与生命周期管理...`。

---

### Task 4: 实例运行时与网络面(start/stop、join/leave/set、networks、events)

**Files:**
- Create: `crates/daemon/src/runtime.rs`
- Modify: `crates/daemon/src/main.rs`(接线 start/stop/join/leave/set/networks/events/status 真数据)

**Interfaces:**
- `pub struct PlanetRuntime { pub planet_id: String, pub node: NodeHandle, pub plane: Arc<Mutex<DevicePlane>>, pub stopping: AtomicBool, pub events: Arc<Ring>, pub registry_entry_snapshot: ... }`
- `pub fn start_planet(state: &AppState, entry: &RegistryEntry) -> Result<(), String>` / `pub fn stop_planet(state: &AppState, id: &str) -> bool`(幂等)
- `struct DaemonSink`(impl NodeSink):持有 planet_id、`Arc<Mutex<DevicePlane>>`、`Arc<Ring>`、`stopping: Arc<AtomicBool>`、frame_in_tx

- [ ] **Step 1: PlanetRuntime 启停**

start_planet:
1. 构造 `DaemonSink`;`DevicePlane::new()`;
2. `NodeHandle::start(NodeParams { data_dir: entry.data_dir, planet_file: <config>/planets/<worldId>.bin, bind_port: entry.port }, sink)` ——在 `tokio::task::spawn_blocking` 内执行(start 含首次 processBackgroundTasks,可能毫秒级阻塞);
3. 存入 state.planets map;registry.running=true + last_node_id=address 十六进制;事件 `planetStarted`;
4. **daemon 启动恢复**:main 启动时对 registry 中 running=true 的逐个 start_planet(失败 → 事件 `planetStartFailed` + running 置 false 继续)。

stop_planet 顺序(zt-core 红线):置 stopping=true → `plane.lock().teardown_all()`(join 全部 reader)→ `node.shutdown()`(join 驱动线程 + delete)→ map 移除 → registry.running=false → 事件 `planetStopped`。

DaemonSink(sink 在驱动线程被调):
- `on_event`:NodeEvent → Ring push(kind="status";Online/Offline/Fatal 消息中文);Fatal 额外置自身 stopping;
- `on_net_config`:stopping=true 时直接返回;op=Down/Destroy → `plane.lock().teardown(nwid)` + 事件;Up/ConfigUpdate → 从 registry 取该 (planet,nwid) 的 NetLocal,**route_via_zero_tier=false 时把 cfg.routes 置空再 apply**(仅存地址不装路由)→ `plane.lock().apply(&cfg, &frame_in_tx)`;apply 后做冲突扫描(Step 3)→ 事件;
- `on_frame`:`plane.lock().send_frame(nwid, ...)`;
- `local_ipv4s`:`plane.lock().applied_v4s()`(全部网络设备的 v4 并集)。

- [ ] **Step 2: join/leave/set + networks/events handler**

- `POST /networks/{nwid}/join`:定位链(resolve_planet 同 mockd 语义:显式→校验 running;省略→单 running 自动/多 running 409 planetAmbiguous+候选/零 running 409 noPlanetRunning)→ nwid 解析(十六进制,长度 3~16,容错 `0x` 前缀)→ `node.join(nwid)`(带 Task 1 应答通道,Err → 500 `"joinFailed"` + 消息)→ net_local 无记录则建默认项 → 事件 `join`;
- `leave`:`node.leave(nwid)` + `plane.lock().teardown(nwid)` + 移除 net_local?**保留**(重新 join 恢复用户配置;mockd 是移除条目,networks 列表来自 node.networks() 所以 leave 后自然消失,net_local 留作配置缓存)→ 事件;
- `POST /networks/{nwid}`(set):只写 registry.net_local(mockd 同款字段校验);dnsMode 值合法校验;**set 生效**=若该网络已 Up,触发重新 apply(sink 侧无直接通道:简化为下一轮 ConfigUpdate 生效 + 文档注明;route_via 变更影响立即性的接受度在 README 标注)——一期接受延迟生效,handler 返回当前 NetworkEntry;
- `GET /networks`:聚合所有运行中 planet 的 `node.networks()` 快照 + net_local 合成 NetworkEntry(status 字符串 = NetStatus 映射,取 mockd 同款大写蛇形 "OK"/"REQUESTING_CONFIGURATION"/"ACCESS_DENIED"/...;kind = is_private ? "PRIVATE" : "PUBLIC";mac 格式 `ba:16:37:84:6b:53`;conflict 见 Step 3);
- `GET /events`:Ring.tail(mockd 同款 after/limit 语义);`GET /status`/`GET /planets` 换真数据:online=node.online()、node_id、core_version=NodeHandle::core_version()、latency_ms=peers 中 Planet 角色 latency 最小值(无则 None)、network_count=node.networks().len()。

- [ ] **Step 3: 冲突检测(事件化,不阻止)**

netconf apply 成功后:扫描 registry 全部 planet 的各网络 managed 网段(从运行时 networks() 快照取),与本次 cfg 的 assigned/routes 网段重叠且属不同 planet → 事件 `conflict`(消息含两侧 planet 名与网段)+ 该网络 conflict="subnet";两 planet 同网段同 IP → conflict="address"(同网不同 IP 不标)。冲突信息存运行时(PlanetRuntime 内 `Mutex<HashMap<nwid,String>>`)供 networks 合成。

- [ ] **Step 4: 离线验证 + 提交**

```bash
# 无 sudo 冒烟(daemon 以普通用户跑,start_planet 会因 feth 权限失败——事件 planetStartFailed,REST 不崩):
# join 返回 500 joinFailed 属预期;networks/events/status 结构与 mockd 一致
git --no-pager add -A && git commit -m "feat(daemon): 实例运行时与网络面..."
```

---

### Task 5: peers/moons/dump

**Files:**
- Modify: `crates/daemon/src/main.rs`(或 `src/api_misc.rs`)

**Interfaces:** 无新类型(复用 api::PeerEntry/MoonEntry)。

- [ ] **Step 1: /peers、/moons、moons 变更、/dump**

- `GET /peers`:各运行 planet 的 `node.peers()` → PeerEntry(address 十六进制 10 位、role 映射 Planet/Moon/Leaf 大写、version `maj.min.rev`(任一 -1 → None)、latency_ms、path=active 路径第一条 `udp/<ip:port>` 无 active 路径 → None=relay);
- `GET /moons`:registry.moons 投影 MoonEntry(cached = `data_dir/moons.d/<moon_id>.moon` 文件存在);
- `POST /moons`:source 本地路径 → 读文件(≤8MB)→ parse_world 校验 **kind 必须为 Moon**(是 planet → 400 `"notAMoon"`)→ 拷到 `<data_dir>/<worldId>/moons.d/<moon_id 十六进制16位>.moon`(daemon 持久化 moon 的唯一真源,核心 stateGet 自会命中)→ registry.moons 增记录(去重:同 moon_id 覆写)→ 事件;**planet 未运行 → 409 planetNotRunning**(moon 落的是该 planet 的 dataDir);
- `POST /moons/{moonid}/orbit`:moonid 十六进制解析;body.seed 解析 u64(可无 seed=0)→ `node.orbit(id, seed)`(Err → 500)→ registry 增/改记录 + 事件;**deorbit**:`node.deorbit(id)` + 删 moons.d 文件 + 删记录 + 事件;未运行 planet 走定位链同 join;
- `GET /dump`:定位链(单 planet,同 mockd)→ 文本输出:planet 头信息、networks(每网络 nwid/status/assigned)、peers(地址/role/latency)、本地配置(net_local)、events 尾部 10 条。

- [ ] **Step 2: 验证 + 提交**

```bash
cargo build --workspace && cargo clippy --workspace   # 零警告
# 冒烟:peers/moons/dump 对运行中(或空)daemon 返回结构正确
```

提交 `feat(daemon): peers/moons/dump 端点...`。

---

### Task 6: 信号处理、CLI service 组、端口文件、README

**Files:**
- Modify: `crates/daemon/src/main.rs`(SIGINT/SIGTERM 优雅退出)
- Modify: `crates/cli/src/main.rs`、`crates/cli/src/cmd/service.rs`(新建)、`crates/cli/src/cmd/mod.rs`、`crates/cli/src/client.rs`
- Modify: `README.md`

**Interfaces:**
- CLI 新命令组:`planet-port service <install|uninstall|start|stop|status>`

- [ ] **Step 1: daemon 优雅退出**

`tokio::signal::unix`(SIGINT+SIGTERM)监听:收到 → 逐 planet `stop_planet`(按红线顺序)→ registry 落盘 → 删 port 文件 → exit 0。**port 文件删除放在最后**(存在性即活性信号,CLI 据此快速判断)。

- [ ] **Step 2: CLI service 组**

- `install`:检测 euid≠0 → 提示需 sudo 并 `std::process::exit(2)`;写 `/Library/LaunchDaemons/com.planetport.daemon.plist`(RunAtLoad=true、KeepAlive=false、ProgramArguments=[daemon 二进制绝对路径(current_exe 上溯),`--config-dir`, `/Library/Application Support/planet-portd`],先 mkdir 该目录)→ `launchctl bootstrap system/ <plist>`;二进制拷贝到 `/Library/Application Support/planet-portd/bin/planet-portd`(current_exe 拷贝,plist 指向拷贝——target/debug 路径不稳定);
- `uninstall`:`launchctl bootout system/<label>`(忽略未加载错误)+ 删 plist + 删 bin(数据目录保留,提示用户手动删);
- `start`:`launchctl kickstart -k system/<label>`;`stop`:`launchctl kill TERM system/<label>`(触发生命周期信号路径);
- `status`:ping `/version`(经 client)成功 → "running (port N)";失败查 `launchctl print system/<label>` 摘要。
- 全部子命令拼好命令后经 `std::process::Command` 执行;提示信息中文。

- [ ] **Step 3: client.rs 端口文件协商**

URL 未显式指定(--url/env)时:默认 `http://127.0.0.1:9998`;若 port_file() 存在且内容是合法端口(1-65535)→ 覆盖为该端口。token 读取不变。

- [ ] **Step 4: README**

新增「真实 daemon(planet-portd)」章节:构建、前台调试运行(sudo + `--config-dir "$HOME/Library/Application Support/planet-portd"` 示例)、service 安装/启动、CLI 常用流程(import→start→join→networks);验证清单 = 原 mockd 27 条回归命令改为对 planet-portd 执行(标注差异:planetImported 数据为实网、URL source 不支持、container 501);移除「端口文件协商尚未实现」注记。

- [ ] **Step 5: 提交**

`feat(cli): service 组命令与端口文件协商` + `feat(daemon): 信号优雅退出`(可合并一次提交,Body 分述)。

---

### Task 7: 实网验证(需用户 sudo/网络配合)

**Files:**
- Modify: `docs/superpowers/specs/2026-10-28-daemon-one.md`(§6 验收勾选 + 实测记录)、`README.md`(如发现文档偏差)

- [ ] **Step 1: 实网闭环(用户执行 sudo 部分)**

```bash
# 1. 前台起 daemon(复用 POC node-a 身份,绕开家宽 UDP 过滤与新授权):
sudo ./target/debug/planet-portd --config-dir "$HOME/Library/Application Support/planet-portd" &
# 2. import(dataDir 覆写指向 POC 数据、port 9994):
planet-port planet import <path>/pocs/fixtures/planet.Gozi25 --data-dir <abs>/target/poc-data/node-a --port 9994
# 3. start → join 5cf54f181554090f → networks(预期 OK + 10.121.16.103,feth 出现)
# 4. ping 10.121.16.229 通(数据面);listpeers 见根 peer 与 latency;info
# 5. events(含 planetStarted/join/status Online);moon import/orbit/deorbit 冒烟
# 6. planet stop(feth 消失、networks 空)→ start 恢复
# 7. SIGTERM 优雅退出:无 feth 残留、registry 完整、port 文件已删
# 8. service install/start/status/stop/uninstall 全流程
```

每步输出由 controller 判定;失败按「先环境后代码」排查基准(家宽 UDP 过滤史,见记忆/spec §8)。

- [ ] **Step 2: spec 验收勾选 + 收尾提交**

实测数字(在线时延、ping RTT、RSS)写回 spec §6;`docs: daemon 一期验收记录`。

---

## 执行注意(给 controller)

- Task 1-6 可全离线推进;Task 7 涉及 sudo/实网,精确命令与预期逐条交用户执行,拿输出判定
- Task 4 是集成核心:若 implementer 对 sink/驱动线程红线有疑问,直接引用 zt-core 模块文档(Task 1 已写入),不要自行发明同步方案
- mockd 与 planet-portd 并存(端口文件未写入时 CLI 默认 9998 仍指向 mockd)——README 说明二者关系,mockd 不删
- 终审走 requesting-code-review 全分支审查(opus),重点:FFI 红线、shutdown 顺序、锁跨 await、错误体契约一致性
