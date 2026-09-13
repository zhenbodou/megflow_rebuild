use crate::channel::{Receiver, Sender};
use crate::error::Result;

pub struct Doubler {
    input: Receiver,
    output: Sender,
}

impl Doubler {
    pub fn new(input: Receiver, output: Sender) -> Self {
        Self { input, output }
    }

    pub async fn exec(&mut self) -> Result<()> {
        let mut message = self.input.recv::<i32>().await?;
        let doubled = message.unpack() * 2;
        self.output.send(message.repack(doubled)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::channel;
    use flow_message::Envelope;

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
}
