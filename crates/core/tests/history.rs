use std::num::NonZeroUsize;

use onionskin_core::{PageLayoutMode, ViewHistory, ViewPoint, ViewRotation, ViewState, ZoomPolicy};

fn state(page: usize, zoom: f32, y: f32) -> ViewState {
    ViewState {
        current_page: page,
        offset: ViewPoint { x: 0.0, y },
        zoom,
        zoom_policy: ZoomPolicy::Fixed,
        mode: PageLayoutMode::SinglePageContinuous,
        show_cover: false,
        rotation: ViewRotation::None,
    }
}

#[test]
fn previous_restores_the_last_recorded_view() {
    let mut history = ViewHistory::new(NonZeroUsize::new(8).unwrap());
    let page_1 = state(1, 1.0, 100.0);
    let page_5 = state(5, 2.0, 500.0);

    history.record(page_1);

    assert_eq!(history.previous(page_5), Some(page_1));
    assert!(!history.can_previous());
    assert!(history.can_next());
}

#[test]
fn next_restores_the_forward_view_after_previous() {
    let mut history = ViewHistory::new(NonZeroUsize::new(8).unwrap());
    let page_1 = state(1, 1.0, 100.0);
    let page_5 = state(5, 2.0, 500.0);

    history.record(page_1);
    let restored = history.previous(page_5).unwrap();

    assert_eq!(restored, page_1);
    assert_eq!(history.next(restored), Some(page_5));
    assert!(history.can_previous());
    assert!(!history.can_next());
}

#[test]
fn a_divergent_record_clears_forward_history() {
    let mut history = ViewHistory::new(NonZeroUsize::new(8).unwrap());
    let page_1 = state(1, 1.0, 100.0);
    let page_5 = state(5, 2.0, 500.0);
    let page_9 = state(9, 1.25, 900.0);

    history.record(page_1);
    let restored = history.previous(page_5).unwrap();

    history.record(restored);

    assert_eq!(history.next(restored), None);
    assert_eq!(history.previous(page_9), Some(page_1));
}

#[test]
fn exact_duplicate_records_are_suppressed() {
    let mut history = ViewHistory::new(NonZeroUsize::new(8).unwrap());
    let page_1 = state(1, 1.0, 100.0);

    history.record(page_1);
    history.record(page_1);
    assert_eq!(history.previous(state(2, 1.0, 200.0)), Some(page_1));
    assert!(!history.can_previous());
}

#[test]
fn capacity_evicts_the_oldest_previous_entries() {
    let mut history = ViewHistory::new(NonZeroUsize::new(2).unwrap());
    let page_1 = state(1, 1.0, 100.0);
    let page_2 = state(2, 1.0, 200.0);
    let page_3 = state(3, 1.0, 300.0);
    let page_4 = state(4, 1.0, 400.0);

    history.record(page_1);
    history.record(page_2);
    history.record(page_3);

    assert_eq!(history.previous(page_4), Some(page_3));
    assert_eq!(history.previous(page_3), Some(page_2));
    assert_eq!(history.previous(page_2), None);
}

#[test]
fn clear_removes_previous_and_forward_entries() {
    let mut history = ViewHistory::new(NonZeroUsize::new(8).unwrap());
    let page_1 = state(1, 1.0, 100.0);
    let page_5 = state(5, 2.0, 500.0);

    history.record(page_1);
    history.previous(page_5).unwrap();
    history.clear();

    assert!(!history.can_previous());
    assert!(!history.can_next());
    assert_eq!(history.previous(page_5), None);
    assert_eq!(history.next(page_1), None);
}

#[test]
fn history_contains_only_view_state() {
    let mut history = ViewHistory::new(NonZeroUsize::new(1).unwrap());
    let previous = state(4, 1.25, 400.0);
    let current = state(5, 2.0, 500.0);

    history.record(previous);

    assert_eq!(history.previous(current), Some(previous));
}
