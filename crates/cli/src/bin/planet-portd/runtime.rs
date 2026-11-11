//! 实例运行时:PlanetRuntime 启停(start/stop)、DaemonSink(驱动线程侧
//! 设备面 glue)、冲突检测与实时状态快照。
//!
//! # 线程与锁
//!
//! - DaemonSink 的全部方法在 zt-core 驱动线程被调:只做短临界区 Mutex
//!   (plane/net_local/net_cache/conflicts/Ring)+ 设备操作,绝不调用
//!   node.join/leave/orbit(阻塞应答 = 自等自死锁),绝不取 registry 锁;
//!   需要异步收尾的动作(Fatal 自动 stop、DNS 写系统 resolver)只经构造时
//!   存入的 `tokio::runtime::Handle` spawn 出去,sink 内绝不等待;
//! - start 在 `spawn_blocking` 执行(NodeHandle::start 含首次
//!   processBackgroundTasks 与 identity 生成/装载,毫秒级阻塞);成功后按
//!   registry net_local 逐网络 re-join(核心不持久化成员关系,
//!   networks.d/<nwid>.conf 只是 netconf 缓存,重启后须显式 join);
//! - stop 红线顺序:stopping=true → plane.teardown_all()(join 设备读线程)
//!   → node.shutdown()(join 驱动线程 → ZT_Node_delete)→ 移出 map →
//!   registry.running=false → 事件。shutdown 同样放 spawn_blocking。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use zt_core::{
    DevicePlane, FrameInTxSlot, IpNet, NetConfig, NetConfigOp, NetStatus, NodeEvent, NodeHandle,
    NodeParams, NodeSink,
};

use crate::dns;
use crate::events::Ring;
use crate::registry::{IdentityMode, NetLocal};
use crate::state::{AppState, LiveInfo, NetCache, NetSnapshot, PlanetRuntime};

/// nwid → NetLocal 的运行时镜像(sink 读,handler 写)
pub type NetLocalCache = Arc<Mutex<HashMap<u64, NetLocal>>>;

// ---- DaemonSink ----

