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
//! 的端口没接线——全部在 `build()` 当场 `Err`，而非等运行时 panic。
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

use crate::channel::{channel, Receiver, Sender};
use crate::config::{Config, PortRef};
use crate::error::{Error, Result};
use crate::node::Actor;
use crate::registry;
use std::collections::HashMap;
use tokio::task::JoinHandle;

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

    /// 解析 + 装配：`Config::from_toml` → [`MainGraph::assemble`]。
    /// 解析失败、跨引用校验失败都在这里返回 `Err`。
    /// Parse then assemble; parse and cross-reference errors surface here.
    pub fn build(self) -> Result<MainGraph> {
        let text = self
            .template
            .ok_or_else(|| Error::Unsupported("builder has no template".to_owned()))?;
        let config = Config::from_toml(&text)?;
        MainGraph::assemble(&config)
    }
}

/// 装配好的主图：一组待运行的节点 + 对外输入/输出句柄。
///
/// - `actors`：造好、接好线的节点，等着被 spawn（Ch3.3 的 `start()` 会接手）。
/// - `inputs`：对外**输入**句柄 `名字 → Sender`——用户 `input(name)` 拿到它往图里发。
/// - `outputs`：对外**输出**句柄 `名字 → Receiver`——用户 `take_output(name)` 拿到它从图里收。
///
/// 注意方向：对外输入的 `Sender` 由用户持有、对应的 `Receiver` 接到某节点的输入端口；
/// 对外输出反过来——节点的输出端口持有 `Sender`、对应的 `Receiver` 交给用户。
pub struct MainGraph {
    actors: Vec<Box<dyn Actor>>,
    inputs: HashMap<String, Sender>,
    outputs: HashMap<String, Receiver>,
}

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
    /// 把一份已解析的 `Config` 装配成主图。见模块级文档的「名字↔位置的桥」。
    fn assemble(config: &Config) -> Result<MainGraph> {
        let g = config
            .main_graph()
            .ok_or_else(|| Error::MainGraphNotFound(config.main.clone()))?;

        // 每个节点各端口的 channel 端，按名收集；对外句柄单独收。
        let mut node_ins: HashMap<String, HashMap<String, Receiver>> = HashMap::new();
        let mut node_outs: HashMap<String, HashMap<String, Sender>> = HashMap::new();
        let mut inputs: HashMap<String, Sender> = HashMap::new();
        let mut outputs: HashMap<String, Receiver> = HashMap::new();

        let node_exists = |n: &str| g.nodes.iter().any(|nd| nd.name == n);

        // 对外**输入**：channel 的 Sender 归用户，Receiver 接到目标节点的输入端口。
        // 一条 channel 只有一个 Receiver，故一个对外输入只能接一个目标端口；扇出（一份
        // 输入喂多个端口）需要广播，留到 Ch4.1 的 bcast。
        for pc in &g.inputs {
            if pc.ports.len() != 1 {
                return Err(Error::Unsupported(format!(
                    "graph input {:?} feeds {} node ports; fan-out (broadcast) lands in Ch4.1",
                    pc.name,
                    pc.ports.len()
                )));
            }
            let pref = PortRef::parse(&pc.ports[0])?;
            if !node_exists(pref.node) {
                return Err(Error::UnknownNode(pref.node.to_owned()));
            }
            let (tx, rx) = channel(pc.cap);
            inputs.insert(pc.name.clone(), tx);
            node_ins
                .entry(pref.node.to_owned())
                .or_default()
                .insert(pref.port.to_owned(), rx);
        }

        // 对外**输出**：channel 的 Receiver 归用户，Sender 接到源节点的输出端口。
        // 多个源端口汇到同一对外输出（扇入）是 mpsc 天生支持的——clone Sender 即可。
        for pc in &g.outputs {
            let (tx, rx) = channel(pc.cap);
            outputs.insert(pc.name.clone(), rx);
            for pref_str in &pc.ports {
                let pref = PortRef::parse(pref_str)?;
                if !node_exists(pref.node) {
                    return Err(Error::UnknownNode(pref.node.to_owned()));
                }
                node_outs
                    .entry(pref.node.to_owned())
                    .or_default()
                    .insert(pref.port.to_owned(), tx.clone());
            }
        }

        // 逐节点：find 构造器 → 按注册表端口名表把命名 channel 排成位置 Vec → 造节点。
        let mut actors: Vec<Box<dyn Actor>> = Vec::with_capacity(g.nodes.len());
        for nd in &g.nodes {
            let reg =
                registry::find(&nd.ty).ok_or_else(|| Error::UnknownNodeType(nd.ty.clone()))?;

            let mut ins_map = node_ins.remove(&nd.name).unwrap_or_default();
            let mut outs_map = node_outs.remove(&nd.name).unwrap_or_default();

            // 按声明顺序取端口——顺序即注册表里的 INPUTS/OUTPUTS，与构造器填字段同序。
            let mut ins = Vec::with_capacity(reg.inputs.len());
            for &port in reg.inputs {
                let rx = ins_map
                    .remove(port)
                    .ok_or_else(|| Error::PortNotConnected {
                        node: nd.name.clone(),
                        port: port.to_owned(),
                    })?;
                ins.push(rx);
            }
            let mut outs = Vec::with_capacity(reg.outputs.len());
            for &port in reg.outputs {
                let tx = outs_map
                    .remove(port)
                    .ok_or_else(|| Error::PortNotConnected {
                        node: nd.name.clone(),
                        port: port.to_owned(),
                    })?;
                outs.push(tx);
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

            actors.push((reg.ctor)(&nd.args, ins, outs)?);
        }

        Ok(MainGraph {
            actors,
            inputs,
            outputs,
        })
    }

    /// 取一个对外输入的发送端（`Sender` 可 `Clone`，返回一份克隆）。找不到 → `None`。
    /// Clone of an external input's sender.
    pub fn input(&self, name: &str) -> Option<Sender> {
        self.inputs.get(name).cloned()
    }

    /// 取走一个对外输出的接收端（`Receiver` 单消费者，只能 move 出来）。找不到 → `None`。
    /// Move out an external output's receiver (single-consumer).
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
    /// 逐个顺序 `await` 是安全的：停机是「关闭涟漪」——上游任务一收尾就 drop 掉它到
    /// 下游的 `Sender`，下游随之 `recv` 到 `ChannelClosed` 而退出，故先 await 谁都不会
    /// 卡住另一个。配套的优雅停机见 [`stop`](Self::stop)。
    ///
    /// Spawn every assembled actor; return one aggregate handle that resolves
    /// after all node tasks finish. A node `Err` or a task panic propagates out.
    pub fn start(&mut self) -> JoinHandle<Result<()>> {
        let handles: Vec<_> = self
            .take_actors()
            .into_iter()
            .map(|actor| actor.start())
            .collect();
        tokio::spawn(async move {
            for handle in handles {
                // 外层 `?`：任务 panic/取消 → JoinError 抬成 TaskJoin。
                // 内层 `?`：节点自己返回的 Err 原样抬出。
                handle.await.map_err(|e| Error::TaskJoin(e.to_string()))??;
            }
            Ok(())
        })
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
