use crate::error::{Error, Result};
use flow_message::{Envelope, SealedEnvelope};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Sender {
    inner: mpsc::Sender<SealedEnvelope>,
}

pub struct Receiver {
    inner: mpsc::Receiver<SealedEnvelope>,
}

/// This introductory stage only accepts positive capacities.
pub fn channel(capacity: usize) -> (Sender, Receiver) {
    assert!(capacity > 0, "basic channel requires positive capacity");
    let (tx, rx) = mpsc::channel(capacity);
    (Sender { inner: tx }, Receiver { inner: rx })
}

impl Sender {
    pub async fn send_any(&self, message: SealedEnvelope) -> Result<()> {
        self.inner
            .send(message)
            .await
            .map_err(|_| Error::ChannelClosed)
    }

    pub async fn send<T: Clone + Send + 'static>(&self, message: Envelope<T>) -> Result<()> {
        self.send_any(message.seal()).await
    }
}

impl Receiver {
    pub async fn recv_any(&mut self) -> Result<SealedEnvelope> {
        self.inner.recv().await.ok_or(Error::ChannelClosed)
    }

    pub async fn recv<T: Send + 'static>(&mut self) -> Result<Envelope<T>> {
        let mut message = self.recv_any().await?;
        message
            .downcast_mut::<Envelope<T>>()
            .map(Envelope::take)
            .ok_or(Error::TypeMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn typed_roundtrip_preserves_metadata() {
        let (tx, mut rx) = channel(1);
        let mut message = Envelope::new(7u32);
        message.info_mut().to_addr = Some(42);
        tx.send(message).await.unwrap();
        let mut received = rx.recv::<u32>().await.unwrap();
        assert_eq!(received.info().to_addr, Some(42));
        assert_eq!(received.unpack(), 7);
    }

    #[tokio::test]
    async fn untyped_roundtrip() {
        let (tx, mut rx) = channel(1);
        tx.send_any(Envelope::new(9u32).seal()).await.unwrap();
        let mut message = rx.recv_any().await.unwrap();
        assert_eq!(message.downcast_mut::<Envelope<u32>>().unwrap().unpack(), 9);
    }

    #[tokio::test]
    async fn wrong_type_consumes_only_that_message() {
        let (tx, mut rx) = channel(2);
        tx.send(Envelope::new(1u32)).await.unwrap();
        tx.send(Envelope::new(2i32)).await.unwrap();
        assert!(matches!(rx.recv::<i32>().await, Err(Error::TypeMismatch)));
        assert_eq!(rx.recv::<i32>().await.unwrap().unpack(), 2);
    }

    #[tokio::test]
    async fn last_sender_drop_drains_before_close() {
        let (tx, mut rx) = channel(1);
        let other = tx.clone();
        drop(tx);
        other.send(Envelope::new(3u32)).await.unwrap();
        drop(other);
        assert_eq!(rx.recv::<u32>().await.unwrap().unpack(), 3);
        assert!(matches!(rx.recv_any().await, Err(Error::ChannelClosed)));
    }

    #[tokio::test]
    async fn receiver_drop_rejects_send() {
        let (tx, rx) = channel(1);
        drop(rx);
        assert!(matches!(
            tx.send(Envelope::new(1u32)).await,
            Err(Error::ChannelClosed)
        ));
    }
}
