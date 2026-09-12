//! flow-rs · graph —— Graph Builder（Ch3.2）：`Config` → 装配 → `MainGraph`。
//!
//! **注册表与配置层在这里合流**。Ch2.4 的注册表能按类型名 `find` 出构造器，但它按
//! **位置**接线；Ch3.1 的配置按**名字**接线（`"add:a"`）。本章的 Builder 就是这两者
//! 之间的桥：拿着 `Config`，为每条对外端口开一条 channel，按 `PortRef` 把 channel 的
//! 两端分派到「对外句柄」与「节点的命名端口」上，再借注册条目里的
//! [`NodeRegistration::inputs`]/[`outputs`] 端口名表，把「按名收集的 channel」排成
//! 「构造器要的按位置 `Vec`」，最后 `(ctor)(args, ins, outs)` 造出节点。
//!
//! **跨引用校验也在这里落地**（Ch3.1 承诺的「校验前移到 build()」）：`main` 指向的图
//! 不存在、类型名查不到、端口引用指向不存在的节点、配置接了节点没有的端口、节点声明
//! 的端口未接线则保留默认端点（原版允许此情况），不作为建图错误。
//!
//! **Ch3.3 在装配之上加了调度层**：[`MainGraph::start`] 把每个装好的节点 spawn 成一个
//! tokio 任务、收敛成一个覆盖全图的聚合句柄，[`MainGraph::stop`] 撤掉所有对外输入触发
//! 优雅停机——`MainGraph` 由此从「静态蓝图」变成「能跑、又能干净停下的机器」。
//!
//! 名字↔位置的桥示意（单节点 `add`，两输入 `a`/`b`、一输出 `c`）：
//!
//! ```text
//!  Config(inputs/outputs)              per-node by-name maps            reg.inputs/outputs
//!  a → ["add:a"]   ─┐                  node_ins["add"] = {a: rx, b: rx}   ["a","b"]  ┐
//!  b → ["add:b"]   ─┼─ channel(cap) ─▶ node_outs["add"] = {c: tx}        ["c"]      ┼▶ (ctor)(args, ins, outs)
//!  c ← ["add:c"]   ─┘                                                                ┘
//! ```
//!
//! Graph builder: `Config` → assembled `MainGraph`. Bridges the registry's
//! positional construction and the config's named wiring; cross-ref validation
//! lands here.

use crate::broker::Broker;
use crate::channel::{channel_with_type, Receiver, Sender};
use crate::config::interlayer::{MsgType, PortInfo, PortType};
use crate::config::{Config, GraphConfig, PortRef};
use crate::context::Context;
use crate::dyn_ports::DynPortsConfig;
use crate::error::{Error, Result};
use crate::node::Actor;
use crate::registry;
use crate::registry::TaggedEndpoint;
use crate::resource::{AnyResource, ResourceCollection};
use std::collections::{HashMap, HashSet};
use tokio::task::JoinHandle;

/// 查一个节点实例的注册条目：节点名不存在 → `UnknownNode`，类型没注册 → `UnknownNodeType`。
/// 接线时要用注册条目里的端口名表 + 数组标记表判断「某端口是不是数组端口」。
/// Look up a node instance's registration (for its port tables + array-ness).
fn node_reg(g: &GraphConfig, node: &str) -> Result<&'static registry::NodeRegistration> {
    let nd = g
        .nodes
        .iter()
        .find(|n| n.name == node)
        .ok_or_else(|| Error::UnknownNode(node.to_owned()))?;
    registry::find(&nd.ty).ok_or_else(|| Error::UnknownNodeType(nd.ty.clone()))
}

/// 把一个 `Receiver` 挂到某节点输入端口的**端口组**上（Ch4.2：一个端口名 → 一组 channel 端）。
/// **标量**输入端口只能接 1 条边，已有边再接 → `PortAlreadyConnected`；**数组**输入端口
/// （`Vec<Receiver>`）可接多条（扇入 Merge）。未声明的端口按标量处理，留到构造期的
/// 「多余端口」检查报 `UnknownPort`。
fn attach_receiver(
    node_ins: &mut HashMap<String, HashMap<String, Vec<TaggedEndpoint<Receiver>>>>,
    reg: &registry::NodeRegistration,
    node: &str,
    port: &str,
    rx: Receiver,
    tag: Option<u64>,
) -> Result<()> {
    let slot = node_ins
        .entry(node.to_owned())
        .or_default()
        .entry(port.to_owned())
        .or_default();
    if !reg.input_is_array(port) && !reg.input_is_dict(port) && !slot.is_empty() {
        return Err(Error::PortAlreadyConnected {
            node: node.to_owned(),
            port: port.to_owned(),
        });
    }
    slot.push(TaggedEndpoint::new(rx, tag));
    Ok(())
}

/// 把一个 `Sender` 挂到某节点输出端口的**端口组**上。**标量**输出端口只能接 1 条边，
/// 已有边再接 → `PortAlreadyConnected`；**数组**输出端口（`Vec<Sender>`）可接多条（扇出 Bcast）。
fn attach_sender(
    node_outs: &mut HashMap<String, HashMap<String, Vec<TaggedEndpoint<Sender>>>>,
    reg: &registry::NodeRegistration,
    node: &str,
    port: &str,
    tx: Sender,
    tag: Option<u64>,
) -> Result<()> {
    let slot = node_outs
        .entry(node.to_owned())
        .or_default()
        .entry(port.to_owned())
        .or_default();
    if !reg.output_is_array(port) && !reg.output_is_dict(port) && !slot.is_empty() {
        return Err(Error::PortAlreadyConnected {
            node: node.to_owned(),
            port: port.to_owned(),
        });
    }
    slot.push(TaggedEndpoint::new(tx, tag));
    Ok(())
}

