use crate::config::interlayer::MsgTypeId;
mod typed;
pub use typed::{ReceiverT, SenderT};

use crate::error::{Error, Result};
use flow_message::{Envelope, SealedEnvelope};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

#[derive(Clone, Default)]
pub struct Sender {
    channel_type: MsgTypeId,
    inner: SendImpl,
}

#[derive(Clone, Default)]
pub struct Receiver {
    channel_type: MsgTypeId,
    connected: bool,
    inner: Arc<Mutex<RecvImpl>>,
}

#[derive(Clone, Default)]
enum SendImpl {
    #[default]
    Unconnected,
    Bounded(mpsc::Sender<SealedEnvelope>),
    Unbounded(mpsc::UnboundedSender<SealedEnvelope>),
}

#[derive(Default)]
enum RecvImpl {
    #[default]
    Unconnected,
    Bounded(mpsc::Receiver<SealedEnvelope>),
    Unbounded(mpsc::UnboundedReceiver<SealedEnvelope>),
}

pub fn channel(capacity: usize) -> (Sender, Receiver) {
    channel_with_type(capacity, MsgTypeId::Any)
}

pub fn channel_with_type(capacity: usize, channel_type: MsgTypeId) -> (Sender, Receiver) {
    if capacity == 0 {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Sender {
                channel_type,
                inner: SendImpl::Unbounded(tx),
            },
            Receiver {
                channel_type,
                connected: true,
                inner: Arc::new(Mutex::new(RecvImpl::Unbounded(rx))),
            },
        )
    } else {
        let (tx, rx) = mpsc::channel(capacity);
        (
            Sender {
                channel_type,
                inner: SendImpl::Bounded(tx),
            },
            Receiver {
                channel_type,
                connected: true,
                inner: Arc::new(Mutex::new(RecvImpl::Bounded(rx))),
            },
        )
    }
}

impl Sender {
    pub fn is_none(&self) -> bool {
        matches!(self.inner, SendImpl::Unconnected)
    }
    pub async fn send_any(&self, message: SealedEnvelope) -> Result<()> {
        match &self.inner {
            SendImpl::Unconnected => Ok(()),
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

pub enum BatchRecvError<T> {
    Closed(Vec<T>),
}
impl<T> std::fmt::Debug for BatchRecvError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BatchRecvError::Closed")
    }
}

impl Receiver {
    pub fn is_none(&self) -> bool {
        !self.connected
    }
    /// 原版 try_recv 是限时等待；超时为 Ok(None)，关闭为 Err。
    pub async fn try_recv_any(&self, dur: std::time::Duration) -> Result<Option<SealedEnvelope>> {
        tokio::select! {
            _ = tokio::time::sleep(dur) => Ok(None),
            message = self.recv_any() => message.map(Some),
        }
    }

    pub async fn try_recv<T: Send + Clone + 'static>(
        &self,
        dur: std::time::Duration,
    ) -> Result<Option<Envelope<T>>> {
        self.try_recv_any(dur).await.map(|item| {
            item.map(|mut item| {
                item.downcast_mut::<Envelope<T>>()
                    .expect("type error when downcast in receiver")
                    .take()
            })
        })
    }
    /// n 是累计权重阈值，不是信封数量。超时返回部分成功结果。
    pub async fn batch_recv_any(
        &self,
        n: usize,
        dur: std::time::Duration,
    ) -> std::result::Result<Vec<SealedEnvelope>, BatchRecvError<SealedEnvelope>> {
        if n == 0 {
            return Ok(Vec::new());
        }
        let timer = tokio::time::sleep(dur);
        tokio::pin!(timer);
        let mut batch = Vec::with_capacity(n);
        let mut weight = 0;
        loop {
            tokio::select! {
                _ = &mut timer => return Ok(batch),
                message = self.recv_any() => match message {
                    Ok(message) => {
                        weight += message.info().weight.unwrap_or(1);
                        batch.push(message);
                        if weight >= n { return Ok(batch); }
                    }
                    Err(_) => return Err(BatchRecvError::Closed(batch)),
                }
            }
        }
    }

