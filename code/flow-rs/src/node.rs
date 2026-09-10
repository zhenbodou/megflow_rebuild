//! flow-rs · node —— 节点接口：`Node` / `Actor` 双 trait（重写版）。
//!
//! 一个**节点（node）**就是 actor 模型里的一个 actor：它被 spawn 成一个独立的
//! 异步任务，从输入端口反复 `recv`、处理、往输出端口 `send`，直到上游关闭。
//! 我们把它拆成**两个** trait，各管一件事——这正是原版的划分：
//!
//! - [`Node`]：与**图装配**打交道。绑定端口、判断输入是否关闭、主动关闭输出。
//!   （原版还带 stats/anchor/动态端口，属 profile 与子图话题——本重写按需后置。）
//! - [`Actor`]：与**调度器**打交道。`start` 把节点交给 tokio 跑成一个任务。
//!
//! 关键设计（也是「学 Rust」的两个点）：
//! 1. `Actor::start` **不是** `async fn`，返回一个 [`JoinHandle`]——于是它**对象安全**，
//!    可以装进 `Box<dyn Actor>`（图里存的就是一堆 `Box<dyn Actor>`）。
//! 2. 节点的 `exec`/`initialize`/`finalize` 是**固有方法（inherent）**，**不进 trait**。
//!    这样就绕开了「async fn in trait 不完全对象安全」的坑：需要 async 的部分留在
//!    固有方法里，`start` 内部直接调它们；trait 只暴露非 async 的 `start`。
//!
//! Two traits: `Node` (talks to graph assembly) and `Actor` (talks to the
//! scheduler). `start` is non-async → object-safe → `Box<dyn Actor>`. The
//! async `exec`/lifecycle methods stay as inherent methods, off the trait.

use crate::context::Context;
use crate::error::Result;
use tokio::task::JoinHandle;

// ANCHOR: node_traits
/// 节点与图装配交互的接口：判断输入是否关闭、主动关闭输出。
///
/// 端口的**动态绑定**（按配置的 `PortInfo` 把 channel 塞进节点字段）需要配置层，
/// 留到 Part 3 的 Graph Builder；本章节点的端口在**构造时**直接注入。
/// Interface toward graph assembly. Dynamic port binding lands in Part 3.
pub trait Node {
    /// 主动关闭：撤掉所有输出端口，让下游收到 `ChannelClosed`。
    /// Drop all output ports so downstreams observe closure.
    fn close(&mut self);

    /// 所有输入端口是否都已关闭 → 调度器据此结束 `exec` 循环。
    /// Whether every input port is closed; the scheduler ends the loop when true.
    fn is_all_input_closed(&self) -> bool;
}

/// 节点与调度器交互的接口：被 spawn 成一个 tokio 任务。
///
/// `start` 非 async、返回 [`JoinHandle`] → **对象安全**，故可 `Box<dyn Actor>`。
/// 约定的任务体（本章手写、Ch2.3 起由 `#[derive(Actor)]` 生成）是三段式生命周期：
/// `initialize → while !is_all_input_closed { exec } → close → finalize`。
///
/// Ch4.3 给 `start` 加了一个 [`Context`] 参数：图装配期一次建好的**共享资源**
/// （模型 / 内存池）随它传进来，节点在 `initialize(&ctx)` 里按名借出、暂存到自己的
/// `#[state]` 字段。`ctx: Context`（`Sized`）不破坏对象安全——`Box<dyn Actor>` 依旧成立。
/// 注意资源**没有**渗进 `exec`：Ch3.4 那条 `exec(&mut self)` 签名原封不动，几十个节点的
/// `exec` 无需改动——这正是「把 Context 只穿过 `start`/`initialize`」这个设计的收益。
///
/// Spawned as a tokio task. `start` is non-async & object-safe; the `Context`
/// (Ch4.3) carries shared resources in, consumed at `initialize`, not `exec`.
pub trait Actor: Node + Send + 'static {
    /// 把节点交给运行时跑成一个任务，返回其 `JoinHandle`。`ctx` 携带图的共享资源。
    /// Hand the node to the runtime; return its `JoinHandle`. `ctx` carries shared resources.
    fn start(self: Box<Self>, ctx: Context) -> JoinHandle<Result<()>>;
}
// ANCHOR_END: node_traits

