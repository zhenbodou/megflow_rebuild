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

// ANCHOR: graph_path
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
    let c = g.take_output("c").unwrap();

    a.send(Envelope::new(1i32)).await.unwrap();
    b.send(Envelope::new(2i32)).await.unwrap();

    // 里程碑：全书第一个「真能跑」的引擎，1 + 2 == 3。
    assert_eq!(c.recv::<i32>().await.unwrap().unpack(), 3);

    drop(a);
    drop(b);
    g.stop();
    handle.await.unwrap().unwrap();
}
// ANCHOR_END: graph_path

// ANCHOR: sandbox_path
#[tokio::test]
async fn sandbox_runs_single_binary_op() {
    // Sandbox：不写 TOML，直接按类型名建单节点，喂数、收数、跑完。
    let args: flow_rs::config::Args = toml::from_str(r#"op = "+""#).unwrap();
    let collected: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = collected.clone();

    let mut sb = Sandbox::with_args("BinaryOp", args).unwrap();
    sb.add_items("a", vec![1i32])
        .add_items("b", vec![2i32])
        .add_check("c", move |v: i32| sink.lock().unwrap().push(v));
    sb.start().await.unwrap();

    assert_eq!(*collected.lock().unwrap(), vec![3]); // 1 + 2 == 3
}
// ANCHOR_END: sandbox_path

#[tokio::test]
async fn closed_output_does_not_stop_consuming_input_pairs() {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut graph = Builder::default()
            .template(BINARY_OP_GRAPH.replace("cap=16", "cap=1"))
            .build()
            .unwrap();
        let a = graph.input("a").unwrap();
        let b = graph.input("b").unwrap();
        drop(graph.take_output("c").unwrap());
        let task = graph.start();
        for value in 0..4i32 {
            a.send(Envelope::new(value)).await.unwrap();
            b.send(Envelope::new(value)).await.unwrap();
        }
        drop(a);
        drop(b);
        graph.stop();
        task.await.unwrap().unwrap();
    })
    .await
    .expect("输出关闭后仍应消费所有有限输入并退出");
}

// ANCHOR: error_path
#[tokio::test]
async fn sandbox_surfaces_node_error() {
    // 原版未知运算符执行 unreachable!。当前运行时将节点 panic 包装成 TaskJoin。
    let args: flow_rs::config::Args = toml::from_str(r#"op = "%""#).unwrap();
    let mut sb = Sandbox::with_args("BinaryOp", args).unwrap();
    sb.add_items("a", vec![1i32]).add_items("b", vec![2i32]);

    let result: Result<()> = tokio::time::timeout(std::time::Duration::from_secs(3), sb.start())
        .await
        .expect("节点 panic 后沙箱应结束");
    assert!(matches!(result, Err(Error::TaskJoin(_))));
}
// ANCHOR_END: error_path

#[tokio::test]
async fn division_panics_are_task_failures() {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        for (left, right) in [(1i32, 0i32), (i32::MIN, -1i32)] {
            let args = toml::from_str("op = '/' ").unwrap();
            let mut sandbox = Sandbox::with_args("BinaryOp", args).unwrap();
            sandbox
                .add_items("a", vec![left])
                .add_items("b", vec![right]);
            assert!(matches!(sandbox.start().await, Err(Error::TaskJoin(_))));
        }
    })
    .await
    .expect("两种整数除法 panic 都应传播出沙箱");
}

// ANCHOR: metadata_test
// 原版 Getting started 的四种运算都要验证；元信息必须来自左输入。
#[tokio::test]
async fn all_operations_preserve_left_envelope_metadata() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for (op, expected) in [("+", 9), ("-", 5), ("*", 14), ("/", 3)] {
            let template = BINARY_OP_GRAPH.replace("op=\"+\"", &format!("op=\"{op}\""));
            let mut g = Builder::default().template(template).build().unwrap();
            let a = g.input("a").unwrap();
            let b = g.input("b").unwrap();
            let c = g.take_output("c").unwrap();
            let handle = g.start();
            let metadata = Arc::new(String::from("left-frame"));
            let mut left = Envelope::new(7i32);
            left.info_mut().partial_id = Some(42);
            left.info_mut().extra_data = Some(metadata.clone());
            a.send(left).await.unwrap();
            let mut right = Envelope::new(2i32);
            right.info_mut().partial_id = Some(99);
            b.send(right).await.unwrap();
            let mut result = c.recv::<i32>().await.unwrap();
            assert_eq!(result.unpack(), expected);
            assert_eq!(result.info().partial_id, Some(42));
            let carried = result
                .info()
                .extra_data
                .clone()
                .unwrap()
                .downcast::<String>()
                .unwrap();
            assert!(Arc::ptr_eq(&metadata, &carried));
            drop(a);
            drop(b);
            g.stop();
            handle.await.unwrap().unwrap();
        }
    })
    .await
    .expect("图应在五秒内完成四种运算并退出");
}
// ANCHOR_END: metadata_test
