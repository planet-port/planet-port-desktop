//! POC 1(本任务最小版):离线验证 Node 构造 → identity 生成/复用 → 析构。
//! Task 3 将扩展为 join/leave/status/networks 控制面。
use zt_core::{NodeHandle, NodeParams, NodeSink};

/// Task 2 阶段事件/配置尚无驱动线程消费,先给一个空实现保住签名。
struct NullSink;

impl NodeSink for NullSink {
    fn on_event(&mut self, _ev: zt_core::NodeEvent) {}
    fn on_net_config(&mut self, _nwid: u64, _op: zt_core::NetConfigOp, _cfg: zt_core::NetConfig) {}
    fn on_frame(&mut self, _nwid: u64, _s: u64, _d: u64, _et: u16, _p: &[u8]) {}
    fn local_ipv4s(&self) -> Vec<std::net::Ipv4Addr> {
        Vec::new()
    }
}

fn main() {
    let dir = std::path::PathBuf::from("target/poc-data/node-a");
    let (major, minor, rev) = NodeHandle::core_version();
    println!("core version = {major}.{minor}.{rev}");
    let h = NodeHandle::start(
        NodeParams {
            data_dir: dir.clone(),
            planet_file: Some("crates/pocs/fixtures/planet.Gozi25".into()),
            bind_port: 9994,
        },
        NullSink,
    )
    .expect("node start");
    println!("address = {:010x}", h.address());
    h.shutdown();
}
