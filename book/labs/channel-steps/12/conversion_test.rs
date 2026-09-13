use flow_message::{Envelope, EnvelopeInfo, SealedEnvelope};
use flow_rs::{
    channel::{add_cvt_func_impl, channel_with_type, ReceiverT, SenderT},
    config::interlayer::MsgTypeId,
};
#[derive(Clone)]
struct Input(u32);
#[derive(Clone)]
struct Wire(u32);
#[derive(Clone)]
struct Output(u32);
fn send_conversion(mut msg: SealedEnvelope) -> SealedEnvelope {
    let mut msg = msg.downcast_mut::<Envelope<Input>>().unwrap().take();
    let value = msg.unpack().0;
    msg.repack(Wire(value + 1)).seal()
}
fn recv_conversion(mut msg: SealedEnvelope) -> SealedEnvelope {
    let mut msg = msg.downcast_mut::<Envelope<Wire>>().unwrap().take();
    let value = msg.unpack().0;
    msg.repack(Output(value * 2)).seal()
}
#[tokio::test]
async fn conversion_runs_on_both_sides_and_preserves_context() {
    add_cvt_func_impl(
        MsgTypeId::of::<Input>(),
        MsgTypeId::of::<Wire>(),
        send_conversion,
    );
    add_cvt_func_impl(
        MsgTypeId::of::<Wire>(),
        MsgTypeId::of::<Output>(),
        recv_conversion,
    );
    for capacity in [0, 1] {
        let (sender, receiver) = channel_with_type(capacity, MsgTypeId::of::<Wire>());
        let sender: SenderT<Input> = sender.into();
        let receiver: ReceiverT<Output> = receiver.into();
        sender
            .send(Envelope::with_info(
                Input(10),
                EnvelopeInfo {
                    partial_id: Some(42),
                    ..Default::default()
                },
            ))
            .await
            .unwrap();
        let mut output = receiver.recv().await.unwrap();
        assert_eq!(output.info().partial_id, Some(42));
        assert_eq!(output.unpack().0, 22);
    }
}

#[derive(Clone)]
struct SlowInput(std::sync::Arc<SlowGate>);
struct SlowGate {
    started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    finished: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
#[derive(Clone)]
struct SlowOutput;
fn slow_conversion(mut msg: SealedEnvelope) -> SealedEnvelope {
    let input = msg.downcast_mut::<Envelope<SlowInput>>().unwrap().unpack();
    input
        .0
        .started
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .send(())
        .unwrap();
    // 超时仅作为失败保险；正常路径由测试显式释放，避免依赖 sleep 猜测调度。
    input
        .0
        .release
        .lock()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    input
        .0
        .finished
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .send(())
        .unwrap();
    Envelope::new(SlowOutput).seal()
}

#[tokio::test]
async fn cancelling_receive_does_not_restore_message_already_in_conversion() {
    add_cvt_func_impl(
        MsgTypeId::of::<SlowInput>(),
        MsgTypeId::of::<SlowOutput>(),
        slow_conversion,
    );
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let gate = std::sync::Arc::new(SlowGate {
        started: std::sync::Mutex::new(Some(started_tx)),
        release: std::sync::Mutex::new(release_rx),
        finished: std::sync::Mutex::new(Some(finished_tx)),
    });
    let (sender, receiver) = channel_with_type(1, MsgTypeId::of::<SlowInput>());
    sender.send(Envelope::new(SlowInput(gate))).await.unwrap();
    drop(sender);
    let receiver: ReceiverT<SlowOutput> = receiver.into();
    let task_receiver = receiver.clone();
    let task = tokio::spawn(async move { task_receiver.recv().await });
    tokio::time::timeout(std::time::Duration::from_secs(2), started_rx)
        .await
        .unwrap()
        .unwrap();
    // 已收到 started：消息确实出队且转换已开始，此时取消等待。
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    release_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), finished_rx)
        .await
        .unwrap()
        .unwrap();
    // 转换继续执行，但结果不回到原队列；下一次接收只能看到关闭。
    assert!(matches!(
        receiver.recv().await,
        Err(flow_rs::error::Error::ChannelClosed)
    ));
}

#[derive(Clone)]
struct PanicInput;
#[derive(Clone)]
struct PanicOutput;
fn panic_conversion(_: SealedEnvelope) -> SealedEnvelope {
    panic!("conversion failure fixture")
}

#[tokio::test]
async fn converter_panic_propagates_as_task_panic_on_send_and_receive() {
    add_cvt_func_impl(
        MsgTypeId::of::<PanicInput>(),
        MsgTypeId::of::<PanicOutput>(),
        panic_conversion,
    );
    // 发送侧：阻塞转换失败，消息未进入队列，外层任务 panic。
    let (sender, receiver) = channel_with_type(1, MsgTypeId::of::<PanicOutput>());
    let sender: SenderT<PanicInput> = sender.into();
    let task = tokio::spawn(async move { sender.send(Envelope::new(PanicInput)).await });
    assert!(matches!(task.await, Err(error) if error.is_panic()));
    assert!(matches!(
        receiver.recv_any().await,
        Err(flow_rs::error::Error::ChannelClosed)
    ));

    // 接收侧：先出队再转换；失败后这条消息不会再次出现。
    let (sender, receiver) = channel_with_type(1, MsgTypeId::of::<PanicInput>());
    sender.send(Envelope::new(PanicInput)).await.unwrap();
    drop(sender);
    let receiver: ReceiverT<PanicOutput> = receiver.into();
    let task_receiver = receiver.clone();
    let task = tokio::spawn(async move { task_receiver.recv().await });
    assert!(matches!(task.await, Err(error) if error.is_panic()));
    assert!(matches!(
        receiver.recv().await,
        Err(flow_rs::error::Error::ChannelClosed)
    ));
}
