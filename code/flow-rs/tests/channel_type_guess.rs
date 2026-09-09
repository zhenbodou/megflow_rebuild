use flow_rs::{
    channel::{add_cvt_func_impl, guess_channel_type},
    config::interlayer::MsgTypeId,
    error::Error,
};
use std::collections::HashSet;
fn set(items: &[MsgTypeId]) -> HashSet<MsgTypeId> {
    items.iter().copied().collect()
}

#[test]
fn abstract_types_and_empty_sets_follow_original_rules() {
    use MsgTypeId::{Any, Template};
    assert!(matches!(
        guess_channel_type(&set(&[]), &set(&[])),
        Err(Error::TemplateInferFault)
    ));
    assert!(matches!(
        guess_channel_type(&set(&[Template(1)]), &set(&[Template(2)])),
        Err(Error::TemplateInferFault)
    ));
    assert_eq!(
        guess_channel_type(&set(&[Any]), &set(&[Template(1)])).unwrap(),
        Any
    );
    let concrete = MsgTypeId::of::<u64>();
    assert_eq!(
        guess_channel_type(&set(&[Any]), &set(&[concrete])).unwrap(),
        concrete
    );
}

#[test]
fn candidate_requires_both_incoming_and_outgoing_direct_conversions() {
    struct A;
    struct B;
    struct C;
    let (a, b, c) = (
        MsgTypeId::of::<A>(),
        MsgTypeId::of::<B>(),
        MsgTypeId::of::<C>(),
    );
    assert!(matches!(
        guess_channel_type(&set(&[a]), &set(&[c])),
        Err(Error::ChannelTypeMismatch)
    ));
    // 这里只登记类型边，不执行载荷转换；不会自动沿 A→B→C 搜索路径。
    add_cvt_func_impl(a, b, |message| message);
    add_cvt_func_impl(b, c, |message| message);
    assert!(matches!(
        guess_channel_type(&set(&[a]), &set(&[c])),
        Err(Error::ChannelTypeMismatch)
    ));
    // B 出现在端口集合后才会成为候选，且是唯一能承接两侧的类型。
    assert_eq!(guess_channel_type(&set(&[a, b]), &set(&[b, c])).unwrap(), b);
    assert!(matches!(
        guess_channel_type(&set(&[c]), &set(&[a])),
        Err(Error::ChannelTypeMismatch)
    ));
}

#[derive(Clone)]
struct Raw(u32);
#[derive(Clone)]
struct Stored(u32);
#[derive(Clone)]
struct Rendered(String);

#[tokio::test]
async fn inferred_type_drives_both_endpoint_conversions() {
    use flow_message::Envelope;
    use flow_rs::channel::{channel_with_type, ReceiverT, SenderT};
    let (raw, stored, rendered) = (
        MsgTypeId::of::<Raw>(),
        MsgTypeId::of::<Stored>(),
        MsgTypeId::of::<Rendered>(),
    );
    add_cvt_func_impl(raw, stored, |mut message| {
        let envelope = message.downcast_mut::<Envelope<Raw>>().unwrap();
        let value = envelope.unpack().0;
        envelope.repack(Stored(value + 1)).seal()
    });
    add_cvt_func_impl(stored, rendered, |mut message| {
        let envelope = message.downcast_mut::<Envelope<Stored>>().unwrap();
        let value = envelope.unpack().0;
        envelope.repack(Rendered(value.to_string())).seal()
    });
    let selected = guess_channel_type(&set(&[raw, stored]), &set(&[stored, rendered])).unwrap();
    assert_eq!(selected, stored);
    let (sender, receiver) = channel_with_type(1, selected);
    let sender: SenderT<Raw> = sender.into();
    let receiver: ReceiverT<Rendered> = receiver.into();
    sender.send(Envelope::new(Raw(9))).await.unwrap();
    assert_eq!(receiver.recv().await.unwrap().unpack().0, "10");
}