/// 建图器：吃一段图拓扑 TOML，产出装配好的 [`MainGraph`]。
///
/// 用 fluent 风格 `Builder::default().template(toml).build()?`，与原版
/// `Builder::default().template(..)` 对齐——也为 Ch3.3/后续往里加 `resources`、
/// 覆盖参数等留出扩展位（just-in-time：现在只有 `template` 一个输入）。
#[derive(Default)]
pub struct Builder {
    template: Option<String>,
}

impl Builder {
    /// 提供图拓扑 TOML 文本。
    /// Provide the graph-topology TOML text.
    pub fn template(mut self, toml: impl Into<String>) -> Self {
        self.template = Some(toml.into());
        self
    }

    /// 解析 + 压平 + 装配：`Config::from_toml` → [`subgraph::flatten`] → [`MainGraph::assemble`]。
    /// 解析失败、子图环、跨引用校验失败都在这里返回 `Err`。
    ///
    /// **压平这趟是 Ch4.4 新插的**：把「主图 + 被引用的子图」内联展开成一张扁平图，之后
    /// `assemble` 一行不改地跑。无子图引用时 `flatten` 是恒等变换，故前几章的单图配置行为不变。
    /// Parse, flatten subgraphs, then assemble; parse/cycle/cross-reference errors surface here.
    pub fn build(self) -> Result<MainGraph> {
        let text = self
            .template
            .ok_or_else(|| Error::Unsupported("builder has no template".to_owned()))?;
        let config = Config::from_toml(&text)?;
        let flat = crate::subgraph::flatten(&config)?;
        MainGraph::assemble(&flat)
    }
}

/// 装配好的主图：一组待运行的节点 + 对外输入/输出句柄 + 图的共享资源。
///
/// - `actors`：造好、接好线的节点，等着被 spawn（Ch3.3 的 `start()` 会接手）。
/// - `node_names`：与 `actors` **同序**的节点实例名（Ch4.3）——`start()` 用它为每个节点
///   构造带名字的 [`Context`]，好让日志/资源借用能认得「我是谁」。
/// - `inputs`：对外**输入**句柄 `名字 → Sender`——用户 `input(name)` 拿到它往图里发。
/// - `outputs`：对外**输出**句柄 `名字 → Receiver`——用户 `take_output(name)` 拿到它从图里收。
/// - `resources`：图的**共享资源集**（Ch4.3）——装配期一次建好、`start()` 时随 `Context`
///   分发给每个节点；`MainGraph` 自己也留一份（`ResourceCollection` 是 `Arc` 共享的廉价克隆），
///   于是测试能在图跑完后 `graph.resource::<T>(..)` 读回同一个实例、验证「只造了一份」。
/// - `broker`：动态子图（Ch4.9b）的通知 broker——只有装配期识别出 `dyn` 连接才 `Some`，
///   `start()` 里**先于**各节点任务 `run()`（订阅先于 run 是 Ch4.8 硬纪律）；无 dyn 连接恒为
///   `None`，`start()` 不跑 broker（「无动态子图即恒等」的直接落点）。
///
/// 注意方向：对外输入的 `Sender` 由用户持有、对应的 `Receiver` 接到某节点的输入端口；
/// 对外输出反过来——节点的输出端口持有 `Sender`、对应的 `Receiver` 交给用户。
// ANCHOR: main_graph_struct
pub struct MainGraph {
    actors: Vec<Box<dyn Actor>>,
    node_names: Vec<String>,
    inputs: HashMap<String, Sender>,
    outputs: HashMap<String, Receiver>,
    resources: ResourceCollection,
    /// 动态子图的通知 broker（Ch4.9b）：无 dyn 连接时为 `None`（恒等）。见结构体文档。
    broker: Option<Broker>,
}
// ANCHOR_END: main_graph_struct

// `Box<dyn Actor>` 不实现 `Debug`，无法 `#[derive]`；手写一个「摘要式」Debug——打印节点
// 数量与对外端口名即可（测试里 `build().unwrap_err()` 要求 `Ok` 侧类型实现 `Debug`）。
// `Box<dyn Actor>` isn't `Debug`; hand-roll a summary impl (actor count + port names).
impl std::fmt::Debug for MainGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainGraph")
            .field("actors", &self.actors.len())
            .field("inputs", &self.input_names())
            .field("outputs", &self.output_names())
            .finish()
    }
}

