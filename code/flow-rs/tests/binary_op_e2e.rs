//! 集成测试：Ch3.4 大里程碑——真实内置节点 `BinaryOp`，端到端跑通 `1 + 2 == 3`。
//!
//! 与前几章的测试不同，这里**不再自己定义节点**：`BinaryOp` 已是 flow-rs 随 crate 发货
//! 的**内置节点**（`src/builtin.rs` 里 `node_register!` 注册），测试只按名字 `"BinaryOp"`
//! 引用它。这恰恰证明「节点真的进了引擎」，而非停留在某个测试文件里。
//!
//! 两条路径各验一次 `1 + 2 == 3`：
//! - **完整图路径**：Ch0.3 契约里那段一字不差的 TOML → `Builder`（Ch3.1 解析 + Ch3.2 装配）
//!   → `start()`（Ch3.3 调度）→ 内置 `BinaryOp`（Ch3.4）计算。全链贯通。
//! - **Sandbox 路径**：不写 TOML，一句话按类型名建单节点、喂数、收数、跑完——Ch3.4 新增
//!   的单节点测试框架。
//!
//! End-to-end milestone: the shipped built-in `BinaryOp` computes `1 + 2 == 3`,
//! driven both through the full graph pipeline and through the `Sandbox` harness.

use flow_message::Envelope;
use flow_rs::error::{Error, Result};
use flow_rs::graph::Builder;
use flow_rs::sandbox::Sandbox;
use std::sync::{Arc, Mutex};

/// Ch0.3 验收契约里那段一字不差的 `BinaryOp` 图配置（`op="+"`）。
const BINARY_OP_GRAPH: &str = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [
    {name="add", ty="BinaryOp", op="+"},
]
inputs = [
    {name="a", cap=16, ports=["add:a"]},
    {name="b", cap=16, ports=["add:b"]}
]
outputs = [{name="c", cap=16, ports=["add:c"]}]
"#;

#[tokio::test]
async fn binary_op_end_to_end_via_graph() {
    // 完整链路：配置解析（Ch3.1）→ 装配校验（Ch3.2）→ 调度（Ch3.3）→ 内置 BinaryOp（Ch3.4）。
    let mut g = Builder::default()
        .template(BINARY_OP_GRAPH)
        .build()
        .unwrap();
    let handle = g.start();

    let a = g.input("a").unwrap();
    let b = g.input("b").unwrap();
    let mut c = g.take_output("c").unwrap();

    a.send(Envelope::new(1i32)).await.unwrap();
    b.send(Envelope::new(2i32)).await.unwrap();

    // 里程碑：全书第一个「真能跑」的引擎，1 + 2 == 3。
    assert_eq!(c.recv::<i32>().await.unwrap().unpack(), 3);

    drop(a);
    drop(b);
    g.stop();
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn sandbox_runs_single_binary_op() {
    // Sandbox：不写 TOML，直接按类型名建单节点，喂数、收数、跑完。
    let args: flow_rs::config::Args = toml::from_str(r#"op = "+""#).unwrap();
    let collected: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = collected.clone();

    let mut sb = Sandbox::with_args("BinaryOp", args).unwrap();
    sb.add_data("a", vec![1i32])
        .add_data("b", vec![2i32])
        .add_check("c", move |v: i32| sink.lock().unwrap().push(v));
    sb.start().await.unwrap();

    assert_eq!(*collected.lock().unwrap(), vec![3]); // 1 + 2 == 3
}

#[tokio::test]
async fn sandbox_surfaces_node_error() {
    // op="/" 是未知运算符：节点 exec 返回 Err(Arg)，该错误经节点任务收尾一路抬到
    // Sandbox::start 的返回值（而非静默吞掉）。
    let args: flow_rs::config::Args = toml::from_str(r#"op = "/""#).unwrap();
    let mut sb = Sandbox::with_args("BinaryOp", args).unwrap();
    sb.add_data("a", vec![1i32]).add_data("b", vec![2i32]);

    let result: Result<()> = sb.start().await;
    assert!(matches!(result, Err(Error::Arg { .. })));
}
