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

// 第十九步：`#[derive(Actor)]` 接手固定的三段式 `start` 循环——原来手写的 `impl Actor` 整块删掉。
// exec 仍手写关闭处理（下一步交给 `#[methods]`）。至此 Doubler 已挂上两个属性宏 + 两个派生宏。
#[flow_derive::inputs(inp)]
#[flow_derive::outputs(out)]
#[derive(flow_derive::Node, flow_derive::Actor)]
pub struct Doubler {}

impl Doubler {
    async fn initialize(&mut self) {}
    async fn finalize(&mut self) {}

    pub async fn exec(&mut self) -> Result<()> {
        match self.inp.recv::<i32>().await {
            Ok(mut message) => {
                let doubled = message.unpack() * 2;
                if let Some(out) = self.out.as_ref() {
                    out.send(message.repack(doubled)).await?;
                }
            }
            Err(Error::ChannelClosed) => self.input_closed = true,
            Err(error) => return Err(error),
        }
        Ok(())
    }
}

// 从这一步起，Doubler 再没有手写的 `impl Actor`——它由 `#[derive(Actor)]` 生成。

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
        assert_eq!(result.info().partial_id, Some(42));
        assert_eq!(result.unpack(), 14);
    }

    #[tokio::test]
    async fn erased_actor_drains_input_and_finalizes() {
        // 现在 `start` 完全由宏生成，端到端行为不变：喂 [1,2,3] 收 [2,4,6]。
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

    // 第十八步的反面用例继续守着：业务字段 `Option<HistorySender>` 不被 `Node::close` 误撤。
    struct HistorySender;

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
}
