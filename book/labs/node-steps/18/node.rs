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

// 与第十七步同：端口字段靠属性宏注入，Doubler 仍手写 exec 与 impl Actor。
// 本步的变化在 derive/src/lib.rs（端口分类精确化）；这里新增一个反面用例 KeepsBusinessState。
#[flow_derive::inputs(inp)]
#[flow_derive::outputs(out)]
#[derive(flow_derive::Node)]
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

impl Actor for Doubler {
    fn start(mut self: Box<Self>) -> JoinHandle<Result<()>> {
        tokio::spawn(async move {
            self.initialize().await;
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
        assert!(!node.is_all_input_closed());
        node.close();
        assert!(node.history.is_some()); // 用第十七步的字符串判据，这里会变成 None（测试失败）
    }
    // ANCHOR_END: keeps_state
}
