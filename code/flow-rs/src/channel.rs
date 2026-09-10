//! flow-rs · channel —— 承载 `SealedEnvelope` 的异步通道（重写版）。
//!
//! 基于 Tokio 队列，共享接收端用异步 Mutex 串行取出消息。
//! Sender 与 Receiver 均可克隆；多个消费者竞争消息，每条仅交给一个消费者。
//! flush epoch、类型转换和统计协议仍需继续对齐原版。

mod conversion;
pub use conversion::{add_cvt_func_impl, guess_channel_type, ConversionRegistration, CvtF};
mod typed;
pub use typed::{ReceiverT, SenderT};

use crate::config::interlayer::MsgTypeId;
use crate::error::{Error, Result};
use flow_message::{Envelope, SealedEnvelope};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// 通道发送端。可 `Clone`（多生产者扇入）。
/// Sending half; `Clone` for multi-producer fan-in.
#[derive(Clone, Default)]
pub struct Sender {
    inner: SendImpl,
    channel_type: MsgTypeId,
    conversion: Option<CvtF>,
}

/// 克隆共享同一队列，竞争接收，不复制消息。
#[derive(Clone, Default)]
pub struct Receiver {
    inner: Option<Arc<Mutex<RecvImpl>>>,
    channel_type: MsgTypeId,
    conversion: Option<CvtF>,
}

#[derive(Clone, Default)]
enum SendImpl {
    #[default]
    Unconnected,
    Bounded(mpsc::Sender<SealedEnvelope>),
    Unbounded(mpsc::UnboundedSender<SealedEnvelope>),
}
enum RecvImpl {
    Bounded(mpsc::Receiver<SealedEnvelope>),
    Unbounded(mpsc::UnboundedReceiver<SealedEnvelope>),
}

/// 与原版 ChannelStorage 一致：正容量有界，0 表示无界而非零容量会合。
pub fn channel(capacity: usize) -> (Sender, Receiver) {
    channel_with_type(capacity, MsgTypeId::Any)
}

/// 创建带通道类型描述的队列。描述不执行转换或验证实际消息载荷。
/// 这是后续 ChannelStorage/类型推断的装配入口，不能替代 CVT_VTABLE。
pub fn channel_with_type(capacity: usize, channel_type: MsgTypeId) -> (Sender, Receiver) {
    if capacity == 0 {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Sender {
                inner: SendImpl::Unbounded(tx),
                channel_type,
                conversion: None,
            },
            Receiver {
                inner: Some(Arc::new(Mutex::new(RecvImpl::Unbounded(rx)))),
                channel_type,
                conversion: None,
            },
        )
    } else {
        let (tx, rx) = mpsc::channel(capacity);
        (
            Sender {
                inner: SendImpl::Bounded(tx),
                channel_type,
                conversion: None,
            },
            Receiver {
                inner: Some(Arc::new(Mutex::new(RecvImpl::Bounded(rx)))),
                channel_type,
                conversion: None,
            },
        )
    }
}

/// 端口声明类型与底层通道类型可以不同，转换表将来负责衔接。
// ANCHOR: type_info_trait
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

impl Sender {
    /// 缓存端口类型 → 通道类型的直接转换，与原版装配方向一致。
    #[doc(hidden)]
    pub fn with_type(&mut self, port_type: &MsgTypeId) {
        self.conversion = conversion::lookup(*port_type, self.chan_tid());
    }

    /// 默认端点尚未接到队列；与已接线后关闭不同。
    pub fn is_none(&self) -> bool {
        matches!(self.inner, SendImpl::Unconnected)
    }

    /// 发送一个已封箱的信封（未类型化）。通道关闭 → `Err(ChannelClosed)`。
    /// Send an already-sealed envelope (untyped).
    pub async fn send_any(&self, msg: SealedEnvelope) -> Result<()> {
        if self.is_none() {
            return Ok(());
        }
        let msg = convert(self.conversion, msg).await?;
        match &self.inner {
            SendImpl::Unconnected => Ok(()),
            SendImpl::Bounded(tx) => tx.send(msg).await.map_err(|_| Error::ChannelClosed),
            SendImpl::Unbounded(tx) => tx.send(msg).map_err(|_| Error::ChannelClosed),
        }
    }

    /// 发送一个类型化信封：内部先 `seal` 再走 `send_any`。
    /// 载荷 `T` 须可 `Clone`——封箱后的 `SealedEnvelope` 要支持类型擦除克隆（广播用），
    /// 这条约束由 `Envelope::seal` 一路传导到这里（见 flow-message envelope.rs）。
    /// Send a typed envelope; seals then delegates to `send_any`. `T: Clone` because
    /// sealed envelopes must be cloneable under type erasure (for broadcast).
    pub async fn send<T>(&self, msg: Envelope<T>) -> Result<()>
    where
        T: 'static + Send + Clone,
    {
        self.send_any(msg.seal()).await
    }

