//! flow-rs · dyn_ports —— 运行期**动态子图实例**（Ch4.9，机制层）。
//!
//! 原版 MegFlow 没有「动态子图节点类型」，而是一条运行期环路：子图作为**惰性构造器**
//! 注册；触发节点在运行期按某个 key（原版按视频流条数等信号）`create` 一份子图实例、
//! 为它开好 channel、`start` 跑起来，再把实例的**边界端点句柄**（[`DynConns`]）经
//! broker **广播**出去；订阅方 `fetch` 取回端点、完成接线。核心业务逻辑就是这条
//! **create → publish → fetch → route** 环路（原版 `node/port.rs` + `node/demux.rs`）。
//!
//! **本章只造机制**，照抄 broker/subgraph/demux 章的既定风格——先显式构造、真实测试，
//! 把 TOML / 派生宏 / config 自动接线的糖留到后续子章（见文件尾「落差与后续」）：
//! - 实例构造器**复用** [`MainGraph::assemble_graph`](crate::graph::MainGraph::assemble_graph)：
//!   一个动态子图实例 = 用**注入的** [`ResourceCollection`] 把某个具名 [`GraphConfig`]
//!   装配成的 [`MainGraph`]；它的边界 `inputs`(Sender)/`outputs`(Receiver) 就是
//!   [`DynConns`] 载荷，`MainGraph::start()` 把它跑起来。
//! - 路由用 `send_any`/`recv_any` 搬无类型 `SealedEnvelope`，机制层（Ch4.9）**只用**无类型
//!   [`DynPorts<Sender>`] / [`DynPorts<Receiver>`]。文件尾的**类型化** 4 特化（`DynPorts<SenderT<T>>`
//!   等）与 `dyn_fetch_methods!` 宏去重是 **Ch4.9a** 的终点形态，与无类型两个并列在同一文件里
//!   （对齐原版 `node/port.rs` 的 `port_impl!`）；本章路由用不到它们。
//!
//! Runtime dynamic-subgraph instances. A subgraph instance is a `MainGraph`
//! assembled from a named `GraphConfig` with an injected `ResourceCollection`;
//! its boundary senders/receivers are broadcast as `DynConns` over the broker,
//! and subscribers `fetch` the endpoint they need. This module builds only the
//! mechanism (explicit `create`/`fetch`); the derive/config sugar comes later.

use crate::broker::BrokerClient;
use crate::channel::{Receiver, ReceiverT, Sender, SenderT};
use crate::config::GraphConfig;
use crate::error::{Error, Result};
use crate::resource::ResourceCollection;
use std::collections::HashMap;
use tokio::task::JoinHandle;

// ANCHOR: dyn_conns
/// 一个动态子图实例的**边界端点句柄**，经 broker 广播给触发方与订阅方。
///
/// - `name`：这次实例化的 key（原版按运行期信号取值，本章直接用作路由地址）。
/// - `graph`：被实例化的子图名（信息字段，便于观测/断言）。
/// - `inputs`：实例的对外**输入**句柄 `边界端口名 → Sender`——谁想往实例里灌数据取它。
/// - `outputs`：实例的对外**输出**句柄 `边界端口名 → Receiver`——谁想收实例产出取它。
///
/// 这正是 Ch4.8 broker 章末所说、要经 broker 传递的「共享端点句柄」。`Sender`/`Receiver`
/// 都 `Clone + Send + 'static`，故整个 `DynConns` 能被 `broker.publish::<DynConns>()` 密封搬运；
/// broker 给**每个**订阅者 fan-out 一份**独立克隆**，于是触发方与消费方各取各的那一端。
#[derive(Clone)]
pub struct DynConns {
    pub name: u64,
    pub graph: String,
    pub inputs: HashMap<String, Sender>,
    pub outputs: HashMap<String, Receiver>,
}
// ANCHOR_END: dyn_conns

