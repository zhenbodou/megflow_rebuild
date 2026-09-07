use flow_message::{str2addr, Envelope, EnvelopeInfo};
use std::sync::Arc;

fn metadata() -> EnvelopeInfo {
    EnvelopeInfo {
        skipped: true,
        weight: Some(0),
        partial_id: Some(7),
        from_addr: Some(11),
        to_addr: Some(12),
        transfer_addr: Some(13),
        extra_data: Some(Arc::new(String::from("frame-context"))),
    }
}

fn assert_same_metadata(actual: &EnvelopeInfo, expected: &EnvelopeInfo) {
    assert_eq!(actual.skipped, expected.skipped);
    assert_eq!(actual.weight, expected.weight);
    assert_eq!(actual.partial_id, expected.partial_id);
    assert_eq!(actual.from_addr, expected.from_addr);
    assert_eq!(actual.to_addr, expected.to_addr);
    assert_eq!(actual.transfer_addr, expected.transfer_addr);
    assert!(Arc::ptr_eq(
        actual.extra_data.as_ref().unwrap(),
        expected.extra_data.as_ref().unwrap()
    ));
}

#[test]
fn all_seven_metadata_defaults_match_original() {
    let info = EnvelopeInfo::default();
    assert!(!info.skipped);
    assert_eq!(info.weight, None);
    assert_eq!(info.partial_id, None);
    assert_eq!(info.from_addr, None);
    assert_eq!(info.to_addr, None);
    assert_eq!(info.transfer_addr, None);
    assert!(info.extra_data.is_none());
}

#[test]
fn clone_unpack_repack_take_and_seal_preserve_all_metadata() {
    let info = metadata();
    let mut source = Envelope::with_info(vec![1, 2], info.clone());
    let mut cloned = source.clone();
    cloned.get_mut().push(3);
    assert_eq!(source.get_ref(), &[1, 2]);
    assert_eq!(cloned.get_ref(), &[1, 2, 3]);
    assert_same_metadata(cloned.info(), &info);
    assert_eq!(source.unpack(), [1, 2]);
    assert!(source.is_none());
    assert_same_metadata(source.info(), &info);
    let mut repacked = source.repack(String::from("result"));
    assert_same_metadata(repacked.info(), &info);
    repacked.repack_inplace(String::from("replacement"));
    let moved = repacked.take();
    assert!(repacked.is_none());
    assert_eq!(moved.get_ref(), "replacement");
    assert_same_metadata(repacked.info(), &info);
    assert_same_metadata(moved.info(), &info);
    let mut sealed = moved.seal().clone();
    assert_same_metadata(sealed.info(), &info);
    sealed.info_mut().to_addr = Some(99);
    assert_eq!(repacked.info().to_addr, Some(12));
    assert_eq!(
        sealed.downcast_mut::<Envelope<String>>().unwrap().unpack(),
        "replacement"
    );
}

#[test]
fn empty_typed_envelope_still_has_metadata() {
    let mut empty = Envelope::<String>::empty();
    *empty.info_mut() = metadata();
    let mut sealed = empty.seal();
    assert!(sealed.is_none());
    assert_eq!(sealed.info().to_addr, Some(12));
    sealed
        .downcast_mut::<Envelope<String>>()
        .unwrap()
        .repack_inplace("later".into());
    assert!(sealed.is_some());
    assert_eq!(sealed.info().weight, Some(0));
}

#[test]
fn addresses_parse_numbers_or_hash_the_entire_original_string() {
    assert_eq!(str2addr("0"), 0);
    assert_eq!(str2addr("00042"), 42);
    assert_eq!(str2addr("18446744073709551615"), u64::MAX);
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    for name in ["camera/front", "入口", " 42", "18446744073709551616", ""] {
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        assert_eq!(str2addr(name), hasher.finish());
    }
}
