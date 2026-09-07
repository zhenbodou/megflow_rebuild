use flow_rs::{channel::channel, prelude::*};
use std::time::Duration;

#[tokio::test]
async fn cloned_receivers_deliver_each_message_exactly_once() {
    tokio::time::timeout(Duration::from_secs(3), async {
        for capacity in [0, 1, 16] {
            let (tx, rx) = channel(capacity);
            let mut workers = Vec::new();
            for _ in 0..4 {
                let rx = rx.clone();
                workers.push(tokio::spawn(async move {
                    let mut result = Vec::new();
                    while let Ok(mut message) = rx.recv::<u32>().await {
                        result.push(message.unpack());
                        tokio::task::yield_now().await;
                    }
                    result
                }));
            }
            drop(rx);
            for i in 0..1000u32 {
                tx.send(Envelope::new(i)).await.unwrap();
            }
            drop(tx);
            let mut all = Vec::new();
            for worker in workers {
                all.extend(worker.await.unwrap());
            }
            all.sort_unstable();
            assert_eq!(all, (0..1000).collect::<Vec<_>>());
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropping_one_receiver_keeps_channel_open_until_last_clone() {
    let (tx, rx) = channel(1);
    let other = rx.clone();
    drop(rx);
    assert!(!tx.is_closed());
    tx.send(Envelope::new(7u32)).await.unwrap();
    assert_eq!(other.recv::<u32>().await.unwrap().unpack(), 7);
    drop(other);
    assert!(tx.is_closed());
}

#[tokio::test]
async fn cancelled_receive_releases_shared_queue_for_another_consumer() {
    tokio::time::timeout(Duration::from_secs(2), async {
        let (tx, rx) = channel(1);
        // 显式 poll 到 Pending，证明被取消的 future 确实已等待接收。
        let mut pending = Box::pin(rx.recv::<u32>());
        assert!(futures_util::poll!(&mut pending).is_pending());
        let other = rx.clone();
        assert!(other
            .try_recv::<u32>(Duration::from_millis(10))
            .await
            .unwrap()
            .is_none());
        drop(pending);
        tx.send(Envelope::new(9u32)).await.unwrap();
        assert_eq!(other.recv::<u32>().await.unwrap().unpack(), 9);
    })
    .await
    .unwrap();
}