impl MainGraph {
    /// 把一份已解析的 `Config` 装配成主图：取 `main` 指向的子图 → 造共享资源 → 交给
    /// `assemble_graph` 完成接线。见模块级文档的「名字↔位置的桥」。
    ///
    /// 装配核心抽成了 `assemble_graph`（吃**单个** `GraphConfig` + 一份**注入的**
    /// `ResourceCollection`），好让**运行期动态子图实例**（Ch4.9 `dyn_ports`）复用同一套
    /// 接线逻辑——那条路径不 `build_resources`、改把外层资源注入进来。主图这条路径则先
    /// `build_resources(g)` 按 `g.resources` 现造，再 `assemble_graph(g, rc)`。
    // ANCHOR: assemble
    fn assemble(config: &Config) -> Result<MainGraph> {
        let g = config
            .main_graph()
            .ok_or_else(|| Error::MainGraphNotFound(config.main.clone()))?;
        let resources = Self::build_resources(g)?;
        // `flatten`（Ch4.4）已把静态子图内联、只把**动态**子图引用保留成惰性 `GraphConfig`
        // 跟在扁平主图之后（Ch4.9b）。这里据此分流：`config.graphs` 里除 `main` 外剩下的，就是
        // 那些被保留的动态子图定义。没有 → 老路径 `assemble_graph`（逐字节恒等，前几章测试的护栏）；
        // 有 → 走 `assemble_dynamic` 自动接线。
        let subgraphs: HashMap<&str, &GraphConfig> = config
            .graphs
            .iter()
            .filter(|sg| sg.name != g.name)
            .map(|sg| (sg.name.as_str(), sg))
            .collect();
        if subgraphs.is_empty() {
            return Self::assemble_graph(g, resources);
        }
        Self::assemble_dynamic(g, &subgraphs, resources)
    }
    // ANCHOR_END: assemble

    /// 按 `g.resources` 造这张图的共享资源集（Ch4.3）：逐条按 `ty` 查资源注册表、
    /// `(ctor)(args)` 造一份类型擦除的 `Arc<dyn Any+..>`，按 `name` 收进 `ResourceCollection`。
    /// 查不到该资源类型 → `UnknownResourceType`（与节点的 `UnknownNodeType` 对偶）。
    /// 「构造一次、多处共享」的「一次」就在这里；分发到各节点的「多处」落在 `start()`。
    /// 动态子图实例路径（Ch4.9）**跳过**这步、改用运行期注入的集合。
    // ANCHOR: build_resources
    fn build_resources(g: &GraphConfig) -> Result<ResourceCollection> {
        let mut res_map: HashMap<String, AnyResource> = HashMap::new();
        for rc in &g.resources {
            let reg = registry::find_resource(&rc.ty)
                .ok_or_else(|| Error::UnknownResourceType(rc.ty.clone()))?;
            res_map.insert(rc.name.clone(), (reg.ctor)(&rc.args)?);
        }
        Ok(ResourceCollection::from_map(res_map))
    }
    // ANCHOR_END: build_resources

