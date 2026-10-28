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
| 核心库来源 | **bootstrap 脚本模式**(对齐 iOS/Android 模板):`scripts/bootstrap.sh` 钉 1.16.2 tag + SHA256 拉官方 tarball,解压裁剪子集到 `Core/`(gitignore,不入库),`.fetched-tag` 版本戳 + 构建期校验 fail-fast;禁止取用 BSL 时代(1.4.6~1.14.x)代码 |
| FFI 契约面 | 官方 `include/ZeroTierOne.h`(ZT_Node_* 回调式纯 C API,MPL-2.0) |
| 线程模型 | 每 Node 一条专属 OS 线程跑核心服务循环(zts 同构),回调经 trait 进 Rust;POC 不引入 tokio |
| dataStore 语义 | 按 Android NodeRuntime 平移:`<dataDir>/<worldId>/` 分目录;planet 文件作为 hook 输入重定向 |
| identity | POC 每节点独立全新 identity(避免与本机在跑的 zerotier-one 同根体系撞身份);共享 identity 语义留给 daemon 阶段 |
| UDP 端口 | 错开本机 zerotier-one 的 9993:POC 节点用 9994、9995 |
| POC 2 设备路线 | **首选 feth L2**(tun-rs macOS TAP,内核处理 ARP,对齐官方 MacEthernetTap 机制——1.16.2 在 Darwin≥17 用 feth 伪以太对而非 utun);退路 utun L3 + 用户态 ARP 垫层(机制已定:订阅 `ff:ff:ff:ff:ff:ff` + ADI=本机各 IPv4 主机序,ARP reply 经 processVirtualNetworkFrame 回喂核心,头文件 1893-1919 行硬性要求) |
| 失败退路 | POC 3 若互扰 → 每 planet 一进程(形态 B);POC 1/2 成果全部保留 |

## 1. 范围

### 做

1. **POC 1(FFI 控制面)**:单 Node 启动 → join 测试网络 → 状态机走到 OK,取得 network config 与分配地址。不建任何网络设备。
2. **POC 2(设备+配置应用)**:tun-rs 建 feth L2 设备 → 应用 managed IP/route → ping 通网内另一成员;内核处理 ARP(对齐官方);utun L3 + ARP 垫层为退路。
3. **POC 3(双 Node 互扰)**:同进程两 Node 并发,验证 identity/dataStore/UDP 端口隔离与互不干扰,双网络同时在线。

### 不做(本轮明确排除)

daemon 进程模型与 REST API 真实现、service 组命令、事件日志、冲突检测、DNS 三模式、跨 planet 共享 identity、Linux(/dev/net/tun)与 Windows(wintun)适配、签名分发。以上归 daemon 及后续计划。

## 2. 工程结构

```
planet-port-desktop/
├── scripts/bootstrap.sh   # 拉 1.16.2 tarball(SHA256 校验)→ 裁剪解压到 Core/
├── Core/                  # gitignore:核心源码子集(bootstrap 产物,含 .fetched-tag 版本戳)
├── crates/
│   ├── zt-sys/            # bindgen 于 Core/include/ZeroTierOne.h;build.rs 编译 Core 子集出静态库
│   ├── zt-core/           # unsafe→safe 封装:Node 句柄、回调 trait、线程循环
│   └── pocs/              # 三个 bin:poc1_control / poc2_device / poc3_multi
└── pocs/fixtures/         # gitignore:planet.Gozi25 副本、运行期 dataStore 输出
```

