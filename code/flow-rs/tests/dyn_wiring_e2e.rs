//! flow-rs · tests/dyn_wiring_e2e —— Ch4.9b：config 认识 `dyn` 连接后的**自动接线**端到端契约。
//!
//! 与 tests/dyn_ports.rs 的区别在于「谁来接线」：那边手搓 `DynPorts` + 手挂 broker 订阅，显式
//! 驱动 create→publish→fetch→route；这边只写一份 **TOML**——一个 dyn 输出/输入的触发节点 +
//! 一张动态子图 + 下游消费——交给 `Builder::build`：flatten **跳过** dyn 子图（留作惰性
//! `GraphConfig`）、assemble 期**自动**为 dyn 端口 `set_port_dynamic` 注入 `DynPortsConfig`、
//! `MainGraph` 自带 broker 生命周期。触发节点 `AutoTrigger` 扮演后续 4.9c 里注册版 `DynDemux`
//! 的角色，其 `exec` 就是那条环路。全部 `tokio::time::timeout` 包裹——teardown 一旦挂起即超时
//! 失败而非卡死 CI（同 tests/dyn_ports.rs 的纪律）。
//!
//! 恒等护栏：本文件只**新增**测试，既有 subgraph/graph/demux/资源/dyn_ports 测试一字不改继续绿。

use flow_rs::config::Config;
use flow_rs::prelude::*;
use flow_rs::resource::ResourceCollection;
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinHandle;

// ANCHOR: fixtures
/// 动态子图里的计数节点：**有状态**——每收一条就 `count += 1`，把计数拼进出口载荷。
/// 用它来从**可观察输出**证明 create-once：同一 key 复用同一实例时 `count` 会累加
/// （`frame-1#1` → `frame-2#2`），换个 key 是另一张独立图、`count` 从 1 重新起（`frame-42#1`）。
#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
struct Seq {
    #[state]
    count: u64,
}

#[methods]
impl Seq {
    async fn exec(&mut self) -> Result<()> {
        let mut msg = self.inp.recv_any().await?;
        let payload = msg
            .downcast_mut::<Envelope<String>>()
            .expect("Seq expects a String payload")
            .unpack();
        self.count += 1;
        if let Some(out) = self.out.as_ref() {
            out.send_any(Envelope::new(format!("{payload}#{}", self.count)).seal())
                .await?;
        }
        Ok(())
    }
}
node_register!("Seq", Seq);

/// 顶层触发节点（手写的 `DynDemux` 前身，4.9c 会把它做成注册 builtin）：
/// - `feed` 是 **dyn 输出**（持 `DynPorts<Sender>`）——反直觉但正确：往实例**入口**灌数据的 `Sender`；
/// - `collect` 是 **dyn 输入**（持 `DynPorts<Receiver>`）——从实例**出口**收结果的 `Receiver`。
///
/// `exec`：按信封 `to_addr` 取 key；空信封 = 拆除信号（撤端点 + await 任务）；有载荷则按需
/// `create`（每 key 一次，`is_cached` 把门）、`fetch` 入口灌进去、再从出口收回、转发到 `out`。
/// 建图期这两个 dyn 端口由 config 自动接线——`feed` 接到 `worker:inp`、`collect` 接到 `worker:out`。
#[inputs(inp, collect: dyn T0)]
#[outputs(out, feed: dyn T0)]
#[derive(Node, Actor, BuildFromPorts)]
struct AutoTrigger {
    #[state]
    tasks: HashMap<u64, JoinHandle<Result<()>>>,
}

#[methods]
impl AutoTrigger {
    async fn exec(&mut self) -> Result<()> {
        let message = self.inp.recv_any().await?;
        let key = message
            .info()
            .to_addr
            .ok_or_else(|| Error::Unsupported("AutoTrigger needs a to_addr key".into()))?;

        // 空信封（is_none）= 拆除：撤掉该 key 的入口/出口端点 → 实例失去唯一外部发送端、停机；await 其任务。
        if message.is_none() {
            if let Some(handle) = self.tasks.remove(&key) {
                self.feed.evict(key);
                self.collect.evict(key);
                handle.await.ok();
            }
            return Ok(());
        }

        // 有载荷：每 key 只 create 一次（is_cached 把门），把 JoinHandle 收进 tasks。
        if !self.feed.is_cached(key) {
            let handle = self.feed.create(key, ResourceCollection::default()).await?;
            self.tasks.insert(key, handle);
        }
        // fetch 入口 Sender、灌数据（send 用 .ok()：避免把「实例通道关闭」误判成「我的输入关闭」）。
        self.feed
            .fetch_with_cache(key)
            .await?
            .send_any(message)
            .await
            .ok();
        // fetch 出口 Receiver、收回结果，转发到对外 out。
        let got = self.collect.fetch_with_cache(key).await?.recv_any().await?;
        if let Some(out) = self.out.as_ref() {
            out.send_any(got).await?;
        }
        Ok(())
    }

