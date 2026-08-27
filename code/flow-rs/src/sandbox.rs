//! flow-rs · sandbox —— 单节点测试沙箱（Ch3.4）。
//!
//! 想单独验一个节点「喂这些、该吐那些」，走完整 `Builder` 得写一整段 TOML（`main`、
//! `graphs`、`inputs`、`outputs`、端口引用……）——为测一个节点铺一张图，太重。`Sandbox`
//! 把这件事收敛成三步：**按类型名建一个节点**、**给输入端口喂数据**、**在输出端口收结果**。
//!
//! 它是原版 `flow-rs/src/sandbox.rs` 的**教学子集**：原版还带 Broker、`ChannelStorage`、
//! 动态端口、资源注入等一整套；这里只保留最小内核——够把单节点跑起来、验证行为即可。
//! 实现上它直接站在 Ch2.4 注册表 + Ch1.4 channel + Ch2.1 `Actor::start` 之上，不经过图装配。
//!
//! ```ignore
//! let args: Args = toml::from_str(r#"op = "+""#).unwrap();
//! let out = Arc::new(Mutex::new(Vec::new()));
//! let sink = out.clone();
//! let mut sb = Sandbox::with_args("BinaryOp", args)?;
//! sb.add_data("a", vec![1i32])
//!   .add_data("b", vec![2i32])
//!   .add_check("c", move |v: i32| sink.lock().unwrap().push(v));
//! sb.start().await?;          // 跑到所有输入耗尽、节点收工
//! assert_eq!(*out.lock().unwrap(), vec![3]);
//! ```
//!
//! A single-node test harness: build by type name, feed inputs, check outputs.
//! A teaching subset of the original `Sandbox` (no broker / dynamic ports).

use crate::channel::{channel, Receiver, Sender};
use crate::config::Args;
use crate::context::Context;
use crate::error::{Error, Result};
use crate::node::Actor;
use crate::registry;
use flow_message::Envelope;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// 沙箱内每条 channel 的容量。取一个小定值即可：喂数任务与节点任务并发跑，
/// 缓冲满了触发背压、节点一取就腾出，不会死锁——容量只影响批量喂数时的并发深度。
const SANDBOX_CAP: usize = 16;

/// 喂数 / 收数任务的类型擦除句柄：一串「跑到自然结束」的 future。
/// `add_data`/`add_check` 是泛型（按端口消息类型 `T` 单态化），把各自的 future 装箱后
/// 统一存在这里，`start` 再把它们一并 spawn。
type Sub = Pin<Box<dyn Future<Output = ()> + Send>>;

/// 单节点测试沙箱。见模块级文档。
pub struct Sandbox {
    /// 待运行的节点（`start` 时 `take` 出来 spawn）。
    actor: Option<Box<dyn Actor>>,
    /// 未被 `add_data` 认领的输入端口 `名字 → Sender`（沙箱侧的发送端）。
    inputs: HashMap<String, Sender>,
    /// 未被 `add_check` 认领的输出端口 `名字 → Receiver`（沙箱侧的接收端）。
    outputs: HashMap<String, Receiver>,
    /// 已登记的喂数 / 收数任务。
    subs: Vec<Sub>,
}

impl Sandbox {
    /// 按类型名建一个**无参数**节点（等价于 `with_args(ty, Args::new())`）。
    /// Build a no-arg node by type name.
    pub fn pure(ty: &str) -> Result<Self> {
        Self::with_args(ty, Args::new())
    }

    /// 按类型名 + 参数表建一个节点：从注册表 `find` 出构造器，为它声明的**每个**输入/输出
    /// 端口各开一条 channel，节点侧的端交给构造器、沙箱侧的端留在 `inputs`/`outputs` 里备用。
    ///
    /// 类型名查不到 → `Error::UnknownNodeType`；参数缺失/类型错 → 构造器返回的 `Error::Arg`。
    /// Build a node by type name + args; wire a fresh channel per declared port.
    pub fn with_args(ty: &str, args: Args) -> Result<Self> {
        let reg = registry::find(ty).ok_or_else(|| Error::UnknownNodeType(ty.to_owned()))?;

        // 输入端口：channel 的 Receiver 给节点，Sender 留给沙箱（供 add_data 喂数）。
        // Ch4.2：构造器要「分组」端口（`Vec<Vec<_>>`）；沙箱为每个声明端口只开一条 channel，
        // 故每个端口都是**恰好 1 个的组**（`vec![rx]`）——数组端口在这里退化成 1 路，真正的
        // N 路扇出/扇入靠 graph 端到端测试覆盖。
        let mut inputs = HashMap::new();
        let mut ins: Vec<Vec<Receiver>> = Vec::with_capacity(reg.inputs.len());
        for &port in reg.inputs {
            let (tx, rx) = channel(SANDBOX_CAP);
            inputs.insert(port.to_owned(), tx);
            ins.push(vec![rx]);
        }
        // 输出端口：channel 的 Sender 给节点，Receiver 留给沙箱（供 add_check 收数）。
        let mut outputs = HashMap::new();
        let mut outs: Vec<Vec<Sender>> = Vec::with_capacity(reg.outputs.len());
        for &port in reg.outputs {
            let (tx, rx) = channel(SANDBOX_CAP);
            outputs.insert(port.to_owned(), rx);
            outs.push(vec![tx]);
        }

        // 端口按注册表的名表顺序排成位置 Vec，交给构造器（与 Graph Builder 同一套接线逻辑）。
        let actor = (reg.ctor)(&args, ins, outs)?;
        Ok(Sandbox {
            actor: Some(actor),
            inputs,
            outputs,
            subs: Vec::new(),
        })
    }

