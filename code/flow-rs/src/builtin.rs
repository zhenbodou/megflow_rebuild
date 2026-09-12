//! flow-rs · builtin —— 随引擎发货的内置节点（Ch3.4 起）。
//!
//! 前几章把定义节点的「工具」逐件造齐了：Part 2 的过程宏（`#[inputs]`/`#[outputs]`/
//! `#[methods]`/`#[derive(Node, Actor, BuildFromPorts)]`/`node_register!`）、Ch2.4 的编译期
//! 注册表、Ch3.1~3.3 的配置 → 装配 → 调度。本模块用这整套工具造出**第一个真正的内置节点**
//! `BinaryOp`——它与下游用户写的节点**长得一模一样**，唯一区别是它住在引擎 crate 里、
//! 随 crate 一起发货。**Ch4.1 又添了两个类型无关节点** `Transform`（1 入 1 出原样透传）与
//! `NoopConsumer`（只吸收不产出的汇）——它们走未类型化的 `recv_any`/`send_any`，搬运封箱
//! 消息而不拆封，与钉死了 `i32` 的 `BinaryOp` 形成鲜明对照。
//!
//! 「住在 crate 内部」带来一个前几章没遇到的坎：`node_register!` 生成的注册代码全用**绝对
//! 路径** `flow_rs::inventory::submit!` / `flow_rs::registry::NodeRegistration`（见 flow-derive）。
//! 这些路径在**下游** crate 里天然成立（`flow_rs` 就是依赖名），但在**本 crate 内部**，
//! `flow_rs` 默认并不指向自己。解法是在 `lib.rs` 加一行 `extern crate self as flow_rs;`——
//! 给自己起个别名。加上它之后，连下面这些 `use flow_rs::...` 都能照抄下游用户的写法。
//!
//! **Ch4.3 再添一对 `Counter`（共享资源）+ `Tally`（用它的节点）**：演示「构造一次、`Arc`
//! 共享给多个节点」的资源机制，以及节点如何用 `#[state]` 字段 + `initialize(&ctx)` 拿到句柄。
//!
//! The first built-in node shipped with the engine. `extern crate self as flow_rs`
//! (in lib.rs) makes the macro-generated `flow_rs::` paths resolve inside the crate.
//! Ch4.3 adds `Counter` (a shared resource) + `Tally` (a node that borrows it).

use flow_derive::{
    inputs, methods, node_register, outputs, resource_register, Actor, BuildFromPorts, Node,
};
use flow_rs::channel::{Receiver, Sender};
use flow_rs::context::Context;
use flow_rs::error::{Error, Result};
use flow_rs::node::{Actor, Node};
use flow_rs::registry::BuildFromPorts;
use flow_rs::resource::{BuildResource, ResourceCollection};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;

/// 二元整数运算节点：从输入端口 `a`、`b` 各取一个 `i32`，按参数 `op` 运算，结果发往
/// 输出端口 `c`。`op` 支持 `"+"` / `"-"` / `"*"` / `"/"`；其余值 → `Err(Error::Arg)`。
///
/// 这正是 Ch0.3 验收契约里那张图用的节点类型（TOML 里 `ty="BinaryOp"`）。它刻意做得极小：
/// 全书第一个「真能跑」的节点，重点是打通「配置 → 装配 → 调度 → 计算 → 出结果」这条链，
/// 而非运算本身的丰富度（更多算子、浮点、多操作数留给读者练习）。
///
/// A tiny built-in: reads one `i32` from each of `a`/`b`, applies `op`, sends to `c`.
// ANCHOR: binary_op
#[inputs(a, b)]
#[outputs(c)]
#[derive(Node, Actor, BuildFromPorts)]
pub struct BinaryOp {
    /// 运算符，从节点参数 `op="+"` 反序列化而来（`#[derive(BuildFromPorts)]` 负责填充）。
    op: String,
}

