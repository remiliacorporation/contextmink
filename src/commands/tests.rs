use super::*;

#[test]
fn slice_tail_cap_is_end_anchored() {
    let plan = plan_slice_window(100, SliceWindowRequest::Tail { lines: 40 }, 10).unwrap();
    assert_eq!(
        plan,
        SliceWindowPlan {
            start: 91,
            end: 100,
            output_truncated: true,
        }
    );
}

#[test]
fn slice_tail_shorter_than_file_and_cap_is_complete() {
    let plan = plan_slice_window(100, SliceWindowRequest::Tail { lines: 10 }, 40).unwrap();
    assert_eq!(
        plan,
        SliceWindowPlan {
            start: 91,
            end: 100,
            output_truncated: false,
        }
    );
}

#[test]
fn slice_window_clips_to_available_lines() {
    let plan =
        plan_slice_window(12, SliceWindowRequest::Inclusive { start: 10, end: 30 }, 50).unwrap();
    assert_eq!(
        plan,
        SliceWindowPlan {
            start: 10,
            end: 12,
            output_truncated: false,
        }
    );
}

#[test]
fn slice_window_rejects_zero_and_inverted_bounds() {
    assert!(
        plan_slice_window(12, SliceWindowRequest::Inclusive { start: 0, end: 3 }, 10,).is_err()
    );
    assert!(
        plan_slice_window(12, SliceWindowRequest::Inclusive { start: 4, end: 3 }, 10,).is_err()
    );
    assert!(plan_slice_window(12, SliceWindowRequest::Tail { lines: 0 }, 10).is_err());
    assert!(plan_slice_window(12, SliceWindowRequest::Tail { lines: 3 }, 0).is_err());
}
