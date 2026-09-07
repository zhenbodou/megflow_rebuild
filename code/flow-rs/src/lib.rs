//! flow-rs —— MegFlow 引擎核心（重写版）。
//!
//! channel / node / registry / config / graph 等模块从 Part 1 起逐章加入；
//! `prelude` 门面在 Part 5（Ch5.1）补齐——`use flow_rs::prelude::*;` 一行带进常用名字。
//! flow-rs —— MegFlow engine core (rewrite). Modules are added chapter by
//! chapter starting in Part 1.

// 给自己起个别名 `flow_rs`（Ch3.4）。`node_register!` 生成的注册代码全用**绝对路径**
// `flow_rs::inventory::submit!` / `flow_rs::registry::NodeRegistration`——这在**下游** crate
// 里天然成立（`flow_rs` 就是依赖名），但在**本 crate 内部**，`flow_rs` 默认不指向自己。
// 加这一行后，crate 内外共用同一套 `flow_rs::` 路径，内置节点（`builtin`）的写法遂与下游
// 用户完全一致。这也证伪了 flow-derive 里「crate 内用宏得靠 proc-macro-crate 换 `crate::`」
// 那句早期猜测——`extern crate self` 是更简单的解。
// Alias ourselves as `flow_rs` so the macro-generated absolute `flow_rs::` paths
// resolve inside this crate too (see `builtin`).
extern crate self as flow_rs;

pub mod builtin;
pub mod channel;
pub mod config;
pub mod context;
pub mod envelope;
pub mod error;
pub mod graph;
pub mod node;
pub mod prelude;
pub mod registry;
pub mod resource;
pub mod sandbox;
pub mod subgraph;

// 重导出 inventory：`node_register!` 生成的注册代码通过 `flow_rs::inventory::submit!`
// 定位到本 crate 的注册表，下游无需再单独依赖 inventory。
// Re-export so `node_register!`-generated code reaches the registry via
// `flow_rs::inventory`, without the downstream depending on inventory directly.
pub use inventory;
