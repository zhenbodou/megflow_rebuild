//! flow-rs · channel —— 承载 `SealedEnvelope` 的异步通道（重写版）。
//!
//! 用 `tokio::sync::mpsc` 薄封装：**多生产者、单消费者**。`Sender` 可 `Clone`
//! （扇入：多个上游发往同一下游），`Receiver::recv` 取 `&mut self`（单消费者）。
//! 这是引擎最基础的一条「边」。**广播**（一份消息发给多路，Ch4.1 的 `bcast`）
//! 与 **work-stealing demux**（Ch4.2）都是**节点级/后续**话题——不塞进这一层，
//! 保持通道本身极简。这正是相对原版重型 `channel/`（stats/storage/协程感知）
//! 的化简：把「一条能异步收发信封的管子」做到最小。
//!
//! Thin wrapper over `tokio::sync::mpsc` (MPSC). Broadcast and demux are
//! node-level / later concerns, kept out of this layer.

use crate::error::{Error, Result};
use flow_message::{Envelope, SealedEnvelope};
use tokio::sync::mpsc;

/// 通道发送端。可 `Clone`（多生产者扇入）。
/// Sending half; `Clone` for multi-producer fan-in.
#[derive(Clone)]
pub struct Sender {
    inner: mpsc::Sender<SealedEnvelope>,
}

/// 通道接收端。单消费者：`recv` 取 `&mut self`。
/// Receiving half; single-consumer (`recv` takes `&mut self`).
pub struct Receiver {
    inner: mpsc::Receiver<SealedEnvelope>,
}

/// 建一条容量为 `capacity` 的有界通道。/ create a bounded channel.
pub fn channel(capacity: usize) -> (Sender, Receiver) {
    let (tx, rx) = mpsc::channel(capacity);
    (Sender { inner: tx }, Receiver { inner: rx })
}

impl Sender {
    /// 发送一个已封箱的信封（未类型化）。通道关闭 → `Err(ChannelClosed)`。
    /// Send an already-sealed envelope (untyped).
    pub async fn send_any(&self, msg: SealedEnvelope) -> Result<()> {
        self.inner.send(msg).await.map_err(|_| Error::ChannelClosed)
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
        self.inner.is_closed()
    }
}

impl Receiver {
    /// 收一个已封箱的信封（未类型化）。所有 `Sender` 均 drop 且队列排空 →
    /// `Err(ChannelClosed)`。/ Receive an untyped sealed envelope.
    pub async fn recv_any(&mut self) -> Result<SealedEnvelope> {
        self.inner.recv().await.ok_or(Error::ChannelClosed)
    }

    /// 收一个类型化信封：`recv_any` 后把类型 `downcast` 回来（Ch1.3 的安全实现）。
    /// 类型不符 → `Err(TypeMismatch)`。
    /// Receive and downcast back to `Envelope<T>`; wrong type → `TypeMismatch`.
    pub async fn recv<T>(&mut self) -> Result<Envelope<T>>
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

    #[tokio::test]
    async fn typed_send_recv_roundtrip() {
        let (tx, mut rx) = channel(4);
        tx.send(Envelope::new(42i32)).await.unwrap();
        let mut e = rx.recv::<i32>().await.unwrap();
        assert_eq!(e.unpack(), 42);
    }

    #[tokio::test]
    async fn recv_wrong_type_is_type_mismatch() {
        let (tx, mut rx) = channel(4);
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
        let (tx, mut rx) = channel(1);
        drop(tx);
        assert!(matches!(rx.recv::<i32>().await, Err(Error::ChannelClosed)));
    }

    #[tokio::test]
    async fn untyped_send_any_recv_any() {
        let (tx, mut rx) = channel(1);
        tx.send_any(Envelope::new(7i32).seal()).await.unwrap();
        let mut sealed = rx.recv_any().await.unwrap();
        let e = sealed.downcast_mut::<Envelope<i32>>().unwrap();
        assert_eq!(e.unpack(), 7);
    }
}