    /// 通道是否已关闭（所有 `Receiver` 均已 drop）。
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            SendImpl::Unconnected => true,
            SendImpl::Bounded(tx) => tx.is_closed(),
            SendImpl::Unbounded(tx) => tx.is_closed(),
        }
    }
}

/// 提前关闭时保留已收到的部分批次，调用者决定如何处理。
pub enum BatchRecvError<T> {
    Closed(Vec<T>),
}
impl<T> std::fmt::Debug for BatchRecvError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BatchRecvError::Closed")
    }
}

impl Receiver {
    /// 缓存通道类型 → 端口类型的直接转换。
    #[doc(hidden)]
    pub fn with_type(&mut self, port_type: &MsgTypeId) {
        self.conversion = conversion::lookup(self.chan_tid(), *port_type);
    }

    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    // ANCHOR: timed_receive
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
    // ANCHOR_END: timed_receive

    // ANCHOR: batch_receive
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
    // ANCHOR_END: batch_receive

    /// 收一个已封箱的信封（未类型化）。所有 `Sender` 均 drop 且队列排空 →
    /// `Err(ChannelClosed)`。/ Receive an untyped sealed envelope.
    pub async fn recv_any(&self) -> Result<SealedEnvelope> {
        let inner = self.inner.as_ref().ok_or(Error::ChannelClosed)?;
        let msg = {
            let mut receiver = inner.lock().await;
            match &mut *receiver {
                RecvImpl::Bounded(rx) => rx.recv().await,
                RecvImpl::Unbounded(rx) => rx.recv().await,
            }
            .ok_or(Error::ChannelClosed)?
        };
        convert(self.conversion, msg).await
    }

    /// 收一个类型化信封：`recv_any` 后把类型 `downcast` 回来（Ch1.3 的安全实现）。
    /// 类型不符 → `Err(TypeMismatch)`。
    /// Receive and downcast back to `Envelope<T>`; wrong type → `TypeMismatch`.
    pub async fn recv<T>(&self) -> Result<Envelope<T>>
    where
        T: 'static + Send,
    {
        let mut sealed = self.recv_any().await?;
        // 认领回 Envelope<T> 的 &mut，再 take() 出一个拥有所有权的信封。
        match sealed.downcast_mut::<Envelope<T>>() {
            Some(e) => Ok(e.take()),
            None => Err(Error::TypeMismatch),
        }
    }
}

// ── 测试：契约钉死（红→绿）──
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use flow_message::Envelope;

    // ANCHOR: channel_tests
    #[tokio::test]
    async fn typed_send_recv_roundtrip() {
        let (tx, rx) = channel(4);
        tx.send(Envelope::new(42i32)).await.unwrap();
        let mut e = rx.recv::<i32>().await.unwrap();
        assert_eq!(e.unpack(), 42);
    }

    #[tokio::test]
    async fn recv_wrong_type_is_type_mismatch() {
        let (tx, rx) = channel(4);
        tx.send(Envelope::new(1i32)).await.unwrap();
        // 用 matches! 断言错误变体：无需 Envelope<T> 实现 Debug
        assert!(matches!(
            rx.recv::<String>().await,
            Err(Error::TypeMismatch)
        ));
    }

    #[tokio::test]
    async fn send_after_receiver_dropped_is_closed() {
        let (tx, rx) = channel(1);
        drop(rx);
        let err = tx.send(Envelope::new(1i32)).await.unwrap_err();
        assert!(matches!(err, Error::ChannelClosed));
    }

    #[tokio::test]
    async fn recv_after_senders_dropped_is_closed() {
        let (tx, rx) = channel(1);
        drop(tx);
        assert!(matches!(rx.recv::<i32>().await, Err(Error::ChannelClosed)));
    }

    #[tokio::test]
    async fn untyped_send_any_recv_any() {
        let (tx, rx) = channel(1);
        tx.send_any(Envelope::new(7i32).seal()).await.unwrap();
        let mut sealed = rx.recv_any().await.unwrap();
        let e = sealed.downcast_mut::<Envelope<i32>>().unwrap();
        assert_eq!(e.unpack(), 7);
    }
    // ANCHOR_END: channel_tests
}

async fn convert(function: Option<CvtF>, msg: SealedEnvelope) -> Result<SealedEnvelope> {
    // DummyEnvelope 属于控制协议，不送入业务转换器；完整 flush 协议仍待迁移。
    if msg.is::<flow_message::DummyEnvelope>() {
        return Ok(msg);
    }
    match function {
        None => Ok(msg),
        // 原版 rt::JoinHandle 的 Future::poll 对 Tokio JoinError 调用 unwrap，
        // 因此转换任务 panic 会让等待它的任务继续 panic，而非业务 Err。
        Some(function) => Ok(tokio::task::spawn_blocking(move || function(msg))
            .await
            .unwrap()),
    }
}