// ── 测试：手写一个 `Doubler` 节点，钉死 exec 循环 + 优雅停机的契约 ──
// A hand-written node (no macros yet) that proves the exec loop & shutdown.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{channel, Receiver, Sender};
    use crate::context::Context;
    use crate::error::Error;
    use flow_message::Envelope;

    // ANCHOR: doubler
    /// 一个「把收到的 i32 翻倍再发出」的最小 worker 节点——**全部手写、不用任何宏**。
    /// 真正的业务逻辑只有 `v * 2` 一行，其余全是样板：端口字段、关闭标志、
    /// `Node`/`Actor` 两个 impl、`start` 里的 spawn + 循环 + 生命周期。
    /// 这堆样板正是 Part 2 后续过程宏要消除的东西。
    struct Doubler {
        inp: Receiver,
        out: Option<Sender>, // Option：close() 时置 None 以 drop 掉 Sender
        input_closed: bool,
    }

    impl Doubler {
        // 生命周期钩子：Ch4.3 起 initialize 收下 `&Context`——节点在这里按名借出共享
        // 资源、暂存到自己的字段。Doubler 不用资源，故签名收下但忽略（`_ctx`）。
        async fn initialize(&mut self, _ctx: &Context) {}
        async fn finalize(&mut self) {}

        // 真正的「一次处理」：收一条、翻倍、发一条。收到关闭就记下标志。
        async fn exec(&mut self) -> Result<()> {
            match self.inp.recv::<i32>().await {
                Ok(mut e) => {
                    let doubled = e.unpack() * 2;
                    if let Some(out) = self.out.as_ref() {
                        out.send(e.repack(doubled)).await?;
                    }
                }
                Err(Error::ChannelClosed) => self.input_closed = true,
                Err(e) => return Err(e),
            }
            Ok(())
        }
    }

    impl Node for Doubler {
        fn close(&mut self) {
            self.out = None; // drop Sender → 下游收到 ChannelClosed
        }
        fn is_all_input_closed(&self) -> bool {
            self.input_closed
        }
    }

    impl Actor for Doubler {
        fn start(mut self: Box<Self>, ctx: Context) -> JoinHandle<Result<()>> {
            tokio::spawn(async move {
                self.initialize(&ctx).await;
                let result = async {
                    while !self.is_all_input_closed() {
                        self.exec().await?;
                    }
                    Ok(())
                }
                .await;
                self.close();
                self.finalize().await;
                result
            })
        }
    }
    // ANCHOR_END: doubler

    // ANCHOR: doubler_tests
    #[tokio::test]
    async fn doubler_pipes_and_shuts_down() {
        let (in_tx, in_rx) = channel(8);
        let (out_tx, out_rx) = channel(8);
        let node = Box::new(Doubler {
            inp: in_rx,
            out: Some(out_tx),
            input_closed: false,
        });
        let handle = node.start(Context::anonymous());

        // 喂 3 条，随后关闭输入端（drop 掉唯一的 Sender）
        for v in [1i32, 2, 3] {
            in_tx.send(Envelope::new(v)).await.unwrap();
        }
        drop(in_tx);

        // 收集输出：应为翻倍值，且上游停机后本端也随之关闭
        let mut got = Vec::new();
        while let Ok(mut e) = out_rx.recv::<i32>().await {
            got.push(e.unpack());
        }
        assert_eq!(got, vec![2, 4, 6]);

        // 任务优雅结束、无 panic、无错误
        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn runs_behind_boxed_dyn_actor() {
        // 擦除成 Box<dyn Actor> 后照样能 start——这就是 start 非 async 换来的对象安全。
        let (in_tx, in_rx) = channel(4);
        let (out_tx, out_rx) = channel(4);
        let actor: Box<dyn Actor> = Box::new(Doubler {
            inp: in_rx,
            out: Some(out_tx),
            input_closed: false,
        });
        let handle = actor.start(Context::anonymous());

        in_tx.send(Envelope::new(21i32)).await.unwrap();
        let mut e = out_rx.recv::<i32>().await.unwrap();
        assert_eq!(e.unpack(), 42);

        drop(in_tx);
        handle.await.unwrap().unwrap();
    }
    // ANCHOR_END: doubler_tests
}
