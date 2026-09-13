use crate::error::{Error, Result};
use flow_message::SealedEnvelope;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Sender {
    inner: mpsc::Sender<SealedEnvelope>,
}

pub struct Receiver {
    inner: mpsc::Receiver<SealedEnvelope>,
}

pub fn channel(capacity: usize) -> (Sender, Receiver) {
    assert!(capacity > 0, "basic channel requires positive capacity");
    let (sender, receiver) = mpsc::channel(capacity);
    (Sender { inner: sender }, Receiver { inner: receiver })
}

impl Sender {
    pub async fn send_any(&self, message: SealedEnvelope) -> Result<()> {
        match self.inner.send(message).await {
            Ok(()) => Ok(()),
            Err(_) => Err(Error::ChannelClosed),
        }
    }
}

impl Receiver {
    pub async fn recv_any(&mut self) -> Result<SealedEnvelope> {
        match self.inner.recv().await {
            Some(message) => Ok(message),
            None => Err(Error::ChannelClosed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_message::Envelope;

    #[tokio::test]
    async fn wrapper_moves_an_envelope() {
        let (sender, mut receiver) = channel(1);
        sender.send_any(Envelope::new(7u32).seal()).await.unwrap();
        let mut message = receiver.recv_any().await.unwrap();
        assert_eq!(message.downcast_mut::<Envelope<u32>>().unwrap().unpack(), 7);
        drop(sender);
        assert!(matches!(
            receiver.recv_any().await,
            Err(Error::ChannelClosed)
        ));
    }
}
