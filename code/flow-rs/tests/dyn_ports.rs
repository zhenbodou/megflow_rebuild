//! flow-rs · tests/dyn_ports —— Ch4.9 运行期动态子图实例的端到端契约。
//!
//! 验证 create → publish → fetch → route 环路：触发方 `create` 一份子图实例并把边界端点经
//! broker 广播；触发方与消费方各 `fetch` 回自己要的那一端；数据经无类型 `send_any`/`recv_any`
//! 穿过实例。全部 `tokio::time::timeout` 包裹——teardown 一旦挂起（例如 R3 没落实、残端钉住
//! 实例），超时即失败而非卡死 CI。参照 tests/broker.rs、tests/demux_e2e.rs 的写法。

use flow_rs::broker::Broker;
use flow_rs::builtin::Counter;
use flow_rs::config::{Config, GraphConfig};
use flow_rs::dyn_ports::{DynPorts, DynPortsConfig};
use flow_rs::prelude::*;
use flow_rs::resource::{AnyResource, BuildResource, ResourceCollection};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// 只有一个 `Transform`（无类型透传）的子图——tests 1/2/5 用它验证纯粹的搬运环路。
const SUB_TRANSFORM: &str = r#"
main="sub"
[[graphs]]
name="sub"
nodes=[{name="t",ty="Transform"}]
inputs=[{name="inp",cap=1,ports=["t:inp"]}]
outputs=[{name="out",cap=1,ports=["t:out"]}]
"#;

// 只有一个 `InitBump`（构造后在注入的共享计数器上 bump 一次）的子图——tests 3/4 用它数「建了几次实例」。
const SUB_INITBUMP: &str = r#"
main="sub"
[[graphs]]
name="sub"
nodes=[{name="t",ty="InitBump",res="counter"}]
inputs=[{name="inp",cap=1,ports=["t:inp"]}]
outputs=[{name="out",cap=1,ports=["t:out"]}]
"#;

/// 计数用节点：每个**实例**在 `initialize` 里对注入的共享计数器 bump **一次**（不是每消息）。
/// 于是「同一 key 只建一次实例」时计数恒为 1，重建则再 +1——用来把 create-once 守卫钉死。
#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
struct InitBump {
    res: String,
}

#[methods]
impl InitBump {
    async fn initialize(&mut self, ctx: &Context) {
        if let Some(counter) = ctx.resource::<Counter>(&self.res) {
            counter.bump();
        }
    }
    async fn exec(&mut self) -> Result<()> {
        let msg = self.inp.recv_any().await?;
        if let Some(out) = self.out.as_ref() {
            out.send_any(msg).await?;
        }
        Ok(())
    }
}
node_register!("InitBump", InitBump);

/// 把一段独立 `Config` TOML 里的 `main` 子图取成一份 owned `GraphConfig`（惰性构造器的原料）。
fn subgraph(toml: &str) -> GraphConfig {
    Config::from_toml(toml)
        .unwrap()
        .main_graph()
        .unwrap()
        .clone()
}

/// 在同一 topic "dyn" 上挂一个触发方（`DynPorts<Sender>`，抽入口 `inp`）+ 一个消费方
/// （`DynPorts<Receiver>`，抽出口 `out`）。二者必须在 `broker.run()` **之前** subscribe。
fn wire(sub: &GraphConfig) -> (Broker, DynPorts<Sender>, DynPorts<Receiver>) {
    let mut broker = Broker::new();
    let creator_client = broker.subscribe("dyn".into());
    let consumer_client = broker.subscribe("dyn".into());
    let creator = DynPorts::<Sender>::new(HashMap::from([(
        "port".to_owned(),
        DynPortsConfig {
            target: "inp".to_owned(),
            cap: 1,
            broker: creator_client,
            graph_config: sub.clone(),
        },
    )]));
    let consumer = DynPorts::<Receiver>::new(HashMap::from([(
        "port".to_owned(),
        DynPortsConfig {
            target: "out".to_owned(),
            cap: 1,
            broker: consumer_client,
            graph_config: sub.clone(),
        },
    )]));
    (broker, creator, consumer)
}

/// 预建一个共享 `Counter` 并按名 "counter" 注入——子图配置里不声明 resources，全靠注入。
fn shared_counter() -> (Arc<Counter>, ResourceCollection) {
    let counter: Arc<Counter> = Arc::new(Counter::build(&Default::default()).unwrap());
    let mut map: HashMap<String, AnyResource> = HashMap::new();
    map.insert("counter".to_owned(), counter.clone());
    (counter, ResourceCollection::from_map(map))
}

