use flow_rs::{broker::Broker, error::Error};
use std::time::Duration;

#[tokio::test]
async fn pending_fetches_wake_and_cancelling_one_does_not_lose_messages() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut broker = Broker::new();
        let client = broker.subscribe("wake".into());
        let task = broker.run();
        {
            let cancelled = client.fetch::<u32>();
            tokio::pin!(cancelled);
            assert!(futures_util::poll!(&mut cancelled).is_pending());
        }
        let first = client.fetch::<u32>();
        let second = client.fetch::<u32>();
        tokio::pin!(first, second);
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        client.publish(11u32).await;
        client.publish(12u32).await;
        let (a, b) = tokio::join!(first, second);
        let mut values = [a.unwrap(), b.unwrap()];
        values.sort();
        assert_eq!(values, [11, 12]);
        let waiting = client.fetch::<u32>();
        tokio::pin!(waiting);
        assert!(futures_util::poll!(&mut waiting).is_pending());
        client.close();
        assert!(matches!(waiting.await, Err(Error::ChannelClosed)));
        task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn closed_subscription_drains_buffer_and_rejects_later_publications() {
    let mut broker = Broker::new();
    let client = broker.subscribe("drain".into());
    let task = broker.run();
    client.publish(21u32).await;
    client.publish(22u32).await;
    // 收到第一条说明主题任务已在同一次无 await 的广播循环中处理完排队消息。
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), client.fetch::<u32>())
            .await
            .unwrap()
            .unwrap(),
        21
    );
    client.close();
    assert_eq!(client.try_fetch::<u32>(), Some(22));
    client.publish(23u32).await;
    assert!(matches!(
        client.fetch::<u32>().await,
        Err(Error::ChannelClosed)
    ));
    assert_eq!(client.try_fetch::<u32>(), None);
    task.await.unwrap().unwrap();
}
#[tokio::test]
async fn every_subscriber_gets_its_own_copy_and_close_is_shared() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut broker = Broker::new();
        let a = broker.subscribe("graph".into());
        let b = broker.subscribe("graph".into());
        let other = broker.subscribe("other".into());
        assert_eq!(a.topic(), "graph");
        assert_eq!(a.try_fetch::<u32>(), None);
        a.publish(1u32).await; // run 前发布不丢失
        let task = broker.run();
        assert_eq!(a.fetch::<u32>().await.unwrap(), 1);
        assert_eq!(b.fetch::<u32>().await.unwrap(), 1);
        assert_eq!(other.try_fetch::<u32>(), None);
        b.publish(2u32).await;
        a.close(); // 关闭主题发布通道；排队通知仍会被广播
        assert!(b.is_closed());
        assert_eq!(b.fetch::<u32>().await.unwrap(), 2);
        assert!(matches!(b.fetch::<u32>().await, Err(Error::ChannelClosed)));
        other.close();
        task.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn run_consumes_snapshot_and_drop_of_last_client_finishes_topic() {
    let mut broker = Broker::new();
    let old = broker.subscribe("same".into());
    let first = broker.run();
    let new = broker.subscribe("same".into());
    let second = broker.run();
    old.close();
    assert!(!new.is_closed());
    new.publish(3u32).await;
    assert_eq!(new.fetch::<u32>().await.unwrap(), 3);
    drop(new);
    tokio::time::timeout(Duration::from_secs(3), async {
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}
