# Windows 支持评估 POC 报告（阶段:编译级验证通过,实机验证待硬件）

日期:2026-10-31
计划:docs/superpowers/plans/2026-10-31-windows-poc.md（基线 49c6a29）
设计:docs/superpowers/specs/2026-10-31-windows-poc-design.md
代码区间:1429645..d8e6638（dev 分支,5 个提交）

## 0. 一句话结论

- **编译级验证通过**:Windows 目标（x86_64-pc-windows-msvc）全链路——zt-sys 的 C++ 核心编译 + bindgen 绑定生成 + poc4 全部代码——在 mac 上经 cargo-xwin 交叉 check 全绿;既有业务 crate 与 release.sh 零改动。
- **实机验证待硬件**:Win10 实机不开机,计划的 Task 1-3（实机盘点/工具链/真机 Gate 0）与各 Gate 的实机实测步骤全部冻结。三道 Gate 的结论只到"代码就绪 + 交叉编译通过"级,**未到实机验证级**,待实测清单见第 3 节。
- **wintun 路线阶段性判断**:交叉编译未出现任何技术性否决信号,Windows 绑定布局已实证、L3 设备/路由/ARP 垫层代码全部就位;但"Win10 ↔ mac 互 ping 通过"这一决定性证据仍缺,**不构成最终结论**。

## 1. 执行过程与计划偏差

1. **顺序调整**:实机不可用,改为先在 mac 侧完成 Task 4-7 全部代码,每任务的实机步骤(4-Step6 / 5-Step4 / 6-Step6 / 7-Step3)冻结待硬件;以 `cargo xwin check --target x86_64-pc-windows-msvc` 作为编译级验证替代。
2. **结构偏差:poc4 独立 crate**(实际 `crates/poc4/`,bin 名 `poc4_windows`;计划原为 `crates/pocs/src/poc4/`)。根因:`pocs` 的 `[dependencies]` 无条件依赖 zt-core,而 zt-core 从未做 Windows 兼容(libc 的 `__in6_u`/`s_addr` 是 glibc 字段布局,`Permissions::from_mode` 等 unix-only API),交叉 check 时 41 个错误全部落在 zt-core,pocs 自身代码根本没轮到编译。独立 crate 后 `cargo xwin check -p poc4` 干净,且 `pocs` 完全回退(git diff 零残留)。
3. **计划外的既有 crate 触碰**:Gate 0 交叉编译暴露两个 Windows 兼容问题,修正进 `crates/zt-sys/build.rs`(提交 1429645)。这是 POC 期间唯一被修改的既有 crate——zt-sys 本就是本次评估的验证对象,不在零改动红线清单(zt-core/daemon/cli/api/release.sh)内,详见 2.1。

## 2. 已验证事实(编译级,本阶段真实产出)

### 2.1 Gate 0:zt-sys Windows 目标交叉编译预验证(通过)

- `cargo xwin check -p zt-sys --target x86_64-pc-windows-msvc` 通过(49s)。cc 驱动 clang-cl 编译 ZeroTier 1.16.2 C++ 核心子集(C++17),bindgen 解析 `ZeroTierOne.h` 生成绑定,全绿。
- 环境:brew llvm(llvm-lib / lld-link)+ cargo-xwin 0.23.1,PATH 需含 `/opt/homebrew/opt/llvm/bin`。
- build.rs 为 Windows 新增两处(均在 `crates/zt-sys/build.rs`,原因如下):
  - **`NOMINMAX` 宏**:windows.h 的 min/max 宏与核心源码 `std::min/std::max` 冲突(官方 Windows 工程同款处理);非 Windows 平台该宏无引用,零影响。
  - **`-mssse3 / -msse4.1 / -maes / -mpclmul`**(仅 Windows 目标,`flag_if_supported`):`AES_aesni.cpp` 的 SSSE3/AES-NI/PCLMUL 内联函数需显式开 CPU feature,clang-cl 下不开会编不过;MSVC 对内联函数不做 feature 检查,探测失败自动跳过。仅 Windows 目标生效,mac/linux 构建零影响。
