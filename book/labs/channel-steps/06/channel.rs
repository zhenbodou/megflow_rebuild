use crate::error::{Error, Result};
use flow_message::{Envelope, SealedEnvelope};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Sender {
    inner: SendImpl,
}

pub struct Receiver {
    inner: RecvImpl,
}

#[derive(Clone)]
enum SendImpl {
    Bounded(mpsc::Sender<SealedEnvelope>),
    Unbounded(mpsc::UnboundedSender<SealedEnvelope>),
}

enum RecvImpl {
    Bounded(mpsc::Receiver<SealedEnvelope>),
    Unbounded(mpsc::UnboundedReceiver<SealedEnvelope>),
}

pub fn channel(capacity: usize) -> (Sender, Receiver) {
    if capacity == 0 {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Sender {
                inner: SendImpl::Unbounded(tx),
            },
            Receiver {
                inner: RecvImpl::Unbounded(rx),
            },
        )
    } else {
        let (tx, rx) = mpsc::channel(capacity);
        (
            Sender {
                inner: SendImpl::Bounded(tx),
            },
            Receiver {
                inner: RecvImpl::Bounded(rx),
            },
        )
    }
}

impl Sender {
    pub async fn send_any(&self, message: SealedEnvelope) -> Result<()> {
        match &self.inner {
            SendImpl::Bounded(sender) => {
                sender.send(message).await.map_err(|_| Error::ChannelClosed)
            }
            SendImpl::Unbounded(sender) => sender.send(message).map_err(|_| Error::ChannelClosed),
        }
    }

    pub async fn send<T: Clone + Send + 'static>(&self, message: Envelope<T>) -> Result<()> {
        self.send_any(message.seal()).await
    }
}

impl Receiver {
    pub async fn recv_any(&mut self) -> Result<SealedEnvelope> {
        match &mut self.inner {
            RecvImpl::Bounded(receiver) => receiver.recv().await,
            RecvImpl::Unbounded(receiver) => receiver.recv().await,
        }
        .ok_or(Error::ChannelClosed)
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
    #[tokio::test]
    async fn zero_capacity_is_unbounded() {
        let (sender, mut receiver) = channel(0);
        for value in 0..10u32 {
            sender.send(Envelope::new(value)).await.unwrap();
        }
        drop(sender);
        for value in 0..10u32 {
            assert_eq!(receiver.recv::<u32>().await.unwrap().unpack(), value);
        }
        assert!(matches!(
            receiver.recv_any().await,
            Err(Error::ChannelClosed)
        ));
    }
}