#[methods]
impl BinaryOp {
    async fn exec(&mut self) -> Result<()> {
        // 各收一个操作数。任一输入关闭 → `recv` 返回 `ChannelClosed`，`#[methods]` 生成的
        // 包装会把它转成「置关闭标志 + Ok」，故这里直接 `?` 即可，无需手写关闭处理。
        let mut ea = self.a.recv::<i32>().await?;
        let mut eb = self.b.recv::<i32>().await?;
        let (x, y) = (ea.unpack(), eb.unpack());
        let r = match self.op.as_str() {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            "/" => x / y,
            other => {
                return Err(Error::Arg {
                    key: "op".into(),
                    msg: format!("未知运算符 {other:?}"),
                })
            }
        };
        // `c` 是 `Option<Sender>`——`close()` 会把它置 `None`。仍在时才发。
        if let Some(out) = self.c.as_ref() {
            out.send(ea.repack(r)).await?;
        }
        Ok(())
    }
}

node_register!("BinaryOp", BinaryOp);
// ANCHOR_END: binary_op

/// 类型无关直通节点：从输入端口 `inp` 收一条消息，原样转发到输出端口 `out`。
///
/// 与 `BinaryOp` 的关键对照：`BinaryOp` 用 `recv::<i32>()` 把消息类型**钉死在节点里**，
/// 而 `Transform` 走**未类型化**的 `recv_any`/`send_any`——它搬运的是**已封箱**的
/// `SealedEnvelope`，全程不拆封、不关心里面装的是 `i32` 还是 `String`。于是同一个
/// `Transform` 能插进任意一条边做「原样透传」（占位、解耦、调试探针都用得上），类型由
/// 上下游决定、与它无关。这正是 Ch1.3「封箱 + downcast」那层设计的兑现场景。
///
/// A type-agnostic passthrough: `recv_any` one sealed envelope, `send_any` it on unchanged.
// ANCHOR: transform
#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
pub struct Transform {}

#[methods]
impl Transform {
    async fn exec(&mut self) -> Result<()> {
        // 收一条封箱消息。输入关闭 → `recv_any` 返回 `ChannelClosed`，`#[methods]` 包装
        // 把它转成「置关闭标志 + Ok」，故这里 `?` 即可，退出循环、走优雅停机。
        let msg = self.inp.recv_any().await?;
        // `out` 是 `Option<Sender>`——`close()` 会把它置 `None`。仍在时才转发。
        if let Some(out) = self.out.as_ref() {
            out.send_any(msg).await?;
        }
        Ok(())
    }
}

node_register!("Transform", Transform);
// ANCHOR_END: transform

/// 汇（sink）节点：只有输入端口 `inp`、没有输出。把收到的每条消息**吸收丢弃**，输入耗尽
/// 后干净收工——用来**终止**一条数据流分支（下游不再需要结果，但仍需有人把消息取走、
/// 让上游的关闭涟漪能正常传导）。
///
/// 同样走 `recv_any`：它连消息类型都不用知道，收下即弃。注意这里**没有** `#[outputs]`——
/// 一个合法的零输出节点，`close()` 无端口可撤，`recv_any` 一旦 `ChannelClosed` 即终止。
///
/// A sink: drains and discards every message via `recv_any`; no outputs.
// ANCHOR: noop_consumer
#[inputs(inp)]
#[outputs]
#[derive(Node, Actor, BuildFromPorts)]
pub struct NoopConsumer {}

#[methods]
impl NoopConsumer {
    async fn exec(&mut self) -> Result<()> {
        // 收一条即弃（不绑定、直接 drop）。输入关闭 → `?` 抛 `ChannelClosed` → 收工。
        self.inp.recv_any().await?;
        Ok(())
    }
}

node_register!("NoopConsumer", NoopConsumer);
// ANCHOR_END: noop_consumer

