//! 第二十二步的集成测试：走通「注册 → 查找 → 构造 → 运行」整条链。
//!
//! 这是一个**独立 crate**（集成测试），扮演「下游使用者」：它定义 Doubler、挂
//! `#[derive(BuildFromPorts)]`，再用一行 `node_register!("Doubler", Doubler)` 把它登记进全局表。
//! 测试只凭字符串 `"Doubler"` 就能 `find` 出构造器、造出节点、跑出 `[2,4,6]`。
//!
//! **注册真的经过了 linker section**：本 crate 的 `submit!`（`node_register!` 生成）与 flow-rs 里的
//! `collect!` 在链接时汇合，证明跨 crate 收集成立——这正是 Part 3 Graph Builder 按 TOML 装配节点
//! 所依赖的底座。
//!
//! 同名巧合第三次：`BuildFromPorts` 既是 trait（`flow_rs::registry`，类型命名空间）又是派生宏
//! （`flow_derive`，宏命名空间），和 `Node`/`Actor` 一样共存——两个都 `use` 了，各归其位。

use flow_derive::{inputs, methods, node_register, outputs, Actor, BuildFromPorts, Node};
use flow_message::Envelope;
use flow_rs::channel::{channel, Receiver, Sender};
use flow_rs::error::{Error, Result};
use flow_rs::node::{Actor, Node};
use flow_rs::registry::{find, registrations, BuildFromPorts};

// ANCHOR: node_def
#[inputs(inp)]
#[outputs(out)]
#[derive(Node, Actor, BuildFromPorts)]
struct Doubler {}

#[methods]
impl Doubler {
    async fn exec(&mut self) -> Result<()> {
        let mut message = self.inp.recv::<i32>().await?;
        let doubled = message.unpack() * 2;
        if let Some(out) = self.out.as_ref() {
            out.send(message.repack(doubled)).await?;
        }
        Ok(())
    }
}

// 编译期把 Doubler 登记进全局表。node_register! 生成 `flow_rs::inventory::submit!`。
node_register!("Doubler", Doubler);
// ANCHOR_END: node_def

// ANCHOR: lookup
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
    // 只凭类型名拿到构造器，位置接线（1 输入 1 输出）造出节点、跑出 [2,4,6]。
    let reg = find("Doubler").expect("Doubler 已注册");
    let (in_tx, in_rx) = channel(8);
    let (out_tx, out_rx) = channel(8);
    let node = (reg.ctor)(vec![in_rx], vec![out_tx]);

    let handle = node.start();
    for value in [1i32, 2, 3] {
        in_tx.send(Envelope::new(value)).await.unwrap();
    }
    drop(in_tx);

    let mut got = Vec::new();
    while let Ok(mut message) = out_rx.recv::<i32>().await {
        got.push(message.unpack());
    }
    // 与手写版 / Ch2.3 宏塌缩版行为完全一致：翻倍 + 优雅停机。
    assert_eq!(got, vec![2, 4, 6]);
    handle.await.unwrap().unwrap();
}
// ANCHOR_END: lookup