- mac 原生 `cargo build --workspace` 全程无影响(每任务均复验)。

### 2.2 三处 Windows 绑定布局实证(bindgen 产物 vs 简报假设)

简报代码按 Linux/glibc 直觉写,以下三处与 Windows 实际产物不符,均已按 bindings.rs 实际形态修正并编译验证:

| 项 | Windows 实际形态 | 处理 |
|---|---|---|
| `in_addr` | 无 `s_addr` 小写字段,是 `S_un` union 内的 `S_addr: ULONG`(大写 S) | `sa.rs` 读写均走 `a.sin_addr.S_un.S_addr` |
| `netconfRevision` | Win32 下 `c_ulong`(32 位) | NetConfig 拷贝时 `u64::from(...)` 收敛到固定宽度 |
| `multicastSubscribe` ADI 形参 | `std::os::raw::c_ulong`(32 位) | `u32::from(*ip) as c_ulong` |

其余已确认一致:`ss_family`/`sin_family` 为 `ADDRESS_FAMILY`(u16)、`AF_INET` 为 `u32`、`in6_addr.u.Byte` 为 `[u8; 16]`、无 `sin_len`。**计划 sa.rs 的字段假设全部落地,无未解绑定疑点。**

### 2.3 Gate 1 / Gate 2 代码全部就绪

poc4 独立 crate(架构理由见 1.2),全部 Windows 路径包 `#[cfg(windows)]`,mac 构建不参与编译:

| 文件 | 内容 | 对应 Gate |
|---|---|---|
| `device_win.rs` | `WinDevice`:tun-rs 2.8.15 wintun **L3** 封装(layer/name/mtu/enable、add_address_v4/remove_address、`try_recv` Windows 专有非阻塞读)+ gate1 自验 CLI | Gate 1a/1b |
| `routes_win.rs` | `RouteSpec`/`RouteApplier`:route_manager 0.2 Windows 后端,**语义平移 zt-core/routes.rs**(默认路由跳过、差分应用、单条失败告警继续) | Gate 1b |
| `driver.rs` | `PocNode`/`PocSink`/`FrameIn`:平移 zt-core/node.rs 全架构——六回调 glue 只拷贝 + channel send、驱动线程串行 process*、catch_unwind 包 sink、关停硬顺序(running=false → wake → Cmd::Stop → join 双线程 → ZT_Node_delete → drop Inner)。Windows 四改造点:UDP 独立读线程(500ms 读超时 + running 检查)、mpsc 唤醒(替代 Unix pipe/poll)、`AsRawSocket`、c_ulong ADI | Gate 2a |
| `arp.rs` | `ArpShim` IPv4 ARP 垫层:出向裸 IP 包补 14 字节以太网头(dst 查学习表,miss 广播);入向学习 (src MAC, src IP);ARP request 问本机 IP 时以 managed IP 应答回喂核心 | Gate 2b |
| `netcfg.rs` / `store.rs` | NetConfig 精简版(裁 DNS 字段)/ 状态落盘(0600 收敛留正式实现,Windows 应换 ACL) | Gate 2 |
| `main.rs` | `Plane` 数据面(netcfg → 建设备/写地址/装路由/更新垫层;读线程 ↔ 核心帧桥)+ 主循环(2s 状态行、5s ping target) | Gate 2b |

关键机制说明:

