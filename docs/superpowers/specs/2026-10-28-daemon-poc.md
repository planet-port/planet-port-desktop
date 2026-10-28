# planet-portd 地基 POC(1-3)设计

> 日期:2026-10-28
> 前置文档: docs/2026-10-28-desktop-port-feasibility.md (架构定稿与 POC 顺序)、docs/superpowers/specs/2026-10-28-cli-api-design.md (CLI/API 契约,已实现)
> 本文档范围:**daemon 一期 = POC 1-3**(FFI 控制面、设备+配置应用、双 Node 互扰)。
> 排除:daemon 本体(tokio+REST)、service 组命令、GUI、Windows/Linux 平台适配、跨 planet 共享 identity——均为后续阶段。

## 0. 设计决策摘要(已与用户确认)

| 决策点 | 结论 |
|---|---|
| 本轮范围 | POC 1-3,每步可独立止步,验证通过后再立 daemon 计划 |
| 工程形态 | 持久地基 crate:`zt-sys`(bindgen)+ `zt-core`(safe 封装)+ `pocs`(三个 bin);POC 代码即 daemon 地基,零弃置 |
| 核心库来源 | git submodule pin **1.16.2 tag**(MPL-2.0),cmake 构建静态库,build.rs 驱动;禁止取用 BSL 时代(1.4.6~1.14.x)代码 |
| FFI 契约面 | 官方 `include/ZeroTierOne.h`(ZT_Node_* 回调式纯 C API,MPL-2.0) |
| 线程模型 | 每 Node 一条专属 OS 线程跑核心服务循环(zts 同构),回调经 trait 进 Rust;POC 不引入 tokio |
| dataStore 语义 | 按 Android NodeRuntime 平移:`<dataDir>/<worldId>/` 分目录;planet 文件作为 hook 输入重定向 |
| identity | POC 每节点独立全新 identity(避免与本机在跑的 zerotier-one 同根体系撞身份);共享 identity 语义留给 daemon 阶段 |
| UDP 端口 | 错开本机 zerotier-one 的 9993:POC 节点用 9994、9995 |
| 失败退路 | POC 3 若互扰 → 每 planet 一进程(形态 B);POC 1/2 成果全部保留 |

## 1. 范围

### 做

1. **POC 1(FFI 控制面)**:单 Node 启动 → join 测试网络 → 状态机走到 OK,取得 network config 与分配地址。不建任何网络设备。
2. **POC 2(设备+配置应用)**:tun-rs 建 utun → 应用 managed IP/route → ping 通网内另一成员;macOS utun(L3)的 ARP 用户态垫层在此验证。
3. **POC 3(双 Node 互扰)**:同进程两 Node 并发,验证 identity/dataStore/UDP 端口隔离与互不干扰,双网络同时在线。

### 不做(本轮明确排除)

daemon 进程模型与 REST API 真实现、service 组命令、事件日志、冲突检测、DNS 三模式、跨 planet 共享 identity、Linux(/dev/net/tun)与 Windows(wintun)适配、签名分发。以上归 daemon 及后续计划。

## 2. 工程结构

```
planet-port-desktop/
├── crates/
│   ├── zt-sys/        # bindgen 于 include/ZeroTierOne.h,链接静态 libzerotiercore
│   ├── zt-core/       # unsafe→safe 封装:Node 句柄、回调 trait、线程循环
│   └── pocs/          # 三个 bin:poc1_control / poc2_device / poc3_multi
├── vendor/ZeroTierOne/  # git submodule,pin 1.16.2 tag
└── pocs/fixtures/       # gitignore:planet.Gozi25 副本、运行期 dataStore 输出
```

- submodule 即官方仓库本体(构建时只编译 core 相关目标);锁 tag 1.16.2,升级需换 tag 重新评估。
- planet 文件含内网根服务器地址,属半敏感配置,副本放 gitignore 的 fixtures 目录,不入库。

## 3. FFI 封装层(zt-core)设计

### 回调模型

ZT 核心 C API 是"Rust 持有资源 + 核心反向回调"结构。zt-core 用 trait 承接四类回调:

