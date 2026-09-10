//! Broker 章节独立编译入口；不依赖图、节点或过程宏 crate。
pub mod broker;
pub mod error;
pub mod envelope {
    pub use flow_message::{Envelope, SealedEnvelope};
}