- **唤醒机制**:UDP 读线程每收一包先投 `udp_rx` 再 `wake.send(1)`(先投后醒,驱动线程醒来即消费,无空醒);驱动线程 `while let Ok(try_recv)` 排空后回 `recv_timeout(≤500ms)`,唤醒只在真实收包时发出,不忙转。
- **WSAECONNRESET**:Windows 下 send 遇对端不可达后,下一次 `recv_from` 会报 ConnectionReset;读线程将其与 WouldBlock/TimedOut 等一并忽略,否则一次不可达打死收包路径(ZeroTier 官方 Windows 端同款处理,POC 关键坑)。
- **收包读循环用 `try_recv` 轮询**(20ms + stop 检查)而非阻塞 `recv`:阻塞读在 0 包场景下 stop 后 join 永不返回,teardown 与退出码全部挂死。`try_recv` 是 tun-rs 的 Windows 专有 API(`WouldBlock` 表示无包)。

### 2.4 零改动红线(终验实证)

`git diff db7a09a..HEAD -- crates/zt-core crates/daemon crates/cli crates/api scripts/release.sh` 输出为空——既有四个业务 crate 与发布脚本零改动,`cfg(windows)` 隔离红线达成;`cargo build --workspace`(dev + release)双通过。唯一被触碰的既有 crate 是 zt-sys(2.1,属验证对象本身)。

### 2.5 已知取舍与风险备忘

- **帧入核心延迟 ≤500ms(既定取舍)**:读线程与 ARP 应答都经 `frame_in_tx` 发送,不唤醒驱动线程,最坏一个 tick 后进核心;对 ping 的观感是 RTT 抖动 + 最高 500ms,ARP 首次握手可能叠加两个 tick,**首包 ping 丢 1 次属预期**。实测体验差则给驱动线程加 frame_in 可读即 wake(复用 `wake_tx`,改动局部、风险低)。
- **clippy 无法直接跑 Windows 目标**:clippy 需要 zt-sys 的 build script,而 Windows 目标的 cc/bindgen 依赖 cargo-xwin 注入的 SDK 环境(直接 clippy 报 `winsock2.h not found`)。类型级验证由 `cargo xwin check` 覆盖,lint 级验证待实机原生工具链。
- **`try_recv` 是 tun-rs Windows 专有 API**,非 Windows 无此代码路径,不影响其他平台。
- **POC 精简点**(正式实现再补):Cmd 只留 Join/Stop(leave/orbit/deorbit 裁剪)、0600/ACL 收敛、DNS 字段、pathCheck/pathLookup 回调(None)、`local_ipv4s` 只返回 managed IPv4(物理口枚举裁剪,组播发现覆盖面收窄,单播互通不受影响)。
- **Minor 备忘清单**(台账 progress.md):gate1 CLI 参数错误 panic(POC 工具可接受)、组播订阅每 5s 跨 FFI 查本机 IP(sink 勿做慢操作)、关停竞态窗口计数可虚高(send 结果未检)、probe_last 避开自身地址仅注释级提示、buf[0] 无 len 防护。

## 3. 待实测清单(实机恢复后执行)

以下全部**未执行**,无任何数据;命令与观察点在此汇总,实机可用后按序直接跑。

### 3.1 环境盘点(Task 1-2 冻结项)

| 动作 | 命令 | 观察点 |
|---|---|---|
| 版本/架构 | `winver` / `systeminfo` | 版本号与 build 号、x64/arm64;过新则结论标注适用范围 |
| HVCI/签名策略 | `powershell -Command "Get-CimInstance -ClassName Win32_DeviceGuard -Namespace root\Microsoft\Windows\DeviceGuard"` | SecurityServicesRunning 是否含 2(HVCI 运行中);wintun.dll 为 WHQL 签名,默认策略理论可过 |
| 工具链 | VS Build Tools(C++ 工作负载)+ LLVM + Rust MSVC;rustup/crate 走 rsproxy 镜像(GitHub 直连受限),wintun.dll 从 wintun.net 取 | 版本号记入 compatibility.md |
| ssh | 内置 OpenSSH Server 可选功能 | mac 侧免密切换 |

### 3.2 Gate 0 实机:Win10 本地编译 zt-sys

```
cargo build -p zt-sys        # MSVC + LLVM 工具链下
```