    pub async fn batch_recv<T: Send + Clone + 'static>(
        &self,
        n: usize,
        dur: std::time::Duration,
    ) -> std::result::Result<Vec<Envelope<T>>, BatchRecvError<Envelope<T>>> {
        let convert = |items: Vec<SealedEnvelope>| {
            items
                .into_iter()
                .map(|mut item| {
                    item.downcast_mut::<Envelope<T>>()
                        .expect("type error when downcast")
                        .take()
                })
                .collect()
        };
        self.batch_recv_any(n, dur)
            .await
            .map(convert)
            .map_err(|error| match error {
                BatchRecvError::Closed(items) => BatchRecvError::Closed(convert(items)),
            })
    }

    pub async fn recv_any(&self) -> Result<SealedEnvelope> {
        let mut receiver = self.inner.lock().await;
        match &mut *receiver {
            RecvImpl::Unconnected => None,
            RecvImpl::Bounded(receiver) => receiver.recv().await,
            RecvImpl::Unbounded(receiver) => receiver.recv().await,
        }
        .ok_or(Error::ChannelClosed)
    }

    pub async fn recv<T: Send + 'static>(&self) -> Result<Envelope<T>> {
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

    #[test]
    fn wrapper_does_not_relabel_queue() {
        let (sender, receiver) = channel_with_type(1, MsgTypeId::of::<String>());
        let sender: SenderT<u32> = sender.into();
        let receiver: ReceiverT<u32> = receiver.into();
        assert_eq!(sender.port_tid(), MsgTypeId::of::<u32>());
        assert_eq!(receiver.port_tid(), MsgTypeId::of::<u32>());
        assert_eq!(sender.chan_tid(), MsgTypeId::of::<String>());
        assert_eq!(receiver.chan_tid(), MsgTypeId::of::<String>());
    }

    #[tokio::test]
    async fn typed_roundtrip_preserves_metadata() {
        let (tx, rx) = channel(1);
        let mut message = Envelope::new(7u32);
        message.info_mut().to_addr = Some(42);
        tx.send(message).await.unwrap();
        let mut received = rx.recv::<u32>().await.unwrap();
        assert_eq!(received.info().to_addr, Some(42));
        assert_eq!(received.unpack(), 7);
    }

    #[tokio::test]
    async fn untyped_roundtrip() {
        let (tx, rx) = channel(1);
        tx.send_any(Envelope::new(9u32).seal()).await.unwrap();
        let mut message = rx.recv_any().await.unwrap();
        assert_eq!(message.downcast_mut::<Envelope<u32>>().unwrap().unpack(), 9);
    }

    #[tokio::test]
    async fn wrong_type_consumes_only_that_message() {
        let (tx, rx) = channel(2);
        tx.send(Envelope::new(1u32)).await.unwrap();
        tx.send(Envelope::new(2i32)).await.unwrap();
        assert!(matches!(rx.recv::<i32>().await, Err(Error::TypeMismatch)));
        assert_eq!(rx.recv::<i32>().await.unwrap().unpack(), 2);
    }

    #[tokio::test]
    async fn last_sender_drop_drains_before_close() {
        let (tx, rx) = channel(1);
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
        let (sender, receiver) = channel(0);
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

    #[tokio::test]
    async fn receiver_clones_share_one_queue() {
        let (sender, first) = channel(0);
        let second = first.clone();
        sender.send(Envelope::new(1u32)).await.unwrap();
        sender.send(Envelope::new(2u32)).await.unwrap();
        drop(sender);
        assert_eq!(first.recv::<u32>().await.unwrap().unpack(), 1);
        assert_eq!(second.recv::<u32>().await.unwrap().unpack(), 2);
        assert!(matches!(first.recv_any().await, Err(Error::ChannelClosed)));
        assert!(matches!(second.recv_any().await, Err(Error::ChannelClosed)));
    }
    #[tokio::test]
    async fn timeout_preserves_receiver_and_batch_returns_partial_on_close() {
        use std::time::Duration;
        let (sender, receiver) = channel(0);
        assert!(receiver
            .try_recv_any(Duration::from_millis(1))
            .await
            .unwrap()
            .is_none());
        let mut message = Envelope::new(7u32);
        message.info_mut().weight = Some(2);
        sender.send(message).await.unwrap();
        drop(sender);
        match receiver.batch_recv::<u32>(3, Duration::from_secs(1)).await {
            Err(BatchRecvError::Closed(mut batch)) => {
                assert_eq!(batch.len(), 1);
                assert_eq!(batch[0].unpack(), 7);
            }
            _ => panic!("expected partial batch on close"),
        }
    }
    #[tokio::test]
    async fn typed_wrapper_preserves_queue_and_metadata() {
        let (sender, receiver) = channel(1);
        let sender: SenderT<u32> = sender.into();
        let receiver: ReceiverT<u32> = receiver.into();
        let mut message = Envelope::new(7u32);
        message.info_mut().partial_id = Some(42);
        sender.send(message).await.unwrap();
        let mut message = receiver.recv().await.unwrap();
        assert_eq!(message.info().partial_id, Some(42));
        assert_eq!(message.unpack(), 7);
    }
    #[tokio::test]
    async fn default_endpoints_have_no_queue() {
        let sender = Sender::default();
        let receiver = Receiver::default();
        assert!(sender.is_none());
        assert!(receiver.is_none());
        sender.send(Envelope::new(7u32)).await.unwrap();
        assert!(matches!(
            receiver.recv_any().await,
            Err(Error::ChannelClosed)
        ));
        let (sender, receiver) = channel(1);
        assert!(!sender.is_none());
        assert!(!receiver.is_none());
        let typed: SenderT<u32> = SenderT::default();
        assert!(typed.is_none());
    }
}

pub trait TypeInfo {
    fn port_tid(&self) -> MsgTypeId;
    fn chan_tid(&self) -> MsgTypeId;
}
// ANCHOR_END: type_info_trait

impl TypeInfo for Sender {
    fn port_tid(&self) -> MsgTypeId {
        MsgTypeId::Any
    }
    fn chan_tid(&self) -> MsgTypeId {
        self.channel_type
    }
}
impl TypeInfo for Receiver {
    fn port_tid(&self) -> MsgTypeId {
        MsgTypeId::Any
    }
    fn chan_tid(&self) -> MsgTypeId {
        self.channel_type
    }
}
