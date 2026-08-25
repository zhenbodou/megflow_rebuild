//! 集成测试：用 Ch2.3 的宏把 Ch2.1 手写的 `Doubler` 塌缩成几行声明，
//! 并验证**生成的代码真的能编译、能跑、行为与手写版一致**。
//!
//! 对照 `flow-rs/src/node.rs` 里手写的 `Doubler`（~50 行样板）——这里业务逻辑
//! 之外的一切（端口字段、关闭标志、`Node`/`Actor` 两个 impl、三段式循环、
//! `ChannelClosed` 处理）都由宏生成。
//!
//! 名字巧合：`Node`/`Actor` 既是 trait（类型命名空间）又是派生宏（宏命名空间），
//! 二者可同名共存——这正是 serde 的 `Serialize` 同名 trait+派生宏的套路。
//!
//! Integration test: the macro-collapsed Doubler compiles, runs, and behaves
//! identically to the hand-written one in `node.rs`.

use flow_derive::{inputs, methods, outputs, Actor, Node};
use flow_message::Envelope;
use flow_rs::channel::{channel, Receiver, Sender};
use flow_rs::error::{Error, Result};
use flow_rs::node::{Actor, Node};

#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor)]
struct Doubler {}

#[methods]
impl Doubler {
    // 只写业务逻辑：收一条 i32、翻倍、发出。无需手写关闭标志/循环/生命周期——
    // `recv().await?` 遇到 ChannelClosed 会被生成的包装 exec 吞掉并置关闭标志。
    async fn exec(&mut self) -> Result<()> {
        let mut e = self.inp.recv::<i32>().await?;
        if let Some(out) = self.out.as_ref() {
            out.send(Envelope::new(e.unpack() * 2)).await?;
        }
        Ok(())
    }
}

#[tokio::test]
async fn macro_doubler_pipes_and_shuts_down() {
    let (in_tx, in_rx) = channel(8);
    let (out_tx, mut out_rx) = channel(8);
    // 端口字段由宏注入，构造时直接填入（同模块可见）。图装配自动接线留到 Part 3。
    let node = Box::new(Doubler {
        inp: in_rx,
        out: Some(out_tx),
        input_closed: false,
    });
    let handle = node.start();

    for v in [1i32, 2, 3] {
        in_tx.send(Envelope::new(v)).await.unwrap();
    }
    drop(in_tx);

    let mut got = Vec::new();
    while let Ok(mut e) = out_rx.recv::<i32>().await {
        got.push(e.unpack());
    }
    assert_eq!(got, vec![2, 4, 6]);
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn macro_doubler_runs_behind_boxed_dyn_actor() {
    // 生成的 Actor::start 依旧非 async → 对象安全 → 可 Box<dyn Actor>。
    let (in_tx, in_rx) = channel(4);
    let (out_tx, mut out_rx) = channel(4);
    let actor: Box<dyn Actor> = Box::new(Doubler {
        inp: in_rx,
        out: Some(out_tx),
        input_closed: false,
    });
    let handle = actor.start();

    in_tx.send(Envelope::new(21i32)).await.unwrap();
    let mut e = out_rx.recv::<i32>().await.unwrap();
    assert_eq!(e.unpack(), 42);

    drop(in_tx);
    handle.await.unwrap().unwrap();
}
