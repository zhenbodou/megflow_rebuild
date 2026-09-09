use flow_message::algo_base::{Rect, TrackState};

#[test]
fn rectangle_preserves_raw_geometry_and_boundary_rules() {
    let rect = Rect {
        x1: 1.0,
        y1: 2.0,
        x2: 5.0,
        y2: 8.0,
        score: Some(0.9),
    };
    assert_eq!((rect.width(), rect.height(), rect.area()), (4.0, 6.0, 24.0));
    assert!(rect.in_frame(5.0, 8.0));
    assert!(!rect.in_frame(4.0, 8.0));
    let inverted = Rect {
        x1: 5.0,
        x2: 1.0,
        ..rect
    };
    assert_eq!(inverted.width(), -4.0);
    assert_eq!(inverted.area(), -24.0);
    assert!(inverted.in_frame(5.0, 8.0));
    let nan = Rect {
        x1: f32::NAN,
        ..rect
    };
    assert!(nan.width().is_nan());
    // 原版仅执行比较，并不额外验证有限数或坐标顺序。
    assert!(nan.in_frame(5.0, 8.0));
}

#[test]
fn track_state_names_discriminants_and_invalid_input_match_reference() {
    assert_eq!(TrackState::default().name(), "no");
    for (index, name) in [
        "no", "new", "update", "miss", "die", "filtered", "select", "max",
    ]
    .into_iter()
    .enumerate()
    {
        let state = TrackState::from(name);
        assert_eq!(state.name(), name);
        assert_eq!(state.to_string(), name);
        assert_eq!(state as usize, index);
    }
    for invalid in ["NEW", " new", "", "unknown"] {
        assert!(std::panic::catch_unwind(|| TrackState::from(invalid)).is_err());
    }
}

#[test]
fn record_vec_keeps_a_separate_checkpoint_and_moves_non_clone_values() {
    use flow_message::algo_base::RecordVec;
    struct OnlyMove(u32);
    let mut values: RecordVec<OnlyMove> = vec![OnlyMove(1), OnlyMove(2)].into();
    assert_eq!(values.1, 2);
    values.push(OnlyMove(3));
    assert_eq!(values.len(), 3);
    assert_eq!(values.1, 2, "push does not advance recorded length");
    assert_eq!(
        values
            .iter()
            .skip(values.1)
            .map(|value| value.0)
            .collect::<Vec<_>>(),
        vec![3]
    );
    values[0].0 = 9;
    values.1 = values.len();
    assert_eq!(values.1, 3);
    let moved: Vec<OnlyMove> = values.into();
    assert_eq!(
        moved.into_iter().map(|value| value.0).collect::<Vec<_>>(),
        vec![9, 2, 3]
    );
    let empty: RecordVec<OnlyMove> = Default::default();
    assert!(empty.is_empty());
    assert_eq!(empty.1, 0);
}