#[tokio::test]
async fn create_publishes_and_endpoint_round_trips_through_broker() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let sub = subgraph(SUB_TRANSFORM);
        let (mut broker, mut creator, mut consumer) = wire(&sub);
        let broker_task = broker.run();

        // create：现装实例、start、把边界端点 publish 上 broker。
        let handle = creator.create(7, ResourceCollection::default()).await.unwrap();
        // 创建者 fetch 回**自己 publish 的**那份 DynConns 的入口 Sender（证 broker 自发自收）。
        let sender = creator.fetch_with_cache(7).await.unwrap();
        // 消费者从**独立的一份** DynConns 拿回同一实例的出口 Receiver。
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

        // 拆除：drop 本地端点 + evict 掉缓存里的入口 Sender → 实例失去唯一外部发送端、停机。
        drop(sender);
        drop(receiver);
        creator.evict(7);
        handle.await.unwrap().unwrap();
        creator.close();
        consumer.close();
        broker_task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn routes_by_to_addr_and_each_key_is_its_own_instance() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let sub = subgraph(SUB_TRANSFORM);
        let (mut broker, mut creator, mut consumer) = wire(&sub);
        let broker_task = broker.run();

        let h7 = creator.create(7, ResourceCollection::default()).await.unwrap();
        let h42 = creator
            .create(42, ResourceCollection::default())
            .await
            .unwrap();

        // 每 key 各取各的入口/出口（fetch_with_cache 顺带把乱序到达的别的 key 缓冲进 cache）。
        let s7 = creator.fetch_with_cache(7).await.unwrap();
        let s42 = creator.fetch_with_cache(42).await.unwrap();
        let r7 = consumer.fetch_with_cache(7).await.unwrap();
        let r42 = consumer.fetch_with_cache(42).await.unwrap();

        s7.send_any(Envelope::new(String::from("seven")).seal())
            .await
            .unwrap();
        s42.send_any(Envelope::new(String::from("forty-two")).seal())
            .await
            .unwrap();

        // 无串台：7 的载荷只从 7 的实例出、42 只从 42 的实例出——各 key 是各自独立的一张图。
        let mut g7 = r7.recv_any().await.unwrap();
        let mut g42 = r42.recv_any().await.unwrap();
        assert_eq!(
            g7.downcast_mut::<Envelope<String>>().unwrap().unpack(),
            "seven"
        );
        assert_eq!(
            g42.downcast_mut::<Envelope<String>>().unwrap().unpack(),
            "forty-two"
        );

        drop((s7, s42, r7, r42));
        creator.evict(7);
        creator.evict(42);
        h7.await.unwrap().unwrap();
        h42.await.unwrap().unwrap();
        creator.close();
        consumer.close();
        broker_task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn instance_created_once_per_key() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let sub = subgraph(SUB_INITBUMP);
        let (mut broker, mut creator, mut consumer) = wire(&sub);
        let broker_task = broker.run();
        let (counter, resources) = shared_counter();

        // 对同一 key=5 连发 3 条，用 !is_cached 把门：只在第一条时 create。
        let mut handle = None;
        for n in 0..3u32 {
            if !creator.is_cached(5) {
                handle = Some(creator.create(5, resources.clone()).await.unwrap());
            }
            let sender = creator.fetch_with_cache(5).await.unwrap();
            sender.send_any(Envelope::new(n).seal()).await.unwrap();
        }
        // 消费者收满 3 条——确保实例 exec 跑过（也就保证 initialize 已跑过、bump 已发生）。
        let receiver = consumer.fetch_with_cache(5).await.unwrap();
        for _ in 0..3 {
            receiver.recv_any().await.unwrap();
        }
        // InitBump 只在 initialize 里 bump 一次；实例只建了一次 → 计数恰为 1（不是 3）。
        assert_eq!(counter.get(), 1);

        drop(receiver);
        creator.evict(5);
        handle.unwrap().await.unwrap().unwrap();
        creator.close();
        consumer.close();
        broker_task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn empty_payload_tears_down_and_recreate_is_fresh() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let sub = subgraph(SUB_INITBUMP);
        let (mut broker, mut creator, mut consumer) = wire(&sub);
        let broker_task = broker.run();
        let (counter, resources) = shared_counter();

        // 第一次：create key 9，送一条数据、消费者收到；计数=1（建了一个实例）。
        let h1 = creator.create(9, resources.clone()).await.unwrap();
        {
            let s = creator.fetch_with_cache(9).await.unwrap();
            s.send_any(Envelope::new(String::from("first")).seal())
                .await
                .unwrap();
        }
        {
            let r = consumer.fetch_with_cache(9).await.unwrap();
            let mut got = r.recv_any().await.unwrap();
            assert_eq!(
                got.downcast_mut::<Envelope<String>>().unwrap().unpack(),
                "first"
            );
        }
        assert_eq!(counter.get(), 1);

        // 拆除信号：一个 to_addr=Some(9) 的**空**信封（is_none 为真）——原版 DynDemux 用它作
        // 生命周期控制消息。本章由测试扮演那个「驱动」角色：见到空信封就拆掉该 key 的实例。
        let mut teardown = Envelope::<u32>::empty();
        teardown.info_mut().to_addr = Some(9);
        let signal = teardown.seal();
        assert!(signal.is_none());
        let key = signal.info().to_addr.expect("teardown signal needs an address");

        // 撤入口 Sender（evict 掉缓存那份；本地那份已在上面的作用域里 drop）→ 实例失去
        // 唯一外部发送端。R3 生效（消费者 fetch 时已 drop 掉 DynConns 里多余的入口 Sender clone），
        // 故没有残端钉住实例，h1 得以解析。
        creator.evict(key);
        consumer.evict(key); // 释放旧实例出口 Receiver：recreate 前必须清掉，否则 fetch 命中旧缓存。
        h1.await.unwrap().unwrap();

        // 重建即全新实例：create(9) 再来一次 → InitBump.initialize 再 bump 一次 → 计数=2。
        let h2 = creator.create(9, resources.clone()).await.unwrap();
        {
            let s = creator.fetch_with_cache(9).await.unwrap();
            s.send_any(Envelope::new(String::from("second")).seal())
                .await
                .unwrap();
        }
        {
            let r = consumer.fetch_with_cache(9).await.unwrap();
            let mut got = r.recv_any().await.unwrap();
            assert_eq!(
                got.downcast_mut::<Envelope<String>>().unwrap().unpack(),
                "second"
            );
        }
        assert_eq!(counter.get(), 2);

        creator.evict(9);
        consumer.evict(9);
        h2.await.unwrap().unwrap();
        creator.close();
        consumer.close();
        broker_task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn two_readers_of_one_output_receiver_compete() {
    // 诚实演示 R5：一个出口 Receiver 的两个 clone 共享同一队列、彼此**竞争**——每条消息只被
    // 其中一个收到（不是各得一份，那要靠 Bcast）。这正是「每 key 单消费者」纪律的由来。
    tokio::time::timeout(Duration::from_secs(3), async {
        let sub = subgraph(SUB_TRANSFORM);
        let (mut broker, mut creator, mut consumer) = wire(&sub);
        let broker_task = broker.run();

        let handle = creator.create(1, ResourceCollection::default()).await.unwrap();
        let sender = creator.fetch_with_cache(1).await.unwrap();
        let reader_a = consumer.fetch_with_cache(1).await.unwrap();
        let reader_b = reader_a.clone(); // 同一队列的第二个持有者

        // 边界通道 cap=1，故**发一条收一条**地推进（一次灌满 4 条会把透传管线顶死）。
        // 交替用两个 clone 来收：无 select（避免 poll-both 丢消息），纯顺序 await。
        let mut seen = Vec::new();
        for i in 0..4u32 {
            sender.send_any(Envelope::new(i).seal()).await.unwrap();
            let reader = if i % 2 == 0 { &reader_a } else { &reader_b };
            seen.push(reader.recv::<u32>().await.unwrap().unpack());
        }
        // 合计恰好 4 条、无重复：每条只被一个 clone 收到。若是 Bcast 广播语义，两个 reader
        // 会各收到全部 4 条（合计 8）；这里 reader_b 能收到、且总数是 4，正说明二者**分食同一队列**。
        seen.sort();
        assert_eq!(seen, vec![0, 1, 2, 3]);

        drop(sender);
        drop(reader_a);
        drop(reader_b);
        creator.evict(1);
        consumer.evict(1);
        handle.await.unwrap().unwrap();
        creator.close();
        consumer.close();
        broker_task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
