# Wintun 适配器自愈(设备面健康巡检)实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** wintun 适配器被外部重启(如 Restart-NetAdapter)后,daemon 无需重启服务即可在 ~40 秒内自动重建设备恢复通信。

**Architecture:** zt-core 驱动线程主循环增加周期性 `on_tick` sink 回调(默认空实现,跨平台接口预留);Windows 设备面 `DevicePlane` 新增 `health_tick`,按适配器名查 `getifaddrs` 的 UP 标志判活,连续 3 次不健康(约 30s)即用缓存的最近 netconf 拆除并重建设备;daemon 的 `DaemonSink` 实现 `on_tick`,重建发生时记事件。

**Tech Stack:** Rust,现有依赖(zt-core 已依赖 `getifaddrs` 0.2,其 Windows 实现内部走 GetAdaptersAddresses 并把 `OperStatus==1` 映射为 `InterfaceFlags::UP|RUNNING`,接口名即 FriendlyName,与 tun-rs 的 `dev.name()` 一致——实测 Get-NetAdapter -Name 显示同名)。不引入任何新依赖。

## Global Constraints

- **不引入新 crate 依赖**;健康判活只用 `getifaddrs`(zt-core 已有 windows-only 依赖)。
- **架构红线不破坏**:`NodeSink` 新回调必须有默认空实现(Unix 侧 device_unix 不改);sink 调用必须经 `call_sink` 包裹(panic 不穿越驱动线程);设备面全部操作仍在 `Arc<Mutex<DevicePlane>>` 临界区内;关停硬顺序(先 teardown 再 NodeHandle::shutdown)不变。
- **不加测试用例**(用户全局规则);验证手段为编译检查 + 实机 e2e(由控制者执行,不在本任务内)。
- 注释风格与文件既有中文注释一致;错误路径只记日志/事件,不得 panic。
- 重建语义:复用**传入 `apply` 的最终 cfg**(daemon 侧可能已按 routeViaZeroTier 清空 routes,设备面存的就是事实),不重新走 netconf。

---

### Task 1: 设备面健康巡检与自愈重建

**Files:**
- Modify: `crates/zt-core/src/config.rs:24-29`(NodeSink trait 加默认空实现 `on_tick`)
- Modify: `crates/zt-core/src/node.rs:653-700`(run_driver 主循环加健康巡检节拍)
- Modify: `crates/zt-core/src/device_win.rs`(NetDevice 缓存 last_cfg/dead_ticks;DevicePlane::health_tick)
- Modify: `crates/daemon/src/runtime.rs`(DaemonSink 实现 on_tick,记事件)

**Interfaces:**
- Consumes: `DevicePlane::apply(cfg, frame_tx)`(既有)、`DevicePlane::teardown(nwid)`(既有)、`getifaddrs::getifaddrs()`、`call_sink`(既有)
- Produces: `NodeSink::on_tick(&mut self)`(默认空实现);`DevicePlane::health_tick(&mut self, frame_tx: &FrameInTxSlot) -> Vec<u64>`(返回本轮触发重建的 nwid 列表,空 = 无事发生)

- [ ] **Step 1: NodeSink 加 on_tick 默认空实现**

`crates/zt-core/src/config.rs` 的 `NodeSink` trait 增加一个方法(trait 默认实现,Unix 侧与 poc4 等既有实现零改动):

```rust
pub trait NodeSink: Send + 'static {
    fn on_event(&mut self, ev: NodeEvent);
    fn on_net_config(&mut self, nwid: u64, op: NetConfigOp, cfg: NetConfig);
    fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]);
    fn local_ipv4s(&self) -> Vec<std::net::Ipv4Addr>;
    /// 驱动线程周期节拍(默认空实现;Windows 设备面健康巡检消费,见
    /// device_win.rs health_tick。频率约 HEALTH_TICK_MS,语义为"巡检机会",
    /// 不承诺精确周期)。
    fn on_tick(&mut self) {}
}
```

- [ ] **Step 2: run_driver 加巡检节拍**

`crates/zt-core/src/node.rs`:

