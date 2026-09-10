//! flow-rs · prelude —— 一行导入常用名字的门面（Ch5.1）。
//!
//! 到 Part 4 为止，写一个节点得铺一屏 `use`：channel 里的 `Sender`/`Receiver`、node 里的
//! `Actor`/`Node`、registry 里的 `BuildFromPorts`、error 里的 `Error`/`Result`、message 里的
//! `Envelope`，再加上 `flow_derive` 的一大把宏……真实下游算法仓里每个节点文件都要重抄一遍。
//! `prelude` 把这些高频名字收拢到一处，让 node / resource / app 作者都只需一行：
//!
//! ```ignore
//! use flow_rs::prelude::*;
//! ```
//!
//! 这对齐了原版 MegFlow 的入口习惯——原版 `flow-rs` 也有一个 `prelude`，下游一律
//! `use flow_rs::prelude::*;` 起手。一处**关键差异**：原版下游还要搭一句 `use anyhow::Result;`
//! （它的错误类型借道 `anyhow`），而我们**自己拥有**错误类型（Ch1.4 的 `thiserror` 枚举），故
//! `Result` 直接从本门面里出——少一个外部依赖、少一行 use。
//!
//! ## 同名的「trait + 派生宏」为什么能并存
//!
//! `Node`/`Actor`/`BuildFromPorts` 在这里各被导出**两次**：一次是 `flow_derive` 的**派生宏**
//! （宏命名空间），一次是本 crate 的**trait**（类型命名空间）。Rust 的宏与类型分属两个不同的
//! 命名空间，故同名不冲突——`#[derive(Node)]` 时编译器查宏、`Box<dyn Node>` / `T: Node` 时查
//! 类型，各取所需。这正是 `serde` 让 `Serialize` 既是派生宏又是 trait 的同款套路。
//!
//! A one-line import facade. `use flow_rs::prelude::*;` brings the common node /
//! resource / graph names into scope, mirroring the original MegFlow's `prelude`
//! (but our `Result` is our own, so no `use anyhow::Result;` is needed alongside).

// ANCHOR: prelude_facade
// ── 消息 ──────────────────────────────────────────────────────────────────
pub use crate::envelope::{
    str2addr, AnyEnvelope, DummyEnvelope, Envelope, EnvelopeInfo, SealedEnvelope,
};

// ── 过程宏（flow-derive）──────────────────────────────────────────────────
// 派生宏 TypeName/Node/Actor/BuildFromPorts、属性宏 inputs/outputs/methods、函数式宏
// node_register!/resource_register!——写节点 / 写资源要用到的全部宏。
pub use flow_derive::{
    add_cvt_func, inputs, methods, node_register, outputs, resource_register, Actor, BuildFromPorts, Node,
    TypeName,
};

// ── 引擎类型与函数 ────────────────────────────────────────────────────────
pub use crate::channel::{channel, BatchRecvError, Receiver, ReceiverT, Sender, SenderT, TypeInfo};
pub use crate::config::Args; // 资源作者写 `BuildResource::build(args: &Args)` 时要命名它。
pub use crate::context::Context;
pub use crate::error::{Error, Result};
pub use crate::graph::{Builder, MainGraph};
pub use crate::node::{Actor, Node}; // trait 形态（与上面的派生宏同名并存）。
pub use crate::registry::BuildFromPorts; // trait 形态。
pub use crate::resource::BuildResource;
pub use crate::sandbox::Sandbox;

// 原版公开的端口/消息类型描述与单个 TOML 参数值。
pub use crate::config::interlayer::{MsgType, MsgTypeId, PortInfo, PortType};
pub type Arg = toml::value::Value;
// ANCHOR_END: prelude_facade