    async fn finalize(&mut self) {
        // 输入关闭后收尾：撤掉所有残留实例的端点、await 任务，确保没有实例挂在后台。
        let keys: Vec<u64> = self.tasks.keys().copied().collect();
        for key in keys {
            self.feed.evict(key);
            self.collect.evict(key);
        }
        for (_, handle) in self.tasks.drain() {
            handle.await.ok();
        }
    }
}
node_register!("AutoTrigger", AutoTrigger);
// ANCHOR_END: fixtures

// ANCHOR: toml
// 一份 TOML 就够：子图 `sub`（有状态 Seq）+ 顶层 `top`（触发节点 + 动态子图站点 worker）。
// 关键在两条 **dyn 连接**——config 层据此自动接线，无需任何手写 `DynPorts`：
//   - `trigger:feed`（dyn 输出）↔ `worker:inp`（子图入口）
//   - `trigger:collect`（dyn 输入）↔ `worker:out`（子图出口）
const TOML: &str = r#"
main = "top"

[[graphs]]
name = "sub"
nodes = [{name="s", ty="Seq"}]
inputs = [{name="inp", cap=1, ports=["s:inp"]}]
outputs = [{name="out", cap=1, ports=["s:out"]}]

[[graphs]]
name = "top"
nodes = [{name="trigger", ty="AutoTrigger"}, {name="worker", ty="sub"}]
inputs = [{name="in", cap=1, ports=["trigger:inp"]}]
outputs = [{name="out", cap=1, ports=["trigger:out"]}]
connections = [
    {cap=1, ports=["trigger:feed", "worker:inp"]},
    {cap=1, ports=["trigger:collect", "worker:out"]},
]
"#;
// ANCHOR_END: toml

/// 造一个带目标地址 key 的有载荷信封（同 examples/dynports_steps.rs）。
fn addressed(payload: &str, key: u64) -> SealedEnvelope {
    Envelope::with_info(
        payload.to_owned(),
        EnvelopeInfo {
            to_addr: Some(key),
            ..Default::default()
        },
    )
    .seal()
}

/// 造一个带目标地址的**空**信封（`is_none` 为真）——生命周期拆除信号。
fn teardown(key: u64) -> SealedEnvelope {
    let mut empty = Envelope::<String>::empty();
    empty.info_mut().to_addr = Some(key);
    empty.seal()
}

/// 从图对外输出收一条载荷，downcast 成 String。
async fn recv_string(output: &Receiver) -> String {
    let mut got = output.recv_any().await.expect("output produced a message");
    got.downcast_mut::<Envelope<String>>()
        .expect("String payload")
        .unpack()
}

// ANCHOR: e2e
/// 主线：一份 TOML 经 `Builder::build` **自动**接线动态子图，驱动 create→route→teardown。
/// 断言三件事：按 key 路由、create-once（有状态子图的计数累加为证）、空信封拆除。
#[tokio::test]
async fn auto_wires_dyn_subgraph_from_toml() {
    tokio::time::timeout(Duration::from_secs(5), async {
        // 只写 TOML：没有一行手搓 DynPorts / 手挂 broker 订阅——全由 config 自动接线。
        let mut graph = Builder::default().template(TOML).build().unwrap();
        let input = graph.input("in").expect("external input 'in'");
        let output = graph.take_output("out").expect("external output 'out'");
        let task = graph.start();

        // key 7 第一帧：首次触发 create（现装一张 Seq 实例），路由进去、收回。count=1。
        input.send_any(addressed("frame-1", 7)).await.unwrap();
        assert_eq!(recv_string(&output).await, "frame-1#1");

        // key 7 第二帧：is_cached 命中、**不重建**，复用同一实例——有状态 count 累加到 2。
        // 这就是 create-once 的可观察证据：若每帧新建实例，这里会是 "frame-2#1" 而断言失败。
        input.send_any(addressed("frame-2", 7)).await.unwrap();
        assert_eq!(recv_string(&output).await, "frame-2#2");

        // key 42 是**另一张独立**的图：count 从 1 重新起，与 key 7 互不串台。
        input.send_any(addressed("frame-42", 42)).await.unwrap();
        assert_eq!(recv_string(&output).await, "frame-42#1");

        // 空信封拆除两个 key 的实例（AutoTrigger 见状撤端点 + await 任务）。
        input.send_any(teardown(7)).await.unwrap();
        input.send_any(teardown(42)).await.unwrap();

        // 停机：撤掉对外输入的两份 Sender（本地克隆 + 图保留的那份）→ 关闭涟漪传导、聚合句柄解析。
        drop(input);
        graph.stop();
        task.await.unwrap().unwrap();
    })
    .await
    .expect("dyn wiring e2e timed out");
}
// ANCHOR_END: e2e

