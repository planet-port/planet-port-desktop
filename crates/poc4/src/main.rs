//! POC 4:Windows 支持可行性(wintun L3 + ZeroTier 核心,最低 Win10)。
//! 三道 Gate:G0 构建链路(已由 zt-sys 构建通过)/ G1 设备层 / G2 组网互通。
//! 本 crate 独立于 pocs,可整包 `cargo xwin check -p poc4` 交叉编译检查;
//! 非 Windows 平台仅编译占位 main,保证 workspace 全平台可编译。
#[cfg(windows)]
mod device_win;
#[cfg(windows)]
pub(crate) mod arp;
#[cfg(windows)]
pub(crate) mod driver;
#[cfg(windows)]
pub(crate) mod netcfg;
#[cfg(windows)]
pub(crate) mod sa;
#[cfg(windows)]
pub(crate) mod store;

#[cfg(windows)]
fn main() {
    // Task 4 先落设备层自验(Gate 1),Task 6/7 在此扩展控制面与数据面入口。
    let code = device_win::gate1_cli();
    std::process::exit(code);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("poc4_windows 仅在 Windows 上运行(Windows 支持评估 POC)");
}
