# planet-portd 一期(daemon 本体 + CLI 对接)设计

> 日期:2026-10-28
> 前置文档: docs/superpowers/specs/2026-10-28-cli-api-design.md (REST 契约,§4 全文有效)、docs/superpowers/specs/2026-10-28-daemon-poc.md (POC 地基与实测结论,§8)
> 范围:**daemon 本体(planet-portd)+ CLI service 对接**,产出可日常使用的真实 daemon。
> 排除:GUI(Tauri)、ZTP 容器导入(二维码,POST /planets 的 container 形态,留 GUI 阶段)、系统 DNS 写入(见 §4)、Linux/Windows 适配、跨 planet 共享 identity。

## 0. 设计决策摘要

| 决策点 | 结论 |
|---|---|
| 进程模型 | 单进程多 Node:每 planet 一个 zt-core NodeHandle(自带专属驱动线程,POC 已验证隔离);REST 用 axum+tokio(mockd 同栈) |
| 身份模型 | **每 planet 独立 identity**(用户 2026-10-28 拍板),dataDir 隔离 |
| 存储布局 | `<config>/planet-portd/`:`registry.json`(planet 登记+本地网络配置+moon 记录)、`planets/<worldId>.bin`、`data/<worldId>/`(StateStore)、`authtoken.secret`、`port` |
| API 端口/鉴权 | 默认 9998,实际端口写 port 文件(api crate 已定义路径);token 0644 首启生成(对齐官方 macOS authtoken.secret 的可读性,理由见 daemon main.rs),X-ZT1-Auth 头(契约同官方) |
| UDP 端口分配 | 避开 9993(官方),从 9994 起顺序分配并记入 registry(稳定复用);冲突时递增重试 |
| 路由共存策略(前置清单⑤定案) | managed routes **一律安装**到本实例接口(与官方 zerotier-one 多网络同 /24 并存的行为一致,由内核路由次序决定);与其它接口同网段时打共存事件;routeViaZeroTier=0 时整个 planet 的 managed routes 不安装 |
| DNS(前置范围裁剪) | dnsMode 三值存储与回显照契约;一期**不写系统 DNS**(none=不动,network/custom=记录+事件提示未应用),系统 resolver 写入留二期 |
| set 语义 | 仅客户端本地配置,存 registry 按 (planetId, nwid);routeViaZeroTier 生效于设备配置;routeViaLocked 不做(单网络行星才允许改,多网络启用时锁定——一期每 planet 多网络直接允许 set,字段回显 false) |
| 冲突检测 | 网段冲突:join/配置应用时跨 planet 扫描 managed 网段重叠 → 事件 + networks 冲突码(subnet);address 冲突(两 planet 同网同 IP)同样事件化;不阻止 join(只告警) |
| moons | orbit/deorbit 真调核心(ZT_Node_orbit/deorbit);moon import 校验 .moon 文件后落 dataDir moons.d/;listmoons 以 daemon 记录为准(C API 无 moons 枚举) |
| 事件 | 内存 ring 1024 条(全局+planet 标签,ts 毫秒游标);daemon 重启即清空(可接受,二期再议持久化) |
| service 组 | launchd(LaunchDaemon,daemon 需 root 建 feth);CLI service 子命令以 `sudo` 执行 launchctl/plist 操作;`planet-portd` 直接运行=前台调试模式 |
| 生命周期 | SIGINT/SIGTERM → 逐 planet teardown 设备→join 线程→delete → 落盘 registry → 退出(前置清单⑥) |
| dump | networks+peers+本地配置+事件尾部,单 planet 定位链 |
| info LATENCY | peers 中根角色(root)peer 的 latency,无数据显示 -1 |
| planet 二进制解析 | World 格式解析器:0x01 头 + worldId(大端 8B)+ roots 稳定端点列表(show/详情需要);校验语义平移 PlanetImporter,上限 8MB |
| 验证策略 | 契约回归 = README 手工验证清单同款命令对 planet-portd 重跑;实网验证复用 POC node-a 的 dataDir(be8c294335 已授权、9994 会话已建立,绕开家宽 UDP 过滤与新授权)——registry 支持 dataDir/port 覆写 |

## 1. 架构

```
planet-portd (root)
├── tokio runtime: axum REST @127.0.0.1:<port>(auth 中间件)
│   └── AppState { registry: RwLock<Registry>, planets: Mutex<HashMap<planetId, PlanetRuntime>>,
│                  events: Mutex<Ring>, share_tokens, auth_token }
├── PlanetRuntime { node: NodeHandle, plane: Arc<Mutex<DevicePlane>>, sink→事件/帧,
│                   stop 信号 }          — 每 planet 一个,独立线程组(POC 模型)
└── spawn_blocking 桥:所有 NodeHandle/DevicePlane 同步调用包进 block_in_place
                                     (NodeHandle 命令式 API 均为快路径:发 channel+唤醒)
```

- **sink→事件桥**:Sink(实现 NodeSink)把 NodeEvent/NetConfig 变化转成 daemon Event 推 ring;帧写设备(POC 同款)。sink 调用发生在 planet 驱动线程内(红线:回调 glue 只拷贝,sink 在驱动线程,POC 已守住)。
- **REST 层永不直接持有 core 锁**:查询走 NodeHandle 快照方法(networks/peers/address/online),变更走命令 channel。

