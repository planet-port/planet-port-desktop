//! 崩溃孤儿 feth 清扫:daemon 被 kill -9(崩溃 / launchd 拉起)时 Drop 不
//! 执行,feth 设备对(dev 端 + I/O peer 端)与挂在其上的路由全部残留;
//! 下一轮实例新建 pair 后,同网段路由因 EEXIST 装不上,数据面还可能走旧
//! 孤儿设备(无人读写)而断网。故启动早期按 registry 登记销毁孤儿。
//!
//! # 红线
//!
//! - 只销毁 registry `ifaces` 里登记过的名字,绝不枚举/猜测系统 feth:
//!   本机官方 zerotier-one 有一批常驻 feth(feth194/345/2527/4684/…),
//!   误销毁会打断其数据面;
//! - 全部走 `std::process::Command`,调用方(main 启动早期)已在
//!   `spawn_blocking` 内执行,本模块不做任何异步操作;
//! - 所有失败只 eprintln,不向上返回错误:清扫失败不该挡住 daemon 启动。

use std::process::Command;

use crate::state::AppState;

/// `ifconfig <name>` 查询接口并解析 peer 端名。返回 `Some(peer)` 表示接口
/// 存在(peer 可为空:无 `peer:` 行);`None` 表示接口不存在或命令执行失败。
///
/// 真实输出形如(feth 对的 dev 端):
///
/// ```text
/// feth123: flags=...<UP,...> mtu 1500
///     ether aa:bb:cc:dd:ee:ff
///     peer: feth124
/// ```
fn probe_iface(name: &str) -> Option<String> {
    let out = Command::new("ifconfig").arg(name).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == "peer" {
                return Some(v.trim().to_string());
            }
        }
    }
    Some(String::new())
}

/// `ifconfig <name> destroy`。失败只打日志并返回 false:名字留在
/// registry 里,下次启动重试——清掉登记会让孤儿永久失去清扫依据。
fn destroy_iface(name: &str) -> bool {
    match Command::new("ifconfig").args([name, "destroy"]).output() {
        Ok(o) if o.status.success() => {
            eprintln!("[cleanup] 销毁孤儿 feth {name}");
            true
        }
        Ok(o) => {
            eprintln!(
                "[cleanup] 销毁 {name} 失败(下次启动重试): {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            false
        }
        Err(e) => {
            eprintln!("[cleanup] 销毁 {name} 失败(下次启动重试): {e}");
            false
        }
    }
}

/// 启动早期清扫(kill -9 未走 stop 路径):销毁 registry 登记的全部孤儿
/// feth 对,然后把登记清空落盘。在恢复 planet 之前调用——此刻本 daemon
/// 尚未创建任何设备,销毁不可能误伤活设备。无登记时为 no-op。
pub fn destroy_leftover(state: &AppState) {
    // 短锁收集登记名(去重);无登记直接返回,不写盘
    let names: Vec<String> = {
        let reg = state
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut v: Vec<String> =
            reg.planets.iter().flat_map(|e| e.ifaces.iter().cloned()).collect();
        v.sort();
        v.dedup();
        v
    };
    if names.is_empty() {
        return;
    }
    // 销毁失败的名字:保留在登记里下次重试,否则该孤儿永久失去清扫依据
    let mut failed: Vec<String> = Vec::new();
    for name in &names {
        // 接口不存在(上次已正常销毁 / 名字失效)→ 跳过
        let Some(peer) = probe_iface(name) else { continue };
        if !destroy_iface(name) {
            failed.push(name.clone());
        }
        // feth 对的 peer 端是独立的内核接口,须一并销毁;此刻 peer 可能已
        // 随 dev 端销毁,存在才动手。peer 不在登记之列,失败时也一并记下,
        // 下轮探测它会因 dev 端销毁成功而自然消失
        if !peer.is_empty() && probe_iface(&peer).is_some() && !destroy_iface(&peer) {
            failed.push(peer);
        }
    }
    // 处理完后收缩登记:确认不存在或销毁成功的移除,失败的保留
    let mut reg = state
        .registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for e in reg.planets.iter_mut() {
        e.ifaces.retain(|n| failed.contains(n));
    }
    if let Err(e) = reg.save(&state.config_dir) {
        eprintln!("[cleanup] registry 写盘失败: {e}");
    }
}