| 回调 | trait 方法 | Rust 侧职责 |
|---|---|---|
| dataStore 读写删 | `DataStore::{get,put,delete}` | 落盘 `<dataDir>/<worldId>/`,原子写,损坏隔离不重建(语义平移 Android store) |
| 虚拟网络配置变更 | `on_network_config(nwid, config)` | POC 期打印 + 缓存;POC 2 据此驱动设备配置 |
| 物理包收发 | `send_wire_packet(addr, data)` | UDP socket 由 Rust 侧持有,bind 各自端口 |
| path/peer 查询 | `get_object_address` 等 | 最小实现,POC 不做多余功能 |

### 线程模型

- 每 Node 一条专属 OS 线程:循环驱动核心后台任务处理与超时调度(具体机制以 ZeroTierOne.h 实际 API 为准——服务循环入口、时间源、退避间隔在实现计划中从该头文件定死,不猜)。
- 回调在 Node 线程上下文执行;跨线程状态经 channel 交主线程。封装层保证 Rust 侧借用不跨越 FFI 边界存活。
- 节点优雅关闭:先 leave/离线处理再销毁句柄,避免 dataStore 半写。

### 错误与安全

- FFI 返回码逐一定义映射,禁止 unwrap 穿越 FFI;
- 封装层单测:identity 持久化往返、dataStore 目录隔离(不依赖真实网络)。

## 4. 验收标准

### POC 1 — FFI 控制面

- [ ] 静态链接 libzerotiercore 1.16.2,bindgen 无手改
- [ ] Node 启动,identity 生成并持久化,二次启动复用(同 dataStore)
- [ ] join `5cf54f181554090f`(Gozi25 planet)后网络状态经 LOOKING_UP/REQUESTING 到 **OK**(需控制器授权新设备)
- [ ] 取得 managed addresses 与 managed routes 并打印
- [ ] 退出时 dataStore 完整(重启后无需重新入网认证)

### POC 2 — 设备 + 配置应用

- [ ] join 后自动创建 utun 设备(sudo 运行),配 managed IP
- [ ] managed routes 写入路由表,不劫持默认路由
- [ ] ARP 垫层对网内 ARP 请求正确应答(utun 为 L3)
- [ ] **ping 通网内另一成员**(网内现存设备或另一 POC 节点)
- [ ] leave/退出后设备与路由清理干净

### POC 3 — 双 Node 互扰

- [ ] 同进程两 Node(同 Gozi25 planet,不同 identity/端口/dataStore)并发运行
- [ ] 两节点 join **同一测试网络**(不同 identity = 两台设备),同时在线——这两个节点同时就是 POC 2 的 ping 对端,POC 3 内直接补验节点间互通
- [ ] 互扰检查:identity 未串、dataStore 目录未串、端口未抢占、一节点重启不影响另一节点
- [ ] 记录双 Node 内存基线(可行性报告 §3.2 遗留的实测项)
- [ ] (可选)第二组用官方 planet 跑通,验证 planet 重定向按实例隔离

## 5. 测试环境与授权点

| 项 | 值 |
|---|---|
| planet | `pocs/fixtures/planet.Gozi25`(源:/Users/zivyuan/GoziLib/Zerotier/switch-planet/planets/planet.Gozi25,worldId 0xD26B6540) |
| 测试网络 | `5cf54f181554090f` |
| 本机共存 | zerotier-one(pid 常驻,9993)不受影响;POC 用 9994/9995 |
| **需用户授权①** | POC 1/3 join 后,控制器侧授权新设备(每节点一个新 identity) |
| **需用户授权②** | POC 2 建 utun 需 sudo;命令由用户执行或明示同意 |

## 6. 风险与退路

| 风险 | 应对 |
|---|---|
| 单进程多 Node 全局状态互扰(核心风险,无官方书面承诺) | POC 3 直接检验;失败 → 形态 B(每 planet 一进程),POC 1/2 成果保留 |
| macOS utun ARP 垫层正确性 | 参照官方 MacEthernetTap 用户态 ARP 模式;现网实测 |
| cmake 子模块构建链复杂度 | build.rs 只做最小 core 目标;若官方构建系统不友好,退路为手写编译清单(仍从 1.16.2 源码) |
| 1.16.2 C API 与预期不符(服务循环/回调形态) | 以头文件为准修正封装层设计;头文件已在案 |

## 7. 许可合规落地

- 仅使用 1.16.2(MPL-2.0)源码;submodule 固定 tag,不 cherry-pick BSL 时代提交。
- 构建产物分发时随附 MPL 许可文本与 ZeroTier, Inc. 署名(义务在本阶段建文件占位,分发阶段生效)。
