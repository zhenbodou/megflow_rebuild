//! 集成测试：Ch3.2 的 Graph Builder。
//!
//! 这是「注册表 × 配置层」的合流点：拿一段图拓扑 TOML，从 Ch2.4 的注册表按类型名
//! `find` 出构造器、按 `PortRef` 把 channel 接到节点的**命名端口**上、把节点自有参数
//! （`op="+"`）从 `args` 里喂进去，装出一张 `MainGraph`。跨引用校验也在这里落地。
//!
//! 关键验证点——**名字↔位置的桥**：注册表按字段顺序生成 `INPUTS`/`OUTPUTS` 端口名表，
//! Builder 用它把「TOML 里按名接的 channel」排成「构造器要的按位置 Vec」。装完手动
//! `start()` 每个 actor，喂 `1` 与 `2`、收到 `3`，端到端证明接线正确（调度器封装留到 Ch3.3）。
//!
//! Integration test for the graph builder: assemble nodes by type name, wire
//! channels to named ports, thread args, and prove `1 + 2 == 3` end-to-end.

use flow_derive::{inputs, methods, node_register, outputs, Actor, BuildFromPorts, Node};
use flow_message::Envelope;
use flow_rs::channel::{Receiver, Sender};
use flow_rs::context::Context;
use flow_rs::error::{Error, Result};
use flow_rs::graph::Builder;
use flow_rs::node::{Actor, Node};
use flow_rs::registry::BuildFromPorts;

/// 一个「二元运算」测试节点：两个输入端口 `a`/`b`、一个输出 `c`、一个自有参数 `op`。
/// 相对 Ch2.4 的 `Doubler`（1 入 1 出、无参），它把 Ch3.2 的三件新事一次性覆盖：
/// **多输入端口**（命名接线必须对号入座）、**输出端口**、**从 `args` 取参数**。
#[inputs(a, b)]
#[outputs(c)]
#[derive(Node, Actor, BuildFromPorts)]
struct TestBinaryOp {
    /// 运算符，由 TOML 里的 `op="+"` 经 `args` 注入（`BuildFromPorts::build` 反序列化）。
    op: String,
}

#[methods]
impl TestBinaryOp {
    async fn exec(&mut self) -> Result<()> {
        // 各收一条；任一输入关闭，`?` 抛 ChannelClosed，被 #[methods] 包装吞成「置标志+Ok」。
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
            out.send(ea.repack(r)).await?;
        }
        Ok(())
    }
}

// 编译期把 TestBinaryOp 登记进全局表——Builder 靠类型名 "TestBinaryOp" find 出它。
node_register!("TestBinaryOp", TestBinaryOp);

/// Ch0.3 契约同款的单节点二元运算图（把 cap 调小些无妨）。
const ADD_GRAPH: &str = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="add", ty="TestBinaryOp", op="+"}]
inputs = [
    {name="a", cap=8, ports=["add:a"]},
    {name="b", cap=8, ports=["add:b"]}
]
outputs = [{name="c", cap=8, ports=["add:c"]}]
"#;

#[tokio::test]
async fn builds_and_runs_binary_op() {
    // 装配：TOML → MainGraph（find 构造器、命名接线、注入 op="+"）。
    let mut g = Builder::default().template(ADD_GRAPH).build().unwrap();

    // 对外端口就是 TOML 里声明的 a / b / c。
    let mut ins = g.input_names();
    ins.sort();
    assert_eq!(ins, vec!["a", "b"]);
    assert_eq!(g.output_names(), vec!["c"]);

    // 取出装好的 actor 手动跑起来（Ch3.3 会把这步封装进 graph.start()）。
    let actors = g.take_actors();
    assert_eq!(actors.len(), 1);
    let handles: Vec<_> = actors
        .into_iter()
        .map(|a| a.start(Context::anonymous()))
        .collect();

    // 从对外输入喂 1、2；从对外输出收 3——名字↔位置的桥若接反，这里立刻变红。
    let a_in = g.input("a").unwrap();
    let b_in = g.input("b").unwrap();
    let c_out = g.take_output("c").unwrap();
    a_in.send(Envelope::new(1i32)).await.unwrap();
    b_in.send(Envelope::new(2i32)).await.unwrap();
    let mut e = c_out.recv::<i32>().await.unwrap();
    assert_eq!(e.unpack(), 3);

    // 优雅停机：drop 掉所有输入 Sender（手上的两份 + 图内 inputs map 里的），
    // 节点 recv 到 ChannelClosed 后退出、close 输出、任务返回 Ok。
    drop(a_in);
    drop(b_in);
    drop(g);
    for h in handles {
        h.await.unwrap().unwrap();
    }
}

#[test]
fn missing_main_graph_errors() {
    // main 指向一张不存在的图 → 建图当场报错（解析层只发现，build 才报错）。
    let toml = r#"
main = "nope"
[[graphs]]
name = "example"
nodes = [{name="add", ty="TestBinaryOp", op="+"}]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(matches!(err, Error::MainGraphNotFound(_)));
}

