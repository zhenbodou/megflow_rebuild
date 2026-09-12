//! flow-rs · tests/dyn_port_derive —— Ch4.9a 派生宏 `dyn` 端口的编译期契约 + `set_port_dynamic` 派发。
//!
//! Ch4.9（机制层）里，触发方要手搓 `DynPorts::<Sender>::new(cfg)` 显式装配动态端口。本章让
//! **派生宏**替节点作者干这件事：写 `#[outputs(out: dyn T0)]` 就长出 `DynPorts<Sender>` 字段（模板
//! 载荷 → 无类型；具体载荷 `dyn i32` → 类型化 `DynPorts<SenderT<i32>>`），并生成 `Node::set_port_dynamic`
//! 把建图期送来的 [`DynPortsConfig`] 按端口名 `push` 进对应字段。对标原版 `node/demux.rs` 的
//! `#[inputs(inp:T0)] #[outputs(out:dyn T0)]` 签名与派生宏 `set_dyn_f`。
//!
//! 断言四件事：
//! - **(a)** `#[outputs(out: dyn T0)]` 生成的字段类型就是 `DynPorts<Sender>`（`let _: &DynPorts<Sender>` 编译期证明）。
//! - **(b)** `set_port_dynamic` 之前字段为空（`create` 报「无配置」）、之后按端口名把配置 `push` 进去。
//! - **(c)** 注入后 `create → fetch → route` 环路跑通（复用 ch09 的 broker + 子图夹具）。
//! - **(d)** 具体载荷 `dyn i32` 得到类型化 `DynPorts<SenderT<i32>>`，typed `fetch`/`send` 正常。

use flow_rs::broker::Broker;
use flow_rs::config::{Config, GraphConfig};
use flow_rs::dyn_ports::{DynPorts, DynPortsConfig};
use flow_rs::prelude::*;
use flow_rs::resource::ResourceCollection;
use std::collections::HashMap;
use std::time::Duration;

// ANCHOR: dyn_nodes
/// 无类型触发节点：模板载荷 `T0` → 派生宏生成 `out: DynPorts<Sender>`（入口侧，抽 `DynConns.inputs`）。
/// 只 `#[derive(Node)]`：本测试手工驱动 `self.out`、不把它跑成 actor，故不需要 `Actor`/`#[methods]`。
/// `#[inputs(inp: T0)]` 顺带注入 `inp` 与 `input_closed`——照抄原版 `DynDemux` 的触发签名。
#[inputs(inp: T0)]
#[outputs(out: dyn T0)]
#[derive(Node)]
struct Trigger {}

/// 类型化触发节点：具体载荷 `i32` → 派生宏生成 `out: DynPorts<SenderT<i32>>`。类型信息只在 `fetch`
/// 侧现场贴回（抽出的无类型 `Sender` 经 `From<Sender> for SenderT<i32>` 打上 `MsgTypeId` 标签）。
#[inputs(inp: T0)]
#[outputs(out: dyn i32)]
#[derive(Node)]
struct TypedTrigger {}
// ANCHOR_END: dyn_nodes

// ── 复用 ch09 的夹具：一张只有无类型 `Transform`（原样透传 `SealedEnvelope`）的子图 ──
const SUB_TRANSFORM: &str = r#"
main="sub"
[[graphs]]
name="sub"
nodes=[{name="t",ty="Transform"}]
inputs=[{name="inp",cap=1,ports=["t:inp"]}]
outputs=[{name="out",cap=1,ports=["t:out"]}]
"#;

/// 把一段独立 `Config` TOML 里的 `main` 子图取成 owned `GraphConfig`（惰性构造器原料）。同 ch09。
fn subgraph(toml: &str) -> GraphConfig {
    Config::from_toml(toml)
        .unwrap()
        .main_graph()
        .unwrap()
        .clone()
}

