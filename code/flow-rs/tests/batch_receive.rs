use flow_rs::{
    channel::{channel, BatchRecvError},
    prelude::*,
};
use std::time::Duration;
const WAIT: Duration = Duration::from_secs(2);
#[tokio::test]
async fn threshold_uses_weights_including_zero_and_keeps_remainder() {
    let (tx, rx) = channel(0);
    for (value, weight) in [(0, Some(0)), (1, None), (2, Some(4)), (3, None)] {
        tx.send(Envelope::with_info(
            value,
            EnvelopeInfo {
                weight,
                ..Default::default()
            },
        ))
        .await
        .unwrap();
    }
    let batch = rx.batch_recv::<i32>(3, WAIT).await.unwrap();
    assert_eq!(
        batch
            .into_iter()
            .map(|mut e| e.unpack())
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(rx.recv::<i32>().await.unwrap().unpack(), 3);
}
#[tokio::test]
async fn close_returns_partial_batch_and_zero_does_not_consume() {
    let (tx, rx) = channel(0);
    tx.send(Envelope::new(7u32)).await.unwrap();
    drop(tx);
    assert!(rx.batch_recv::<u32>(0, WAIT).await.unwrap().is_empty());
    let Err(BatchRecvError::Closed(items)) = rx.batch_recv::<u32>(3, WAIT).await else {
        panic!("提前关闭应返回部分错误")
    };
    assert_eq!(
        items
            .into_iter()
            .map(|mut e| e.unpack())
            .collect::<Vec<_>>(),
        [7]
    );
}
#[tokio::test]
async fn timeout_is_partial_success_and_receiver_remains_usable() {
    let (tx, rx) = channel(0);
    tx.send(Envelope::new(7u32)).await.unwrap();
    let batch = rx
        .batch_recv::<u32>(2, Duration::from_millis(20))
        .await
        .unwrap();
    assert_eq!(
        batch
            .into_iter()
            .map(|mut e| e.unpack())
            .collect::<Vec<_>>(),
        [7]
    );
    tx.send(Envelope::new(8u32)).await.unwrap();
    assert_eq!(rx.recv::<u32>().await.unwrap().unpack(), 8);
}

// 原版函数引用 crate::rt::time；适配到相同的 Tokio 时间源，不改原函数文本。
mod rt {
    pub mod time {
        pub use tokio::time::sleep;
    }
}
mod reference {
    use super::*;
    use flow_message::SealedEnvelope;
    use futures_util::{pin_mut, select, stream::FuturesUnordered, FutureExt, StreamExt};
    use std::result::Result;
    use std::{cell::RefCell, collections::VecDeque};
    pub struct ReferenceReceiver(RefCell<VecDeque<SealedEnvelope>>);
    impl ReferenceReceiver {
        pub fn new(messages: Vec<Envelope<u32>>) -> Self {
            Self(RefCell::new(
                messages.into_iter().map(Envelope::seal).collect(),
            ))
        }
        async fn recv_any(&self) -> std::result::Result<SealedEnvelope, ()> {
            self.0.borrow_mut().pop_front().ok_or(())
        }
        pub fn remaining(&self) -> usize {
            self.0.borrow().len()
        }
    }
    include!("reference/batch_recv_any.rs");
}

fn batch_observation(
    result: std::result::Result<
        Vec<flow_message::SealedEnvelope>,
        BatchRecvError<flow_message::SealedEnvelope>,
    >,
) -> (bool, Vec<(u32, Option<usize>)>) {
    let (success, items) = match result {
        Ok(items) => (true, items),
        Err(BatchRecvError::Closed(items)) => (false, items),
    };
    (
        success,
        items
            .into_iter()
            .map(|mut item| {
                let envelope = item.downcast_mut::<Envelope<u32>>().unwrap();
                (envelope.unpack(), envelope.info().weight)
            })
            .collect(),
    )
}

#[tokio::test]
async fn weighted_batches_match_original_method_exhaustively() {
    tokio::time::timeout(Duration::from_secs(10), async {
        // 6 条消息，每条取 None、Some(0)、Some(3)，共 729 种权重序列。
        for pattern in 0..729usize {
            let mut digits = pattern;
            let messages: Vec<_> = (0..6u32)
                .map(|id| {
                    let weight = [None, Some(0), Some(3)][digits % 3];
                    digits /= 3;
                    Envelope::with_info(
                        id,
                        EnvelopeInfo {
                            weight,
                            ..Default::default()
                        },
                    )
                })
                .collect();
            for threshold in 0..=7 {
                let reference = reference::ReferenceReceiver::new(messages.clone());
                let (tx, rx) = channel(0);
                for envelope in messages.clone() {
                    tx.send(envelope).await.unwrap();
                }
                drop(tx);
                let expected = batch_observation(reference.batch_recv_any(threshold, WAIT).await);
                let actual = batch_observation(rx.batch_recv_any(threshold, WAIT).await);
                assert_eq!(actual, expected, "pattern={pattern}, threshold={threshold}");
                let mut remaining = 0;
                while rx.recv_any().await.is_ok() {
                    remaining += 1;
                }
                assert_eq!(remaining, reference.remaining());
            }
        }
    })
    .await
    .unwrap();
}
