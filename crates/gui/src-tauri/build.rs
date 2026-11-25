fn main() {
    // 构建时间只注入 epoch,显示期由 build_time() 转查看者本地时区——
    // 编译期格式化会把构建机时区烧进二进制,异地用户看到的时间即失真。
    // tauri_build 的 rerun 规则决定 build.rs 重跑时机,本地增量编译可能
    // 沿用上次时间,CI 全新环境必新鲜
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    println!("cargo:rustc-env=PP_BUILD_EPOCH={secs}");
    tauri_build::build()
}
