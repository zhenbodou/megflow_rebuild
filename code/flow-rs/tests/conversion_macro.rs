use flow_rs::prelude::*;
use flow_rs::{channel::channel_with_type, config::interlayer::MsgTypeId};

#[derive(Clone)]
struct Count(u32);
#[derive(Clone)]
struct Label(String);

#[add_cvt_func(_, _)]
fn label(Count(value): Count) -> Label {
    Label(format!("frame-{value}"))
}

#[tokio::test]
async fn attribute_registers_without_manual_initialization_and_preserves_metadata() {
    // 原函数仍可调用，参数模式仍然有效。
    assert_eq!(label(Count(3)).0, "frame-3");
    let (sender, receiver) = channel_with_type(1, MsgTypeId::of::<Label>());
    let sender: SenderT<Count> = sender.into();
    let receiver: ReceiverT<Label> = receiver.into();
    sender
        .send(Envelope::with_info(
            Count(7),
            EnvelopeInfo {
                partial_id: Some(42),
                weight: Some(3),
                ..Default::default()
            },
        ))
        .await
        .unwrap();
    let mut envelope = receiver.recv().await.unwrap();
    assert_eq!(envelope.info().partial_id, Some(42));
    assert_eq!(envelope.info().weight, Some(3));
    assert_eq!(envelope.unpack().0, "frame-7");
}

// 禁用的转换函数及其仅存在于该配置下的类型不应泄漏进登记代码。
#[add_cvt_func]
#[cfg(any())]
fn disabled(_: MissingInput) -> MissingOutput {
    unreachable!()
}

#[add_cvt_func]
#[cfg_attr(all(), cfg(any()))]
fn also_disabled(_: MissingInput) -> MissingOutput {
    unreachable!()
}

#[derive(Clone)]
struct Enabled(u32);
#[derive(Clone)]
struct Converted(u32);
#[add_cvt_func("ignored Rust source hint", "ignored Rust target hint")]
#[cfg_attr(all(), inline)]
fn enabled(value: Enabled) -> Converted {
    Converted(value.0 + 5)
}

#[tokio::test]
async fn function_only_attributes_do_not_leak_to_registration() {
    let (sender, receiver) = channel_with_type(1, MsgTypeId::of::<Converted>());
    let sender: SenderT<Enabled> = sender.into();
    sender.send(Envelope::new(Enabled(1))).await.unwrap();
    assert_eq!(receiver.recv::<Converted>().await.unwrap().unpack().0, 6);
}
