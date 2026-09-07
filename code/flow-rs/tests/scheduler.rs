//! 集成测试：Ch3.3 的 tokio 调度封装。
//!
//! Ch3.2 把图装到了「能被 `start()`」的地步，但要调用方手动逐个 `actor.start()`、
//! 手动收每个 `JoinHandle`。本章把这套封进 `MainGraph::start()`——一次 spawn 全部节点、
//! 返回**一个聚合句柄**（所有节点任务收尾后才 resolve）；`stop()` 丢掉图自己持有的对外
//! 输入 Sender，配合调用方丢掉克隆句柄，触发优雅停机的关闭涟漪。
//!
//! 拓扑仍限单节点通路（节点间内连 `connections` 留到 Part 4），故这里放**两个互相独立**
//! 的单节点（`add` 求和、`mul` 求积）——正好演练「聚合句柄跨多个任务 join」：两个节点都
//! 停机后，`start()` 的句柄才 resolve 成 `Ok`。
//!
//! Integration test for the scheduler wrapper: one `start()` spawns every node
//! and returns a single aggregate handle; `stop()` triggers graceful shutdown.

use flow_derive::{inputs, methods, node_register, outputs, Actor, BuildFromPorts, Node};
use flow_message::Envelope;
use flow_rs::channel::{Receiver, Sender};
use flow_rs::error::{Error, Result};
use flow_rs::graph::Builder;
use flow_rs::node::{Actor, Node};
use flow_rs::registry::BuildFromPorts;

/// 与 Ch3.2 同款的二元运算节点（两输入 a/b、一输出 c、参数 op）。
#[inputs(a, b)]
#[outputs(c)]
#[derive(Node, Actor, BuildFromPorts)]
struct TestBinaryOp {
    op: String,
}

#[methods]
impl TestBinaryOp {
    async fn exec(&mut self) -> Result<()> {
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
        if let Some(out) = self.c.as_ref() {
            out.send(Envelope::new(r)).await?;
        }
        Ok(())
    }
}

node_register!("TestBinaryOp", TestBinaryOp);

/// 两个独立单节点：add（a+b→sum）与 mul（x*y→prod）。彼此不接线，各跑各的。
const TWO_NODE_GRAPH: &str = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [
    {name="add", ty="TestBinaryOp", op="+"},
    {name="mul", ty="TestBinaryOp", op="*"},
]
inputs = [
    {name="a", cap=8, ports=["add:a"]},
    {name="b", cap=8, ports=["add:b"]},
    {name="x", cap=8, ports=["mul:a"]},
    {name="y", cap=8, ports=["mul:b"]}
]
outputs = [
    {name="sum",  cap=8, ports=["add:c"]},
    {name="prod", cap=8, ports=["mul:c"]}
]
"#;

#[tokio::test]
async fn start_runs_all_nodes_then_stop_shuts_down() {
    let mut g = Builder::default().template(TWO_NODE_GRAPH).build().unwrap();

    // 一次 start() spawn 全部节点，拿到覆盖整张图的聚合句柄。
    let handle = g.start();

    let a = g.input("a").unwrap();
    let b = g.input("b").unwrap();
    let x = g.input("x").unwrap();
    let y = g.input("y").unwrap();
    let sum = g.take_output("sum").unwrap();
    let prod = g.take_output("prod").unwrap();

    a.send(Envelope::new(1i32)).await.unwrap();
    b.send(Envelope::new(2i32)).await.unwrap();
    x.send(Envelope::new(3i32)).await.unwrap();
    y.send(Envelope::new(4i32)).await.unwrap();

    assert_eq!(sum.recv::<i32>().await.unwrap().unpack(), 3); // 1 + 2
    assert_eq!(prod.recv::<i32>().await.unwrap().unpack(), 12); // 3 * 4

    // 优雅停机：丢掉自己克隆的输入 Sender + 图自己持有的（stop 消费 g）。
    drop(a);
    drop(b);
    drop(x);
    drop(y);
    g.stop();

    // 聚合句柄：两个节点任务都收尾后 resolve，返回 Ok。
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn node_error_propagates_through_aggregate_handle() {
    // op="%" 是未知运算符：节点收到数据后 exec 返回 Err(Arg)，任务以 Err 收尾，
    // 该错误经聚合句柄一路抬到 handle.await。
    let toml = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="div", ty="TestBinaryOp", op="%"}]
inputs = [
    {name="a", cap=8, ports=["div:a"]},
    {name="b", cap=8, ports=["div:b"]}
]
outputs = [{name="c", cap=8, ports=["div:c"]}]
"#;
    let mut g = Builder::default().template(toml).build().unwrap();
    let handle = g.start();

    let a = g.input("a").unwrap();
    let b = g.input("b").unwrap();
    a.send(Envelope::new(1i32)).await.unwrap();
    b.send(Envelope::new(2i32)).await.unwrap();

    // 聚合句柄抬出节点的 Err(Arg)（外层 unwrap 拆掉 JoinError，内层拿到我们的 Result）。
    let result = handle.await.unwrap();
    assert!(matches!(result, Err(Error::Arg { .. })));

    g.stop();
}
