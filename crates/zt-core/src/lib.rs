//! libzerotiercore 的 safe 封装:Node 生命周期、dataStore、回调 glue。
//!
//! # 架构红线(全模块适用)
//!
//! - 回调 glue 只做「拷贝 + channel send」,绝不直接调 sink;
//! - 全部 sink 调用(on_event/on_net_config/on_frame/local_ipv4s)必须且只能
//!   在驱动线程执行,并以 catch_unwind 包裹;
//! - FrameOut(核心→设备)与 FrameIn(设备→核心)两条通道方向不可混用;
//! - 关停硬顺序:teardown 设备(停 reader)→ running=false → 唤醒 → Cmd::Stop
//!   → join 驱动线程 → ZT_Node_delete → drop Inner;
//! - `networks()/peers()` 返回的核心查询缓冲必须在 zt-core 内用
//!   `ZT_Node_freeQueryResult` 释放,指针不外借;
//! - FFI 边界禁 panic/unwrap。
pub mod config;
pub mod device;
pub mod ifaddr;
pub mod node;
pub mod routes;
pub mod sa;
pub mod state;

pub use config::{IpNet, ManagedRoute, NetConfig, NetConfigOp, NetStatus, NodeEvent, NodeParams, NodeSink};
pub use device::{DevicePlane, FrameIn, FrameInTx, FrameInTxSlot};
pub use ifaddr::local_ipv4s;
pub use node::{NodeError, NodeHandle, PeerInfo, PeerPath, PeerRole};
pub use routes::RouteApplier;