- **裁剪子集与 iOS/Android 完全一致**:`node/` 整目录 + `include/`(ZeroTierOne.h/ZeroTierDebug.h)+ `osdep/` 精选 6 文件(Binder/BlockingQueue/ExtOsdep/OSUtils/Phy)+ `version.h` + `ext/` 精选头(nlohmann/json.hpp、prometheus-cpp-lite);不含 controller/service 等其余部分。
- bootstrap 细节对齐两端模板:`ZT_VERSION=1.16.2` + `ZT_SHA256`(与 iOS/Android 同值)、`ZT_TARBALL_URL` 镜像与 `ZT_CACHE_DIR` 离线缓存支持;build.rs 检查 `Core/.fetched-tag` 不匹配即报错终止(Android GradleException / iOS preBuildScript 的等价物)。
- zt-sys build.rs 用 cc 或 cmake 驱动 Core 子集编译(实现计划定,倾向对齐 Android 树外 CMakeLists 的 GLOB 方案);升级版本 = 换 tag + SHA 重新评估。
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

### 线程模型(C API 事实已核实,1.16.2)

- 驱动方式:`ZT_Node_processBackgroundTasks(now, &deadline)` 轮询,deadline 为 in/out(ms 墙钟,下次绝对时刻,下限 60ms 粒度、常态 5000ms);官方 OneService 即"到点才调 + poll(timeout=deadline-now)"单线程串行模型。
- 每 Node 一条专属驱动线程:poll [UDP socket(非阻塞)、wakeup pipe];设备帧由独立设备线程阻塞读出后经 channel + pipe 唤醒投递驱动线程——保证对同一 Node 的全部 `process*` 调用串行(官方约定,`ZT_Node_delete` 前必须停掉其它线程)。
- `ZT_Node_new` 的 `ZT_Node_Config` 指针必须非 NULL(全零即可);`callbacks.version = 0`;无 stateDeleteFunction——删除 = statePut(len=-1)。
- planet 注入:stateGet 回调命中 `(ZT_STATE_OBJECT_PLANET, id={0,0})` 返回自定义 World 二进制(构造时读取,首次加载不校验签名;运行期换 planet = 重建 Node);statePut 的回写按官方语义落盘。
- IPv4 ARP 硬性要求(头文件注释明文):tap 实现者须对每个本机 IPv4 调 `multicastSubscribe(nwid, 0xffffffffffff, ADI=IPv4 主机序)`,官方每 5000ms 扫描 diff——L2 设备同样需要(core 据此决定投递哪些广播)。
- 回调在 Node 线程上下文执行;跨线程状态经 channel 交主线程。封装层保证 Rust 侧借用不跨越 FFI 边界存活。
- 节点优雅关闭:先停驱动/设备线程并等其真正退出,再 `ZT_Node_delete`(防 UAF,Android shutdown 同序)。

### 错误与安全

