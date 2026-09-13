use flow_message::Envelope;
use flow_rs::{channel::channel, error::Error};
use std::time::Duration;

#[tokio::test]
async fn close_from_either_side_rejects_sends_but_drains_queue() {
    for capacity in [0, 1] {
        for close_sender in [false, true] {
            let (tx, rx) = channel(capacity);
            let tx_clone = tx.clone();
            let rx_clone = rx.clone();
            tx.send(Envelope::new(7)).await.unwrap();
            if close_sender {
                tx.close();
            } else {
                rx.close();
            }
            rx.close(); // repeated close is harmless
            assert!(tx_clone.is_closed());
            assert!(matches!(
                tx_clone.send(Envelope::new(8)).await,
                Err(Error::ChannelClosed)
            ));
            assert_eq!(rx_clone.recv::<i32>().await.unwrap().unpack(), 7);
            assert!(matches!(rx.recv::<i32>().await, Err(Error::ChannelClosed)));
        }
    }
}

#[tokio::test]
async fn close_wakes_blocked_sender_without_dropping_receivers() {
    let (tx, rx) = channel(1);
    tx.send(Envelope::new(1)).await.unwrap();
    let pending = tx.send(Envelope::new(2));
    tokio::pin!(pending);
    assert!(futures_util::poll!(&mut pending).is_pending());
    rx.close();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap(),
        Err(Error::ChannelClosed)
    ));
    assert_eq!(rx.recv::<i32>().await.unwrap().unpack(), 1);
}

#[tokio::test]
async fn close_wakes_all_competing_receivers_with_senders_alive() {
    for capacity in [0, 1] {
        let (tx, rx) = channel(capacity);
        let other = rx.clone();
        let first = rx.recv::<i32>();
        let second = other.recv::<i32>();
        tokio::pin!(first, second);
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        tx.close();
        for result in [
            tokio::time::timeout(Duration::from_secs(1), first)
                .await
                .unwrap(),
            tokio::time::timeout(Duration::from_secs(1), second)
                .await
                .unwrap(),
        ] {
            assert!(matches!(result, Err(Error::ChannelClosed)));
        }
    }
}
