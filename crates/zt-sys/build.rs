// 编译 libzerotiercore 静态库并生成 bindgen 绑定。
// 源码由 scripts/bootstrap.sh 拉取(不入库)；此处校验版本戳防错版编译。
use std::{fs, path::PathBuf};

const ZT_VERSION: &str = "1.16.2";

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().unwrap().parent().unwrap().to_path_buf();
    let core = root.join("Core");
    let stamp = core.join(".fetched-tag");

    match fs::read_to_string(&stamp) {
        Ok(v) if v.trim() == ZT_VERSION => {}
        _ => panic!(
            "ZeroTier 核心 {ZT_VERSION} 未就绪:先执行 ./scripts/bootstrap.sh(见 Core/.fetched-tag)"
        ),
    }

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .define("ZT_NO_PEER_METRICS", None)
        .warnings(false)
        .include(core.join("include"))
        .include(core.join("node"))
        .include(core.join("osdep"))
        .include(core.join("ext"))
        .include(core.join("ext/prometheus-cpp-lite-1.0/core/include"))
        .include(core.join("ext/prometheus-cpp-lite-1.0/simpleapi/include"))
        .file(core.join("osdep/OSUtils.cpp"));
    for e in fs::read_dir(core.join("node")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) == Some("cpp") {
            build.file(p);
        }
    }
    build.compile("zerotiercore");

    let bindings = bindgen::Builder::default()
        .header(core.join("include/ZeroTierOne.h").to_str().unwrap())
        // 头文件结构体内直接用裸 bool,是按 C++ 语义消费的,须以 C++ 解析
        .clang_args(&["-x", "c++", "-std=c++17"])
        .layout_tests(false)
        .generate_comments(false)
        .generate()
        .expect("bindgen ZeroTierOne.h 失败");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out.join("bindings.rs"))
        .expect("写 bindings.rs 失败");

    println!("cargo:rerun-if-changed={}", core.join(".fetched-tag").display());
}