- FFI 返回码逐一定义映射,禁止 unwrap 穿越 FFI;
- 封装层行为验证(不依赖真实网络的离线用例):identity 持久化往返、dataStore 目录隔离——通过 POC bin 的可观察行为验证,不引入测试框架。

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
| 单进程多 Node 全局状态互扰(核心风险,无官方书面承诺) | POC 3 直接检验;源码级证据已降低风险:核心无全局可变状态(仅随机数池带锁共享、prometheus 计数器共享,RuntimeEnvironment 为实例成员);失败 → 形态 B(每 planet 一进程),POC 1/2 成果保留 |
| tun-rs macOS feth TAP 成熟度 | POC 2 直接检验;退路 utun L3 + 用户态 ARP 垫层(机制已定死,见 §0) |
| Core 子集构建链复杂度 | 对齐 Android 树外 CMakeLists(GLOB node/*.cpp)成熟方案;退路为 build.rs cc 逐文件编译清单(仍从 1.16.2 tarball) |
| 1.16.2 C API 与预期不符(服务循环/回调形态) | 以头文件为准修正封装层设计;头文件已在案 |

## 7. 许可合规落地

- 仅使用 1.16.2(MPL-2.0)源码;submodule 固定 tag,不 cherry-pick BSL 时代提交。
- 构建产物分发时随附 MPL 许可文本与 ZeroTier, Inc. 署名(义务在本阶段建文件占位,分发阶段生效)。

## 8. POC 结果记录(2026-10-28 实测)

### POC 1 — FFI 控制面:✓

- `ZT_Node_new` → identity 生成并落盘;重启同 dataDir 复用(address 不变)。
- join `5cf54f181554090f`:UP → ONLINE,网络 REQUESTING_CONFIGURATION → ACCESS_DENIED
  →(控制器授权)→ **OK rev=6**,assigned `10.121.16.103/24`,routes `10.121.16.0/24`(on-link)。
- 事件序列与 dataStore 落盘(identity/planet/peers.d/networks.d)完整。

### POC 2 — feth L2 设备 + 配置应用:✓

- netconf Up 时建 feth 对(内核自动单元号,实测 `feth0`,与官方 zerotier-one 的
  feth194/345/1986/2527/4684 不冲突),MAC 取核心分配值,managed 地址/路由差分应用。
- **ping 网内真实成员 `10.121.16.229` 通**(3.3~23ms),帧双向计数递增
  (设备→核心 `frames_in`、核心→设备 `frames_to_dev`),ARP 由内核完成。
- teardown:停读线程 → 删路由 → 销毁 feth,退出干净。
- 附带实测:同机两个同 /24 接口的路由并存;**内核对本地地址直接交付、绕过 ZT
  数据面**——因此双节点都在本机时,互 ping 只能作为「双方 netconf OK 且地址配齐」
  的控制面证据,数据面证据以网内真实设备对照为准。

### POC 3 — 同进程双 Node 并发与隔离:✓(2026-10-28 收口)

poc3_multi 五项断言(最终态,含 2ddb458 框架修正):

| # | 检查项 | 结果 |
|---|---|---|
| 1 | identity 独立(内容不同/各在己目录/首字段=节点 address/address 互异) | **PASS** |
| 2 | dataStore 隔离(目录不同、对方 identity 字节零出现、公共文件内容不同) | **PASS** |
| 3 | UDP 9994/9995 各自 bind,同进程重复 bind 均 `EADDRINUSE` | **PASS** |
| 4 | node-b shutdown→重建复用 dataStore(address 不变),重启窗口内 node-a 持续 online | **PASS** |
| 5 | a↔b 互 ping | 信息性,不判定(见下) |

- **双节点同时在线同时 OK**(手机热点网络实测):node-a `be8c294335`/9994/feth0
  (`10.121.16.103`)与 node-b `92c680e88f`/9995/feth2(`10.121.16.1`)在同一进程内
  并发运行,网络状态均为 OK rev=6。
- 内存基线:sudo 全量跑(双 Node + 双 feth + 双读线程,`/usr/bin/time -l`)
  进程峰值 RSS **21.0 MB**。
- **结论:同进程多 Node 全局状态互扰未发现反例**(Task 5 核心风险检验通过,
  形态 A「单进程多 Node」可行)。
- 断言 5 说明:双节点都在本机,内核对本地地址直接交付、绕过 ZT 数据面
  (POC 2 已证),单机上互 ping 不可能走 ZT 链路,方向结果取决于三个同 /24 接口
  (feth0/feth2/官方 feth1986)的内核路由次序,与互扰无关——降级为记录性输出。
  数据面证据以 POC 2 的网内真实设备对照(tcpdump + 帧计数)为准。

### 环境问题记录(2026-10-28 定案)

- **家宽出口(上海电信 122.193.x)对根服务器 `124.222.91.224:9993` 的新建 UDP
  会话丢包,已建立的 NAT 会话放行**:新节点长时间 online=false 后偶发自愈;
  换手机热点后新会话秒通。经远端 tcpdump 定案(12 个不同源端口探测包无一到达
  服务器,同 IP 已建立会话双向正常);根服务器侧无过滤(iptables INPUT ACCEPT,
  zerotier ONLINE),服务器未做任何改动。**排查基准:新节点长时间无应答先换网络
  再查代码;daemon 重试策略需容忍「首连长时间静默后突然成功」。**
- 同一台 Gozi25 根服务器同时是网络控制器(`5cf54f1815`,1.14.2)。
