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

// `#[inputs(inp)]` 注入 `inp: Receiver` + `input_closed: bool`；`#[outputs(out)]` 注入
// `out: Option<Sender>`；`#[derive(Node)]` 追加 `impl Node`。结构体本身写成空壳——
// 所有端口字段都由属性宏注入（宏调用写成 `flow_derive::` 全路径，避免与本模块的 trait 同名）。
#[flow_derive::inputs(inp)]
#[flow_derive::outputs(out)]
#[derive(flow_derive::Node)]
pub struct Doubler {}

impl Doubler {
    async fn initialize(&mut self) {}
    async fn finalize(&mut self) {}

    // 本步 exec 仍手写关闭处理：`match` 到 `ChannelClosed` 就置标志。第二十步交给 `#[methods]`。
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

// 本步 impl Actor 仍手写三段式循环（教学版不带 Context）。第十九步交给 `#[derive(Actor)]`。
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
        // 端口字段由宏注入，构造时直接填入（同模块可见）。图装配自动接线留到 Part 3。
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
}