/// 每 planet 实例的 sink:驱动线程回调 → Ring 事件 + DevicePlane 设备操作。
/// 字段全部为短临界区共享体;不持有 registry / planets 锁(见模块注释)。
pub struct DaemonSink {
    planet_id: String,
    planet_name: String,
    events: Arc<Ring>,
    plane: Arc<Mutex<DevicePlane>>,
    stopping: Arc<AtomicBool>,
    /// 设备→核心帧通道槽位(start 完成后由 start_planet 填充)
    frame_tx: FrameInTxSlot,
    net_local: NetLocalCache,
    /// 本网络冲突标记(与 PlanetRuntime.conflicts 同体共享)
    conflicts: Arc<Mutex<HashMap<u64, String>>>,
    /// 全部 planet 的网段快照(冲突扫描数据源)
    net_cache: NetCache,
    /// 构造时的 tokio runtime 句柄:sink 在驱动线程,异步收尾(Fatal 自动
    /// stop、DNS 写系统)全靠它 spawn,自身绝不阻塞等待
    rt: tokio::runtime::Handle,
    /// 共享状态的克隆(AppState 全为 Arc):Fatal 自动 stop 与 DNS 路径需要
    /// config_dir / events / runtime_gate。与 planets map 存在临时引用环,
    /// stop 摘除 NodeHandle 后即断开,不会泄漏
    app: AppState,
    /// Fatal 已触发自动收尾标记(防重入:Fatal 只触发一次 stop_planet)
    fatal_done: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl NodeSink for DaemonSink {
    fn on_event(&mut self, ev: NodeEvent) {
        // kind 一律 "status";Fatal 额外置 stopping(节点已不可用)
        let (msg, fatal) = match ev {
            NodeEvent::Up => ("节点已启动".to_string(), false),
            NodeEvent::Online => ("节点上线".to_string(), false),
            NodeEvent::Offline => ("节点离线".to_string(), false),
            NodeEvent::Down => ("核心已停止".to_string(), false),
            NodeEvent::IdentityCollision => ("身份冲突:与本 planet 已有节点地址相同".to_string(), true),
            NodeEvent::TransportChanged(m) => (format!("通道变更: {m}"), false),
            NodeEvent::Fatal(m) => (format!("致命错误: {m}"), true),
        };
        // 先记 status 事件,再触发自动收尾:保证 events 里 Fatal 在 autoStopped 之前
        self.events.push(&self.planet_id, "status", &msg);
        if fatal {
            self.stopping.store(true, Ordering::SeqCst);
            // Fatal 后核心已不可用,只记事件会留下僵尸实例(占端口/数据目录):
            // spawn 一次异步收尾(走 stop_planet 正常路径取 runtime_gate),
            // sink 不等待 —— 收尾任务里 node.shutdown 会 join 本驱动线程,
            // 若在此同步等待即自等自死锁
            if !self.fatal_done.swap(true, Ordering::SeqCst) {
                let app = self.app.clone();
                let pid = self.planet_id.clone();
                // daemon 退出中 runtime 已关闭:这里 spawn 的任务可能被静默
                // 取消(不会报错),退出路径的逐 planet stop_planet 自带同样
                // 的收尾兜底,故不关心句柄
                let handle = self.rt.spawn(async move {
                    app.events.push(
                        &pid,
                        "autoStopped",
                        "因致命错误自动收尾:实例已停止",
                    );
                    stop_planet(&app, &pid, StopMark::Stop).await;
                });
                drop(handle);
            }
        }
    }

    fn on_net_config(&mut self, nwid: u64, op: NetConfigOp, cfg: NetConfig) {
        // stop 正在拆设备面:不再 apply/teardown,直接返回
        if self.stopping.load(Ordering::SeqCst) {
            return;
        }
        match op {
            NetConfigOp::Down | NetConfigOp::Destroy => {
                lock(&self.plane).teardown(nwid);
                // 设备已拆,feth 登记随之收敛(崩溃孤儿清扫依据,见下)
                self.sync_iface_registry();
                let cleared = lock(&self.net_cache).remove(&(self.planet_id.clone(), nwid)).is_some();
                if cleared {
                    self.clear_conflict(nwid);
                }
                let what = if op == NetConfigOp::Destroy { "销毁" } else { "停用" };
                self.events.push(
                    &self.planet_id,
                    "netconf",
                    &format!("网络 {nwid:016x} 已{what}"),
                );
            }
            NetConfigOp::Up | NetConfigOp::ConfigUpdate => {
                // routeViaZeroTier=false:仅存地址不装路由(清空 managed routes 再 apply)
                let route_via = lock(&self.net_local)
                    .get(&nwid)
                    .map(|nl| nl.route_via_zero_tier)
                    .unwrap_or(true);
                let mut cfg = cfg;
                if !route_via {
                    cfg.routes.clear();
                }
                let r = lock(&self.plane).apply(&cfg, &self.frame_tx);
                // feth 登记落盘必须在崩溃前完成:此刻就把当前设备名快照写进
                // registry,kill -9 后启动清扫才有依据(见 sync_iface_registry)
                self.sync_iface_registry();
                // 快照先行更新,再据此扫描冲突(快照即本网络的对外网段事实)
                let had_conflict = self.update_snapshot(nwid, &cfg);
                let hit = self.scan_conflicts(nwid, &cfg);
                let status = status_name(&cfg.status);
                let assigned: Vec<String> =
                    cfg.assigned.iter().map(|a| format!("{}/{}", a.addr, a.prefix)).collect();
                let msg = format!(
                    "网络 {nwid:016x} status={status} rev={} assigned=[{}]",
                    cfg.netconf_revision,
                    assigned.join(", "),
                );
                self.events.push(&self.planet_id, "netconf", &msg);
                // 只在状态翻转时报事件,避免每次 ConfigUpdate 刷屏
                match (&hit, had_conflict) {
                    (Some(h), false) => {
                        self.events.push(&self.planet_id, "conflict", &h.message(nwid))
                    }
                    (None, true) => self.events.push(
                        &self.planet_id,
                        "conflict",
                        &format!("网络 {nwid:016x} 网段冲突已消除"),
                    ),
                    _ => {}
                }
                if let Err(e) = r {
                    self.events.push(
                        &self.planet_id,
                        "netconf",
                        &format!("网络 {nwid:016x} 设备应用失败: {e}"),
                    );
                }
                // DNS 写系统 resolver:dnsMode=network 用 netconf 下发的
                // dns_servers,=custom 用本地配置;servers 为空(none /
                // network 未下发 / custom 无配置)即「本网络不再托管」,若当前
                // 标记正归本网络所有则立即还原。domain 是网络下发的搜索域,
                // 仅 Windows NRPT(按域分流)消费,posix 忽略。networksetup /
                // powershell 阻塞且可能失败,放 spawn_blocking 执行、失败仅记
                // 事件;全程持 dns_gate,与 net_leave / stop 的 restore 及其它
                // 网络的 apply 互斥(标记文件的 check-then-act 不能交叠)。
                // 锁只包 DNS 操作,设备面与冲突扫描已在上方完成,不受影响
                let nl_dns = lock(&self.net_local).get(&nwid).cloned();
                let servers: Vec<String> = match nl_dns.as_ref().map(|n| n.dns_mode.as_str()) {
                    Some("network") => cfg.dns_servers.iter().map(|s| s.to_string()).collect(),
                    Some("custom") => nl_dns.map(|n| n.custom_dns).unwrap_or_default(),
                    _ => Vec::new(),
                };
                let dns_domain = cfg.dns_domain.clone();
                let app = self.app.clone();
                let pid = self.planet_id.clone();
                let events = Arc::clone(&self.events);
                self.rt.spawn(async move {
                    let _gate = app.dns_gate.lock().await;
                    if servers.is_empty() {
                        // restore 内部校验 owner(planet id + nwid),不匹配
                        // 即不动,沿用「后应用者拥有」语义;无标记/非本网络
                        // 同样静默跳过
                        tokio::task::spawn_blocking(move || {
                            dns::restore(&app.config_dir, &events, &pid, Some(nwid));
                        })
                        .await
                        .ok();
                    } else {
                        tokio::task::spawn_blocking(move || {
                            dns::apply(
                                &app.config_dir,
                                &events,
                                &pid,
                                nwid,
                                dns_domain.as_deref(),
                                &servers,
                            );
                        })
                        .await
                        .ok();
                    }
                });
            }
        }
    }

    fn on_frame(&mut self, nwid: u64, src_mac: u64, dst_mac: u64, ether_type: u16, payload: &[u8]) {
        // 方向:核心→设备(FrameIn 通道是反方向,互不混用)
        if let Err(e) =
            lock(&self.plane).send_frame(nwid, src_mac, dst_mac, ether_type, payload)
        {
            eprintln!("[sink {}] 帧写设备失败: {e}", self.planet_id);
        }
    }

    fn local_ipv4s(&self) -> Vec<std::net::Ipv4Addr> {
        lock(&self.plane).applied_v4s()
    }

    /// 设备健康巡检(驱动线程 10s 节拍):适配器被外部重启等失效时设备面
    /// 自愈重建,重建发生记事件供排查(stop 收尾期不巡检,与 on_net_config 同
    /// 语义)。仅 Windows 覆写:Unix 设备面无 health_tick,沿用 trait 默认空实现
    #[cfg(windows)]
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
}

impl DaemonSink {
    /// 把当前 feth 设备名快照登记进 registry(崩溃孤儿清扫的依据:kill -9
    /// 后 Drop 不执行,只有落过盘的名字才可能在下次启动被销毁)。
    ///
    /// 本函数在驱动线程被调:只做 plane 短锁取快照 + spawn,锁与 IO 全在
    /// 任务内(见模块注释的 sink 红线);快照与登记值一致时跳过写盘,
    /// netconf 反复 ConfigUpdate 不会反复落盘;entry 已不运行(并发收尾已
    /// 置 running=false)时跳过,避免把快照写回已清空的登记。
    fn sync_iface_registry(&self) {
        let app = self.app.clone();
        let pid = self.planet_id.clone();
        let plane = Arc::clone(&self.plane);
        let handle = self.rt.spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                // 快照在任务内重读而非沿用 sink 时刻的旧值:netconf 触发的
                // 多个登记任务在阻塞线程池无序执行,旧快照后落盘会覆盖新
                // 值,崩溃清扫据此漏设备。HashMap 遍历序不定,排序后再
                // 比对/落盘,避免同值不同序误判变更
                let mut names = lock(&plane).iface_names();
                names.sort();
                let mut reg = app
                    .registry
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) else {
                    return;
                };
                if !e.running || e.ifaces == names {
                    return;
                }
                e.ifaces = names;
                if let Err(err) = reg.save(&app.config_dir) {
                    eprintln!("[runtime] registry 写盘失败: {err}");
                }
            })
            .await;
        });
        drop(handle);
    }

    /// 更新本网络快照;返回「更新前是否带冲突标记」(供冲突消除事件判定)。
    /// 仅 status=Ok 时登记(REQUESTING 等阶段 assigned/routes 无意义)。
    fn update_snapshot(&self, nwid: u64, cfg: &NetConfig) -> bool {
        let had = lock(&self.conflicts).contains_key(&nwid);
        let mut cache = lock(&self.net_cache);
        if cfg.status == NetStatus::Ok {
            let mut subnets: Vec<IpNet> = cfg.assigned.clone();
            subnets.extend(cfg.routes.iter().map(|r| r.target.clone()));
            cache.insert(
                (self.planet_id.clone(), nwid),
                NetSnapshot {
                    planet_name: self.planet_name.clone(),
                    subnets,
                    addrs: cfg.assigned.iter().map(|a| a.addr).collect(),
                },
            );
        } else {
            cache.remove(&(self.planet_id.clone(), nwid));
        }
        had
    }

    /// 冲突扫描:本网络网段与其它 planet 的快照网段重叠 → subnet;
    /// 重叠且同 IP → address(address 级优先)。写入 conflicts 表并返回命中。
    fn scan_conflicts(&self, nwid: u64, cfg: &NetConfig) -> Option<ConflictHit> {
        let subnets: Vec<IpNet> = cfg
            .assigned
            .iter()
            .cloned()
            .chain(cfg.routes.iter().map(|r| r.target.clone()))
            .collect();
        let addrs: Vec<IpAddr> = cfg.assigned.iter().map(|a| a.addr).collect();
        let cache = lock(&self.net_cache);
        let mut found: Option<ConflictHit> = None;
        for ((pid, _nid), snap) in cache.iter() {
            // 只与其它 planet 比:同一网络经两个 planet 接入时网段/IP 冲突
            // 最要紧,故不按 nwid 跳过,仅排除本 planet 自身的网络
            if *pid == self.planet_id {
                continue;
            }
            let nets: Vec<String> = subnets
                .iter()
                .filter(|s| snap.subnets.contains(*s))
                .map(|s| format!("{}/{}", s.addr, s.prefix))
                .collect();
            if nets.is_empty() {
                continue;
            }
            let level = if snap.addrs.iter().any(|a| addrs.contains(a)) {
                "address"
            } else {
                "subnet"
            };
            let worse = found.as_ref().map(|f| f.level != "address").unwrap_or(true);
            if worse {
                found = Some(ConflictHit {
                    level: level.to_string(),
                    other_planet: snap.planet_name.clone(),
                    other_planet_id: pid.clone(),
                    subnets: nets,
                });
            }
        }
        drop(cache);
        if let Some(h) = &found {
            lock(&self.conflicts).insert(nwid, h.level.clone());
        } else {
            lock(&self.conflicts).remove(&nwid);
        }
        found
    }

    /// 冲突标记清除(Down/Destroy 或冲突消除)
    fn clear_conflict(&self, nwid: u64) {
        lock(&self.conflicts).remove(&nwid);
    }
}

