use crate::channel::{Receiver, Sender};
use crate::error::{Error, Result};
use tokio::task::JoinHandle;

pub trait Node {
    fn close(&mut self);
    fn is_all_input_closed(&self) -> bool;
}

pub trait Actor: Node + Send + 'static {
    fn start(self: Box<Self>) -> JoinHandle<Result<()>>;
}

// ANCHOR: doubler
// 第二十步：五个宏塌缩完成。结构体是空壳（端口靠属性宏注入），`impl` 里只剩纯业务的 exec——
// 关闭处理、生命周期默认、Node/Actor 全交给宏。这正是 Ch2.1 手写节点想抵达的终点形态。
#[flow_derive::inputs(inp)]
#[flow_derive::outputs(out)]
#[derive(flow_derive::Node, flow_derive::Actor)]
pub struct Doubler {}

#[flow_derive::methods]
impl Doubler {
    pub async fn exec(&mut self) -> Result<()> {
        let mut message = self.inp.recv::<i32>().await?;
        let doubled = message.unpack() * 2;
        if let Some(out) = self.out.as_ref() {
            out.send(message.repack(doubled)).await?;
        }
        Ok(())
    }
}
// ANCHOR_END: doubler

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::channel;
    use flow_message::Envelope;
    use std::time::Duration;

    #[tokio::test]
    async fn one_call_processes_one_message() {
        let (source, input) = channel(1);
        let (output, sink) = channel(1);
        let mut node = Doubler {
            inp: input,
            out: Some(output),
            input_closed: false,
        };
        let mut message = Envelope::new(7i32);
        message.info_mut().partial_id = Some(42);
        source.send(message).await.unwrap();
        node.exec().await.unwrap();
        let mut result = sink.recv::<i32>().await.unwrap();
        assert_eq!(result.info().partial_id, Some(42)); // repack 保留元信息
        assert_eq!(result.unpack(), 14);
    }

    // ANCHOR: doubler_test
    #[tokio::test]
    async fn erased_actor_drains_input_and_finalizes() {
        // 塌缩后的 Doubler 端到端仍是「喂 [1,2,3] 收 [2,4,6]」，且可作 `Box<dyn Actor>`。
        tokio::time::timeout(Duration::from_secs(2), async {
            let (source, input) = channel(1);
            let (output, sink) = channel(1);
            let actor: Box<dyn Actor> = Box::new(Doubler {
                inp: input,
                out: Some(output),
                input_closed: false,
            });
            let task = actor.start();
            let producer = tokio::spawn(async move {
                for value in [1i32, 2, 3] {
                    source.send(Envelope::new(value)).await.unwrap();
                }
            });
            let mut values = Vec::new();
            loop {
                match sink.recv::<i32>().await {
                    Ok(mut message) => values.push(message.unpack()),
                    Err(Error::ChannelClosed) => break,
                    Err(error) => panic!("unexpected receive error: {error}"),
                }
            }
            producer.await.unwrap();
            task.await.unwrap().unwrap();
            assert_eq!(values, [2, 4, 6]);
        })
        .await
        .unwrap();
    }
    // ANCHOR_END: doubler_test

    // ANCHOR: keeps_state
    struct HistorySender;

    // 业务类型恰好含 Sender，但它是 `Option<HistorySender>`、不是 `Option<Sender>`，
    // 精确分类后不应被 `Node::close` 当成输出端口撤掉。
    #[derive(flow_derive::Node)]
    struct KeepsBusinessState {
        history: Option<HistorySender>,
        input_closed: bool,
    }

    #[test]
    fn node_close_does_not_erase_business_type_containing_sender() {
        let mut node = KeepsBusinessState {
            history: Some(HistorySender),
            input_closed: false,
        };
        node.close();
        assert!(node.history.is_some());
    }
    // ANCHOR_END: keeps_state
}
