//! 类型化端点：固定端口载荷类型，仍共享类型擦除队列。
//! From 按方向查询转换表并缓存函数；不自动推导多步转换链。
use super::{BatchRecvError, Receiver, Sender, TypeInfo};
use crate::error::Result;
use flow_message::Envelope;
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    time::Duration,
};

#[derive(Clone)]
pub struct SenderT<T>(Sender, PhantomData<T>);
#[derive(Clone)]
pub struct ReceiverT<T>(Receiver, PhantomData<T>);

impl<T> Default for SenderT<T> {
    fn default() -> Self {
        Self(Sender::default(), PhantomData)
    }
}
impl<T> Default for ReceiverT<T> {
    fn default() -> Self {
        Self(Receiver::default(), PhantomData)
    }
}

impl<T: 'static> From<Sender> for SenderT<T> {
    fn from(mut sender: Sender) -> Self {
        sender.conversion = super::conversion::lookup(
            crate::config::interlayer::MsgTypeId::of::<T>(),
            sender.chan_tid(),
        );
        Self(sender, PhantomData)
    }
}
impl<T: 'static> From<Receiver> for ReceiverT<T> {
    fn from(mut receiver: Receiver) -> Self {
        receiver.conversion = super::conversion::lookup(
            receiver.chan_tid(),
            crate::config::interlayer::MsgTypeId::of::<T>(),
        );
        Self(receiver, PhantomData)
    }
}
impl<T> Deref for SenderT<T> {
    type Target = Sender;
    fn deref(&self) -> &Sender {
        &self.0
    }
}
impl<T> DerefMut for SenderT<T> {
    fn deref_mut(&mut self) -> &mut Sender {
        &mut self.0
    }
}
impl<T> Deref for ReceiverT<T> {
    type Target = Receiver;
    fn deref(&self) -> &Receiver {
        &self.0
    }
}
impl<T> DerefMut for ReceiverT<T> {
    fn deref_mut(&mut self) -> &mut Receiver {
        &mut self.0
    }
}
impl<T: Send + Clone + 'static> SenderT<T> {
    pub async fn send(&self, envelope: Envelope<T>) -> Result<()> {
        self.0.send(envelope).await
    }
}
impl<T: Send + Clone + 'static> ReceiverT<T> {
    pub async fn recv(&self) -> Result<Envelope<T>> {
        self.0.recv_any().await.map(|mut item| {
            item.downcast_mut::<Envelope<T>>()
                .expect("type error when downcast")
                .take()
        })
    }
    pub async fn try_recv(&self, duration: Duration) -> Result<Option<Envelope<T>>> {
        self.0.try_recv::<T>(duration).await
    }
    pub async fn batch_recv(
        &self,
        n: usize,
        duration: Duration,
    ) -> std::result::Result<Vec<Envelope<T>>, BatchRecvError<Envelope<T>>> {
        self.0.batch_recv::<T>(n, duration).await
    }
}

impl<T: 'static> super::TypeInfo for SenderT<T> {
    fn port_tid(&self) -> crate::config::interlayer::MsgTypeId {
        crate::config::interlayer::MsgTypeId::of::<T>()
    }
    fn chan_tid(&self) -> crate::config::interlayer::MsgTypeId {
        self.0.chan_tid()
    }
}
impl<T: 'static> super::TypeInfo for ReceiverT<T> {
    fn port_tid(&self) -> crate::config::interlayer::MsgTypeId {
        crate::config::interlayer::MsgTypeId::of::<T>()
    }
    fn chan_tid(&self) -> crate::config::interlayer::MsgTypeId {
        self.0.chan_tid()
    }
}
