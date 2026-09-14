extern crate self as flow_rs;
pub mod channel;
pub mod config;
pub mod error;
pub mod node;
pub mod registry;

// 重导出 inventory，让下游（及本 crate 内部）都能写 `flow_rs::inventory::submit!`——
// 下一步的 node_register! 生成的正是这个绝对路径，下游因此无需自己再依赖 inventory。
pub use inventory;