常量(放在文件顶部既有常量区,`PHY_BUF` 等附近):

```rust
/// 设备健康巡检周期(ms):驱动线程以此节拍回调 sink.on_tick
const HEALTH_TICK_MS: i64 = 10_000;
```

`run_driver` 循环体:初始化 `let mut last_health_ms: i64 = i64::MIN / 4;` 放在 `last_mcast_ms` 旁;循环内(建议放在命令处理之后、netcfg 处理之前的位置)加:

```rust
// 设备健康巡检节拍:Windows 侧检测适配器被外部重启等设备失效并自愈
if now - last_health_ms >= HEALTH_TICK_MS {
    last_health_ms = now;
    if call_sink(&mut d.sink, &inner.running, "on_tick", |s| s.on_tick()).is_none() {
        return;
    }
}
```

- [ ] **Step 3: device_win.rs 健康判活与重建**

`crates/zt-core/src/device_win.rs`:

3a. 常量(READER_BACKOFF 旁):

```rust
/// 适配器连续多少个巡检周期不健康才触发重建(3×10s=30s:给 enable 后
/// 瞬态 Down 与服务启动期留余量,避免误重建)
const DEAD_TICKS_TO_REBUILD: u32 = 3;
```

3b. `NetDevice` 增加两个字段:

```rust
    /// 最近一次 apply 的最终 netconf(自愈重建用:外部重启适配器后 ring 失效,
    /// 按此 cfg 拆旧建新,地址/路由原样重放)
    last_cfg: NetConfig,
    /// 连续不健康巡检计数(适配器存在且 UP 即清零)
    dead_ticks: u32,
```

`create_device` 返回的 `NetDevice { .. }` 补 `last_cfg: cfg.clone(), dead_ticks: 0`;`DevicePlane::apply` 在拿到 `&mut d` 后(`lock_arp` 赋值附近)补 `d.last_cfg = cfg.clone();`。

3c. `DevicePlane` 新增方法(放在 `teardown_all` 之后):

```rust
    /// 设备健康巡检(驱动线程 on_tick 调用,Windows 专有自愈路径):
    /// 按 if_name 在系统接口表查 UP 标志,连续 DEAD_TICKS_TO_REBUILD 次
    /// 不健康(适配器消失/被外部重启致 ring 失效)即拆旧建新。
    /// 返回本轮触发重建的 nwid 列表(供上层记事件)。
    pub fn health_tick(&mut self, frame_tx: &FrameInTxSlot) -> Vec<u64> {
        // 系统接口表一次性快照:name → 是否 UP
        let mut up_names: HashMap<String, bool> = HashMap::new();
        if let Ok(iters) = getifaddrs::getifaddrs() {
            for ifa in iters {
                up_names.insert(ifa.name.clone(), ifa.flags.contains(getifaddrs::InterfaceFlags::UP));
            }
        }
        // 枚举失败(快照为空)视作"信息不足":清零不计数,绝不据此重建
        if up_names.is_empty() {
            return Vec::new();
        }
        let mut rebuilt = Vec::new();
        let nwids: Vec<u64> = self.devices.keys().copied().collect();
        for nwid in nwids {
            let Some(d) = self.devices.get(&nwid) else { continue };
            let healthy = up_names.get(&d.if_name).copied().unwrap_or(false);
            if healthy {
                if let Some(d) = self.devices.get_mut(&nwid) {
                    d.dead_ticks = 0;
                }
                continue;
            }
            let (cfg, ticks) = {
                let d = self.devices.get_mut(&nwid).expect("上方已查到");
                d.dead_ticks += 1;
                (d.last_cfg.clone(), d.dead_ticks)
            };
            if ticks < DEAD_TICKS_TO_REBUILD {
                continue;
            }
            // 拆旧建新:teardown 停 reader/删路由/销毁适配器,apply 按
            // last_cfg 重建(新 ring、新 if_index,地址/路由重放)。失败仅记
            // 日志并保留在表(dead_ticks 已达阈值,下个 tick 会重试——注意
            // 重试前需再累计,故此处重置为阈值-1,保证连续每 tick 重试)。
            eprintln!(
                "[device] {:016x} 适配器 {} 连续 {ticks} 次巡检不健康,拆除重建",
                nwid, d.if_name
            );
            self.teardown(nwid);
            match self.apply(&cfg, frame_tx) {
                Ok(()) => rebuilt.push(nwid),
                Err(e) => {
                    eprintln!("[device] {:016x} 适配器重建失败(下周期重试): {e}", nwid);
                    // apply 失败时设备可能已插入(部分成功)也可能不在表:
                    // 统一兜底——不在表则塞回一个占位由后续 ConfigUpdate 收敛
                    if !self.devices.contains_key(&nwid) {
                        if let Ok(mut d) = rebuild_placeholder(&cfg) {
                            d.dead_ticks = DEAD_TICKS_TO_REBUILD - 1;
                            self.devices.insert(nwid, d);
                        }
                    }
                }
            }
        }
        rebuilt
    }
```

