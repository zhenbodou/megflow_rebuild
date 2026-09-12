//! flow-rs · examples/dynports_steps —— Ch4.9 动态子图 create→publish→fetch→route→teardown 环路的最小驱动。
//!
//! 镜像 examples/demux_steps.rs 的写法：先写一个裸的 `route` 驱动（扮演后续 4.9c 里注册版
//! `DynDemux` 节点在 `exec` 里要做的事），在 `main` 里显式跑通「按 key 现装实例 → 经 broker 广播
//! 端点 → 两侧各 fetch 接线 → 无类型灌数据 → 空信封拆除」。全程只用无类型 `send_any`/`recv_any`。

use flow_rs::broker::Broker;
use flow_rs::config::{Config, GraphConfig};
use flow_rs::dyn_ports::{DynPorts, DynPortsConfig};
use flow_rs::prelude::*;
use flow_rs::resource::ResourceCollection;
use std::collections::HashMap;
use tokio::task::JoinHandle;

// 目标子图（惰性构造器）：一个无类型透传 `Transform`，边界端口 inp/out。
const SUBGRAPH: &str = r#"
main="worker"
[[graphs]]
name="worker"
nodes=[{name="t",ty="Transform"}]
inputs=[{name="inp",cap=1,ports=["t:inp"]}]
outputs=[{name="out",cap=1,ports=["t:out"]}]
"#;

/// 动态路由：扮演后续 4.9c 里 `DynDemux` 节点 `exec` 要做的事。
/// - **空载荷**（`is_none`）= 拆除信号：撤掉该 key 的入口 `Sender`（+ await 其任务），实例停机。
/// - **有载荷**：按需 `create`（每 key 只建一次，用 `is_cached` 把门）、`fetch` 入口、`send_any` 送进对应实例。
async fn route(
    creator: &mut DynPorts<Sender>,
    handles: &mut HashMap<u64, JoinHandle<Result<()>>>,
    resources: &ResourceCollection,
    message: SealedEnvelope,
) -> Result<()> {
    let key = message
        .info()
        .to_addr
        .expect("dynamic route needs a destination key");
    if message.is_none() {
        if let Some(handle) = handles.remove(&key) {
            creator.evict(key); // drop 缓存入口 Sender → 实例唯一外部发送端消失
            handle.await.ok(); // 实例收到 ChannelClosed、优雅停机（R3 生效才不会挂起）
            println!("· 拆除 key {key}：撤入口端点，实例停机");
        }
        return Ok(());
    }
    if !creator.is_cached(key) {
        let handle = creator.create(key, resources.clone()).await?;
        handles.insert(key, handle);
        println!("· 新建 key {key} 的实例，边界端点已广播上 broker");
    }
    let sender = creator.fetch_with_cache(key).await?;
    sender.send_any(message).await.ok();
    Ok(())
}

/// 造一个带目标地址的有载荷信封。
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

/// 造一个带目标地址的**空**信封（`is_none` 为真）——动态子图的生命周期拆除信号。
fn teardown(key: u64) -> SealedEnvelope {
    let mut empty = Envelope::<String>::empty();
    empty.info_mut().to_addr = Some(key);
    empty.seal()
}

/// 收消费者侧某 key 出口的一条载荷，downcast 成 String。
async fn recv_string(consumer: &mut DynPorts<Receiver>, key: u64) -> Result<String> {
    let mut got = consumer.fetch_with_cache(key).await?.recv_any().await?;
    Ok(got
        .downcast_mut::<Envelope<String>>()
        .expect("payload is a String envelope")
        .unpack())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let worker: GraphConfig = Config::from_toml(SUBGRAPH)?
        .main_graph()
        .expect("worker subgraph")
        .clone();

    // 单 topic "dyn" 上挂：一个创建者（抽入口 inp）+ 一个消费者（抽出口 out）。
    // 二者都必须在 broker.run() **之前** subscribe。
    let mut broker = Broker::new();
    let creator_client = broker.subscribe("dyn".into());
    let consumer_client = broker.subscribe("dyn".into());
    let mut creator = DynPorts::<Sender>::new(HashMap::from([(
        "port".to_owned(),
        DynPortsConfig {
            target: "inp".to_owned(),
            cap: 1,
            broker: creator_client,
            graph_config: worker.clone(),
        },
    )]));
    let mut consumer = DynPorts::<Receiver>::new(HashMap::from([(
        "port".to_owned(),
        DynPortsConfig {
            target: "out".to_owned(),
            cap: 1,
            broker: consumer_client,
            graph_config: worker,
        },
    )]));
    let broker_task = broker.run();

    let resources = ResourceCollection::default();
    let mut handles: HashMap<u64, JoinHandle<Result<()>>> = HashMap::new();

    // 1) 给 key 7 送第一帧：首次触发 create（现装实例 + 广播端点），再 fetch 入口、灌数据。
    route(&mut creator, &mut handles, &resources, addressed("frame-1", 7)).await?;
    println!("key 7 收到：{}", recv_string(&mut consumer, 7).await?);

    // 2) 再给 key 7 送一帧：is_cached 命中，不重建，复用同一实例（fetch 命中缓存、不再走 broker）。
    route(&mut creator, &mut handles, &resources, addressed("frame-2", 7)).await?;
    println!("key 7 收到：{}", recv_string(&mut consumer, 7).await?);

    // 3) key 42 是另一张**独立**的图：它的载荷只从 42 的实例冒出，与 7 互不串台。
    route(&mut creator, &mut handles, &resources, addressed("frame-42", 42)).await?;
    println!("key 42 收到：{}", recv_string(&mut consumer, 42).await?);

    // 4) 拆除：给 key 7 发一个空信封（is_none）。route 见状撤入口端点 + await 任务。
    route(&mut creator, &mut handles, &resources, teardown(7)).await?;
    // key 42 同样拆掉，收尾干净。
    route(&mut creator, &mut handles, &resources, teardown(42)).await?;

    // 5) 关闭两个 broker client → topic 任务结束 → run() 句柄解析。
    creator.close();
    consumer.close();
    broker_task.await.ok();
    println!("create→publish→fetch→route→teardown 环路跑通。");
    Ok(())
}
