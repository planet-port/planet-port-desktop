// 注入构建时间(epoch 秒)供 --version 与 /version 展示。纯 std 取时,
// 不跑外部命令(交叉编译宿主不可控),时区与格式化留到运行时由 chrono
// 完成。不声明 rerun-if-changed:默认任何源码变化都重跑,每次重编译都
// 刷新时间,"构建时间"语义 = 该二进制最后一次被编译的时刻。
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=PLANET_PORT_BUILD_EPOCH={epoch}");
}
