//! 集成测试：Ch4.4 子图 subgraph / 多图 graphs——把一张图当「可复用部件」嵌进另一张图。
//!
//! 本章的机制是**内联展开（flattening）**：装配前的一趟 `Config → Config` 预处理，把
//! 「主图 + 若干被引用的子图」压平成**一张**扁平图，之后 Ch3.2 的 `assemble` 一行不改地
//! 跑在这张扁平图上。子图引用无需新语法——**一个节点的 `ty` 恰好等于某张图的名字**，它就
//! 是一次子图引用（与原版 `graph_names.contains(&ty)` 的自动识别一致）。
//!
//! - `reusable_subgraph_shares_top_level_resource`：可复用子图 `Branch = Transform → Tally`
//!   被实例化**两份**（`b1`/`b2`），主图一个 `Bcast` 扇出给两份；主图声明**一个** `Counter`，
//!   两份子图里的 `Tally` 都借它。喂 3 条 → 广播两路 → 每份子图各 `bump()` 3 次。图跑完后
//!   从图外读回计数器 == **6**——既证明「子图被正确展开、接线」，又回答了 Ch4.3 末尾留的钩子
//!   「资源怎么跨图共享」：压平后就是一张图，主图资源自然被所有（原属不同子图实例的）节点共享。
//! - `subgraph_cycle_is_rejected`：两张图互相把对方当子图引用（A→B→A）→ 展开会无限递归，
//!   `flatten` 靠「祖先链」检测并报 `SubgraphCycle`，而非爆栈。
//!
//! Subgraph e2e: a reusable `Branch` subgraph instantiated twice, both sharing one
//! top-level `Counter` (reads back 6). Plus mutual-cycle rejection.

use flow_message::Envelope;
use flow_rs::builtin::Counter;
use flow_rs::error::Error;
use flow_rs::graph::Builder;

// ANCHOR: e2e
/// 可复用子图 `Branch`（`Transform` 透传 → `Tally` 计数转发）被主图 `top` 实例化两份。
/// 主图声明**一个** `Counter`，两份子图里的 `Tally` 都写 `res="counter"` 借它——于是它们
/// bump 的是同一个计数器。子图引用 = 主图节点 `b1`/`b2` 的 `ty="Branch"` 恰好是图名。
const SUBGRAPH_SHARED: &str = r#"
main = "top"

# 可复用子图：Branch = Transform（原样透传）→ Tally（先 bump 再转发）
[[graphs]]
name = "Branch"
nodes = [
    {name="tf", ty="Transform"},
    {name="t", ty="Tally", res="counter"},
]
inputs = [{name="inp", cap=16, ports=["tf:inp"]}]
outputs = [{name="out", cap=16, ports=["t:out"]}]
connections = [
    {cap=16, ports=["tf:out", "t:inp"]},
]

# 主图：一个 Bcast 扇出给两份 Branch 子图实例；共享一个 Counter
[[graphs]]
name = "top"
resources = [{name="counter", ty="Counter"}]
nodes = [
    {name="bc", ty="Bcast"},
    {name="b1", ty="Branch"},
    {name="b2", ty="Branch"},
]
inputs = [{name="in", cap=16, ports=["bc:inp"]}]
outputs = [
    {name="o1", cap=16, ports=["b1:out"]},
    {name="o2", cap=16, ports=["b2:out"]},
]
connections = [
    {cap=16, ports=["bc:out", "b1:inp"]},
    {cap=16, ports=["bc:out", "b2:inp"]},
]
"#;

#[tokio::test]
async fn reusable_subgraph_shares_top_level_resource() {
    let mut g = Builder::default()
        .template(SUBGRAPH_SHARED)
        .build()
        .unwrap();
    let handle = g.start();

    let tx = g.input("in").unwrap();
    let o1 = g.take_output("o1").unwrap();
    let o2 = g.take_output("o2").unwrap();

    for v in [1i32, 2, 3] {
        tx.send(Envelope::new(v)).await.unwrap();
    }

    // 定量收取每路恰好 3 条（不能 drain-到-close：图保留着对外输入 Sender，o1/o2 在
    // g.stop() 之前不会关闭，drain 会死锁——守 Ch4.2 的硬教训）。
    let mut got1 = Vec::new();
    for _ in 0..3 {
        got1.push(o1.recv::<i32>().await.unwrap().unpack());
    }
    let mut got2 = Vec::new();
    for _ in 0..3 {
        got2.push(o2.recv::<i32>().await.unwrap().unpack());
    }
    assert_eq!(
        got1,
        vec![1, 2, 3],
        "子图实例 b1 应把广播来的 3 条透传+转发"
    );
    assert_eq!(
        got2,
        vec![1, 2, 3],
        "子图实例 b2 应把广播来的 3 条透传+转发"
    );

    // 关键断言：两份**不同子图实例**里的 Tally 借的是主图**同一个** Counter。
    // 各 bump 3 次 = 6。收满 6 条输出后 6 次 bump 已全部完成，故此刻读必为 6（确定性）。
    // 这正是「资源跨图共享」——若不共享（各实例各造一份），图外这份将是 0。
    let counter = g
        .resource::<Counter>("counter")
        .expect("counter 资源应存在");
    assert_eq!(
        counter.get(),
        6,
        "两份子图实例共享主图同一个 Counter → 合计 bump 6 次"
    );

    drop(tx);
    g.stop();
    handle.await.unwrap().unwrap();
}

/// 两张图互相把对方当子图（`a` 里有个 `ty="b"` 的节点、`b` 里有个 `ty="a"` 的节点）→
/// 内联展开会无限递归。`flatten` 用「祖先链」检测：展开到某张已在展开路径上的图 → 报
/// `SubgraphCycle`，在装配前当场拦下，而非爆栈。
const CYCLIC_GRAPHS: &str = r#"
main = "a"
[[graphs]]
name = "a"
nodes = [{name="nb", ty="b"}]
[[graphs]]
name = "b"
nodes = [{name="na", ty="a"}]
"#;

#[test]
fn subgraph_cycle_is_rejected() {
    let err = Builder::default()
        .template(CYCLIC_GRAPHS)
        .build()
        .unwrap_err();
    assert!(
        matches!(err, Error::SubgraphCycle(ref name) if name == "a"),
        "互相引用的子图应报 SubgraphCycle，实际：{err:?}"
    );
}
// ANCHOR_END: e2e