    /// 装配核心：把**一个** `GraphConfig` + 一份**注入的** `ResourceCollection` 装成 `MainGraph`。
    ///
    /// 从 `assemble` 抽出来的复用点，两条路径共用同一套接线逻辑：
    /// - **主图**路径（`assemble`）：`resources` 来自 `build_resources(g)`——按 `g.resources` 现造。
    /// - **动态子图实例**路径（Ch4.9 `dyn_ports`）：`resources` 是运行期**注入**的集合，触发
    ///   节点在 `create(key, resources)` 里穿进来，于是子图实例与外层图共享同一份资源。
    ///
    /// 三趟接线（对外输入 / 对外输出 / 图内连接）+ 造节点 + 收集与 `actors` 同序的
    /// `node_names` 都在这里。`type_infer::infer(g)` 作用于**单个** `GraphConfig`，故子图能
    /// 独立推断边界与内部连接的类型——这正是它能被拿来现装成实例的前提。
    // ANCHOR: assemble_graph
    pub(crate) fn assemble_graph(
        g: &GraphConfig,
        resources: ResourceCollection,
    ) -> Result<MainGraph> {
        let inference = crate::config::type_infer::infer(g)?;
        let mut inferred = inference.connections.iter().copied();

        // 每个节点各端口的 channel 端，按名收集（Ch4.2：一个端口名 → 一组 channel 端——
        // 标量端口是恰 1 个的组、数组端口是 N 个的组）；对外句柄单独收。
        let mut node_ins: HashMap<String, HashMap<String, Vec<TaggedEndpoint<Receiver>>>> =
            HashMap::new();
        let mut node_outs: HashMap<String, HashMap<String, Vec<TaggedEndpoint<Sender>>>> =
            HashMap::new();
        let mut inputs: HashMap<String, Sender> = HashMap::new();
        let mut outputs: HashMap<String, Receiver> = HashMap::new();

        // 同一对外输入对应一条队列，多个目标竞争接收；广播另用 Bcast。
        for pc in &g.inputs {
            if pc.ports.is_empty() {
                return Err(Error::BadConnection(format!(
                    "graph input {:?} has no target",
                    pc.name
                )));
            }
            let ty = inferred.next().expect("input connection type");
            let (tx, rx) = channel_with_type(pc.cap, ty);
            inputs.insert(pc.name.clone(), tx);
            for port in &pc.ports {
                let pref = PortRef::parse(port)?;
                let reg = node_reg(g, pref.node)?;
                let mut endpoint = rx.clone();
                endpoint.with_type(&inference.port_type(pref.node, pref.port, true));
                attach_receiver(&mut node_ins, reg, pref.node, pref.port, endpoint, pref.tag)?;
            }
        }

        // 对外**输出**：channel 的 Receiver 归用户，Sender 接到源节点的输出端口。
        // 多个源端口汇到同一对外输出（扇入）是 mpsc 天生支持的——clone Sender 即可。
        for pc in &g.outputs {
            if pc.ports.is_empty() {
                return Err(Error::BadConnection(format!(
                    "graph output {:?} has no source",
                    pc.name
                )));
            }
            let ty = inferred.next().expect("output connection type");
            let (tx, rx) = channel_with_type(pc.cap, ty);
            outputs.insert(pc.name.clone(), rx);
            for pref_str in &pc.ports {
                let pref = PortRef::parse(pref_str)?;
                let reg = node_reg(g, pref.node)?;
                let mut endpoint = tx.clone();
                endpoint.with_type(&inference.port_type(pref.node, pref.port, false));
                attach_sender(
                    &mut node_outs,
                    reg,
                    pref.node,
                    pref.port,
                    endpoint,
                    pref.tag,
                )?;
            }
        }

        // 图**内部**连接（Ch4.1）：一条连接 = 一条 channel，方向由端点的端口角色推断。
        // 指向某节点**输出**端口的引用是发送端、指向**输入**端口的是接收端（判据就是
        // Ch3.2 注册表里的 `inputs`/`outputs` 端口名表）。多个接收端竞争同一队列；
        // 若每个消费者都要收到一份，应通过 bcast 接不同队列。
        for conn in &g.connections {
            // 先把这条连接上的每个端口引用按「角色」分成发送端 / 接收端两拨。
            let mut senders: Vec<PortRef> = Vec::new();
            let mut receivers: Vec<PortRef> = Vec::new();
            for pref_str in &conn.ports {
                let pref = PortRef::parse(pref_str)?;
                let nd = g
                    .nodes
                    .iter()
                    .find(|n| n.name == pref.node)
                    .ok_or_else(|| Error::UnknownNode(pref.node.to_owned()))?;
                let reg =
                    registry::find(&nd.ty).ok_or_else(|| Error::UnknownNodeType(nd.ty.clone()))?;
                // 端口名表是 `&'static [&'static str]`，直接用 `contains` 比对端口名即可。
                if reg.outputs.contains(&pref.port) {
                    senders.push(pref);
                } else if reg.inputs.contains(&pref.port) {
                    receivers.push(pref);
                } else {
                    return Err(Error::UnknownPort {
                        node: pref.node.to_owned(),
                        port: pref.port.to_owned(),
                    });
                }
            }
            // 共享队列：至少一个接收端和一个发送端，多个下游竞争接收。
            if receivers.is_empty() || senders.is_empty() {
                return Err(Error::BadConnection(format!(
                    "connection {:?} has {} receiver(s) and {} sender(s); \
                     need ≥1 receiver (input port) and ≥1 sender (output port)",
                    conn.ports,
                    receivers.len(),
                    senders.len()
                )));
            }

            let ty = inferred.next().expect("internal connection type");
            let (tx, rx) = channel_with_type(conn.cap, ty);
            // 接收端：把这条 channel 的 rx 挂到该输入端口。标量端口重复接 → PortAlreadyConnected；
            // 数组输入端口（Merge 扇入）允许多条连接各挂一个 Receiver，攒成一组。
            for rcv in &receivers {
                let rcv_reg = node_reg(g, rcv.node)?;
                let mut endpoint = rx.clone();
                endpoint.with_type(&inference.port_type(rcv.node, rcv.port, true));
                attach_receiver(
                    &mut node_ins,
                    rcv_reg,
                    rcv.node,
                    rcv.port,
                    endpoint,
                    rcv.tag,
                )?;
            }
            drop(rx);
            // 发送端：每个输出端口挂一份 tx.clone()（同一条连接多个发送端即经典扇入）。
            // 标量端口跨连接重复接 → PortAlreadyConnected；数组输出端口（Bcast 扇出）允许
            // 多条连接各挂一个 Sender，攒成一组。
            for snd in &senders {
                let snd_reg = node_reg(g, snd.node)?;
                let mut endpoint = tx.clone();
                endpoint.with_type(&inference.port_type(snd.node, snd.port, false));
                attach_sender(
                    &mut node_outs,
                    snd_reg,
                    snd.node,
                    snd.port,
                    endpoint,
                    snd.tag,
                )?;
            }
        }

        // 逐节点：find 构造器 → 按注册表端口名表把命名 channel 排成位置 Vec → 造节点。
        let mut actors: Vec<Box<dyn Actor>> = Vec::with_capacity(g.nodes.len());
        for nd in &g.nodes {
            let reg =
                registry::find(&nd.ty).ok_or_else(|| Error::UnknownNodeType(nd.ty.clone()))?;

            let mut ins_map = node_ins.remove(&nd.name).unwrap_or_default();
            let mut outs_map = node_outs.remove(&nd.name).unwrap_or_default();

            // 按声明顺序把命名端口组排成位置分组 Vec——顺序即注册表 INPUTS/OUTPUTS，
            // 与构造器填字段同序。未接线标量补默认端点，数组端口保留空组。
            // 原版 conn_check 对断开端口仅警告，默认 Receiver 关闭、Sender 丢弃并成功。
            let mut ins: Vec<Vec<TaggedEndpoint<Receiver>>> = Vec::with_capacity(reg.inputs.len());
            for &port in reg.inputs {
                // 动态输入端口（Ch4.9b：`DynPorts<Receiver>`）**不**从静态连接消费端点——它的
                // 端点在运行期由 `set_port_dynamic` 注入。跳过它，与 `build` 里 dyn 字段走
                // `Default::default()`（不消费 ins/outs）严格对齐；否则会给 dyn 字段错配一组端点。
                // 既有节点 `INPUT_DYN` 为空表 → `input_is_dyn` 恒 false → 不跳，逐字节恒等。
                if reg.input_is_dyn(port) {
                    continue;
                }
                let mut group = ins_map.remove(port).unwrap_or_default();
                if group.is_empty() && !reg.input_is_array(port) && !reg.input_is_dict(port) {
                    group.push(Default::default());
                }
                ins.push(group);
            }
            let mut outs: Vec<Vec<TaggedEndpoint<Sender>>> = Vec::with_capacity(reg.outputs.len());
            for &port in reg.outputs {
                // 动态输出端口（`DynPorts<Sender>`）同理跳过——端点运行期注入，不占位置。
                if reg.output_is_dyn(port) {
                    continue;
                }
                let mut group = outs_map.remove(port).unwrap_or_default();
                if group.is_empty() && !reg.output_is_array(port) && !reg.output_is_dict(port) {
                    group.push(Default::default());
                }
                outs.push(group);
            }
            // 消费完注册表声明的端口后还有剩 = 配置接了节点类型上不存在的端口。
            if let Some((port, _)) = ins_map.into_iter().next() {
                return Err(Error::UnknownPort {
                    node: nd.name.clone(),
                    port,
                });
            }
            if let Some((port, _)) = outs_map.into_iter().next() {
                return Err(Error::UnknownPort {
                    node: nd.name.clone(),
                    port,
                });
            }

            actors.push((reg.tagged_ctor)(&nd.args, ins, outs)?);
        }

        // 与 `actors` 同序的节点名（`actors` 就是按 `g.nodes` 顺序 push 的）——`start()` 靠
        // 这层同序把「名字」zip 回「节点」，为每个节点造带名字的 `Context`。
        let node_names: Vec<String> = g.nodes.iter().map(|n| n.name.clone()).collect();

        // `resources` 由调用方传入：主图路径是 `build_resources(g)` 现造的、动态子图实例路径
        // 是运行期注入的同一份（`ResourceCollection` 是 `Arc` 共享的廉价克隆）。
        // `assemble_graph` 只装**单张**图（主图恒等路径 or 运行期动态实例本身），不含 dyn 连接
        // 的自动接线，故 `broker` 恒 `None`；带 dyn 连接的自动接线走 `assemble_dynamic`。
        Ok(MainGraph {
            actors,
            node_names,
            inputs,
            outputs,
            resources,
            broker: None,
        })
    }
    // ANCHOR_END: assemble_graph

