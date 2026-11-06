//! planet 注册表:已导入 planet 的持久化清单(`<config_dir>/registry.json`)。
//! 仅在发生变更(import 等,Task 3 起)时写盘;启动时若文件缺失/损坏按空注册表处理。

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// daemon 级身份模式:Shared = 所有 planet 共享 config 根的全局身份
/// (identity.public/secret,缺省);Independent = 每 planet 用各自
/// dataDir 内的独立身份。全局开关,不按 planet 单独设置。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityMode {
    #[default]
    Shared,
    Independent,
}

/// 一条已导入的 planet 记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    /// worldId 十进制字符串,作为对外 id
    pub id: String,
    pub world_id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// file | url …(导入来源;官方/自定义判定不看来源,按 worldId 见
    /// api_planet::planet_kind)
    pub source: String,
    /// 各根稳定端点("ip:port"),来自导入时解析的 world(Task 3 起填充)
    #[serde(default)]
    pub roots: Vec<String>,
    pub file_size: u64,
    pub revision_time_ms: u64,
    pub imported_at_ms: u64,
    /// 上次运行时的节点 id(十字节 hex);不在线时对外不下发
    #[serde(default)]
    pub last_node_id: Option<String>,
    /// 该 planet 实例的数据目录(绝对路径)
    pub data_dir: std::path::PathBuf,
    /// 实例 zerotier-one 监听端口
    pub port: u16,
    pub running: bool,
    /// 持久启用开关:禁用后即使有网络也不运行,join 报 planetDisabled。
    /// 与 running(实际运行登记)正交;旧文件可能残留 manualStop 字段,
    /// serde 忽略未知键,自然淘汰
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// nwid hex → 本地网络配置
    #[serde(default)]
    pub net_local: HashMap<String, NetLocal>,
    #[serde(default)]
    pub moons: Vec<MoonRecord>,
    /// 该 planet 当前占用的 feth 设备名(dev 端);启动清扫孤儿用,
    /// 优雅退出/teardown 时清空
    #[serde(default)]
    pub ifaces: Vec<String>,
}

/// 单个网络的本地配置(route/dns 策略)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetLocal {
    /// 默认 true:managed 路由经 ZeroTier 虚拟接口下发
    pub route_via_zero_tier: bool,
    /// "none" | "network" | "custom"
    pub dns_mode: String,
    pub custom_dns: Vec<String>,
}

/// 一条已 orbit/缓存的 moon 记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoonRecord {
    pub moon_id: String,
    #[serde(default)]
    pub seed: Option<u64>,
    pub source: String,
    pub cached_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    #[serde(default)]
    pub planets: Vec<RegistryEntry>,
    /// daemon 级身份模式;save 恒写此字段,缺字段仅见于旧格式文件(load 迁移)
    #[serde(default)]
    pub identity_mode: IdentityMode,
}

/// serde 缺省:enabled 恒为 true(旧格式文件无此字段 → 未禁用)
fn default_true() -> bool {
    true
}

impl Registry {
    /// 读取 `<dir>/registry.json`;文件缺失或内容非法 → 空注册表(不报错、不覆盖写)
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("registry.json");
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => return Self::default(),
        };
        let v: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };
        let mode_missing = v.get("identityMode").is_none();
        let mut reg: Self = serde_json::from_value(v).unwrap_or_else(|_| Self::default());
        // 旧格式迁移(identityMode 缺失):planets 空或 config 根已有全局
        // 身份 → Shared(缺省语义);否则 → Independent——老 planet 各持
        // dataDir 独立身份,按 Independent 保持节点地址不变,不隐式换地址。
        if mode_missing {
            reg.identity_mode = if reg.planets.is_empty() || dir.join("identity.secret").exists() {
                IdentityMode::Shared
            } else {
                IdentityMode::Independent
            };
        }
        reg
    }

    /// 原子写盘:tmp → rename(同 zt-core StateStore 模式);Task 3 起的变更端点调用
    #[allow(dead_code)]
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let path = dir.join("registry.json");
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(&tmp, bytes)?;
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }
}
