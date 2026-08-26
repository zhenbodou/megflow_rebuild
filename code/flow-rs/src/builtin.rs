//! flow-rs · builtin —— 随引擎发货的内置节点（Ch3.4）。
//!
//! 前几章把定义节点的「工具」逐件造齐了：Part 2 的过程宏（`#[inputs]`/`#[outputs]`/
//! `#[methods]`/`#[derive(Node, Actor, BuildFromPorts)]`/`node_register!`）、Ch2.4 的编译期
//! 注册表、Ch3.1~3.3 的配置 → 装配 → 调度。本模块用这整套工具造出**第一个真正的内置节点**
//! `BinaryOp`——它与下游用户写的节点**长得一模一样**，唯一区别是它住在引擎 crate 里、
//! 随 crate 一起发货。
//!
//! 「住在 crate 内部」带来一个前几章没遇到的坎：`node_register!` 生成的注册代码全用**绝对
//! 路径** `flow_rs::inventory::submit!` / `flow_rs::registry::NodeRegistration`（见 flow-derive）。
//! 这些路径在**下游** crate 里天然成立（`flow_rs` 就是依赖名），但在**本 crate 内部**，
//! `flow_rs` 默认并不指向自己。解法是在 `lib.rs` 加一行 `extern crate self as flow_rs;`——
//! 给自己起个别名。加上它之后，连下面这些 `use flow_rs::...` 都能照抄下游用户的写法。
//!
//! The first built-in node shipped with the engine. `extern crate self as flow_rs`
//! (in lib.rs) makes the macro-generated `flow_rs::` paths resolve inside the crate.

use flow_derive::{inputs, methods, node_register, outputs, Actor, BuildFromPorts, Node};
use flow_message::Envelope;
use flow_rs::channel::{Receiver, Sender};
use flow_rs::error::{Error, Result};
use flow_rs::node::{Actor, Node};
use flow_rs::registry::BuildFromPorts;

/// 二元整数运算节点：从输入端口 `a`、`b` 各取一个 `i32`，按参数 `op` 运算，结果发往
/// 输出端口 `c`。`op` 支持 `"+"` / `"-"` / `"*"`；其余值 → `Err(Error::Arg)`。
///
/// 这正是 Ch0.3 验收契约里那张图用的节点类型（TOML 里 `ty="BinaryOp"`）。它刻意做得极小：
/// 全书第一个「真能跑」的节点，重点是打通「配置 → 装配 → 调度 → 计算 → 出结果」这条链，
/// 而非运算本身的丰富度（更多算子、浮点、多操作数留给读者练习）。
///
/// A tiny built-in: reads one `i32` from each of `a`/`b`, applies `op`, sends to `c`.
#[inputs(a, b)]
#[outputs(c)]
#[derive(Node, Actor, BuildFromPorts)]
pub struct BinaryOp {
    /// 运算符，从节点参数 `op="+"` 反序列化而来（`#[derive(BuildFromPorts)]` 负责填充）。
    op: String,
}

#[methods]
impl BinaryOp {
    async fn exec(&mut self) -> Result<()> {
        // 各收一个操作数。任一输入关闭 → `recv` 返回 `ChannelClosed`，`#[methods]` 生成的
        // 包装会把它转成「置关闭标志 + Ok」，故这里直接 `?` 即可，无需手写关闭处理。
        let mut ea = self.a.recv::<i32>().await?;
        let mut eb = self.b.recv::<i32>().await?;
        let (x, y) = (ea.unpack(), eb.unpack());
        let r = match self.op.as_str() {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            other => {
                return Err(Error::Arg {
                    key: "op".into(),
                    msg: format!("未知运算符 {other:?}"),
                })
            }
        };
        // `c` 是 `Option<Sender>`——`close()` 会把它置 `None`。仍在时才发。
        if let Some(out) = self.c.as_ref() {
            out.send(Envelope::new(r)).await?;
        }
        Ok(())
    }
}

node_register!("BinaryOp", BinaryOp);
