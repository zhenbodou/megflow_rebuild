use flow_rs::prelude::*;
use std::time::Duration;
#[tokio::test]
async fn typed_methods_infer_payload_and_preserve_batch_contract() {
    let (sender, receiver) = channel(0);
    let sender: SenderT<u32> = sender.into();
    let receiver: ReceiverT<u32> = receiver.into();
    sender.send(Envelope::new(7)).await.unwrap();
    assert_eq!(receiver.recv().await.unwrap().unpack(), 7);
    assert!(receiver
        .try_recv(Duration::from_millis(10))
        .await
        .unwrap()
        .is_none());
    sender.send(Envelope::new(8)).await.unwrap();
    drop(sender);
    let Err(BatchRecvError::Closed(mut batch)) =
        receiver.batch_recv(2, Duration::from_secs(1)).await
    else {
        panic!("部分批次应随关闭错误返回")
    };
    assert_eq!(batch.len(), 1);
    assert_eq!(batch.remove(0).unpack(), 8);
}
#[tokio::test]
async fn typed_clones_share_queue_and_expose_untyped_operations() {
    let (sender, receiver) = channel(0);
    let sender: SenderT<u32> = sender.into();
    let receiver: ReceiverT<u32> = receiver.into();
    let other = receiver.clone();
    sender
        .clone()
        .send_any(Envelope::new(3u32).seal())
        .await
        .unwrap();
    assert_eq!(other.recv().await.unwrap().unpack(), 3);
    drop(sender);
    assert!(matches!(receiver.recv().await, Err(Error::ChannelClosed)));
}
#[tokio::test]
#[should_panic(expected = "type error when downcast")]
async fn typed_endpoint_does_not_silently_convert_wrong_payload() {
    let (sender, receiver) = channel(0);
    sender
        .send(Envelope::new(String::from("wrong type")))
        .await
        .unwrap();
    let receiver: ReceiverT<u32> = receiver.into();
    let _ = receiver.recv().await;
}

#[tokio::test]
async fn unconnected_default_endpoints_match_original_asymmetry() {
    let sender = Sender::default();
    let receiver = Receiver::default();
    assert!(sender.is_none());
    assert!(sender.is_closed());
    assert!(receiver.is_none());
    // 原版未接线发送端吞掉消息并返回成功，而已接线后关闭会报错。
    sender.send(Envelope::new(1u32)).await.unwrap();
    assert!(matches!(
        receiver.recv_any().await,
        Err(Error::ChannelClosed)
    ));
    let (connected, rx) = channel(1);
    drop(rx);
    assert!(!connected.is_none());
    assert!(matches!(
        connected.send(Envelope::new(1u32)).await,
        Err(Error::ChannelClosed)
    ));
    let sender: SenderT<u32> = Default::default();
    let receiver: ReceiverT<u32> = Default::default();
    assert!(sender.is_none() && receiver.is_none());
    sender.send(Envelope::new(2)).await.unwrap();
    assert!(matches!(receiver.recv().await, Err(Error::ChannelClosed)));
    struct NoDefault;
    let _: SenderT<NoDefault> = Default::default();
    let _: ReceiverT<NoDefault> = Default::default();
}
