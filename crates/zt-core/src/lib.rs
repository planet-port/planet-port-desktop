//! libzerotiercore 的 safe 封装:Node 生命周期、dataStore、回调 glue。
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
pub use node::{NodeError, NodeHandle};
pub use routes::RouteApplier;