    /// 自动接线**动态子图**（Ch4.9b）。`flatten`（Ch4.4）把静态子图内联了，但把**动态**子图
    /// 引用保留成了惰性 `GraphConfig`（`subgraphs`）——主图里那个 `ty` 是子图名的节点，就是一个
    /// **动态实例位点**。本方法在装配期把「TOML 里的 dyn 连接」翻译成「运行期 broker 接线」：
    ///
    /// 1. 扫主图连接，认出每条 **dyn 连接**（一端是某注册节点的 `dyn` 端口、另一端引用一个动态
    ///    子图位点）——收成一张 [`DynWiring`] 清单，并记下这些连接的下标；
    /// 2. 造一张**过滤图**（剔掉动态子图位点节点 + 剔掉 dyn 连接），交 `assemble_graph` 把触发
    ///    节点等**真实节点**照常装配——触发节点的 dyn 端口本就在构造循环里被跳过（Edits E/F）；
    /// 3. 建一个 `Broker`，为每条 dyn 连接 `subscribe`（**topic = 动态位点节点名**，故同一位点的
    ///    feed/collect 共用一个 topic，触发方一次 `create` 广播的 `DynConns` 两端都 `fetch` 得到），
    ///    构造 `DynPortsConfig` 经 `set_port_dynamic` 注入触发节点（**节点构造之后**）；
    /// 4. 把 broker 交给 `MainGraph`——`start()` 里**先于**各节点任务 `run()`（订阅先于 run）。
    ///
    /// 对齐原版 `graph/mod.rs` 数 `dyn_rxn`/`dyn_txn` + 建 `DynPortsConfig` 那段。恒等性：`flatten`
    /// 无 dyn 连接时不产生任何保留子图，`assemble` 根本不会走到这条路径（前几章测试的护栏）。
    // ANCHOR: assemble_dynamic
    fn assemble_dynamic(
        g: &GraphConfig,
        subgraphs: &HashMap<&str, &GraphConfig>,
        resources: ResourceCollection,
    ) -> Result<MainGraph> {
        // 1) 扫连接：认 dyn 连接、收 DynWiring、记下其下标（过滤图要剔掉它们）。
        let mut wirings: Vec<DynWiring> = Vec::new();
        let mut dyn_conn_idx: HashSet<usize> = HashSet::new();
        for (idx, conn) in g.connections.iter().enumerate() {
            if let Some(w) = DynWiring::from_conn(g, subgraphs, conn)? {
                dyn_conn_idx.insert(idx);
                wirings.push(w);
            }
        }
        if wirings.is_empty() {
            // 有保留子图却没认出任何 dyn 连接 = 本子集不支持的形态（例如子图没被 dyn 端口引用）。
            return Err(Error::Unsupported(
                "dynamic subgraph present but no dyn connection found".to_owned(),
            ));
        }

        // 2) 过滤图：剔掉动态子图位点节点（`ty` 是子图名、非注册类型）+ 剔掉 dyn 连接；其余照旧。
        //    触发节点、它的**非** dyn 端口、对外边界都原样保留，交给恒等的 `assemble_graph`。
        let filtered = GraphConfig {
            name: g.name.clone(),
            nodes: g
                .nodes
                .iter()
                .filter(|n| !subgraphs.contains_key(n.ty.as_str()))
                .cloned()
                .collect(),
            inputs: g.inputs.clone(),
            outputs: g.outputs.clone(),
            connections: g
                .connections
                .iter()
                .enumerate()
                .filter(|(i, _)| !dyn_conn_idx.contains(i))
                .map(|(_, c)| c.clone())
                .collect(),
            resources: g.resources.clone(),
        };
        let mut mg = Self::assemble_graph(&filtered, resources)?;

        // 3) 建 broker、按 topic 订阅、把 DynPortsConfig 注入触发节点（构造后）。
        let mut broker = Broker::new();
        for w in wirings {
            let client = broker.subscribe(w.topic.clone());
            let cfg = DynPortsConfig {
                target: w.target,
                cap: w.cap,
                broker: client,
                graph_config: w.graph_config,
            };
            // `port_info.name` 必须等于触发节点上的 dyn 端口字段名——派生宏生成的
            // `set_port_dynamic` 按它 match 到对应字段、把 cfg `push` 进去（见 Ch4.9a）。
            let port_info = PortInfo {
                name: w.port.clone(),
                ty: PortType::Dyn,
                mty: MsgType::any(),
            };
            // 先算下标（借用 `node_names`），再可变借用 `actors`——两者是不同字段、借用不重叠。
            let idx = mg
                .node_names
                .iter()
                .position(|n| n == &w.node)
                .ok_or_else(|| Error::UnknownNode(w.node.clone()))?;
            mg.actors[idx].set_port_dynamic(&port_info, cfg);
        }

        // 4) broker 交给图：`start()` 会先于各节点任务 `run()` 它。
        mg.broker = Some(broker);
        Ok(mg)
    }
    // ANCHOR_END: assemble_dynamic

