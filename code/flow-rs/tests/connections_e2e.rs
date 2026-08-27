//! 集成测试：Ch4.1 内部连接 `connections`——节点间成链，不再只靠对外端口。
//!
//! 前三部的引擎里，节点只能接到图的**对外**输入/输出端口；要把 `a` 的输出喂给 `b`
//! 的输入，得绕一圈对外端口。本测试钉死**内部连接**：一条 `connections` 边把 `add1:c`
//! 直接接到 `add2:a`，两个 `BinaryOp` 串成链——`(1+2) + 10 == 13`。
//!
//! 注意：这里**不引入新节点**，纯用已发货的 `BinaryOp` 串链，专验 `connections` 接线
//! 这一件事本身成立。方向由端点的端口角色推断：`add1:c` 是输出端口→发送端，
//! `add2:a` 是输入端口→接收端。
//!
//! Internal node-to-node `connections`: chain two `BinaryOp`s so (1+2)+10 == 13,
//! proving edges that don't route through external ports.

use flow_message::Envelope;
use flow_rs::error::Error;
use flow_rs::graph::Builder;

/// 两个加法节点串链：add1 算 a1+b1，其结果经内部连接喂给 add2 的 a，add2 再加上外部 b2。
const CHAIN_GRAPH: &str = r#"
main = "chain"
[[graphs]]
name = "chain"
nodes = [
    {name="add1", ty="BinaryOp", op="+"},
    {name="add2", ty="BinaryOp", op="+"},
]
inputs = [
    {name="a1", cap=16, ports=["add1:a"]},
    {name="b1", cap=16, ports=["add1:b"]},
    {name="b2", cap=16, ports=["add2:b"]},
]
outputs = [{name="out", cap=16, ports=["add2:c"]}]
connections = [
    {cap=16, ports=["add1:c", "add2:a"]},
]
"#;

#[tokio::test]
async fn internal_connection_chains_two_nodes() {
    let mut g = Builder::default().template(CHAIN_GRAPH).build().unwrap();
    let handle = g.start();

    let a1 = g.input("a1").unwrap();
    let b1 = g.input("b1").unwrap();
    let b2 = g.input("b2").unwrap();
    let mut out = g.take_output("out").unwrap();

    a1.send(Envelope::new(1i32)).await.unwrap();
    b1.send(Envelope::new(2i32)).await.unwrap(); // add1: 1 + 2 = 3
    b2.send(Envelope::new(10i32)).await.unwrap(); // add2: 3 + 10 = 13

    assert_eq!(out.recv::<i32>().await.unwrap().unpack(), 13);

    drop(a1);
    drop(b1);
    drop(b2);
    g.stop();
    handle.await.unwrap().unwrap();
}

// ── 构建期错误校验：连接形态非法 / 端口重复接线，都在 build() 当场报错 ──
// 这三条都不是 `#[tokio::test]`——它们在 `build()` 就返回 `Err`，根本跑不到运行时。
// 这正是「校验前移到 build()」：接线错误在建图那一刻暴露，而非等节点跑起来才 panic。

/// 一条连接挂了两个**输入**端口（两个接收端、零发送端）：mpsc 单消费者不允许，
/// 且没有发送端这条 channel 也永远收不到数据 → `BadConnection`。
#[test]
fn connection_with_two_receivers_is_rejected() {
    let toml = r#"
main = "g"
[[graphs]]
name = "g"
nodes = [
    {name="add1", ty="BinaryOp", op="+"},
    {name="add2", ty="BinaryOp", op="+"},
]
connections = [
    {cap=16, ports=["add1:a", "add2:a"]},
]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(matches!(err, Error::BadConnection(_)), "got {err:?}");
}

/// 一条连接全是**输出**端口（零接收端）：没有消费者，数据无处可去 → `BadConnection`。
#[test]
fn connection_with_no_receiver_is_rejected() {
    let toml = r#"
main = "g"
[[graphs]]
name = "g"
nodes = [
    {name="add1", ty="BinaryOp", op="+"},
    {name="add2", ty="BinaryOp", op="+"},
]
connections = [
    {cap=16, ports=["add1:c", "add2:c"]},
]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(matches!(err, Error::BadConnection(_)), "got {err:?}");
}

/// 同一个输入端口既被对外输入 `x` 接了、又被内部连接接了：一个输入端口只能有一个来源
/// channel（一对多的扇出需要 bcast）→ `PortAlreadyConnected`。
#[test]
fn port_wired_twice_is_rejected() {
    let toml = r#"
main = "g"
[[graphs]]
name = "g"
nodes = [
    {name="add1", ty="BinaryOp", op="+"},
    {name="add2", ty="BinaryOp", op="+"},
]
inputs = [
    {name="x", cap=16, ports=["add2:a"]},
]
connections = [
    {cap=16, ports=["add1:c", "add2:a"]},
]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(
        matches!(err, Error::PortAlreadyConnected { .. }),
        "got {err:?}"
    );
}