// ANCHOR: dyn_ports_config
/// 一个动态端口的**惰性构造器 + 广播信道**配置（教学子集）。
///
/// - `target`：要从 [`DynConns`] 里抽出的**边界端口名**——`DynPorts<Sender>` 抽
///   `inputs[target]`（入口）、`DynPorts<Receiver>` 抽 `outputs[target]`（出口）。
/// - `cap`：预留的通道容量。本章边界通道容量由子图配置自身的 `PortConfig.cap` 决定，
///   此字段暂未参与 `create`，留给后续 `args` 合并（4.9a）用。
/// - `broker`：这个动态端口所在 topic 的 client——`create` 用它 `publish`、`fetch` 用它取回。
/// - `graph_config`：直接持有目标子图的 [`GraphConfig`]（`GraphConfig: Clone`）当**惰性构造器**。
///   「构造实例」就是 `MainGraph::assemble_graph(&graph_config, resources)`。这替代了原版
///   `GraphSlice.cons` / `registry_local()` 那套全局注册；原版的 `local_key` / `typeinfo` / `args`
///   本章略去：`typeinfo` 由 `assemble_graph` 内的 `type_infer::infer` 自行推断，`args` 覆盖留后续。
pub struct DynPortsConfig {
    pub target: String,
    pub cap: usize,
    pub broker: BrokerClient,
    pub graph_config: GraphConfig,
}
// ANCHOR_END: dyn_ports_config

// ANCHOR: dyn_ports_struct
/// 一个节点的动态端口集合 + 已建实例的端点缓存。
///
/// `V` 是「从 [`DynConns`] 抽出、缓存起来的端点类型」——本章两个无类型特化：
/// [`DynPorts<Sender>`]（入口侧，触发方用）与 [`DynPorts<Receiver>`]（出口侧，消费方用）。
///
/// - `cfg`：`节点的动态端口名 → 该端口的 [`DynPortsConfig`]`。教学子集里通常只有一个
///   动态端口（单 topic），故内部方法取 [`single_config`](Self::single_config)。
/// - `cache`：`key → 已抽出的端点`——按 key 记住「这个实例已经建过、端点已取回」。触发方
///   在 `create` 前用 [`is_cached`](Self::is_cached) 把门，实现原版「每 key 只建一次」。
///
/// （去掉了原版多余的 `_v_holder: PhantomData<V>`——`cache: HashMap<u64, V>` 已经约束 `V`。）
pub struct DynPorts<V> {
    cfg: HashMap<String, DynPortsConfig>,
    cache: HashMap<u64, V>,
}
// ANCHOR_END: dyn_ports_struct

// ANCHOR: dyn_ports_default
/// 空的 `DynPorts`（无配置、无缓存）。派生宏（Ch4.9a）给 `#[outputs(out: dyn T0)]` 生成的
/// 动态端口字段就靠它初始化——同 `#[state]` 字段：`build()` 不从 `ins`/`outs` 消费，先 `default()`
/// 占位，端口配置随后由 `set_port_dynamic` 在建图期 `push` 进来（Ch4.9b）。
impl<V> Default for DynPorts<V> {
    fn default() -> Self {
        Self {
            cfg: HashMap::new(),
            cache: HashMap::new(),
        }
    }
}
// ANCHOR_END: dyn_ports_default

// ANCHOR: dyn_ports_common
impl<V> DynPorts<V> {
    /// 用一组「动态端口名 → 配置」建一个 `DynPorts`。教学子集里通常只放一个端口。
    pub fn new(cfg: HashMap<String, DynPortsConfig>) -> Self {
        Self {
            cfg,
            cache: HashMap::new(),
        }
    }

    /// 追加一个动态端口配置（键 = 该动态端口在节点上的端口名）。
    pub fn push(&mut self, port: impl Into<String>, cfg: DynPortsConfig) {
        self.cfg.insert(port.into(), cfg);
    }

    /// 取这个动态端口集合里**唯一**的配置（教学子集：一个节点一个动态端口 = 单 topic）。
    /// 多动态端口按端口名分派留到派生宏那章（4.9a）；这里没有配置就是用法错误。
    fn single_config(&self) -> Result<&DynPortsConfig> {
        self.cfg
            .values()
            .next()
            .ok_or_else(|| Error::Unsupported("DynPorts has no configured port".to_owned()))
    }

