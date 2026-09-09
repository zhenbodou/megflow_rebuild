use flow_message::{
    algo_base::{RecordVec, Rect},
    dr::{Dr, SyncWith},
    Envelope, EnvelopeInfo,
};

#[derive(Clone)]
struct Detection(Rect);
impl SyncWith<&mut Vec<f32>> for Detection {
    fn sync_with(&mut self, areas: &mut Vec<f32>) {
        areas.push(self.0.area());
    }
}

fn main() {
    let mut detection = Dr::new(Detection(Rect {
        x1: 0.0,
        y1: 0.0,
        x2: 3.0,
        y2: 4.0,
        score: Some(0.8),
    }));
    let mut areas = Vec::new();
    detection.sync_with(&mut areas);
    detection.sync_with(&mut areas);
    assert_eq!(areas, [12.0]);

    detection.0.x2 = 6.0;
    detection.sync_with(&mut areas);
    assert_eq!(areas, [12.0, 24.0]);

    let mut records: RecordVec<f32> = areas.into();
    let old_checkpoint = records.1;
    records.push(30.0);
    assert_eq!(old_checkpoint, 2);
    assert_eq!(
        records.iter().skip(records.1).copied().collect::<Vec<_>>(),
        [30.0]
    );
    records.1 = records.len();

    let rect = detection.into_inner().0;
    let mut message = Envelope::with_info(
        rect,
        EnvelopeInfo {
            partial_id: Some(42),
            ..Default::default()
        },
    );
    let rect = message.unpack();
    let mut result = message.repack(rect.area());
    assert_eq!(result.info().partial_id, Some(42));
    assert_eq!(result.unpack(), 24.0);
    println!("同步面积：[12, 24]；新增记录：[30]；消息面积：24，序号：42");
}