/// 一次冲突命中的详情(事件文案 + conflicts 表的级别标记)
struct ConflictHit {
    /// "subnet" | "address"
    level: String,
    other_planet: String,
    other_planet_id: String,
    /// 重叠网段,"a.b.c.d/p"
    subnets: Vec<String>,
}

impl ConflictHit {
    fn message(&self, nwid: u64) -> String {
        format!(
            "网络 {nwid:016x} 与 planet \"{}\"({})网段冲突({}):{}",
            self.other_planet,
            self.other_planet_id,
            self.level,
            self.subnets.join(", ")
        )
    }
}

/// NetStatus → 大写蛇形字符串(REST 契约定义)
pub fn status_name(s: &NetStatus) -> &'static str {
    match s {
        NetStatus::RequestingConfiguration => "REQUESTING_CONFIGURATION",
        NetStatus::Ok => "OK",
        NetStatus::AccessDenied => "ACCESS_DENIED",
        NetStatus::NotFound => "NOT_FOUND",
        NetStatus::PortError => "PORT_ERROR",
        NetStatus::ClientTooOld => "CLIENT_TOO_OLD",
        NetStatus::AuthenticationRequired => "AUTHENTICATION_REQUIRED",
    }
}

// ---- start / stop ----

/// 拉起一个 planet 实例。失败时已完成登记(事件 planetStartFailed +
/// registry.running=false),调用方拿 Err(message) 转 REST 错误即可。
/// 已在运行则幂等成功。成功后按 registry net_local 逐网络 re-join
/// (重启恢复;Core 不持久化成员关系,见函数尾部注释)。
pub async fn start_planet(state: &AppState, id: &str) -> Result<(), String> {
    // 启停全程持门闩:防并发双 start 双实例(后者覆盖前者即泄漏线程/端口)
    let _gate = state.runtime_gate.lock().await;
    let pid = id.to_string();
    if state.planets.lock().unwrap().contains_key(&pid) {
        return Ok(());
    }
    // 门闩内重读 registry 取新鲜 entry:调用方的克隆可能与 disable 落盘
    // 交错(读时 enabled=true、启动时已 false),陈旧克隆会放行已禁用的
    // planet。禁用是持久的"别跑"意志,一律拒绝(enable/boot/join 全走这)
    let entry = {
        let reg = state.registry.lock().unwrap();
        match reg.planets.iter().find(|e| e.id == pid) {
            Some(e) => e.clone(),
            None => {
                return Err(format!("planet \"{pid}\" is not registered"));
            }
        }
    };
    if !entry.enabled {
        return Err(format!(
            "planet \"{}\" is disabled; enable it first",
            crate::api_planet::name_of(&entry)
        ));
    }
    let planet_file = state.config_dir.join("planets").join(format!("{pid}.bin"));
    let plane = Arc::new(Mutex::new(DevicePlane::new()));
    let frame_tx: FrameInTxSlot = Arc::new(OnceLock::new());
    let stopping = Arc::new(AtomicBool::new(false));
    let conflicts = Arc::new(Mutex::new(HashMap::new()));
    // registry.net_local(nwid hex 字符串键)→ 运行时镜像(u64 键);非法键跳过
    let net_local: NetLocalCache = Arc::new(Mutex::new(
        entry
            .net_local
            .iter()
            .filter_map(|(k, v)| u64::from_str_radix(k, 16).ok().map(|n| (n, v.clone())))
            .collect(),
    ));
    let sink = DaemonSink {
        planet_id: pid.clone(),
        planet_name: crate::api_planet::name_of(&entry),
        events: Arc::clone(&state.events),
        plane: Arc::clone(&plane),
        stopping: Arc::clone(&stopping),
        frame_tx: Arc::clone(&frame_tx),
        net_local: Arc::clone(&net_local),
        conflicts: Arc::clone(&conflicts),
        net_cache: Arc::clone(&state.net_cache),
        // 当前异步上下文的 runtime 句柄与共享状态:供 sink 在驱动线程发起
        // Fatal 自动收尾与 DNS 应用(见 DaemonSink 字段注释)
        rt: tokio::runtime::Handle::current(),
        app: state.clone(),
        fatal_done: AtomicBool::new(false),
    };
    let identity_mode = {
        let reg = state
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.identity_mode
    };
    let params = NodeParams {
        data_dir: entry.data_dir.clone(),
        planet_file,
        bind_port: entry.port,
        identity_dir: match identity_mode {
            IdentityMode::Shared => Some(state.config_dir.clone()),
            IdentityMode::Independent => None,
        },
    };
    // start 含首次 processBackgroundTasks(阻塞),放阻塞线程池
    let node = tokio::task::spawn_blocking(move || {
        NodeHandle::start(params, sink).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("启动任务失败: {e}"))?
    .map_err(|e| start_failed(state, &pid, &e.to_string()))?;

    frame_tx
        .set(node.frame_in_tx())
        .map_err(|_| "FrameInTx 槽位已占用".to_string())
        .map_err(|m| start_failed(state, &pid, &m))?;

    let node_id = format!("{:010x}", node.address());
    let rt = Arc::new(PlanetRuntime {
        planet_id: pid.clone(),
        node: Mutex::new(Some(node)),
        plane,
        stopping,
        conflicts,
        net_local: Arc::clone(&net_local),
    });
    state.planets.lock().unwrap().insert(pid.clone(), Arc::clone(&rt));
    {
        let mut reg = state.registry.lock().unwrap();
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) {
            e.running = true;
            e.last_node_id = Some(node_id);
        }
        if let Err(e) = reg.save(&state.config_dir) {
            eprintln!("[runtime] registry 写盘失败: {e}");
        }
    }
    state.events.push(&pid, "planetStarted", "planet started");

    // 重启恢复:registry net_local 中的网络必须显式 re-join。Core 的
    // networks.d/<nwid>.conf 只是 netconf 缓存,不承载成员关系,核心重启后
    // 不会自动恢复 join。失败仅记事件不判 start 失败(网络未授权时核心本就
    // 静默,join 调用自身几乎不会失败;个别失败不应把整个实例拉回 stopped)。
    let nwids: Vec<u64> = lock(&rt.net_local).keys().copied().collect();
    for nwid in nwids {
        let rt2 = Arc::clone(&rt);
        match tokio::task::spawn_blocking(move || rt2.node_join(nwid)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => state.events.push(
                &pid,
                "join",
                &format!("重启恢复 re-join 网络 {nwid:016x} 失败: {e}"),
            ),
            Err(e) => {
                eprintln!("[runtime] re-join 任务失败: {e}");
            }
        }
    }
    Ok(())
}