- 观察点:cc/cl.exe 编核心 + bindgen/libclang 是否全绿(主要风险是 bindgen 与 MSVC 环境 libclang 版本匹配,失败则钉 LLVM 版本)。
- 通过标准:`cargo build -p zt-sys` 成功;随后 `poc4_windows` 整包构建。

### 3.3 Gate 1:wintun 设备层(管理员权限运行)

```
poc4_windows --gate1-create                  # 建设备,检查后回车销毁
poc4_windows --gate1-ip 10.244.0.1/24        # 建设备 + 落地址
poc4_windows --gate1-read 10.244.0.1/24      # 建设备 + 落地址 + 装探针路由 + 60s 收包自验
```

- 观察点:适配器在网络面板可见(`Get-NetAdapter`);`ipconfig` / `route print` 地址与 /24 on-link 路由生效;`ping 10.244.0.200`(探针地址尾字节,勿与自身地址重叠)能否让 `--gate1-read` 读到包——**ring 包形态**:每包打印 `len/ipver/proto`(预期 ICMP 即 proto=1),进程退出码 `seen>0` 为 PASS。
- 探针尾字节可参数化(`[probe_last]`,默认 200)。

### 3.4 Gate 2:组网互通(与 mac/linux 节点同 planet)

```
poc4_windows --planet <planet文件> --join <nwid> --dir <工作目录> [--port 9994] [--duration 600] --target <mac侧节点IP>
```

- mac 侧反向 ping Win10 节点,**双向通 = Gate 2 通过**。
- 观察点:
  - `address=… online=… peers=… planet=… 活跃路径=…` 控制面行(join OK、rev 增长);
  - `[plane] ARP request 命中本机 IP` 日志(mac 先 ping 触发)——**ARP 垫层命中数**;
  - `plane dev=… 核心→设备=N 设备→核心=N v6drop=N` 双向帧计数增长(出向计数已修,不再恒 0);
  - **互 ping 方向与延迟数值**(预期 RTT 含 ≤500ms tick 抖动;首包丢 1 次属预期,须与 tick 延迟归因核对);
  - 退出 teardown 干净(设备销毁、路由无残留)。

### 3.5 实测后的产出动作

- compatibility.md 的 Win10 行从"未实测"更新为实测结果;
- 第 4/6 节的阶段性判断升级为最终结论;ARP 垫层"实测代价"(首包延迟/命中数)回填。

## 4. wintun 路线阶段性判断(非最终结论)

**已证实(编译级)**:

- ZeroTier 1.16.2 C++ 核心在 Windows 目标可编译,bindgen 绑定无未解疑点(2.1/2.2);
- zt-core/routes.rs 的路由语义可直接平移到 route_manager 的 Windows 后端,默认路由保护等逻辑无损;
- ARP 垫层实现量小:`ArpShim` 一个结构(出向补 14 字节以太网头 + 入向学习/应答,无 checksum 逻辑),百行级,与可行性调研 §2.3"同 macOS utun 处理方式"的预估一致;
- Windows 版 node 驱动的四个改造点(UDP 读线程/唤醒/AsRawSocket/c_ulong ADI)全部有确定解法并已实现。

**未证实(全部压在实机)**:

- wintun.dll 在目标机的加载与 HVCI/签名策略拦截;
- 设备层实际收发包、ring 包形态、地址/路由生效;
- 互 ping 的延迟与抖动(tick 取舍的实测观感)、ARP 首包代价;
- IPv6/NDP 垫层(POC 明确不做,纸面预估工作量远大于 ARP);
- 多适配器共存与官方 zerotier-one 同机干扰情况。

**判断**:wintun L3 路线维持"可行、待实机确认",无否决信号;tap-windows6(L2,免垫层但需装 NDIS 驱动)保留为实机失败时的备选路线。

## 5. 零影响终验记录(2026-10-31,HEAD d8e6638)

