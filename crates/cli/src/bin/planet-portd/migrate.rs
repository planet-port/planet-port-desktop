//! registry 遗留条目通用迁移(daemon 启动、恢复 planet 前执行一次)。
//!
//! 1. dataDir 归位:不在 <config>/data/ 下的条目(如 POC 时代指向项目
//!    target/ 的路径)整树搬迁到 <config>/data/<id>/;
//! 2. 端口卫生:port_explicit=false 的条目从 9993 起重探测
//!    (9994 等 POC 遗留自然归位)。
//!
//! 容错:单条搬迁失败保留原路径继续运行(仅告警事件),不阻断启动;
//! 迁移只在启动时做一次,运行中不触发。

use std::collections::HashSet;
use std::fs;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};

use crate::state::AppState;

/// 迁移入口。须在 events 系统可用后、恢复 planet 实例之前调用。
pub fn migrate(st: &AppState) {
    let mut reg = st.registry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let data_root = st.config_dir.join("data");
    let _ = fs::create_dir_all(&data_root);
    let mut changed = false;

    // ---- 1. dataDir 归位 ----
    let root = fs::canonicalize(&data_root).unwrap_or_else(|_| data_root.clone());
    let entries: Vec<(usize, PathBuf)> = reg
        .planets
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let old = fs::canonicalize(&e.data_dir).unwrap_or_else(|_| e.data_dir.clone());
            !old.starts_with(&root)
        })
        .map(|(i, e)| (i, fs::canonicalize(&e.data_dir).unwrap_or_else(|_| e.data_dir.clone())))
        .collect();

    // 守卫:条目间嵌套。A 的 dataDir 严格包含另一条目的 dataDir 时,A 先迁
    // 会把 B 的整树拖进 A 的新位置,B 下次启动静默重置身份。跳过外层 A
    // (不迁、告警、原路径继续运行),保 B 留在原地独立搬迁。
    let nested_outer: HashSet<usize> = entries
        .iter()
        .filter(|(a, da)| {
            entries.iter().any(|(b, db)| {
                a != b && db.starts_with(da.as_path()) && da.as_path() != db.as_path()
            })
        })
        .map(|(a, _)| *a)
        .collect();

    for (i, old) in entries {
        let id = reg.planets[i].id.clone();
        // 守卫:自嵌套。old 是数据根的祖先(如 dataDir 指 config-dir 本身,
        // 或更上层)时,归位等于把数据根塞进自身子孙:rename 报 EINVAL,
        // 回退 copy 逐层下钻无底。放弃自动归位,须手工修正 registry。
        if root.starts_with(&old) {
            st.events.push(
                &id,
                "registryMigrateFailed",
                "dataDir 为数据根祖先,放弃自动归位(须手工修正 registry)",
            );
            continue;
        }
        if nested_outer.contains(&i) {
            st.events.push(
                &id,
                "registryMigrateFailed",
                "dataDir 与其他条目 dataDir 嵌套,放弃自动归位(须手工处理)",
            );
            continue;
        }
        let new_dir = data_root.join(&id);
        match relocate(&old, &new_dir) {
            Ok(()) => {
                let settled = fs::canonicalize(&new_dir).unwrap_or(new_dir);
                st.events.push(
                    &id,
                    "registryMigrated",
                    &format!("dataDir {} → {}", old.display(), settled.display()),
                );
                reg.planets[i].data_dir = settled;
                changed = true;
            }
            Err(e) => {
                // 保留原路径继续运行:数据可达性优先于布局规范
                st.events.push(
                    &id,
                    "registryMigrateFailed",
                    &format!("dataDir 搬迁失败(保留原路径): {}", e),
                );
            }
        }
    }

    // ---- 2. 端口卫生(显式指定者不动) ----
    // used 只登记「不可动」与「已定案」的端口:显式端口的条目,以及本轮
    // 已探测完的条目。自身端口不算占用——已在 9993+ 且可绑的端口保持原值,
    // 否则每次重启都会被挤向下一个空闲口,端口随重启次数无限漂移
    let mut used: HashSet<u16> = reg
        .planets
        .iter()
        .filter(|e| e.port_explicit)
        .map(|e| e.port)
        .collect();
    for e in reg.planets.iter_mut() {
        if e.port_explicit {
            continue;
        }
        let Some(p) = (9993..=u16::MAX).find(|p| !used.contains(p) && udp_free(*p)) else {
            // 无可用口:保持原值继续(探测只是收口,不阻断启动)
            used.insert(e.port);
            continue;
        };
        if p != e.port {
            st.events.push(
                &e.id,
                "registryMigrated",
                &format!("port {} → {}", e.port, p),
            );
            e.port = p;
            changed = true;
        }
        used.insert(p);
    }

    if changed {
        if let Err(e) = reg.save(&st.config_dir) {
            eprintln!("[migrate] registry 写回失败: {e}");
        }
    }
}

fn udp_free(p: u16) -> bool {
    UdpSocket::bind(("0.0.0.0", p)).is_ok()
}

/// 整树搬迁:优先 rename(同卷零拷贝),跨设备回退递归 copy+校验+删源。
fn relocate(old: &Path, new: &Path) -> std::io::Result<()> {
    if !old.exists() {
        // 原目录不存在(如被 cargo clean):直接用新目录,无数据可搬
        fs::create_dir_all(new)?;
        return Ok(());
    }
    fs::create_dir_all(new.parent().unwrap_or(new))?;
    if fs::rename(old, new).is_ok() {
        return Ok(());
    }
    // 跨设备:递归拷贝后逐文件校验大小,全对才删源
    copy_dir(old, new)?;
    verify_dir(old, new)?;
    fs::remove_dir_all(old).map_err(|e| {
        // 删源失败不视为搬迁失败(新副本已可用),仅日志
        eprintln!("[migrate] 旧目录删除失败(可手工清理): {e}");
        e
    })?;
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for ent in fs::read_dir(src)? {
        let ent = ent?;
        let ty = ent.file_type()?;
        let to = dst.join(ent.file_name());
        if ty.is_dir() {
            copy_dir(&ent.path(), &to)?;
        } else {
            fs::copy(ent.path(), &to)?;
        }
    }
    Ok(())
}

fn verify_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    for ent in fs::read_dir(src)? {
        let ent = ent?;
        let ty = ent.file_type()?;
        let to = dst.join(ent.file_name());
        if ty.is_dir() {
            verify_dir(&ent.path(), &to)?;
        } else {
            let a = fs::metadata(ent.path())?.len();
            let b = fs::metadata(&to).map_err(|e| {
                std::io::Error::other(format!("校验缺文件 {}: {e}", to.display()))
            })?;
            if a != b.len() {
                return Err(std::io::Error::other(format!(
                    "校验大小不符: {}",
                    to.display()
                )));
            }
        }
    }
    Ok(())
}