// ANCHOR: flatten_skip
/// flatten 对 dyn 子图的处理：**不内联**，保留成惰性 `GraphConfig`；站点节点原样留在扁平主图里
/// （`ty` 仍是子图名，assemble 据此识别并装成运行期实例）。恒等性：无 dyn 连接时行为一字不变
/// （既有 subgraph 单元测试是护栏），这里验证「有 dyn 连接」这条新分支。
#[test]
fn flatten_retains_dyn_subgraph_as_lazy_constructor() {
    let cfg = Config::from_toml(TOML).unwrap();
    let flat = flow_rs::subgraph::flatten(&cfg).unwrap();

    // 扁平主图在前、被保留的 dyn 子图在后：共 2 张图（对比静态子图会被内联掉、只剩 1 张）。
    assert_eq!(flat.graphs.len(), 2, "扁平主图 + 保留的 dyn 子图");
    assert_eq!(flat.graphs[0].name, "top", "扁平主图在前");
    assert_eq!(flat.graphs[1].name, "sub", "dyn 子图被保留为惰性构造器");

    // 主图里：触发节点是叶子；worker **没有被内联**，仍是一个 ty="sub" 的站点节点。
    let top = &flat.graphs[0];
    assert!(
        top.nodes.iter().any(|n| n.name == "trigger" && n.ty == "AutoTrigger"),
        "触发节点原样保留"
    );
    let worker = top
        .nodes
        .iter()
        .find(|n| n.name == "worker")
        .expect("dyn 子图站点节点 worker 仍在扁平主图里");
    assert_eq!(worker.ty, "sub", "worker 未被内联，ty 仍是子图名（assemble 据此装配实例）");
    assert!(
        !top.nodes.iter().any(|n| n.name.contains('/')),
        "dyn 子图不内联 → 没有带前缀的内部叶子节点（如 worker/s）"
    );
}
// ANCHOR_END: flatten_skip

// ANCHOR: validation
/// 校验规则之一（对齐原版 `graph/mod.rs` 的 dyn 端口约束）：dyn 端点与子图站点的边界必须**方向匹配**。
/// dyn **输出** `feed`（持 `Sender`，往实例入口灌）只能接子图的**入口** `inp`；这里错接成出口 `out`
/// → 反直觉的接法被拒，报 `UnknownPort`（子图 sub 的 inputs 里没有名为 "out" 的边界）。
#[test]
fn reversed_boundary_is_rejected() {
    let toml = r#"
main = "top"

[[graphs]]
name = "sub"
nodes = [{name="s", ty="Seq"}]
inputs = [{name="inp", cap=1, ports=["s:inp"]}]
outputs = [{name="out", cap=1, ports=["s:out"]}]

[[graphs]]
name = "top"
nodes = [{name="trigger", ty="AutoTrigger"}, {name="worker", ty="sub"}]
inputs = [{name="in", cap=1, ports=["trigger:inp"]}]
outputs = [{name="out", cap=1, ports=["trigger:out"]}]
connections = [
    {cap=1, ports=["trigger:feed", "worker:out"]},
    {cap=1, ports=["trigger:collect", "worker:inp"]},
]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(
        matches!(err, Error::UnknownPort { ref node, ref port } if node == "worker" && port == "out"),
        "实际：{err:?}"
    );
}

/// 校验规则之二：一条连接里**不能有两个 dyn 端口**（原版数 `dyn_rxn`/`dyn_txn`，同连接双 dyn 报错）。
/// 这里把 `feed` 与 `collect` 塞进同一条连接 → `BadConnection`。
#[test]
fn two_dyn_ports_in_one_connection_is_rejected() {
    let toml = r#"
main = "top"

[[graphs]]
name = "sub"
nodes = [{name="s", ty="Seq"}]
inputs = [{name="inp", cap=1, ports=["s:inp"]}]
outputs = [{name="out", cap=1, ports=["s:out"]}]

[[graphs]]
name = "top"
nodes = [{name="trigger", ty="AutoTrigger"}, {name="worker", ty="sub"}]
inputs = [{name="in", cap=1, ports=["trigger:inp"]}]
outputs = [{name="out", cap=1, ports=["trigger:out"]}]
connections = [
    {cap=1, ports=["trigger:feed", "trigger:collect", "worker:inp"]},
]
"#;
    let err = Builder::default().template(toml).build().unwrap_err();
    assert!(
        matches!(err, Error::BadConnection(_)),
        "实际：{err:?}"
    );
}
// ANCHOR_END: validation