    /// 取一个对外输入的发送端（`Sender` 可 `Clone`，返回一份克隆）。找不到 → `None`。
    /// Clone of an external input's sender.
    pub fn input(&self, name: &str) -> Option<Sender> {
        self.inputs.get(name).cloned()
    }

    /// 取走并移除图持有的输出接收端。调用者可以克隆接收端，克隆之间竞争消息。
    /// Remove and return the graph's retained receiver.
    pub fn take_output(&mut self, name: &str) -> Option<Receiver> {
        self.outputs.remove(name)
    }

    /// 所有对外输入端口名。/ external input names.
    pub fn input_names(&self) -> Vec<&str> {
        self.inputs.keys().map(String::as_str).collect()
    }

    /// 所有对外输出端口名。/ external output names.
    pub fn output_names(&self) -> Vec<&str> {
        self.outputs.keys().map(String::as_str).collect()
    }

    /// 取走装配好的节点，交给调用方 spawn。Ch3.3 的 [`start`](Self::start) 就建在
    /// 这个接缝之上；Ch3.2 的集成测试也用它手动跑起来、端到端验证接线。
    /// Take the assembled actors out to be spawned; `start()` wraps this.
    pub fn take_actors(&mut self) -> Vec<Box<dyn Actor>> {
        std::mem::take(&mut self.actors)
    }

    /// 启动整张图：把装配好的每个节点 spawn 成一个 tokio 任务，返回一个覆盖全图的
    /// **聚合句柄**——它在**所有**节点任务收尾后才 resolve。
    ///
    /// 相比 Ch3.2 让调用方 `take_actors()` 再逐个 `actor.start()`、逐个收 `JoinHandle`，
    /// 这里把「spawn 全部 + join 全部」封成一次调用、收敛成一个句柄。错误怎么抬：
    /// - 某节点 `exec` 返回 `Err` → 它的任务以该 `Err` 收尾 → 经内层 `?` 原样抬出；
    /// - 某节点任务 panic 或被取消 → `await` 得 `JoinError` → 经外层 `?` 抬成
    ///   [`Error::TaskJoin`]。
    ///
    /// Ch4.3：每个节点 `start` 时收一份 [`Context`]——把它自己的实例名 + 图的共享资源集
    /// （`self.resources` 的廉价 `Arc` 克隆）带进去，节点在 `initialize(&ctx)` 里按名借出
    /// 资源。`node_names` 与 `take_actors()` 同序，`zip` 即对齐；`resources` 只克隆
    /// 不搬走（`self` 留着那份），故图跑完后仍能 [`resource`](Self::resource) 读回同一实例。
    ///
    /// 逐个顺序 `await` 是安全的：停机是「关闭涟漪」——上游任务一收尾就 drop 掉它到
    /// 下游的 `Sender`，下游随之 `recv` 到 `ChannelClosed` 而退出，故先 await 谁都不会
    /// 卡住另一个。配套的优雅停机见 [`stop`](Self::stop)。
    ///
    /// Spawn every assembled actor; return one aggregate handle that resolves
    /// after all node tasks finish. Each actor gets a `Context` (its name + the
    /// shared resources). A node `Err` or a task panic propagates out.
    pub fn start(&mut self) -> JoinHandle<Result<()>> {
        let resources = self.resources.clone();
        let names = std::mem::take(&mut self.node_names);
        // 动态子图（Ch4.9b）：若装配期识别出 dyn 连接、建了 broker，就**先**把它 `run` 起来。
        // 订阅早在 `assemble_dynamic` 接线时就为每个 dyn 端点做完了（**订阅先于 run** 是 Ch4.8
        // 硬纪律），这里 `run()` 用 `mem::take` 快照订阅、spawn 出每 topic 的 fan-out 任务。
        // 无 dyn 连接时 `broker` 为 `None`、这步是空操作——`start()` 与前几章逐字节等价。
        let broker_handle = self.broker.take().map(|mut b| b.run());
        let handles: Vec<_> = self
            .take_actors()
            .into_iter()
            .zip(names)
            .map(|(actor, name)| actor.start(Context::new(name, resources.clone())))
            .collect();
        tokio::spawn(async move {
            for handle in handles {
                // 外层 `?`：任务 panic/取消 → JoinError 抬成 TaskJoin。
                // 内层 `?`：节点自己返回的 Err 原样抬出。
                handle.await.map_err(|e| Error::TaskJoin(e.to_string()))??;
            }
            // 节点任务都收尾了 → 它们 dyn 字段里持有的 `BrokerClient` 随之 drop → 每个 topic 的
            // fan-out 任务收到「全部发布端关闭」而结束 → broker 句柄可解析。放到节点之后 await
            // **不**死锁：所有任务开头就已并发 spawn，await 顺序只决定谁先被观测、不阻塞彼此推进
            // （详见 Ch4.8 的 broker 生命周期分析）。
            if let Some(bh) = broker_handle {
                bh.await.map_err(|e| Error::TaskJoin(e.to_string()))??;
            }
            Ok(())
        })
    }

