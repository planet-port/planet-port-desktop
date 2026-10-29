pub mod endpoints;
pub mod types;

use std::path::PathBuf;

/// daemon 默认 API 端口(真实 planet-portd 启动时将实际端口写入 port 文件)
pub const DEFAULT_API_PORT: u16 = 9998;

/// config 目录三级解析:
/// ① env `PLANET_PORT_CONFIG_DIR`(开发/测试显式指定);
/// ② 系统目录已存在则用之(service 安装态:daemon 以 root 运行时 HOME 指向
///    /var/root(macOS)或 /root(Linux),dirs::config_dir() 会与普通用户 CLI
///    解析结果不一致,故复用已存在的系统目录保证两端落到同一目录);
/// ③ `dirs::config_dir()/planet-portd`(用户级回退,开发期)。
pub fn config_dir() -> PathBuf {
    // 空字符串等同未设置,避免 PathBuf("") 泄漏为相对路径
    if let Some(d) = std::env::var_os("PLANET_PORT_CONFIG_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    // 系统目录:macOS 为 LaunchDaemon 安装路径,Linux 对齐 zerotier-one 的 /var/lib 惯例
    #[cfg(target_os = "macos")]
    let system_dir = PathBuf::from("/Library/Application Support/planet-portd");
    #[cfg(not(target_os = "macos"))]
    let system_dir = PathBuf::from("/var/lib/planet-portd");
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
