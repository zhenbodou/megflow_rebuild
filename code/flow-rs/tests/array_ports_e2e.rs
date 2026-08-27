//! 集成测试：Ch4.2 数组端口——广播（Bcast，扇出）与汇聚（Merge，扇入）。
//!
//! 前面几章的端口都是**标量**（一名一端）：`BinaryOp:c`、`Transform:out` 只能接一条边。
//! 本章加了**数组端口**——`#[outputs(out[])]` → `Vec<Sender>`（一名多端，扇出）、
//! `#[inputs(inps[])]` → `Vec<Receiver>`（一名多端，扇入）。装配期，同一个数组端口名出现在
//! **多条**边上，每条边往这个端口的「组」里塞一个 channel 端（标量端口重复接则报
//! `PortAlreadyConnected`）。
//!
//! 这里用**图端到端**验真正的 N 路：
//! - `bcast_fans_out_to_two_downstreams`：一条输入经 `Bcast` **复制**给两个下游，两条对外
//!   输出都收到每一条消息的副本——兑现 Ch1.4「广播是节点的职责、不塞进 channel 层」。
//! - `merge_fans_in_from_two_upstreams`：两条**独立** channel 的输入经 `Merge` 轮询汇成一路，
//!   输出端收齐两路的全部消息。
//!
//! 另有 `sandbox_*` 两例走单节点沙箱：沙箱给每个端口只开一条 channel（数组端口退化成
//! **1 路的组**），验证退化边界也成立。
//!
//! Array-port e2e: Bcast copies one input to two downstreams; Merge polls two
//! independent inputs into one output. Plus the sandbox group-of-1 degenerate case.

use flow_message::Envelope;
use flow_rs::graph::Builder;
use flow_rs::sandbox::Sandbox;
use std::sync::{Arc, Mutex};

/// 广播扇出：`in → Bcast:inp`，`Bcast:out`（数组）经两条连接分别接到 `t1`/`t2`，
/// 两个 Transform 各自转发到对外输出 `o1`/`o2`。`Bcast:out` 这个数组输出端口出现在两条
/// 连接上，装配期攒成 2 个 Sender 的组——广播时对两路各发一份副本。
const BCAST_GRAPH: &str = r#"
main = "g"
[[graphs]]
name = "g"
nodes = [
    {name="bc", ty="Bcast"},
    {name="t1", ty="Transform"},
    {name="t2", ty="Transform"},
]
inputs = [
    {name="in", cap=16, ports=["bc:inp"]},
]
outputs = [
    {name="o1", cap=16, ports=["t1:out"]},
    {name="o2", cap=16, ports=["t2:out"]},
]
connections = [
    {cap=16, ports=["bc:out", "t1:inp"]},
    {cap=16, ports=["bc:out", "t2:inp"]},
]
"#;

#[tokio::test]
async fn bcast_fans_out_to_two_downstreams() {
    let mut g = Builder::default().template(BCAST_GRAPH).build().unwrap();
    let handle = g.start();

    let tx = g.input("in").unwrap();
    let mut o1 = g.take_output("o1").unwrap();
    let mut o2 = g.take_output("o2").unwrap();

    for v in [1i32, 2, 3] {
        tx.send(Envelope::new(v)).await.unwrap();
    }

    // 两条对外输出各应收到全部三条消息的副本（广播 = 复制给每一路）。
    // **定量收取**每路恰好 3 条，而非 drain-到-close：`MainGraph::input` 返回的是
    // 对外输入 Sender 的克隆，图自身**保留**着原件直到 `g.stop()`；因此在 stop 之前
    // 输入 channel 不会关闭，Bcast 不收工，o1/o2 也永不关闭——若在此 `while let Ok`
    // drain-到-close 会死锁（关闭依赖 stop，而 stop 排在 drain 之后）。
    // 与 connections_e2e.rs 一致：定量 recv → drop 克隆 → g.stop() → handle.await。
    let mut got1 = Vec::new();
    for _ in 0..3 {
        got1.push(o1.recv::<i32>().await.unwrap().unpack());
    }
    let mut got2 = Vec::new();
    for _ in 0..3 {
        got2.push(o2.recv::<i32>().await.unwrap().unpack());
    }
    assert_eq!(got1, vec![1, 2, 3], "下游 1 应收到广播的全部副本");
    assert_eq!(got2, vec![1, 2, 3], "下游 2 应收到广播的全部副本");

    drop(tx);
    g.stop();
    handle.await.unwrap().unwrap();
}

/// 汇聚扇入：两条对外输入 `in1`/`in2` 都接到 `Merge:inps`（数组输入端口，两条独立 channel），
/// `Merge:out` 接对外输出 `out`。Merge 轮询两路、汇成一路。
const MERGE_GRAPH: &str = r#"
main = "g"
[[graphs]]
name = "g"
nodes = [
    {name="mg", ty="Merge"},
]
inputs = [
    {name="in1", cap=16, ports=["mg:inps"]},
    {name="in2", cap=16, ports=["mg:inps"]},
]
outputs = [
    {name="out", cap=16, ports=["mg:out"]},
]
"#;

#[tokio::test]
async fn merge_fans_in_from_two_upstreams() {
    let mut g = Builder::default().template(MERGE_GRAPH).build().unwrap();
    let handle = g.start();

    let in1 = g.input("in1").unwrap();
    let in2 = g.input("in2").unwrap();
    let mut out = g.take_output("out").unwrap();

    for v in [1i32, 2, 3] {
        in1.send(Envelope::new(v)).await.unwrap();
    }
    for v in [10i32, 20, 30] {
        in2.send(Envelope::new(v)).await.unwrap();
    }

    // 两路的全部消息都应汇到输出（顺序可能交错，排序后比对集合）。
    // **定量收取**共 6 条，同样不能 drain-到-close（理由见 bcast 用例：图保留输入
    // Sender，out 在 g.stop() 之前不会关闭）。
    let mut got = Vec::new();
    for _ in 0..6 {
        got.push(out.recv::<i32>().await.unwrap().unpack());
    }
    got.sort();
    assert_eq!(got, vec![1, 2, 3, 10, 20, 30], "两路上游的消息应全部汇入");

    drop(in1);
    drop(in2);
    g.stop();
    handle.await.unwrap().unwrap();
}

/// 沙箱 group-of-1：Bcast 只接一个下游（数组输出退化成 1 路的组）也能原样广播。
#[tokio::test]
async fn sandbox_bcast_single_downstream() {
    let out = Arc::new(Mutex::new(Vec::new()));
    let sink = out.clone();
    let mut sb = Sandbox::pure("Bcast").unwrap();
    sb.add_data("inp", vec![1i32, 2, 3])
        .add_check("out", move |v: i32| sink.lock().unwrap().push(v));
    sb.start().await.unwrap();
    assert_eq!(*out.lock().unwrap(), vec![1, 2, 3]);
}

/// 沙箱 group-of-1：Merge 只接一个上游（数组输入退化成 1 路的组）也能轮询转发。
#[tokio::test]
async fn sandbox_merge_single_upstream() {
    let out = Arc::new(Mutex::new(Vec::new()));
    let sink = out.clone();
    let mut sb = Sandbox::pure("Merge").unwrap();
    sb.add_data("inps", vec![7i32, 8, 9])
        .add_check("out", move |v: i32| sink.lock().unwrap().push(v));
    sb.start().await.unwrap();
    assert_eq!(*out.lock().unwrap(), vec![7, 8, 9]);
}