    /// 按名字 + 类型借出图的一个共享资源（Ch4.3）：类型不符或查无此名 → `None`。
    ///
    /// 与节点在 `Context` 里拿到的是**同一个** `Arc<T>`（`ResourceCollection` 是 `Arc`
    /// 共享的）。这让调用方/测试能在图外读回资源的运行时状态——例如图跑完后读一个共享
    /// 计数器，验证「多个节点确实共用了同一份实例」而非各造各的。
    /// Borrow one shared resource by name + type; `None` on type/name mismatch.
    pub fn resource<T: std::any::Any + Send + Sync>(
        &self,
        name: &str,
    ) -> Option<std::sync::Arc<T>> {
        self.resources.get(name)
    }

    /// 停机：丢掉图自己持有的对外输入 `Sender`（本方法**消费 `self`**，顺带把尚未被
    /// [`take_output`](Self::take_output) 取走的 `Receiver` 一并释放）。
    ///
    /// 配合调用方丢掉它从 [`input`](Self::input) 克隆的那些 `Sender`，一条对外输入
    /// channel 的发送端就全部消失，接在它上面的节点 `recv` 到 `ChannelClosed`、优雅
    /// 退出，关闭涟漪顺着图一路传导，最终 [`start`](Self::start) 的聚合句柄 resolve。
    ///
    /// 效果等价于直接 `drop(graph)`，但给它一个名字，把「停机 = 撤掉所有对外输入」
    /// 这层意图讲明白——对齐原版 `stop(self)` 的语义。
    ///
    /// Drop the graph's retained input senders (consumes `self`), triggering the
    /// graceful-shutdown ripple once the caller also drops its cloned senders.
    pub fn stop(self) {
        // self 在此 drop：inputs 里的 Sender、outputs 里未取走的 Receiver 一并释放。
    }
}

// ANCHOR: dyn_wiring
/// 从一条主图连接里解析出的一条**动态接线**（Ch4.9b）：把某注册节点的一个 `dyn` 端口接到
/// 一个动态子图位点的边界端口上。`assemble_dynamic` 据此 `subscribe` broker + 注入 `DynPortsConfig`。
struct DynWiring {
    /// 持有 dyn 端口的**触发节点**实例名（`set_port_dynamic` 的注入目标）。
    node: String,
    /// 该节点上的 dyn 端口字段名（= `port_info.name`，派生宏据此把 cfg push 进对应字段）。
    port: String,
    /// broker topic = **动态子图位点节点名**。同一位点的多条 dyn 连接（feed/collect）共用它，
    /// 故触发方一次 `create` 广播的 `DynConns`，feed（抽入口）与 collect（抽出口）两端都收得到。
    topic: String,
    /// 要从 `DynConns` 抽出的**边界端口名**：dyn 输出抽 `inputs[target]`、dyn 输入抽 `outputs[target]`。
    target: String,
    /// 边界通道容量（取这条连接的 `cap`）。
    cap: usize,
    /// 目标子图定义（惰性构造器，`create` 时 `assemble_graph` 它）。
    graph_config: GraphConfig,
}

