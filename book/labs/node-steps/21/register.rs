//! 第二十一步的集成测试：验证 `#[derive(BuildFromPorts)]` 能「从端口造出一个能跑的节点」。
//!
//! 这是一个**独立 crate**（集成测试），扮演「下游使用者」：它像 Ch2.3 那样定义 Doubler，
//! 只多挂一个 `#[derive(BuildFromPorts)]`，就能用 `<Doubler as BuildFromPorts>::build` 从端口
//! 直接造出节点。下一步（第二十二步）再用 `node_register!` 把它登记进全局表、按名字查出来跑。
//!
//! 同名巧合第三次登场：`BuildFromPorts` 既是 trait（`flow_rs::registry`，类型命名空间）又是
//! 派生宏（`flow_derive`，宏命名空间），和 `Node`/`Actor` 一样共存——两个都 `use` 了，各归其位。

use flow_derive::{inputs, methods, outputs, Actor, BuildFromPorts, Node};
use flow_message::Envelope;
use flow_rs::channel::{channel, Receiver, Sender};
use flow_rs::error::{Error, Result};
use flow_rs::node::{Actor, Node};
use flow_rs::registry::BuildFromPorts;

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

#[tokio::test]
async fn build_from_ports_produces_runnable_node() {
    // 直接用派生出的 build 从端口造节点——还没经过注册表，先单独验证「填端口」这步。
    let (in_tx, in_rx) = channel(8);
    let (out_tx, out_rx) = channel(8);
    let node = <Doubler as BuildFromPorts>::build(vec![in_rx], vec![out_tx]);

    let handle = node.start();
    for value in [1i32, 2, 3] {
        in_tx.send(Envelope::new(value)).await.unwrap();
    }
    drop(in_tx);

    let mut got = Vec::new();
    while let Ok(mut message) = out_rx.recv::<i32>().await {
        got.push(message.unpack());
    }
    assert_eq!(got, vec![2, 4, 6]);
    handle.await.unwrap().unwrap();
}