3d. 占位重建辅助函数(health_tick 失败兜底用:尽力新建设备对象,保持 dead_ticks 接近阈值使下个 tick 继续重试;新建也失败则只能等 on_net_config 的 Up/ConfigUpdate 走常规 apply 恢复):

```rust
/// health_tick 重建失败的兜底:尽力重建设备对象(新 ring),dead_ticks 置
/// 阈值-1 使下个巡检周期继续重试;连设备都建不出来返回 Err(等 netconf 恢复)。
fn rebuild_placeholder(cfg: &NetConfig) -> io::Result<NetDevice> {
    // 与 create_device 同参数开新适配器(wintun 同名复用,ring 为新建)
    let mtu = cfg.mtu.max(1280);
    let want_name = format!("{ADAPTER_PREFIX}{:016x}", cfg.nwid);
    let dev = tun_rs::DeviceBuilder::new()
        .layer(tun_rs::Layer::L3)
        .name(&want_name)
        .mtu(mtu)
        .enable(true)
        .build_sync()?;
    // ……后续 if_index/stop/reader 构造与 create_device 相同
}
```

(实现时若发现 rebuild_placeholder 与 create_device 重复度过高,直接重构:create_device 抽出 `build_adapter(cfg) -> io::Result<NetDevice>` 供 apply 与兜底共用,**禁止复制粘贴整段构造逻辑**。)

- [ ] **Step 4: daemon DaemonSink 实现 on_tick**

`crates/daemon/src/runtime.rs` 的 `impl NodeSink for DaemonSink`,与 `on_net_config` 平级新增:

```rust
    /// 设备健康巡检(驱动线程 10s 节拍):适配器被外部重启等失效时设备面
    /// 自愈重建,重建发生记事件供排查(stop 收尾期不巡检,与 on_net_config 同语义)
    fn on_tick(&mut self) {
        if self.stopping.load(Ordering::SeqCst) {
            return;
        }
        let rebuilt = lock(&self.plane).health_tick(&self.frame_tx);
        for nwid in rebuilt {
            self.events.push(
                &self.planet_id,
                "deviceRebuilt",
                &format!("网络 {nwid:016x} 适配器失效,已自动重建"),
            );
        }
    }
```

- [ ] **Step 5: 编译验证**

```bash
cargo check -p zt-core -p daemon -p planet-port            # macOS 宿主(device_unix 路径)
cargo xwin check --target x86_64-pc-windows-msvc -p zt-core -p daemon -p planet-port   # Windows 路径(device_win 编译进)
```

Expected: 两条均 0 error(既有 warning 不新增)。

- [ ] **Step 6: Commit**

```bash
git add crates/zt-core/src/config.rs crates/zt-core/src/node.rs crates/zt-core/src/device_win.rs crates/daemon/src/runtime.rs
git commit   # feat(daemon): Windows 适配器失效自愈重建
```

提交信息 Body 说明背景:实测 Restart-NetAdapter 后 wintun ring 失效,适配器停留 Disconnected,须重启服务恢复;现由驱动线程 10s 巡检、连续 3 次不健康自动拆旧建新。