impl DynWiring {
    /// 判定一条连接是否 **dyn 连接**并解析。遍历端口引用认两端：「注册节点的 dyn 端口」与
    /// 「动态子图位点引用」。
    ///
    /// - 两端都不 dyn（普通连接）→ `Ok(None)`，交回 `assemble_graph` 照常建 channel。
    /// - 两端齐备 → `Ok(Some(DynWiring))`，并按**反直觉规则**（Ch4.9a）校验边界端口归属：dyn
    ///   **输出**端口（`DynPorts<Sender>`）抽实例**入口**、边界须在 `sub.inputs`；dyn **输入**端口
    ///   （`DynPorts<Receiver>`）抽实例**出口**、边界须在 `sub.outputs`。
    /// - 只有一端（dyn 端口没接子图位点 / 子图位点没接 dyn 端口）或双 dyn → `Err`（对齐原版
    ///   `graph/mod.rs` 的 dyn 连接校验：一条连接恰一个 dyn 端口 + 一个位点）。
    fn from_conn(
        g: &GraphConfig,
        subgraphs: &HashMap<&str, &GraphConfig>,
        conn: &crate::config::ConnConfig,
    ) -> Result<Option<Self>> {
        // 「注册节点的 dyn 端口」端：(节点名, 端口名, 是否输出 dyn)。
        let mut dyn_endpoint: Option<(String, String, bool)> = None;
        // 「动态子图位点」端：(位点节点名, 边界端口名, 子图定义)。
        let mut site_endpoint: Option<(String, String, &GraphConfig)> = None;

        for pref_str in &conn.ports {
            let pref = PortRef::parse(pref_str)?;
            let nd = g
                .nodes
                .iter()
                .find(|n| n.name == pref.node)
                .ok_or_else(|| Error::UnknownNode(pref.node.to_owned()))?;
            if let Some(sub) = subgraphs.get(nd.ty.as_str()) {
                if site_endpoint.is_some() {
                    return Err(Error::BadConnection(format!(
                        "dyn connection {:?} references more than one dynamic subgraph site",
                        conn.ports
                    )));
                }
                site_endpoint = Some((pref.node.to_owned(), pref.port.to_owned(), *sub));
            } else if let Some(reg) = registry::find(&nd.ty) {
                let is_out = reg.output_is_dyn(pref.port);
                let is_in = reg.input_is_dyn(pref.port);
                if is_out || is_in {
                    if dyn_endpoint.is_some() {
                        return Err(Error::BadConnection(format!(
                            "dyn connection {:?} has more than one dyn port",
                            conn.ports
                        )));
                    }
                    dyn_endpoint = Some((pref.node.to_owned(), pref.port.to_owned(), is_out));
                }
            }
        }

        match (dyn_endpoint, site_endpoint) {
            // 普通连接：无 dyn 端口、也没引用动态子图位点 → 交回 assemble_graph 照常处理。
            (None, None) => Ok(None),
            // dyn 端口 + 子图位点齐备 → 一条 dyn 连接。
            (Some((node, port, is_out)), Some((site, boundary, sub))) => {
                let in_inputs = sub.inputs.iter().any(|p| p.name == boundary);
                let in_outputs = sub.outputs.iter().any(|p| p.name == boundary);
                // 反直觉但正确（Ch4.9a）：dyn 输出→抽入口（须在 sub.inputs）；dyn 输入→抽出口（sub.outputs）。
                if (is_out && !in_inputs) || (!is_out && !in_outputs) {
                    return Err(Error::UnknownPort {
                        node: site,
                        port: boundary,
                    });
                }
                Ok(Some(DynWiring {
                    node,
                    port,
                    topic: site,
                    target: boundary,
                    cap: conn.cap,
                    graph_config: sub.clone(),
                }))
            }
            // 只有一端 dyn：形态非法。
            (Some(_), None) => Err(Error::BadConnection(format!(
                "dyn port in connection {:?} is not wired to a dynamic subgraph site",
                conn.ports
            ))),
            (None, Some(_)) => Err(Error::BadConnection(format!(
                "dynamic subgraph site in connection {:?} is not wired to a dyn port",
                conn.ports
            ))),
        }
    }
}
// ANCHOR_END: dyn_wiring

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::Counter;
    use crate::config::Config;
    use crate::envelope::Envelope;
    use crate::resource::{AnyResource, BuildResource};
    use std::sync::Arc;

    // 一个只有 `Tally` 的子图：inp→out 透传，并在名为 "counter" 的共享资源上按消息 `bump()`。
    // 它**自己不声明** `resources`——资源由 `assemble_graph` 的调用方注入（对齐 Ch4.9 动态子图实例）。
    const SUB: &str = r#"
main="sub"
[[graphs]]
name="sub"
nodes=[{name="t",ty="Tally",res="counter"}]
inputs=[{name="inp",cap=1,ports=["t:inp"]}]
outputs=[{name="out",cap=1,ports=["t:out"]}]
"#;

    // 守住 Ch4.9 重构接缝：`assemble_graph(g, injected)` 把注入的 `ResourceCollection` 一路穿到
    // 每个节点的 `Context`——子图节点据此借出共享资源。这是动态子图实例路径的资源注入证据；
    // 主图路径（`build_resources` 现造资源）的回归由 `tests/graph_builder.rs` 等既有测试覆盖。
    #[tokio::test]
    async fn assemble_graph_injects_resources() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            // 预建一个共享 Counter，按名 "counter" 注入（子图配置里没有 resources 段）。
            let counter: Arc<Counter> = Arc::new(Counter::build(&Default::default()).unwrap());
            let mut map: HashMap<String, AnyResource> = HashMap::new();
            map.insert("counter".to_owned(), counter.clone());
            let injected = ResourceCollection::from_map(map);

            let g = Config::from_toml(SUB).unwrap().main_graph().unwrap().clone();
            let mut mg = MainGraph::assemble_graph(&g, injected).unwrap();
            let input = mg.input("inp").unwrap();
            let output = mg.take_output("out").unwrap();
            let task = mg.start();

            // 发两条：Tally 每消息 bump 一次，注入的 counter 应读回 2——证明注入集合确实穿到了节点。
            input.send(Envelope::new(1u32)).await.unwrap();
            output.recv::<u32>().await.unwrap();
            input.send(Envelope::new(2u32)).await.unwrap();
            output.recv::<u32>().await.unwrap();

            drop(input);
            mg.stop();
            task.await.unwrap().unwrap();
            assert_eq!(counter.get(), 2);
        })
        .await
        .unwrap();
    }
}
