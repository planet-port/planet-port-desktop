//! dataStore 对象 ↔ 落盘文件。语义平移 Android NodeRuntime.stateFile:
//! identity 跨 planet 共享于根;moon/peer/netconf 按 planet worldId 分目录;
//! planet 重定向到注入文件(get)/根下 planet(put 回写)。

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct StateStore {
    root: PathBuf,
    world_dir: PathBuf,
    planet_in: PathBuf,
}

impl StateStore {
    /// planet_in 必填:node 启动即校验 planet 文件可读,StateStore 不再处理未注入分支。
    pub fn new(root: PathBuf, world_id: u64, planet_in: PathBuf) -> Self {
        let world_dir = root.join(world_id.to_string());
        Self { root, world_dir, planet_in }
    }

    /// 返回 None = 未知对象(get/put 各按错误码上报)。
    fn file_for(&self, obj: zt_obj, id: &[u64; 2]) -> Option<PathBuf> {
        use zt_obj::*;
        match obj {
            IdentityPublic => Some(self.root.join("identity.public")),
            IdentitySecret => Some(self.root.join("identity.secret")),
            // get 读注入的 planet 副本(start 已保证存在);put 回写根下 planet
            PlanetGet => Some(self.planet_in.clone()),
            PlanetPut => Some(self.root.join("planet")),
            Moon => Some(self.world_dir.join(format!("moons.d/{:016x}.moon", id[0]))),
            Peer => Some(self.world_dir.join(format!("peers.d/{:010x}.peer", id[0]))),
            NetworkConfig => Some(self.world_dir.join(format!("networks.d/{:016x}.conf", id[0]))),
        }
    }

    /// 返回字节数;未找到 -1;IO 错误 -2。buffer 为 NULL 时仅探测大小。
    ///
    /// # Safety
    /// `buf` 为 NULL 或指向可写 `cap` 字节的缓冲;`id` 指向 2 个 u64。
    pub unsafe fn get(&self, obj: zt_obj, id: &[u64; 2], buf: *mut u8, cap: usize) -> isize {
        let Some(f) = self.file_for(obj, id) else { return -2 };
        let data = match fs::read(&f) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return -1,
            Err(_) => return -2,
        };
        if buf.is_null() {
            return data.len() as isize;
        }
        let n = data.len().min(cap);
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, n); }
        n as isize
    }

    /// len == -1 表示删除。返回 0 成功,-1 失败。
    ///
    /// # Safety
    /// `len > 0` 时 `data` 须指向至少 `len` 字节可读内存。
    pub unsafe fn put(
        &self,
        obj: zt_obj,
        id: &[u64; 2],
        data: *const u8,
        len: isize,
        secure: bool,
    ) -> isize {
        let Some(f) = self.file_for(obj, id) else { return -1 };
        if len == -1 {
            let _ = fs::remove_file(&f);
            return 0;
        }
        if data.is_null() || len < 0 {
            return -1;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
        if let Some(p) = f.parent() {
            let _ = fs::create_dir_all(p);
        }
        let tmp = f.with_extension("tmp");
        if fs::write(&tmp, bytes).is_err() {
            return -1;
        }
        if fs::rename(&tmp, &f).is_err() {
            let _ = fs::remove_file(&tmp);
            return -1;
        }
        // identity.secret(与 secure 标记)收紧为 0600
        if secure {
            let _ = fs::set_permissions(&f, fs::Permissions::from_mode(0o600));
        }
        0
    }
}

/// 把 C 枚举收敛为 store 内部对象种类(node.rs 的 glue 里做数值映射)。
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub enum zt_obj { IdentityPublic, IdentitySecret, PlanetGet, PlanetPut, Moon, Peer, NetworkConfig }