/// 广播节点（扇出）：从输入端口 `inp` 收一条消息，**复制**给**数组输出端口** `out` 上挂着的
/// 每一个下游。`#[outputs(out[])]` 把 `out` 声明成数组端口（字段类型 `Vec<Sender>`）——图里
/// 可以把它接到任意多条边上，装配期每条边往这个组里塞一个 `Sender`（见 Graph Builder 的
/// `attach_sender` 与「数组端口允许多接」）。
///
/// 这解释了 Ch1.4 为何把 channel 钉成 **mpsc（单消费者）**、又为何 Ch4.2 要给 `SealedEnvelope`
/// 补上「类型擦除的 `Clone`」：一条 channel 喂不了多个消费者，广播只能靠**在节点里复制消息、
/// 分别发往多条独立 channel** 来实现——`Bcast` 正是这件事的落点。原版把广播糅进 channel 层
/// （`bcast`），重写版把它上提成一个**普通节点**：channel 保持极简，扇出是节点的职责。
///
/// 发送策略用 `split_last`：对「除最后一个之外」的下游发 `msg.clone()`（类型擦除克隆），
/// 最后一个直接把 `msg` **搬**过去——省掉一次多余的克隆。`.ok()` **吞掉发送错误**：某个下游
/// 关了不该拖垮整场广播，其余下游照发；等自己的输入 `inp` 关闭时才随 `?` 收工。
///
/// A broadcast (fan-out) node: clones one input message to every sender in its array
/// output port. Clone-to-all-but-last, move-into-last; send errors are swallowed.
// ANCHOR: bcast
#[inputs(inp: T0)]
#[outputs(out: [T0])]
#[derive(Node, Actor, BuildFromPorts)]
pub struct Bcast {}

#[methods]
impl Bcast {
    async fn exec(&mut self) -> Result<()> {
        // 收一条封箱消息。输入关闭 → `recv_any` 返回 `ChannelClosed`，`?` 交给包装收工。
        let msg = self.inp.recv_any().await?;
        // 数组输出端口 `out: Vec<Sender>`。split_last：前 n-1 个发克隆、最后一个搬原件。
        if let Some((last, rest)) = self.out.split_last() {
            for out in rest {
                // 类型擦除克隆（Ch4.2 给 SealedEnvelope 补的 Clone）。某路关了就跳过（.ok()）。
                out.send_any(msg.clone()).await.ok();
            }
            last.send_any(msg).await.ok();
        }
        // out 为空组（图里没接任何下游）→ 消息直接 drop，退化成一个 drain。
        Ok(())
    }
}

node_register!("Bcast", Bcast);
// ANCHOR_END: bcast

/// 汇聚节点（扇入）：从**数组输入端口** `inps` 上挂着的多个上游里，**谁先来收谁**，把消息转发到
/// 输出端口 `out`。`#[inputs(inps[])]` 把 `inps` 声明成数组端口（字段类型 `Vec<Receiver>`）——
/// 图里可以把多条边接到它，装配期每条边往组里塞一个 `Receiver`（见 `attach_receiver`）。
///
/// 与 `Bcast` 对偶，但**扇入本可以不要节点**——mpsc 本就多生产者，多个上游 clone 同一个
/// `Sender` 发往一条 channel 即可（Ch4.1 的经典扇入就是这么做的，无需 `Merge`）。`Merge` 的
/// 存在价值是另一种扇入语义：上游各自持有**独立** channel（互不背压、可分别关闭），由本节点
/// **按 future 顺序尝试接收**它们（不保证公平）——这正是 `Vec<Receiver>` 而非「一条共享 channel」的意义。
///
/// 实现用 `futures_util::future::select_ok`：它并发 race 一组 future，返回**第一个成功**的结果，
/// 且**跳过**先返回 `Err` 的（某路已关闭 → `ChannelClosed` 是 `Err`，会被跳过，继续等其余路），
/// 直到某路拿到消息（`Ok`）、或**所有路都 `Err`**（全部上游关闭）才整体返回 `Err`。这与
/// `select_all`（返回第一个**完成**的、不分成败）截然不同——用 `select_all` 会把「某路关闭」
/// 误当成有消息。全部关闭时 `select_ok` 返回的 `Err(ChannelClosed)` 经 `?` 交给 `#[methods]`
/// 包装 → 置关闭标志 → 收工。
///
/// `select_ok` 拿到结果后返回 `(msg, 其余未完成的 future)`；我们用 `let (msg, _) = ..` 立即
/// **丢弃**那些未完成的 future。`tokio` 的 `recv` 是**可取消的**（cancel-safe）：丢弃一个尚未
/// 就绪的 `recv` future 不会吞掉任何消息，下一轮 `exec` 会为每路重新发起 `recv`。
///
/// A merge (fan-in) node: races independent input receivers via `select_ok`, forwarding
/// the first ready message; skips closed receivers until one is ready or all are closed.
// ANCHOR: merge
#[inputs(inps: [T0])]
#[outputs(out: T0)]
#[derive(Node, Actor, BuildFromPorts)]
pub struct Merge {}

