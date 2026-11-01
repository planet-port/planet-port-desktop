//! 设备层门面:按平台选择实现。公共 API 由两侧保持一致(见 device_unix/
//! device_win 模块头),daemon 只依赖本模块 re-export 的类型与函数。
// 子模块文件与本门面同在 src/ 根(Rust 2018 下需 #[path] 显式指路)。
#[cfg(windows)]
#[path = "arp.rs"]
mod arp;
#[cfg(windows)]
#[path = "device_win.rs"]
mod device_win;
#[cfg(not(windows))]
#[path = "device_unix.rs"]
mod device_unix;

#[cfg(windows)]
pub use device_win::*;
#[cfg(not(windows))]
pub use device_unix::*;
