use flow_rs::{channel::channel, prelude::*};
#[tokio::test]
async fn zero_capacity_queues_before_consumption_and_drains_on_close() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let (tx, mut rx) = channel(0);
        let other = tx.clone();
        for i in 0..1000u32 {
            other.send(Envelope::new(i)).await.unwrap();
        }
        drop(other);
        drop(tx);
        for i in 0..1000u32 {
            assert_eq!(rx.recv::<u32>().await.unwrap().unpack(), i);
        }
        assert!(matches!(rx.recv::<u32>().await, Err(Error::ChannelClosed)));
        let (tx, rx) = channel(0);
        drop(rx);
        assert!(tx.is_closed());
        assert!(matches!(
            tx.send(Envelope::new(1u32)).await,
            Err(Error::ChannelClosed)
        ));
    })
    .await
    .expect("无界发送不应等待消费者");
}