// ANCHOR: injects_test
#[tokio::test]
async fn derive_dyn_output_injects_then_round_trips() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let sub = subgraph(SUB_TRANSFORM);
        let mut broker = Broker::new();
        // 订阅必须先于 `broker.run()`（Ch4.8 硬纪律）：触发方与消费方各挂一个 client 在 topic "dyn"。
        let creator_client = broker.subscribe("dyn".into());
        let consumer_client = broker.subscribe("dyn".into());

        // 构造触发节点。`inp` 是 DynDemux 式的真实输入（本测试不读），塞个丢弃的 Receiver 占位。
        let (_tx, rx) = channel(1);
        let mut trigger = Trigger {
            inp: rx,
            input_closed: false,
            out: Default::default(),
        };
        // (a) 模板载荷 `dyn T0` 生成的字段就是无类型 `DynPorts<Sender>`——类型不符则此行编译不过。
        let _: &DynPorts<Sender> = &trigger.out;

        // (b) 注入前：派生宏把 `out` 初始化成空 `DynPorts`（同 `#[state]` 字段，`build` 不接线）——
        // 没有配置，`create` 立即报错。这钉死「端口配置是 `set_port_dynamic` 在建图期才 push 进来的」。
        assert!(trigger
            .out
            .create(1, ResourceCollection::default())
            .await
            .is_err());

        // (b) `set_port_dynamic` 按 `port_info.name`（= 字段名 "out"）把 `DynPortsConfig` push 进 `self.out`。
        let port_info = PortInfo {
            name: "out".to_owned(),
            ty: PortType::Dyn,
            mty: MsgType::any(),
        };
        trigger.set_port_dynamic(
            &port_info,
            DynPortsConfig {
                target: "inp".to_owned(), // 抽实例的**入口** Sender（往实例里灌数据）
                cap: 1,
                broker: creator_client,
                graph_config: sub.clone(),
            },
        );

        // 消费方仍用 ch09 的手写 `DynPorts<Receiver>`（抽实例出口），作对照的另一端。
        let mut consumer = DynPorts::<Receiver>::new(HashMap::from([(
            "port".to_owned(),
            DynPortsConfig {
                target: "out".to_owned(),
                cap: 1,
                broker: consumer_client,
                graph_config: sub.clone(),
            },
        )]));

        let broker_task = broker.run();

        // (c) 注入后：`create → fetch → route` 跑通，证明 push 进去的配置确实可用。
        let handle = trigger
            .out
            .create(7, ResourceCollection::default())
            .await
            .unwrap();
        let sender = trigger.out.fetch_with_cache(7).await.unwrap();
        let receiver = consumer.fetch_with_cache(7).await.unwrap();
        sender
            .send_any(Envelope::new(String::from("frame")).seal())
            .await
            .unwrap();
        let mut got = receiver.recv_any().await.unwrap();
        assert_eq!(
            got.downcast_mut::<Envelope<String>>().unwrap().unpack(),
            "frame"
        );

        // 拆除（R3）：drop 本地端点 + evict 缓存里的入口 Sender → 实例失去唯一外部发送端、停机，handle 解析。
        drop(sender);
        drop(receiver);
        trigger.out.evict(7);
        handle.await.unwrap().unwrap();
        trigger.out.close();
        consumer.close();
        broker_task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
// ANCHOR_END: injects_test

// ANCHOR: typed_test
#[tokio::test]
async fn derive_dyn_output_with_concrete_payload_is_typed() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let sub = subgraph(SUB_TRANSFORM);
        let mut broker = Broker::new();
        let creator_client = broker.subscribe("dyn".into());
        let consumer_client = broker.subscribe("dyn".into());

        let (_tx, rx) = channel(1);
        let mut trigger = TypedTrigger {
            inp: rx,
            input_closed: false,
            out: Default::default(),
        };
        // (d) 具体载荷 `dyn i32` → 派生宏生成**类型化** `DynPorts<SenderT<i32>>`——类型不符则编译不过。
        let _: &DynPorts<SenderT<i32>> = &trigger.out;

        let port_info = PortInfo {
            name: "out".to_owned(),
            ty: PortType::Dyn,
            mty: MsgType::of::<i32>(),
        };
        trigger.set_port_dynamic(
            &port_info,
            DynPortsConfig {
                target: "inp".to_owned(),
                cap: 1,
                broker: creator_client,
                graph_config: sub.clone(),
            },
        );

        let mut consumer = DynPorts::<Receiver>::new(HashMap::from([(
            "port".to_owned(),
            DynPortsConfig {
                target: "out".to_owned(),
                cap: 1,
                broker: consumer_client,
                graph_config: sub.clone(),
            },
        )]));

        let broker_task = broker.run();

        let handle = trigger
            .out
            .create(7, ResourceCollection::default())
            .await
            .unwrap();
        // typed `fetch`：抽出的无类型 `Sender` 经 `From<Sender> for SenderT<i32>` 打上 `MsgTypeId` 标签。
        let sender: SenderT<i32> = trigger.out.fetch_with_cache(7).await.unwrap();
        let receiver = consumer.fetch_with_cache(7).await.unwrap();
        // 类型化 `send` 只收 `Envelope<i32>`（编译期挡住类型不符）；经无类型 Transform 边界原样透传。
        sender.send(Envelope::new(42i32)).await.unwrap();
        let mut got = receiver.recv_any().await.unwrap();
        assert_eq!(got.downcast_mut::<Envelope<i32>>().unwrap().unpack(), 42);

        drop(sender);
        drop(receiver);
        trigger.out.evict(7);
        handle.await.unwrap().unwrap();
        trigger.out.close();
        consumer.close();
        broker_task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
// ANCHOR_END: typed_test