/// 启动失败登记:事件 planetStartFailed + registry.running=false(重启恢复
/// 与 REST 共用),返回 message 供上层转错误体。
fn start_failed(state: &AppState, pid: &str, message: &str) -> String {
    state.events.push(pid, "planetStartFailed", message);
    let mut reg = state.registry.lock().unwrap();
    if let Some(e) = reg.planets.iter_mut().find(|e| e.id == pid) {
        e.running = false;
    }
    if let Err(e) = reg.save(&state.config_dir) {
        eprintln!("[runtime] registry 写盘失败: {e}");
    }
    message.to_string()
}

/// stop 的登记语义。registry 的 `running` 字段是**期望状态**:
/// - [`StopMark::Stop`]:无条件停(disable / 致命自停 / 删除收尾),
///   落 `running=false`;
/// - [`StopMark::Idle`]:资源性停止(leave 清空全部网络),持门闩后按
///   派生规则重评——`enabled ∧ net_local 非空` 即**跳过停机**(并发 join
///   恰在判空后补进网络时保住实例),disable 走 Stop 不受影响;
/// - [`StopMark::ShutdownKeep`]:daemon 优雅退出(SIGTERM),保留 `running=true`,
///   daemon 下次启动时按既有的恢复逻辑自动拉起。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopMark {
    Stop,
    Idle,
    ShutdownKeep,
}

