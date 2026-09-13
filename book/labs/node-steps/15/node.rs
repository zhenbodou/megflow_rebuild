use crate::channel::{Receiver, Sender};
use crate::error::{Error, Result};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

pub trait Node {
    fn close(&mut self);
    fn is_all_input_closed(&self) -> bool;
}

pub trait Actor: Node + Send + 'static {
    fn start(self: Box<Self>) -> JoinHandle<Result<()>>;
}

pub struct Doubler {
    input: Receiver,
    output: Sender,
    input_closed: bool,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl Doubler {
    pub fn new(input: Receiver, output: Sender) -> Self {
        Self {
            input,
            output,
            input_closed: false,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn record(&self, event: &'static str) {
        self.events.lock().unwrap().push(event);
    }

    async fn initialize(&mut self) {
        self.record("initialize");
    }
    async fn finalize(&mut self) {
        self.record("finalize");
    }

    pub async fn exec(&mut self) -> Result<()> {
        self.record("exec");
        match self.input.recv::<i32>().await {
            Ok(mut message) => {
                let doubled = message.unpack() * 2;
                self.output.send(message.repack(doubled)).await?;
            }
            Err(Error::ChannelClosed) => self.input_closed = true,
            Err(error) => return Err(error),
        }
        Ok(())
    }
}

impl Node for Doubler {
    fn close(&mut self) {
        self.record("close");
        self.output.close();
    }

    fn is_all_input_closed(&self) -> bool {
        self.input_closed
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
        let mut node = Doubler::new(input, output);
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
            let node = Doubler::new(input, output);
            let events = node.events.clone();
            let actor: Box<dyn Actor> = Box::new(node);
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
            assert_eq!(
                *events.lock().unwrap(),
                [
                    "initialize",
                    "exec",
                    "exec",
                    "exec",
                    "exec",
                    "close",
                    "finalize"
                ]
            );
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn send_error_still_closes_and_finalizes() {
        tokio::time::timeout(Duration::from_secs(2), async {
            let (source, input) = channel(1);
            let (output, sink) = channel(1);
            sink.close();
            source.send(Envelope::new(7i32)).await.unwrap();
            let node = Doubler::new(input, output);
            let events = node.events.clone();
            let result = Box::new(node).start().await.unwrap();
            assert!(matches!(result, Err(Error::ChannelClosed)));
            assert_eq!(
                *events.lock().unwrap(),
                ["initialize", "exec", "close", "finalize"]
            );
        })
        .await
        .unwrap();
    }
}