## 2. 存储

`registry.json`(原子写,与 StateStore 同款 tmp→rename):

```
{ "planets": [ { "id": "<worldId 十进制字符串>", "worldId": u64, "name": Option<String>,
    "icon": Option<String>, "color": Option<String>, "source": "<path|url>",
    "fileSize": u64, "mtimeMs": i64, "importedAtMs": i64, "lastNodeId": Option<String>,
    "dataDir": "<默认 data/<worldId>,可覆写>", "port": u16, "running": bool,
    "netLocal": { "<nwid hex>": { "routeViaZeroTier": bool, "dnsMode": "none|network|custom",
                                  "customDns": [String] } },
    "moons": [ { "moonId": String, "seed": Option<String>, "source": String, "cachedAtMs": i64 } ] } ] }
```

- planet 定位:按 name(唯一性校验,重名 import 时拒绝)与 worldId(十进制/十六进制输入均可)解析。
- `running` 为持久化意图(stop 后重启 daemon 不自动拉起);daemon 启动时对 running=true 的 planet 逐个恢复实例。

## 3. REST 实现(契约逐条映射,错误体 `{"error","message","candidates?"}`)

契约 §4 全部端点照 mockd 语义实现(mockd 是行为基准,CLI 回归清单直接复用)。差异/补充:

- `POST /planets`:一期支持 `{source, name?, icon?, color?}`;`{container}` 返回 501 notImplemented(GUI 阶段)。
- `GET /networks`:每网络字段来自 zt-core networks() 快照 + registry 本地配置 + 冲突码(跨 planet 网段重叠 → subnet 冲突码)。
- `GET /events?planet=&limit=`:ring 尾部过滤;ts 游标语义与 CLI `--follow` 轮询兼容(mockd 同款)。
- `GET /planets/{id}/share` → `POST` 修正:按契约 POST,token 内存表 TTL 默认 300s,`GET /share/<token>` 服务 planet 二进制(一次性,下载即失效)。
- `GET /status`:聚合各实例(node address/online/版本/根时延/FATAL)。
- 定位链:实现于 daemon(变更端点 `?planet=` 解析①显式②单 running 自动③多 running 409+候选);查询端点 `?planet=` 为过滤。**与 CLI 定位链互补:CLI 变更类不带 --planet 时由 daemon 端执行③④**(CLI 已按此实现)。

## 4. zt-core 加固(前置清单落地,一期 Task 1)

1. DevicePlane 落 Drop(join reader→删路由→销毁 feth)与 NodeHandle 落 Drop(未 shutdown 时 debug_assert + 尽力 shutdown);
2. run_driver 的 sink 调用包 catch_unwind,panic → NodeEvent::Fatal + 停机(不杀进程);
3. reader 线程错误改为退避重试(事件上报),spawn_reader 失败不再 `.ok()` 吞;
4. poll 处理 POLLERR/POLLNVAL(ERR 仅日志,暂态不视为停机;NVAL 日志+事件并退出驱动线程);
5. 路由共存策略(§0);6. 信号处理(daemon 层);7. `crates/zt-core/LICENSE-MPL.txt` 占位 + bootstrap 增拷 LICENSE-MPL.txt 入 Core/;
8. 新增 `NodeHandle::peers()`(包装 ZT_Node_peers,role/latency/paths/版本/active 标志,快照拷贝);
9. 架构红线写入 zt-core 模块文档(回调 glue 只拷贝+send;sink 必须在驱动线程;FrameIn/FrameOut 方向;shutdown join→delete 顺序)。

## 5. CLI 对接

- 新增 `service` 子命令组(install/uninstall/start/stop/status):sudo 写 `/Library/LaunchDaemons/com.planetport.daemon.plist` + `launchctl bootstrap/bootout`;status 查 launchctl + ping /version;
- client.rs:URL 未显式指定时读 port 文件(存在则覆盖默认 9998);
- README:真实 daemon 验证清单(对 mockd 的 27 条回归基准改跑 planet-portd)+ daemon 使用说明。

## 6. 验收

- [ ] mockd 回归清单全命令对 planet-portd 通过(结构一致,数据为实网)
- [ ] planet import(Gozi25,复用 POC dataDir 覆写)→ running → join `5cf54f181554090f` → networks 显示 OK + 10.121.16.103(POC 身份复用,无需新授权)
- [ ] listpeers 显示根 peer 与时延;listmoons/orbit/deorbit/moon import 闭环
- [ ] planet stop → 网络全部离线、feth 清理;start → 恢复
- [ ] events 记录状态变更与网段共存事件;`--follow` 增量
- [ ] SIGTERM 优雅退出(无 feth/路由残留、registry 完整)
- [ ] service install/start/stop/status 在 macOS 可用(用户 sudo 执行)
- [ ] workspace build + clippy 零警告

## 7. 许可

仅 MPL-2.0 核心源码(bootstrap 钉 1.16.2);`crates/zt-core/LICENSE-MPL.txt` 占位随仓库分发;分发阶段的完整 NOTICE 二期统一。