/// 停一个 planet 实例(红线顺序)。返回 false = 本就未运行(幂等)。
/// registry 中 running=true 但运行时缺失的残留(如上次启动失败)也会被纠正。
pub async fn stop_planet(state: &AppState, id: &str, mark: StopMark) -> bool {
    // 与 start_planet 同一门闩,防启停交叠
    let _gate = state.runtime_gate.lock().await;
    // Idle(资源性停止)在此重评派生规则:门闩已与 start_planet 互斥,
    // 此刻 registry 的 enabled/net_local 即终局值——并发 join 已落盘的
    // 网络保住实例,不再停机(disable 走 Stop 无条件停,不受此影响)
    if mark == StopMark::Idle {
        let should_run = {
            let reg = state.registry.lock().unwrap();
            reg.planets
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.enabled && !e.net_local.is_empty())
                .unwrap_or(false)
        };
        if should_run {
            return false;
        }
    }
    // 先短锁取运行时 Arc(不从 map 摘除,红线顺序见 teardown_runtime),锁即释放:
    // 后续任何路径都不得带着 planets 锁碰 registry
    // (net_join 等是 registry → planets 的取用序,反向嵌套会成环)
    let rt = state.planets.lock().unwrap().get(id).cloned();
    let Some(rt) = rt else {
        // 无运行时但 registry 仍标 running:残留状态。显式 stop 才纠正登记;
        // 优雅退出保持期望状态,留给下次启动恢复
        if mark == StopMark::ShutdownKeep {
            state.events.push(
                id,
                "planetStopped",
                "planet stopped (daemon 优雅退出,重启后自动恢复)",
            );
            return true;
        }
        let mut reg = state.registry.lock().unwrap();
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == id) {
            if e.running {
                e.running = false;
                e.last_node_id = None;
                if let Err(err) = reg.save(&state.config_dir) {
                    eprintln!("[runtime] registry 写盘失败: {err}");
                }
                drop(reg);
                state.events.push(id, "planetStopped", "planet stopped");
                return true;
            }
        }
        return false;
    };
    // 1. 置 stopping → 2. 拆设备面/关核心 → 3. 摘出 map → 4. registry + 事件
    // (红线顺序集中在 teardown_runtime;摘出 map 后 handler 若仍持 Arc,其
    //  node 槽位必空,join/leave 走「实例已停止」错误路径,查询按已停处理)
    teardown_runtime(state, rt, mark).await;
    true
}

