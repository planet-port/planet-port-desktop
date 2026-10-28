//! libzerotiercore 的 safe 封装:Node 生命周期、dataStore、回调 glue。
pub mod config;
pub mod ifaddr;
pub mod node;
pub mod sa;
pub mod state;

pub use config::{IpNet, ManagedRoute, NetConfig, NetConfigOp, NetStatus, NodeEvent, NodeParams, NodeSink};
pub use ifaddr::local_ipv4s;
pub use node::{NodeError, NodeHandle};
