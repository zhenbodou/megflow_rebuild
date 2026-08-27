//! 集成测试：Ch2.4 的编译期注册表。
//!
//! `#[derive(BuildFromPorts)]` 给节点生成「从端口构造」的 `build`；`node_register!`
//! 在编译期把 `{ "Doubler", <Doubler as BuildFromPorts>::build }` 提交进 inventory 表。
//! 运行时只凭类型名字符串 `"Doubler"` 就能 `find` 出构造器、把节点造出来跑——这正是
//! Part 3 的 Graph Builder 按 TOML 装配节点所依赖的底座。
//!
//! 同名巧合再现：`BuildFromPorts` 既是 trait（类型命名空间）又是派生宏（宏命名空间），
//! 与 `Node`/`Actor` 一样共存。
//!
//! Integration test for the compile-time registry: derive a constructor, submit
//! it via `node_register!`, then build & run the node looked up by name only.

use flow_derive::{inputs, methods, node_register, outputs, Actor, BuildFromPorts, Node};
use flow_message::Envelope;
use flow_rs::channel::{channel, Receiver, Sender};
use flow_rs::error::{Error, Result};
use flow_rs::node::{Actor, Node};
use flow_rs::registry::{find, registrations, BuildFromPorts};

#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
struct Doubler {}

#[methods]
impl Doubler {
    async fn exec(&mut self) -> Result<()> {
        let mut e = self.inp.recv::<i32>().await?;
        if let Some(out) = self.out.as_ref() {
            out.send(Envelope::new(e.unpack() * 2)).await?;
        }
        Ok(())
    }
}

// 编译期把 Doubler 登记进全局表。`node_register!` 生成 `flow_rs::inventory::submit!`。
node_register!("Doubler", Doubler);

#[test]
fn doubler_is_registered() {
    // link 期汇总的表里应能按名字查到 Doubler。
    assert!(find("Doubler").is_some());
    assert!(registrations().any(|r| r.name == "Doubler"));
    // 未注册的名字查不到。
    assert!(find("Nonexistent").is_none());
}

#[tokio::test]
async fn build_via_registry_and_run() {
    // 只凭类型名拿到构造器，位置接线（1 输入 1 输出）造出节点。
    // Ch3.2 起 ctor 多了 `&Args` 入参并返回 `Result`——Doubler 没有自有参数，传空表即可。
    // Ch4.2 起端口按「分组」传（`Vec<Vec<_>>`，每组一个端口名）：标量端口是恰 1 个的组，
    // 故这里把单个 channel 端包成 `vec![vec![..]]`。
    let reg = find("Doubler").expect("Doubler 已注册");
    let (in_tx, in_rx) = channel(8);
    let (out_tx, mut out_rx) = channel(8);
    let node = (reg.ctor)(
        &flow_rs::config::Args::new(),
        vec![vec![in_rx]],
        vec![vec![out_tx]],
    )
    .unwrap();

    let handle = node.start();
    for v in [1i32, 2, 3] {
        in_tx.send(Envelope::new(v)).await.unwrap();
    }
    drop(in_tx);

    let mut got = Vec::new();
    while let Ok(mut e) = out_rx.recv::<i32>().await {
        got.push(e.unpack());
    }
    // 与手写版 / Ch2.3 宏塌缩版行为完全一致：翻倍 + 优雅停机。
    assert_eq!(got, vec![2, 4, 6]);
    handle.await.unwrap().unwrap();
}