```
$ cargo build --workspace          → Finished `dev` profile(通过)
$ cargo build --workspace --release → Finished `release` profile in 39.85s(通过)
$ git --no-pager log --oneline -8
d8e6638 feat(poc4): poc4 Windows 数据面(wintun 帧桥 + IPv4 ARP 垫层)
3572106 feat(poc4): poc4 Windows 版 ZeroTier node 驱动(控制面)
62e60ec feat(poc4): poc4 Windows 地址路由应用与收包自验
673f228 docs: 实施计划同步 poc4 独立 crate 结构
475ed55 feat(poc4): poc4 Windows 设备层骨架(wintun L3)
1429645 build(zt-sys): Windows 目标交叉编译兼容修正
49c6a29 docs: Windows 支持评估 POC 实施计划
84bc997 docs: 新增 Windows 支持评估 POC 设计
$ git --no-pager diff db7a09a..HEAD --stat -- \
    crates/zt-core crates/daemon crates/cli crates/api scripts/release.sh
(输出为空)
```

## 6. 正式 Windows 支持工作量清单(按 POC 修订)

调研报告(docs/2026-10-28-desktop-port-feasibility.md)纸面工作量的实测修订——标"已验证"的项为 POC 编译级或代码级证实,标"纸面"的项无实测支撑:

| 工作项 | 状态 | 说明 |
|---|---|---|
| zt-sys Windows 编译(NOMINMAX + SSSE3/AES-NI flags + bindgen) | **交叉编译已验证**,实机 MSVC 待验证 | build.rs 修正已落库;实机风险仅 bindgen/libclang 版本匹配 |
| sockaddr winsock 布局(3 处实证) | **已验证** | in_addr/netconfRevision/multicastSubscribe ADI |
| Windows 版 node 驱动 | **代码就绪**(poc4 精简版),**回迁纸面** | 正式化 = driver.rs 架构回迁 zt-core/node.rs 的 cfg 改造 + POC 精简点回补(leave/orbit、pathCheck/pathLookup、ACL、信号处理) |
| wintun L3 设备 + 地址应用 | **代码就绪**,实机待验证 | WinDevice/WinDevice 地址差分 |
| managed 路由应用 | **代码就绪**(语义平移自 zt-core/routes.rs),实机待验证 | RouteApplier 回迁 zt-core 时与 daemon 期路由共存策略(台账 Important ⑤)一并定 |
| IPv4 ARP 垫层 | **代码就绪**,实测代价待数据 | ArpShim 直接回迁;出向计数/v6drop 计数已具备可观测性 |
| IPv6 NDP 垫层 | 纸面 | POC 明确不做,双向 v6 静默丢弃已计数(v6drop) |
| 服务化 windows.rs(Windows Service/SYSTEM、named pipe 或 loopback+token) | 纸面 | 参考 cli/src/cmd/service 的 macos.rs/linux.rs 平台模块形态新增 |
| DNS NRPT | 纸面 | netcfg POC 版已裁 DNS 字段;Task 33 的 DNS 三模式框架待加 Windows 分支 |
| release.sh 增加 x86_64-pc-windows-msvc 目标 | 纸面 | 红线清单内,POC 未触碰;交叉产物(a.exe)可行性待 Gate 0 实机后评估 |
| 签名/分发 | 纸面 | wintun.dll 自带 WHQL 签名;安装包与守护服务需代码签名证书,否则 SmartScreen 拦截 |

## 7. 结论

本阶段完成的是"Windows 支持评估"中**不依赖实机的全部工作**:构建链路交叉编译预验证、Windows 绑定布局实证、Gate 1/2 全部代码与自验工具、既有平台零改动证明。三道 Gate 的 PASS/FAIL 判定**全部悬置待实机**,wintun 路线维持阶段性"可行、待确认"。实机恢复后按第 3 节清单执行,预计一次会话即可补齐全部实测数据并升级为最终结论。
