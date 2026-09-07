use flow_rs::{channel::channel, prelude::*};
use std::time::Duration;
#[tokio::test]
async fn timeout_does_not_close_or_consume_future_messages() {
    tokio::time::timeout(Duration::from_secs(2), async {
        for capacity in [0, 1] {
            let (tx, mut rx) = channel(capacity);
            assert!(rx
                .try_recv::<u32>(Duration::from_millis(10))
                .await
                .unwrap()
                .is_none());
            tx.send(Envelope::new(42u32)).await.unwrap();
            assert_eq!(
                rx.try_recv::<u32>(Duration::from_secs(1))
                    .await
                    .unwrap()
                    .unwrap()
                    .unpack(),
                42
            );
            drop(tx);
            assert!(matches!(
                rx.try_recv_any(Duration::from_secs(1)).await,
                Err(Error::ChannelClosed)
            ));
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn empty_payload_is_a_received_message_and_metadata_survives() {
    let (tx, mut rx) = channel(0);
    let mut envelope = Envelope::<u32>::empty();
    envelope.info_mut().partial_id = Some(8);
    tx.send(envelope).await.unwrap();
    drop(tx);
    let envelope = rx
        .try_recv::<u32>(Duration::from_secs(1))
        .await
        .unwrap()
        .expect("有消息，载荷为空不等于超时");
    assert!(envelope.is_none());
    assert_eq!(envelope.info().partial_id, Some(8));
}
