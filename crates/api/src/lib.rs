pub mod endpoints;
pub mod types;

use std::path::PathBuf;

/// daemon 默认 API 端口(真实 planet-portd 启动时将实际端口写入 port 文件)
pub const DEFAULT_API_PORT: u16 = 9998;

pub fn config_dir() -> PathBuf {
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
