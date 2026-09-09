use flow_message::dr::{Dr, SyncWith};
#[derive(Clone)]
struct Value(u32);
impl SyncWith<&mut Vec<u32>> for Value {
    fn sync_with(&mut self, output: &mut Vec<u32>) {
        output.push(self.0);
    }
}
#[test]
fn dirty_flag_tracks_mutable_access_not_reads_or_private_data() {
    let mut value = Dr::new(Value(1));
    let mut output = Vec::new();
    value.sync_with(&mut output);
    value.sync_with(&mut output);
    assert_eq!(output, vec![1]);
    assert_eq!(value.0, 1); // 通过 Deref 读取，不置脏。
    value.add_private_data("cache".into(), 5u32);
    *value.private_data_mut::<u32>("cache").unwrap() = 6;
    value.sync_with(&mut output);
    assert_eq!(output, vec![1]);
    let _: &mut Value = &mut value; // 即使没有真正写值，可变解引用也置脏。
    value.sync_with(&mut output);
    assert_eq!(output, vec![1, 1]);
    value.0 = 2;
    value.sync_with(&mut output);
    assert_eq!(output, vec![1, 1, 2]);
}
#[test]
fn clone_discards_private_data_and_requires_a_new_sync() {
    let mut value = Dr::from(Value(7));
    value.add_private_data("cache".into(), String::from("private"));
    assert!(value.private_data::<u32>("cache").is_none());
    assert!(value.private_data::<String>("missing").is_none());
    let mut output = Vec::new();
    value.sync_with(&mut output);
    let mut copied = value.clone();
    assert!(copied.private_data::<String>("cache").is_none());
    assert_eq!(value.private_data::<String>("cache").unwrap(), "private");
    copied.sync_with(&mut output);
    assert_eq!(output, vec![7, 7]);
    assert_eq!(copied.into_inner().0, 7);
}