    /// **create**：按 `key` 现装一份子图实例、跑起来、把边界端点经 broker 广播出去。
    ///
    /// 复用 [`MainGraph::assemble_graph`](crate::graph::MainGraph::assemble_graph)——用**注入的**
    /// `resources` 把 `cfg.graph_config` 装成一张 `MainGraph`，采集它的边界端点（入口 `Sender`
    /// 克隆一份、出口 `Receiver` 移出来）塞进 [`DynConns`]，`start()` 跑起来，`publish` 广播。
    ///
    /// **只返回 `JoinHandle`**：端点一律经 broker `fetch` 取回，不抄近路直接返回本地 `Sender`——
    /// 否则 broker 章白讲，也证不出「创建者能 `fetch` 回自己 `publish` 的那份 `DynConns`」。
    /// `instance` 在函数结束时 drop 是**安全**的：`start()` 已把 actors 取走跑成独立任务；
    /// 我们 publish 的是端点的**克隆**（入口）/已移出的那份（出口），实例的边界通道靠它们续命。
    pub async fn create(
        &self,
        key: u64,
        resources: ResourceCollection,
    ) -> Result<JoinHandle<Result<()>>> {
        let cfg = self.single_config()?;
        let mut instance = crate::graph::MainGraph::assemble_graph(&cfg.graph_config, resources)?;

        // 采集边界端点。`*_names()` 借的是 `&str`，先 own 下来断开对 instance 的借用，
        // 再遍历 `input()`（克隆 Sender）/ `take_output()`（移出 Receiver）取端点。
        let input_names: Vec<String> = instance
            .input_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut inputs = HashMap::new();
        for name in input_names {
            if let Some(sender) = instance.input(&name) {
                inputs.insert(name, sender);
            }
        }
        let output_names: Vec<String> = instance
            .output_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut outputs = HashMap::new();
        for name in output_names {
            if let Some(receiver) = instance.take_output(&name) {
                outputs.insert(name, receiver);
            }
        }

        let handle = instance.start();
        cfg.broker
            .publish(DynConns {
                name: key,
                graph: cfg.graph_config.name.clone(),
                inputs,
                outputs,
            })
            .await;
        Ok(handle)
    }

    /// 这个 key 的端点是否已 `fetch` 回并缓存——`create` 前用它把门（每 key 只建一次）。
    pub fn is_cached(&self, key: u64) -> bool {
        self.cache.contains_key(&key)
    }

    /// 只读借用某 key 已缓存的端点。/ borrow a cached endpoint.
    pub fn cache(&self, key: u64) -> Option<&V> {
        self.cache.get(&key)
    }

    /// 可变借用某 key 已缓存的端点。/ borrow a cached endpoint mutably.
    pub fn cache_mut(&mut self, key: u64) -> Option<&mut V> {
        self.cache.get_mut(&key)
    }

    /// **拆除**：移除并交还某 key 的缓存端点。触发方把返回的端点一 drop，这个 key 的
    /// 那一端边界通道就断了——入口侧就是撤掉实例**唯一**的外部 `Sender`，实例随之收到
    /// `ChannelClosed`、优雅停机，`create` 时拿到的 `JoinHandle` 得以 resolve（见测试 4）。
    pub fn evict(&mut self, key: u64) -> Option<V> {
        self.cache.remove(&key)
    }

    /// 关闭所有动态端口的 broker client（topic 任务随之结束，`Broker::run` 句柄可解析）。
    pub fn close(&self) {
        for cfg in self.cfg.values() {
            cfg.broker.close();
        }
    }
}
// ANCHOR_END: dyn_ports_common

