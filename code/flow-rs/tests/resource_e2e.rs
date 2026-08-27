//! 集成测试：Ch4.3 共享资源与 Context——多个节点共用**同一份**资源。
//!
//! 前面几章每个节点实例的字段都各造各的；本章证明另一件事：一份资源**构造一次**、通过
//! `Arc` 被图里多个节点**共享**（真实场景就是一个检测模型 / 一块内存池）。这里用一个原子
//! 计数器 `Counter` 当可断言的替身：
//!
//! - `resource_shared_across_two_nodes`：`in → Bcast → {t1, t2}`，两个 `Tally` 都声明
//!   `res="counter"`，图里只放**一个** `Counter` 资源。喂 3 条 → 广播给两路 → 两个 Tally
//!   各转发 3 条、各在**同一个**计数器上 `bump()` 3 次。图跑完后从图外读回计数器 == **6**，
//!   证明「只造了一份、被两个节点共享」——若各造各的，读到的会是 0（图外这份从没被 bump）。
//! - `sandbox_tally_without_resource_forwards`：沙箱不注入资源，Tally 的资源句柄保持 `None`，
//!   优雅降级为纯转发——证明「依赖资源」的节点在没有资源时也不崩。
//! - `unknown_resource_type_is_rejected`：`resources` 里写个没注册的类型 → 装配期
//!   `UnknownResourceType`（与节点的 `UnknownNodeType` 对偶，校验前移到 build）。
//!
//! Resource e2e: one shared `Counter` bumped by two `Tally` nodes; the graph reads
//! back 6, proving a single shared instance. Plus sandbox degrade + unknown-type reject.

use flow_message::Envelope;
use flow_rs::builtin::Counter;
use flow_rs::error::Error;
use flow_rs::graph::Builder;
use flow_rs::sandbox::Sandbox;
use std::sync::{Arc, Mutex};

/// `in → Bcast:inp`，`Bcast:out`（数组）经两条连接分别接到 `t1`/`t2` 两个 `Tally`，
/// 两个 Tally 各自转发到对外输出 `o1`/`o2`。图里声明**一个** `Counter` 资源，两个 Tally
/// 都用 `res="counter"` 借它——于是它们 bump 的是同一个计数器。
const SHARED_GRAPH: &str = r#"
main = "g"
[[graphs]]
name = "g"
resources = [
    {name="counter", ty="Counter"},
]
nodes = [
    {name="bc", ty="Bcast"},
    {name="t1", ty="Tally", res="counter"},
    {name="t2", ty="Tally", res="counter"},
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
async fn resource_shared_across_two_nodes() {
    let mut g = Builder::default().template(SHARED_GRAPH).build().unwrap();
    let handle = g.start();

    let tx = g.input("in").unwrap();
    let mut o1 = g.take_output("o1").unwrap();
    let mut o2 = g.take_output("o2").unwrap();

    for v in [1i32, 2, 3] {
        tx.send(Envelope::new(v)).await.unwrap();
    }

    // **定量收取**每路恰好 3 条（不能 drain-到-close：图保留着对外输入 Sender，o1/o2 在
    // g.stop() 之前不会关闭，drain 会死锁——与 array_ports_e2e / connections_e2e 一致）。
    // Tally 是「先 bump 再转发」，故收到第 3 条时，该路的 3 次 bump 必已完成。
    let mut got1 = Vec::new();
    for _ in 0..3 {
        got1.push(o1.recv::<i32>().await.unwrap().unpack());
    }
    let mut got2 = Vec::new();
    for _ in 0..3 {
        got2.push(o2.recv::<i32>().await.unwrap().unpack());
    }
    assert_eq!(got1, vec![1, 2, 3], "t1 应把广播来的 3 条原样转发");
    assert_eq!(got2, vec![1, 2, 3], "t2 应把广播来的 3 条原样转发");

    // 关键断言：图外读回**同一个**共享计数器。两个 Tally 各 bump 3 次 = 6。
    // 收满 6 条输出后，6 次 bump 已全部先于各自的 send 完成，故此刻读必为 6（确定性）。
    let counter = g
        .resource::<Counter>("counter")
        .expect("counter 资源应存在");
    assert_eq!(
        counter.get(),
        6,
        "两个节点共享同一个 Counter → 合计 bump 6 次"
    );

    drop(tx);
    g.stop();
    handle.await.unwrap().unwrap();
}

/// 沙箱不注入任何资源：Tally 拿 `Context::anonymous()`，`res="missing"` 借不到 → 句柄 `None`
/// → 优雅降级为纯转发。证明「依赖资源」的节点在资源缺席时不崩、仍能干活。
#[tokio::test]
async fn sandbox_tally_without_resource_forwards() {
    let out = Arc::new(Mutex::new(Vec::new()));
    let sink = out.clone();
    // Tally 有个自有参数 `res`（要借用的资源名）——沙箱里随便给个名字，反正沙箱没有资源。
    let args: flow_rs::config::Args = toml::from_str(r#"res = "missing""#).unwrap();
    let mut sb = Sandbox::with_args("Tally", args).unwrap();
    sb.add_data("inp", vec![1i32, 2, 3])
        .add_check("out", move |v: i32| sink.lock().unwrap().push(v));
    sb.start().await.unwrap();
    assert_eq!(
        *out.lock().unwrap(),
        vec![1, 2, 3],
        "资源缺席时 Tally 应降级为纯转发"
    );
}

/// `resources` 里写个没注册的类型名 → 装配期报 `UnknownResourceType`（校验前移到 build）。
const BAD_RESOURCE_GRAPH: &str = r#"
main = "g"
[[graphs]]
name = "g"
resources = [
    {name="pool", ty="NoSuchResource"},
]
nodes = [
    {name="t", ty="Transform"},
]
inputs = [{name="in", cap=8, ports=["t:inp"]}]
outputs = [{name="out", cap=8, ports=["t:out"]}]
"#;

#[test]
fn unknown_resource_type_is_rejected() {
    let err = Builder::default()
        .template(BAD_RESOURCE_GRAPH)
        .build()
        .unwrap_err();
    assert!(
        matches!(err, Error::UnknownResourceType(ref ty) if ty == "NoSuchResource"),
        "未注册的资源类型应报 UnknownResourceType，实际：{err:?}"
    );
}
