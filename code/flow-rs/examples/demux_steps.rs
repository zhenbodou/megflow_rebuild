use flow_rs::prelude::*;
use std::collections::HashMap;

// 先验证原版静态 Demux 的单消息业务步骤；字典端口装配另行接入。
async fn route(message: SealedEnvelope, outputs: &HashMap<u64, Sender>) {
    let address = message
        .info()
        .to_addr
        .expect("the envelope has no destination address");
    if let Some(output) = outputs.get(&address) {
        output.send_any(message).await.ok();
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (camera_a, receiver_a) = channel(1);
    let (camera_b, receiver_b) = channel(1);
    let outputs = HashMap::from([(7, camera_a), (42, camera_b)]);
    let message = Envelope::with_info(
        String::from("frame"),
        EnvelopeInfo {
            to_addr: Some(42),
            partial_id: Some(3),
            ..Default::default()
        },
    );
    route(message.seal(), &outputs).await;
    let mut received = receiver_b.recv::<String>().await.unwrap();
    assert_eq!(received.info().partial_id, Some(3));
    assert_eq!(received.info().to_addr, Some(42));
    assert_eq!(received.unpack(), "frame");

    // 未知地址丢弃，不广播，也不退回任意一个端口。
    route(
        Envelope::with_info(
            1u32,
            EnvelopeInfo {
                to_addr: Some(99),
                ..Default::default()
            },
        )
        .seal(),
        &outputs,
    )
    .await;
    // 静态 Demux 仍转发带地址的空载荷；动态 Demux 才有另一个生命周期协议。
    let mut empty = Envelope::<u32>::empty();
    empty.info_mut().to_addr = Some(7);
    route(empty.seal(), &outputs).await;
    assert!(receiver_a.recv::<u32>().await.unwrap().is_none());
    // 目标关闭后忽略发送错误，调用仍然结束。
    drop(receiver_b);
    route(
        Envelope::with_info(
            2u32,
            EnvelopeInfo {
                to_addr: Some(42),
                ..Default::default()
            },
        )
        .seal(),
        &outputs,
    )
    .await;
    drop(outputs);
    assert!(matches!(
        receiver_a.recv_any().await,
        Err(Error::ChannelClosed)
    ));
    println!("按地址单路转发；未知地址丢弃；空载荷保留；关闭目标的发送错误被忽略。");
}