#[methods]
impl Merge {
    async fn exec(&mut self) -> Result<()> {
        // 没接任何上游（空组）→ 无可轮询，直接以 ChannelClosed 收工（交给包装置标志）。
        if self.inps.is_empty() {
            return Err(Error::ChannelClosed);
        }
        // 为每路发起一个 recv_any future，pin 后交给 select_ok 并发 race。
        let futs: Vec<_> = self
            .inps
            .iter_mut()
            .map(|r| Box::pin(r.recv_any()))
            .collect();
        // 谁先拿到消息用谁；先 Err（某路关闭）的被跳过；全 Err → 整体 Err(ChannelClosed)。
        // `_` 立即丢弃其余未完成的 future（recv 可取消，不丢消息）。
        let (msg, _) = futures_util::future::select_ok(futs).await?;
        // `out` 是 Option<Sender>——close() 会把它置 None。仍在时才转发。
        if let Some(out) = self.out.as_ref() {
            out.send_any(msg).await?;
        }
        Ok(())
    }
}

node_register!("Merge", Merge);
// ANCHOR_END: merge

// ── Ch4.3：共享资源 `Counter` + 用它的节点 `Tally` ──────────────────────────────
// 到这里为止，节点的所有字段要么是端口、要么是「从 args 反序列化的自有参数」——每个节点
// 实例各造各的。但真实算法仓里有一类东西**必须多个节点共用同一份**：一个几百 MB 的检测
// 模型、一块预分配的内存池。给每个流各造一份既费内存又费加载时间。Ch4.3 的答案是**资源**：
// 装配期**构造一次**，通过 `Arc` 把同一份**共享**给声明要用它的每个节点。
//
// 下面用一个最小、可断言的资源 `Counter`（一个原子计数器）替身来演示这套机制——把它换成
// 「模型」或「内存池」，代码骨架一字不变。

/// 一个最小的共享资源：线程安全的原子计数器。多个节点共用**同一个** `Counter`，各自 `bump()`
/// 累加到同一个数上——这正是「共享模型 / 内存池」的可断言替身（`get()` 能在图外读回总数）。
///
/// 为什么字段是 `AtomicU64` 而非 `u64`？因为资源是通过 `Arc<Counter>`（**共享引用**，非独占）
/// 被多个并发节点持有的——拿不到 `&mut`，只能用**内部可变性**。原子类型让「读引用也能改值」
/// 且无需锁，是共享计数最省的选择。
///
/// A minimal shared resource: a thread-safe atomic counter (stand-in for a shared model/pool).
pub struct Counter {
    count: AtomicU64,
}

