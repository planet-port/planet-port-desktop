pub mod endpoints;
pub mod types;

use std::path::PathBuf;

/// daemon 默认 API 端口(真实 planet-portd 启动时将实际端口写入 port 文件)
pub const DEFAULT_API_PORT: u16 = 9998;

/// config 目录三级解析:
/// ① env `PLANET_PORT_CONFIG_DIR`(开发/测试显式指定);
/// ② `/Library/Application Support/planet-portd` 已存在则用之(service/LaunchDaemon 安装态:
///    daemon 以 root 运行时 HOME=/var/root,dirs::config_dir() 会解析到 /var/root/Library/...,
///    与普通用户 CLI 解析结果不一致,故复用已存在的系统目录保证两端落到同一目录);
/// ③ `dirs::config_dir()/planet-portd`(用户级回退,开发期)。
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PLANET_PORT_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let system_dir = PathBuf::from("/Library/Application Support/planet-portd");
    if system_dir.is_dir() {
        return system_dir;
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("planet-portd")
}
pub fn port_file() -> PathBuf {
    config_dir().join("port")
}
pub fn token_file() -> PathBuf {
    config_dir().join("authtoken.secret")
}