    /// 给输入端口 `port` 登记一串待喂数据：`start` 时逐个发进去，发完 drop 掉发送端——
    /// 该输入 channel 随之关闭，节点据此判定「这路输入到头了」。
    ///
    /// **把发送端从 `inputs` 里移走**是关键：沙箱不再留一份，故喂完即彻底关闭，不会出现
    /// 「喂数任务放手了、沙箱却还攥着一份 Sender、channel 迟迟不关」的挂起。端口不存在 → panic
    /// （测试里写错端口名应尽早炸出来）。
    /// Register data for an input port; the sender is moved out and dropped after feeding.
    pub fn add_data<T: Send + 'static + Clone>(&mut self, port: &str, items: Vec<T>) -> &mut Self {
        let tx = self
            .inputs
            .remove(port)
            .unwrap_or_else(|| panic!("sandbox: 节点无此输入端口 {port:?}"));
        self.subs.push(Box::pin(async move {
            for item in items {
                if tx.send(Envelope::new(item)).await.is_err() {
                    break; // 下游已关闭，停止喂数
                }
            }
            // tx 在此 drop：该输入 channel 关闭。
        }));
        self
    }

    /// 给输出端口 `port` 登记一个校验闭包：`start` 时每收到一条消息就调一次 `check`，直到
    /// 该输出关闭（节点收工时 drop 掉输出 Sender）。端口不存在 → panic。
    /// Register a checker for an output port; invoked per received message.
    pub fn add_check<T, F>(&mut self, port: &str, mut check: F) -> &mut Self
    where
        T: Send + 'static,
        F: FnMut(T) + Send + 'static,
    {
        let mut rx = self
            .outputs
            .remove(port)
            .unwrap_or_else(|| panic!("sandbox: 节点无此输出端口 {port:?}"));
        self.subs.push(Box::pin(async move {
            while let Ok(mut env) = rx.recv::<T>().await {
                check(env.unpack());
            }
        }));
        self
    }

    /// 跑起来：spawn 节点 + 全部喂数/收数任务，等它们统统收尾，返回节点的运行结果。
    ///
    /// 收尾链条：喂数任务发完 → drop 掉输入 Sender → 节点 `recv` 到 `ChannelClosed` → 退出
    /// exec 循环 → `close()` drop 掉输出 Sender → 收数任务 `recv` 到关闭 → 收尾。任一节点
    /// 业务错误（如未知 `op`）经节点任务原样带出，成为本方法的 `Err`。
    ///
    /// 消费 `self`：沙箱一次性用完。未被 `add_data`/`add_check` 认领的端口在这里一并释放
    /// （未喂的输入 → channel 立即关闭，节点不干等它；未收的输出 → Receiver 关闭）。
    /// Spawn the node and all feeders/checkers; await completion; return the node's result.
    pub async fn start(mut self) -> Result<()> {
        let actor = self.actor.take().expect("sandbox: start 只能调用一次");
        let subs = std::mem::take(&mut self.subs);
        // 未认领的端口现在就释放——务必在 await 节点**之前**，否则沙箱攥着的 Sender 会让
        // 未喂的输入永不关闭，节点干等到天荒地老。
        self.inputs.clear();
        self.outputs.clear();

        // 沙箱不注入任何共享资源：给节点一个空 `Context`（Ch4.3）。于是「依赖某资源」的
        // 节点在沙箱里会拿到 `None`、优雅降级（如 Tally 少了计数器就只转发不计数）——单节点
        // 测试聚焦端口行为，共享资源的真正验证留给 graph 端到端测试。
        let node = actor.start(Context::anonymous());
        let sub_handles: Vec<_> = subs.into_iter().map(tokio::spawn).collect();
        for handle in sub_handles {
            handle.await.map_err(|e| Error::TaskJoin(e.to_string()))?;
        }
        // 两层 Result（同 Ch3.3 聚合句柄）：外层任务崩没崩、内层节点逻辑成没成。
        node.await.map_err(|e| Error::TaskJoin(e.to_string()))?
    }
}
