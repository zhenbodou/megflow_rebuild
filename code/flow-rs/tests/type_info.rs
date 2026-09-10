use flow_rs::channel::{channel, channel_with_type, ReceiverT, SenderT, TypeInfo};
use flow_rs::config::interlayer::{MsgType, MsgTypeId};

// ANCHOR: wrapper_test
#[test]
fn wrapper_type_does_not_relabel_channel() {
    for capacity in [0, 1] {
        let (sender, receiver) = channel_with_type(capacity, MsgTypeId::of::<String>());
        assert_eq!(sender.port_tid(), MsgTypeId::Any);
        assert_eq!(receiver.chan_tid(), MsgTypeId::of::<String>());
        let sender: SenderT<u32> = sender.into();
        let receiver: ReceiverT<u32> = receiver.into();
        assert_eq!(sender.port_tid(), MsgTypeId::of::<u32>());
        assert_eq!(receiver.port_tid(), MsgTypeId::of::<u32>());
        assert_eq!(sender.clone().chan_tid(), MsgTypeId::of::<String>());
        assert_eq!(receiver.clone().chan_tid(), MsgTypeId::of::<String>());
    }
}
// ANCHOR_END: wrapper_test

#[test]
fn unconnected_and_untyped_channels_default_to_any() {
    let sender = SenderT::<u32>::default();
    let receiver = ReceiverT::<String>::default();
    assert_eq!(sender.port_tid(), MsgTypeId::of::<u32>());
    assert_eq!(receiver.port_tid(), MsgTypeId::of::<String>());
    assert_eq!(sender.chan_tid(), MsgTypeId::Any);
    assert_eq!(receiver.chan_tid(), MsgTypeId::Any);
    let (sender, receiver) = channel(1);
    assert_eq!(sender.chan_tid(), MsgTypeId::Any);
    assert_eq!(receiver.port_tid(), MsgTypeId::Any);
}

#[test]
fn descriptions_preserve_original_identity_rules() {
    assert_eq!(MsgType::any().name, "Any");
    assert_eq!(MsgType::template(3).name, "T3");
    assert_eq!(MsgType::template(3).id, MsgTypeId::Template(3));
    assert_eq!(MsgType::of::<u32>().id, MsgTypeId::of::<u32>());
    assert_eq!(MsgType::python(" Frame ").name, "Frame");
    assert_eq!(MsgTypeId::python(" Frame "), MsgTypeId::python("Frame"));
    assert_ne!(MsgTypeId::of::<u32>(), MsgTypeId::of::<i32>());
}