/// 对单个运行时执行红线收尾(调用方须已持 `runtime_gate`):
/// stopping → plane.teardown_all → node.shutdown → 摘出 map → registry → 事件。
/// stop_planet 走的正是这段;planets_delete 在 gate 内发现「running=false 但
/// map 仍有运行时」的异常残留时也用它收尾,避免 NodeHandle 走 Drop 兜底。
/// 停止期间 handler 仍可能持同一 Arc:node 槽位已空,join/leave 返回
/// 「实例已停止」,查询一律按已停处理,不会 panic。
async fn teardown_runtime(state: &AppState, rt: Arc<PlanetRuntime>, mark: StopMark) {
    // 1. 置 stopping:sink 见即不再动设备;node 槽位取空(此后查询视为已停)
    rt.stopping.store(true, Ordering::SeqCst);
    let node = {
        let mut g = rt.node.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.take()
    };
    // 2. 先拆设备面(join 全部设备读线程),再关核心(join 驱动线程 → delete)
    lock(&rt.plane).teardown_all();
    if let Some(node) = node {
        let _ = tokio::task::spawn_blocking(move || node.shutdown()).await;
    }
    // 3. 核心已停,此刻才从 map 摘除(gate 在手,map 内不会换入新运行时)
    state.planets.lock().unwrap().remove(&rt.planet_id);
    // 4. 快照/冲突缓存清理 + registry + 事件
    state
        .net_cache
        .lock()
        .unwrap()
        .retain(|(p, _), _| *p != rt.planet_id);
    // registry 锁放进独立作用域:MutexGuard 非 Send,async fn 的 generator
    // 按变量作用域判定跨 await 持有,显式 drop 不影响该判定
    {
        let mut reg = state.registry.lock().unwrap();
        if let Some(e) = reg.planets.iter_mut().find(|e| e.id == rt.planet_id) {
            // 期望状态:优雅退出保留 true 供下次启动恢复,其余一律落 false。
            // last_node_id 是展示用的上次节点地址(身份本体在 dataDir),保留
            if mark != StopMark::ShutdownKeep {
                e.running = false;
            }
            // 设备已随 teardown 销毁,登记清空(sink 后续的登记任务见
            // running=false 也会自行跳过)
            e.ifaces.clear();
        }
        if let Err(e) = reg.save(&state.config_dir) {
            eprintln!("[runtime] registry 写盘失败: {e}");
        }
    }
    let stop_msg = match mark {
        StopMark::ShutdownKeep => "planet stopped (daemon 优雅退出,重启后自动恢复)",
        StopMark::Idle => "planet idled (no networks)",
        StopMark::Stop => "planet stopped",
    };
    state.events.push(&rt.planet_id, "planetStopped", stop_msg);
    // DNS:系统 resolver 若仍归本 planet 所有(stop/删除/shutdown 共用此路径)
    // 则还原为自动获取;owner 不匹配(被后应用者覆盖)时内部自行跳过。
    // networksetup 阻塞,放阻塞线程池;失败仅记事件,不影响收尾结果。
    // 持 dns_gate:与 netconf apply / net_leave restore 互斥,标记与系统状态
    // 不能交错写。此处已无其它网络面逻辑,跨 await 持异步锁无碍
    let _dns_gate = state.dns_gate.lock().await;
    {
        let app = state.clone();
        let pid = rt.planet_id.clone();
        let events = Arc::clone(&state.events);
        let _ = tokio::task::spawn_blocking(move || {
            dns::restore(&app.config_dir, &events, &pid, None)
        })
        .await;
    }
}

