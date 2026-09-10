//! 动态图通知广播；run 消费当前订阅快照，每个订阅者有独立队列。
use crate::{
    envelope::{Envelope, SealedEnvelope},
    error::{Error, Result},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::{sync::Notify, task::JoinHandle};

#[derive(Default)]
struct State {
    items: VecDeque<SealedEnvelope>,
    closed: bool,
    senders: usize,
}
#[derive(Default)]
struct Mailbox {
    state: Mutex<State>,
    ready: Notify,
}
struct Tx(Arc<Mailbox>);
struct Rx(Arc<Mailbox>);
fn queue() -> (Tx, Rx) {
    let mailbox = Arc::new(Mailbox::default());
    mailbox.state.lock().unwrap().senders = 1;
    (Tx(mailbox.clone()), Rx(mailbox))
}
impl Mailbox {
    fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.ready.notify_waiters();
    }
}
impl Clone for Tx {
    fn clone(&self) -> Self {
        self.0.state.lock().unwrap().senders += 1;
        Self(self.0.clone())
    }
}
impl Drop for Tx {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.senders -= 1;
        if state.senders == 0 {
            state.closed = true;
        }
        drop(state);
        self.0.ready.notify_waiters();
    }
}
impl Drop for Rx {
    fn drop(&mut self) {
        self.0.close();
    }
}
impl Tx {
    fn send(&self, message: SealedEnvelope) {
        let mut state = self.0.state.lock().unwrap();
        if state.closed {
            return;
        }
        state.items.push_back(message);
        drop(state);
        self.0.ready.notify_one();
    }
}
impl Rx {
    fn try_recv(&self) -> Option<SealedEnvelope> {
        self.0.state.lock().unwrap().items.pop_front()
    }
    async fn recv(&self) -> Result<SealedEnvelope> {
        loop {
            let ready = self.0.ready.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            {
                let mut state = self.0.state.lock().unwrap();
                if let Some(item) = state.items.pop_front() {
                    return Ok(item);
                }
                if state.closed {
                    return Err(Error::ChannelClosed);
                }
            }
            ready.await;
        }
    }
}

#[derive(Default)]
pub struct Broker {
    subs: HashMap<String, (Tx, Rx, Vec<Tx>)>,
}
pub struct BrokerClient {
    notify: Tx,
    sub: Rx,
    topic: String,
}
impl Broker {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn subscribe(&mut self, topic: String) -> BrokerClient {
        let (notify, _, subscribers) = self.subs.entry(topic.clone()).or_insert_with(|| {
            let (tx, rx) = queue();
            (tx, rx, Vec::new())
        });
        let (tx, rx) = queue();
        subscribers.push(tx);
        BrokerClient {
            notify: notify.clone(),
            sub: rx,
            topic,
        }
    }
    pub fn run(&mut self) -> JoinHandle<Result<()>> {
        let topics = std::mem::take(&mut self.subs);
        tokio::spawn(async move {
            let mut tasks = Vec::new();
            for (_, (publisher, receiver, subscribers)) in topics {
                drop(publisher);
                tasks.push(tokio::spawn(async move {
                    while let Ok(message) = receiver.recv().await {
                        for subscriber in &subscribers {
                            subscriber.send(message.clone());
                        }
                    }
                }));
            }
            for task in tasks {
                task.await.expect("broker topic task failed");
            }
            Ok(())
        })
    }
}
impl BrokerClient {
    pub async fn publish<T: Clone + Send + 'static>(&self, message: T) {
        self.notify.send(Envelope::new(message).seal());
    }
    pub async fn fetch<T: Clone + Send + 'static>(&self) -> Result<T> {
        let mut message = self.sub.recv().await?;
        Ok(message
            .downcast_mut::<Envelope<T>>()
            .expect("type error when downcast in broker")
            .unpack())
    }
    pub fn try_fetch<T: Clone + Send + 'static>(&self) -> Option<T> {
        self.sub.try_recv().map(|mut message| {
            message
                .downcast_mut::<Envelope<T>>()
                .expect("type error when downcast in broker")
                .unpack()
        })
    }
    pub fn topic(&self) -> &str {
        &self.topic
    }
    pub fn is_closed(&self) -> bool {
        self.notify.0.state.lock().unwrap().closed
    }
    pub fn close(&self) {
        self.notify.0.close();
        self.sub.0.close();
    }
}
