use flow_rs::{
    channel::{channel, BatchRecvError},
    prelude::*,
};
use std::time::Duration;
const WAIT: Duration = Duration::from_secs(2);
#[tokio::test]
async fn threshold_uses_weights_including_zero_and_keeps_remainder() {
    let (tx, mut rx) = channel(0);
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
    let (tx, mut rx) = channel(0);
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
    let (tx, mut rx) = channel(0);
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