/// 已持 `runtime_gate` 的调用方(如 planets_delete)用:该 id 若仍有运行时
/// 残留(running=false 但 map 未清),按红线收尾,避免 NodeHandle 走 Drop 兜底。
/// 返回是否收尾了运行时。
pub async fn teardown_leftover(state: &AppState, id: &str) -> bool {
    let rt = state.planets.lock().unwrap().get(id).cloned();
    match rt {
        Some(rt) => {
            teardown_runtime(state, rt, StopMark::Stop).await;
            true
        }
        None => false,
    }
}

/// 各 planet 的实时状态快照(GET /status、/planets、/planets/{id} 共用)。
/// node 查询在 planets 锁外做(先短锁取 Arc 列表)。
pub fn live_infos(state: &AppState) -> HashMap<String, LiveInfo> {
    let rts: Vec<Arc<PlanetRuntime>> = state.planets.lock().unwrap().values().cloned().collect();
    let (major, minor, rev) = NodeHandle::core_version();
    let core_version = format!("{major}.{minor}.{rev}");
    rts.into_iter()
        .map(|rt| {
            let online = rt.online();
            let info = LiveInfo {
                online,
                node_id: rt.node_id().unwrap_or_default(),
                core_version: core_version.clone(),
                // PLANET 角色对端的最小延迟(无则 None)
                latency_ms: if online {
                    rt.peers()
                        .iter()
                        .filter(|p| p.role == zt_core::PeerRole::Planet)
                        .filter_map(|p| p.latency_ms)
                        .min()
                        .map(|v| v.max(0) as u64)
                } else {
                    None
                },
                network_count: rt.networks().len(),
            };
            (rt.planet_id.clone(), info)
        })
        .collect()
}
