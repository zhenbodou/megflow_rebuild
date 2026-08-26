//! flow-rs —— MegFlow 引擎核心（重写版）。
//!
//! channel / node / registry / config / graph / rt 等模块从 Part 1 起逐章加入；
//! `prelude` 门面在 Part 5（Ch5.1）补齐。
//! flow-rs —— MegFlow engine core (rewrite). Modules are added chapter by
//! chapter starting in Part 1.

pub mod channel;
pub mod config;
pub mod error;
pub mod node;
pub mod registry;

// 重导出 inventory：`node_register!` 生成的注册代码通过 `flow_rs::inventory::submit!`
// 定位到本 crate 的注册表，下游无需再单独依赖 inventory。
// Re-export so `node_register!`-generated code reaches the registry via
// `flow_rs::inventory`, without the downstream depending on inventory directly.
pub use inventory;