impl Counter {
    /// 计数 +1，返回自增**后**的值。`Relaxed`：只要计数最终正确、不与其他内存操作定序，
    /// 单个计数器用最宽松的内存序即可。
    /// Increment by one, return the new value.
    pub fn bump(&self) -> u64 {
        self.count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 读当前计数。/ read the current count.
    pub fn get(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

/// 让 `Counter` 成为可注册资源：`build` 从配置参数造一份初值。
///
/// 对照 Ch1.3 的关键教学点：那里为了让**封箱消息**能被类型擦除地 `clone`，我们**自定义**了
/// 一个 `AnyEnvelope` trait（带 `clone_box`）——因为标准库的 `Any` 给不了「克隆」这种行为。
/// 而资源**不需要任何自定义行为**：装配期造一次、之后只读地共享，标准库的
/// `Arc<dyn Any + Send + Sync>::downcast` 就够了（见 `resource.rs`）。所以 `BuildResource`
/// 只有一个「怎么造」的 `build`，没有 `clone_box` 之类——**需求决定抽象**，这份对照本身就是本章的一课。
///
/// Make `Counter` a registrable resource; only `build` is needed — no custom vtable
/// (contrast Ch1.3's `AnyEnvelope`, which needed `clone_box`).
impl BuildResource for Counter {
    fn build(_args: &flow_rs::config::Args) -> Result<Self> {
        // 这个替身不读任何参数；真实资源会在这里读 `args`（模型路径、池容量……）。
        Ok(Counter {
            count: AtomicU64::new(0),
        })
    }
}

// 把 "Counter" 这个类型名登进**资源注册表**（与 `node_register!` 对偶）。TOML 的
// `[[graphs]].resources` 里写 `ty="Counter"` 就能造出它。
resource_register!("Counter", Counter);

/// 计数转发节点：把从 `inp` 收到的每条消息原样转发到 `out`，**顺带**在共享的 `Counter` 上 `bump()`。
///
/// 它示范一个节点**怎么拿到并使用共享资源**，全套只有三个动作：
/// 1. 用一个**自有参数** `res: String` 记下「我要用的资源叫什么名字」（TOML 里 `res="counter"`）；
/// 2. 用一个 `#[state]` 字段 `counter: Option<Arc<Counter>>` 存放**运行期**才拿到的资源句柄——
///    `#[state]` 告诉 `#[derive(BuildFromPorts)]`：这个字段**不**从 args 反序列化，而是
///    `Default::default()`（即 `None`）初始化，留到运行期填；
/// 3. 在 `initialize(&ctx)` 里按名 `ctx.resource::<Counter>(&self.res)` 借出、存进那个字段。
///
/// 关键设计：资源只穿过 `initialize`，**没有**渗进 `exec`——Ch3.4 那条 `exec(&mut self)` 签名
/// 原封不动。多个 `Tally` 实例（`res` 都填 `"counter"`）会拿到**同一个** `Arc<Counter>`，`bump()`
/// 累加到同一个数上：这就是「共享」。若资源不存在（如沙箱里），`counter` 保持 `None`，节点
/// **优雅降级**为纯转发——`if let Some(c) = ..` 正是为此。
///
/// A tally-and-forward node: borrows a shared `Counter` at `initialize`, bumps it per message.
// ANCHOR: tally
#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
pub struct Tally {
    /// 自有参数：要借用的资源名（TOML 里 `res="counter"`）。由 `#[derive(BuildFromPorts)]`
    /// 从 args 填充。
    res: String,
    /// 运行期资源句柄：`#[state]` → 不从 args 来，`Default::default()`（`None`）初始化，
    /// 在 `initialize` 里按 `res` 名从 `Context` 借出后填入。
    #[state]
    counter: Option<Arc<Counter>>,
}

#[methods]
impl Tally {
    async fn initialize(&mut self, ctx: &Context) {
        // 按名 + 类型借出共享资源。借到 → Some(Arc<Counter>)，多个节点借到的是同一个；
        // 名字错/图里没这个资源 → None，节点降级为纯转发。
        self.counter = ctx.resource::<Counter>(&self.res);
    }

    async fn exec(&mut self) -> Result<()> {
        // 收一条封箱消息。输入关闭 → `?` 交给 `#[methods]` 包装收工。
        let msg = self.inp.recv_any().await?;
        // 有资源就 bump（先记账，再转发——e2e 测试据此在收满 N 条后断言总数恰为 N）。
        if let Some(c) = self.counter.as_ref() {
            c.bump();
        }
        // `out` 是 Option<Sender>——close() 会把它置 None。仍在时才转发。
        if let Some(out) = self.out.as_ref() {
            out.send_any(msg).await?;
        }
        Ok(())
    }
}

node_register!("Tally", Tally);
// ANCHOR_END: tally

// ANCHOR: reorder
/// 按 partial_id 从 0 开始逐个转发。未来 ID 的重复消息替换缓存；旧 ID 和缺口关闭 panic。
/// 与原版 node/reorder.rs 的常规消息语义对齐；通道 flush 协议仍需单独实现。
#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
pub struct Reorder {
    #[state]
    cache: std::collections::BTreeMap<u64, flow_message::SealedEnvelope>,
    #[state]
    seq_id: u64,
}

#[methods]
impl Reorder {
    async fn exec(&mut self) -> Result<()> {
        let message = match self.inp.recv_any().await {
            Ok(message) => message,
            Err(Error::ChannelClosed) => {
                assert!(self.cache.is_empty(), "reorder closed with a sequence gap");
                return Err(Error::ChannelClosed);
            }
            Err(error) => return Err(error),
        };
        let id = message
            .info()
            .partial_id
            .expect("partial_id required by reorder");
        assert!(
            id >= self.seq_id,
            "reorder received an already emitted sequence id"
        );
        // 原版对未来重复 ID 使用 insert：保留最后一封，而不是报错或广播两次。
        self.cache.insert(id, message);
        // 只取当前缺少的那个序号。队列里虽有更大的 ID，也必须等待缺口补齐。
        while let Some(message) = self.cache.remove(&self.seq_id) {
            self.seq_id += 1;
            if let Some(out) = self.out.as_ref() {
                // 原版忽略已关闭下游的发送错误，仍消费输入并推进序号。
                out.send_any(message).await.ok();
            }
        }
        Ok(())
    }
}

node_register!("Reorder", Reorder);
// ANCHOR_END: reorder

// ANCHOR: static_demux
/// 静态地址分流：保留原版 inp:T0 / out:{T0} 的模板关联。
#[inputs(inp: T0)]
#[outputs(out: {T0})]
#[derive(Default, Node, Actor, BuildFromPorts)]
pub struct Demux {}
#[methods]
impl Demux {
    async fn exec(&mut self) -> Result<()> {
        let message = self.inp.recv_any().await?;
        let address = message
            .info()
            .to_addr
            .expect("the envelope has no destination address");
        if let Some(output) = self.out.get(&address) {
            output.send_any(message).await.ok();
        }
        Ok(())
    }
}
node_register!("Demux", Demux);
// ANCHOR_END: static_demux

// ── Ch4.9c：动态地址分流的**注册版** `DynDemux`（内置节点）───────────────────────
// Ch4.9（机制层）造出运行期 create→publish→fetch→route 环路、Ch4.9a 让 `dyn` 端口语法长出
// `DynPorts` 字段 + `set_port_dynamic`、Ch4.9b 让 config 建图期**自动接线**动态子图——但触发方
// 一直是**测试夹具**（Ch4.9 的 example、Ch4.9b 的 `AutoTrigger`）。本节把那条环路封装成一个
// **注册内置节点**：真实 TOML 里写 `ty="DynDemux"` 就能用，由 `Builder::build` 的自动接线驱动。
// 对齐原版 `node/demux.rs` 的 `DynDemux`——单个 `dyn` 输出、按 `to_addr` 把每条流分到「按 key
// 现装一次」的**汇子图实例**里，是原版「按视频流条数现装实例」的忠实重写。

/// 动态地址分流（注册版）：从 `inp` 收无类型消息，按信封 `to_addr` 把它路由到一份**按需现装、
/// 每 key 只建一次**的动态子图实例。`out` 是**动态输出**（`#[outputs(out: dyn T0)]`，持
/// `DynPorts<Sender>`）——建图期由 config 自动接线（[Ch4.9b](ch09b-config-auto-wiring.md)）指向某个
/// 动态子图位点，运行期 `create` 出实例、`fetch` 回入口 `Sender`、`send_any` 灌进去。
///
/// 是**喂入侧单向**的（对齐原版 `node/demux.rs`）：只有一个 dyn 输出、只往实例入口喂，不从实例
/// 收结果。实例是**汇子图**（末端落进资源，无对外出口），这与 Ch4.9b `AutoTrigger` 夹具的
/// feed+collect 双向不同——原版 `logical_test.toml` 的 `destination` 子图正是这种「只收不还」的汇。
///
/// - **有载荷信封**：`is_cached` 没命中就 `create`（把实例 `JoinHandle` 收进 `tasks`），再
///   `fetch_with_cache` 取回该 key 的入口 `Sender`、把消息灌进实例。`initialize` 时从 `Context`
///   取的 `resources` 随 `create` 一路穿进子图实例——于是实例与外层图**共享同一份资源集**
///   （Ch4.9 记过的「注入不链接」：`ext_resource.chain` 的链接语义本弧仍 defer）。
/// - **空信封**（`is_none`）：拆除信号——`evict` 撤掉该 key 的入口端点（实例失去唯一外部
///   `Sender` → 优雅停机）、`await` 它的 `JoinHandle`。
/// - **`finalize`**：输入关闭后，把残留实例逐一 `evict` 再 `await`，确保没有实例挂在后台
///   （比原版「只 await」更稳——不必依赖每个 key 都收到过拆除信号；对齐 Ch4.9b `AutoTrigger`）。
///
/// 与静态 `Demux`（上面，`out: {T0}` 字典端口、路由到**建图期就接好**的固定下游）的对照就是
/// 这条弧的主题：`DynDemux` 的下游是**运行期现装**的子图实例，一条流一份、可创建可拆除。
///
/// A registered feed-only dynamic demux: routes by `to_addr` into per-key, created-once
/// dynamic subgraph instances; injects its Context resources into each `create`.
// ANCHOR: dyn_demux
#[inputs(inp: T0)]
#[outputs(out: dyn T0)]
#[derive(Node, Actor, BuildFromPorts)]
pub struct DynDemux {
    /// 每个 key 的实例任务句柄：`create` 现装时收进来，拆除 / `finalize` 时 `await` 它收尾。
    #[state]
    tasks: HashMap<u64, JoinHandle<Result<()>>>,
    /// 注入给实例的共享资源集：`initialize` 从 `Context` 取一份，`create` 时穿进子图实例。
    #[state]
    resources: Option<ResourceCollection>,
}

#[methods]
impl DynDemux {
    async fn initialize(&mut self, ctx: &Context) {
        // 取外层图的共享资源集（`ResourceCollection` 是 `Arc` 共享的廉价克隆）。
        self.resources = Some(ctx.resources.clone());
    }

    async fn exec(&mut self) -> Result<()> {
        let message = self.inp.recv_any().await?;
        let key = message
            .info()
            .to_addr
            .expect("the envelope has no destination address");

        // 空信封 = 拆除：撤入口端点（实例失去唯一外部 Sender → 停机），再 await 其任务收尾。
        if message.is_none() {
            if let Some(handle) = self.tasks.remove(&key) {
                self.out.evict(key);
                handle.await.ok();
            }
            return Ok(());
        }

        // 有载荷：每 key 只 create 一次（is_cached 把门），把注入的 resources 穿进实例。
        if !self.out.is_cached(key) {
            let resources = self.resources.clone().unwrap_or_default();
            let handle = self.out.create(key, resources).await?;
            self.tasks.insert(key, handle);
        }
        // fetch 回入口 Sender、灌数据。send 用 `.ok()`：别把「实例通道已关」误判成「我的输入关」。
        self.out
            .fetch_with_cache(key)
            .await?
            .send_any(message)
            .await
            .ok();
        Ok(())
    }

    async fn finalize(&mut self) {
        // 收尾：撤掉所有残留实例的入口端点、再 await 各任务，确保没有实例挂在后台。
        let keys: Vec<u64> = self.tasks.keys().copied().collect();
        for key in keys {
            self.out.evict(key);
        }
        for (_, handle) in self.tasks.drain() {
            handle.await.ok();
        }
    }
}
node_register!("DynDemux", DynDemux);
// ANCHOR_END: dyn_demux