// ANCHOR: dyn_fetch_macro
/// `fetch` 三方法（`fetch` / `try_fetch` / `fetch_with_cache`）在四个特化里**逻辑完全相同**，
/// 只差两点：从 `DynConns` 的哪张表抽端点（入口抽 `inputs`、出口抽 `outputs`），以及抽出的
/// 无类型 `Sender`/`Receiver` 要不要**包壳成类型化端点**。后者用 `.map(Into::into)` / `.into()`
/// 归一：无类型特化走 `From<Sender> for Sender` 的**恒等**转换、类型化特化走 `From<Sender>
/// for SenderT<T>`（打上 `MsgTypeId` 标签）。于是四份实现收进这一个宏，按 `$field` / `$endpoint`
/// / `$noun`（错误文案）实例化——对齐原版 `node/port.rs` 的 `port_impl!`。
macro_rules! dyn_fetch_methods {
    ($endpoint:ty, $field:ident, $noun:literal) => {
        /// 阻塞式取回 `key` 实例的端点（无缓存版）：一直 `fetch` 到 `name == key` 的
        /// [`DynConns`]，只抽出 `target` 那**一个**端点、其余当场 drop（**R3**：不残留端点钉住实例）。
        pub async fn fetch(&self, key: u64) -> Result<$endpoint> {
            let cfg = self.single_config()?;
            loop {
                let mut conns = cfg.broker.fetch::<DynConns>().await?;
                if conns.name == key {
                    return conns.$field.remove(&cfg.target).map(Into::into).ok_or_else(|| {
                        Error::Unsupported(format!(
                            "dynamic {} port {:?} missing in instance {key}",
                            $noun, cfg.target
                        ))
                    });
                }
                // 非目标 key 的 DynConns 在此 drop（无缓存版本按单实例用法设计）。
            }
        }

        /// 非阻塞版：队首恰是目标 key 才取出，否则返回 `None`（best-effort，单实例用）。
        pub fn try_fetch(&self, key: u64) -> Option<$endpoint> {
            let cfg = self.single_config().ok()?;
            let mut conns = cfg.broker.try_fetch::<DynConns>()?;
            if conns.name == key {
                conns.$field.remove(&cfg.target).map(Into::into)
            } else {
                None
            }
        }

        /// 带缓存版（多 key 用这个）：命中缓存直接返回克隆；否则把陆续 `fetch` 到的每份
        /// `DynConns` 都抽出其 `target` 端点存进 `cache[name]`（**顺带缓冲乱序到达的别的 key**），
        /// 直到 `key` 就位。每份 `DynConns` 只留 `target` 那个端点、其余 drop（**R3**）。
        pub async fn fetch_with_cache(&mut self, key: u64) -> Result<$endpoint> {
            let target = self.single_config()?.target.clone();
            while !self.cache.contains_key(&key) {
                // 把 `self.cfg` 的借用限制在这个块里、fetch 完即释放，好与随后的 `&mut self.cache` 分裂借用。
                let mut conns = {
                    let cfg = self.single_config()?;
                    cfg.broker.fetch::<DynConns>().await
                }?;
                let name = conns.name;
                if let Some(endpoint) = conns.$field.remove(&target) {
                    self.cache.insert(name, endpoint.into());
                }
                // conns 其余端点（另一张表、别的端口）在此 drop（R3）。
            }
            self.cache
                .get(&key)
                .cloned()
                .ok_or_else(|| Error::Unsupported(format!("dynamic {} for key {key} missing", $noun)))
        }
    };
}
// ANCHOR_END: dyn_fetch_macro

// ANCHOR: dyn_ports_sender
/// 入口侧无类型特化：从 `DynConns.inputs` 抽出实例的**输入 `Sender`**（往实例里灌数据）。
/// 方法体全部来自 `dyn_fetch_methods!`——抽 `inputs`、端点即无类型 `Sender`（`Into` 恒等）。
impl DynPorts<Sender> {
    dyn_fetch_methods!(Sender, inputs, "input");
}
// ANCHOR_END: dyn_ports_sender

// ANCHOR: dyn_ports_receiver
/// 出口侧无类型特化：从 `DynConns.outputs` 抽出实例的**输出 `Receiver`**（收实例产出）。
///
/// 与 `DynPorts<Sender>` 结构对称，只是抽 `outputs` 而非 `inputs`。**单消费者纪律**：
/// `Receiver` 的多个 clone 共享同一队列、彼此**竞争**消息（Ch1.4 的忠实语义，非 bug），
/// 故每个 key 的出口只应有一个持有者在收——broker 已保证每订阅者一份独立 `DynConns`。
impl DynPorts<Receiver> {
    dyn_fetch_methods!(Receiver, outputs, "output");
}
// ANCHOR_END: dyn_ports_receiver

// ANCHOR: dyn_ports_typed
/// 类型化特化（Ch4.9a）：`#[outputs(out: dyn T0)]` 带**具体载荷**时派生宏生成的字段类型是
/// `DynPorts<SenderT<T>>` / `DynPorts<ReceiverT<T>>`。fetch 侧比无类型多一步——抽出的
/// `Sender`/`Receiver` 经 `From<Sender> for SenderT<T>` 打上 `MsgTypeId::of::<T>()` 标签。
/// **打标签只发生在 fetch 侧**：`create` 恒发无类型 `DynConns`（端点擦除入队），类型信息在
/// 订阅方按声明的载荷 `T` 现场贴回——与原版 `port_impl!` 的 typed 臂一致。`T: Clone` 由
/// `fetch_with_cache` 的 `cache.get().cloned()` 逼出（`SenderT<T>` 的 `#[derive(Clone)]` 需要它）；
/// 进引擎的消息本就要求 `Clone`（Ch1.3），故这不是额外负担。
impl<T: Clone + 'static> DynPorts<SenderT<T>> {
    dyn_fetch_methods!(SenderT<T>, inputs, "input");
}
impl<T: Clone + 'static> DynPorts<ReceiverT<T>> {
    dyn_fetch_methods!(ReceiverT<T>, outputs, "output");
}
// ANCHOR_END: dyn_ports_typed