#[test]
fn unknown_node_type_errors() {
    // 节点类型名在注册表里查不到。
    let toml = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="add", ty="NoSuchNode", op="+"}]
inputs = [{name="a", cap=8, ports=["add:a"]}, {name="b", cap=8, ports=["add:b"]}]
outputs = [{name="c", cap=8, ports=["add:c"]}]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(matches!(err, Error::UnknownNodeType(_)));
}

#[test]
fn port_ref_to_missing_node_errors() {
    // 端口引用 "ghost:a" 指向 nodes 里没有的节点。
    let toml = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="add", ty="TestBinaryOp", op="+"}]
inputs = [{name="a", cap=8, ports=["ghost:a"]}, {name="b", cap=8, ports=["add:b"]}]
outputs = [{name="c", cap=8, ports=["add:c"]}]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(matches!(err, Error::UnknownNode(_)));
}

#[test]
fn unknown_port_errors() {
    // add 没有名为 z 的输入端口，却被 TOML 接了线。
    let toml = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="add", ty="TestBinaryOp", op="+"}]
inputs = [
    {name="a", cap=8, ports=["add:a"]},
    {name="z", cap=8, ports=["add:z"]},
    {name="b", cap=8, ports=["add:b"]}
]
outputs = [{name="c", cap=8, ports=["add:c"]}]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(matches!(err, Error::UnknownPort { .. }));
}

#[tokio::test]
async fn unconnected_input_builds_and_closes_without_data() {
    let toml = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="add", ty="TestBinaryOp", op="+"}]
inputs = [{name="a", cap=8, ports=["add:a"]}]
outputs = [{name="c", cap=8, ports=["add:c"]}]
"#;
    let mut graph = Builder::default().template(toml).build().unwrap();
    let input = graph.input("a").unwrap();
    let output = graph.take_output("c").unwrap();
    let handle = graph.start();
    // a 先收到消息，b 的默认接收端随后立即报告关闭。
    input.send(Envelope::new(10i32)).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        handle.await.unwrap().unwrap();
        assert!(matches!(output.recv_any().await, Err(Error::ChannelClosed)));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unconnected_output_discards_and_node_finishes_normally() {
    let toml = ADD_GRAPH.replace(
        "outputs = [{name=\"c\", cap=8, ports=[\"add:c\"]}]",
        "outputs = []",
    );
    let mut graph = Builder::default().template(toml).build().unwrap();
    let a = graph.input("a").unwrap();
    let b = graph.input("b").unwrap();
    let handle = graph.start();
    a.send(Envelope::new(10i32)).await.unwrap();
    b.send(Envelope::new(3i32)).await.unwrap();
    drop(a);
    drop(b);
    graph.stop();
    tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[test]
fn declared_boundary_connections_must_not_be_empty() {
    // “未声明边界”与“声明了无端点的边界”不同：原版 translate_conn 拒绝后者。
    for direction in ["inputs", "outputs"] {
        let text = format!(
            "main=\"g\"\n[[graphs]]\nname=\"g\"\n{direction}=[{{name=\"edge\",cap=1,ports=[]}}]"
        );
        assert!(matches!(
            Builder::default().template(text).build(),
            Err(Error::BadConnection(_))
        ));
    }
}

#[test]
fn missing_arg_errors() {
    // 缺了 op 参数 → build 反序列化 args 时报错。
    let toml = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="add", ty="TestBinaryOp"}]
inputs = [{name="a", cap=8, ports=["add:a"]}, {name="b", cap=8, ports=["add:b"]}]
outputs = [{name="c", cap=8, ports=["add:c"]}]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(matches!(err, Error::Arg { .. }));
}

// ANCHOR: wiring_by_name
#[tokio::test]
async fn configuration_order_does_not_swap_operands_or_metadata() {
    // 配置顺序 b、a，注册表字段顺序 a、b；结果必须按名字接线。
    let text = r#"
main = "example"
[[graphs]]
name = "example"
nodes = [{name="subtract", ty="TestBinaryOp", op="-"}]
inputs = [
    {name="right", cap=1, ports=["subtract:b"]},
    {name="left", cap=1, ports=["subtract:a"]}
]
outputs = [{name="answer", cap=1, ports=["subtract:c"]}]
"#;
    let mut graph = Builder::default().template(text).build().unwrap();
    let left = graph.input("left").unwrap();
    let right = graph.input("right").unwrap();
    let output = graph.take_output("answer").unwrap();
    let handle = graph.start();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        left.send(Envelope::with_info(
            10i32,
            flow_message::EnvelopeInfo {
                partial_id: Some(42),
                ..Default::default()
            },
        ))
        .await
        .unwrap();
        right
            .send(Envelope::with_info(
                3i32,
                flow_message::EnvelopeInfo {
                    partial_id: Some(99),
                    ..Default::default()
                },
            ))
            .await
            .unwrap();
        let mut result = output.recv::<i32>().await.unwrap();
        assert_eq!(result.info().partial_id, Some(42));
        assert_eq!(result.unpack(), 7);
        drop(left);
        drop(right);
        graph.stop();
        handle.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
// ANCHOR_END: wiring_by_name
