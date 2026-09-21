//! The grid's arithmetic and its selection rules, without a window.

use super::*;

/// Four 100-point cells to a row: 12 + 4 x 112 = 460 wide.
fn layout() -> GridLayout {
    GridLayout::new(470.0, 100.0)
}

/// The middle of `page`'s cell.
fn centre(page: PageIndex) -> (f32, f32) {
    let (x, y) = layout().origin(page);
    (x + 50.0, y + 50.0)
}

#[test]
fn cells_are_laid_out_in_rows_and_found_again_under_the_pointer() {
    let grid = layout();
    assert_eq!(grid.columns, 4);
    assert_eq!(grid.origin(0), (12.0, 12.0));
    assert_eq!(grid.origin(5), (124.0, 124.0));
    for page in 0..10 {
        assert_eq!(grid.page_at(centre(page), 10), Some(page));
    }
    assert_eq!(grid.page_at((5.0, 50.0), 10), None, "the gap");
    assert_eq!(grid.page_at((117.0, 50.0), 10), None, "between cells");
    assert_eq!(grid.page_at(centre(11), 10), None, "past the last page");
    assert_eq!(
        GridLayout::new(10.0, 100.0).columns,
        1,
        "at least one column"
    );
}

#[test]
fn a_drop_lands_before_or_after_the_page_it_is_over() {
    let grid = layout();
    let (x, y) = grid.origin(5);
    assert_eq!(
        grid.drop_slot((x + 10.0, y + 50.0), 10),
        5,
        "left half: before"
    );
    assert_eq!(
        grid.drop_slot((x + 90.0, y + 50.0), 10),
        6,
        "right half: after"
    );
    assert_eq!(
        grid.drop_slot((460.0, y + 50.0), 10),
        8,
        "past the row's end"
    );
    assert_eq!(grid.drop_slot((50.0, 1000.0), 10), 10, "below the last row");
}

#[test]
fn only_the_visible_rows_and_one_either_side_are_asked_for() {
    let grid = layout();
    // 112 a row, 4 a row: a 300-high view scrolled to row 25 of 250.
    let band = grid.visible(25.0 * 112.0 + 12.0, 300.0, 1000);
    assert_eq!(
        band,
        96..120,
        "rows 24 to 29: a screenful and slack, not a document"
    );
    assert_eq!(grid.visible(0.0, 300.0, 3), 0..3);
    assert_eq!(grid.visible(0.0, 0.0, 1000), 0..0);
}

#[test]
fn click_shift_click_and_cmd_click_select_as_acrobat_does() {
    let mut state = OrganizeState::new(gpui::EntityId::from(1u64), 0);
    state.click(2, Held::Nothing);
    assert_eq!(state.selected(), [2]);
    state.click(5, Held::Extend);
    assert_eq!(
        state.selected(),
        [2, 3, 4, 5],
        "a contiguous range from the anchor"
    );
    state.click(0, Held::Extend);
    assert_eq!(state.selected(), [0, 1, 2], "still from the same anchor");
    state.click(7, Held::Toggle);
    assert_eq!(state.selected(), [0, 1, 2, 7]);
    state.click(1, Held::Toggle);
    assert_eq!(state.selected(), [0, 2, 7], "toggled off");
    state.select_all(4);
    assert_eq!(state.selected(), [0, 1, 2, 3]);
}

#[test]
fn a_marquee_selects_what_it_covers_and_adds_with_a_modifier() {
    let grid = layout();
    let mut state = OrganizeState::new(gpui::EntityId::from(1u64), 9);
    // From the gap left of page 4 across to the middle of page 5 and down.
    state.press((5.0, 130.0), Held::Nothing, grid, 12);
    state.drag_to((150.0, 250.0), grid, 12);
    assert_eq!(state.selected(), [4, 5, 8, 9]);
    assert_eq!(state.release((150.0, 250.0), grid, 12), None);
    state.press((5.0, 5.0), Held::Toggle, grid, 12);
    state.drag_to((60.0, 60.0), grid, 12);
    assert_eq!(state.selected(), [0, 4, 5, 8, 9], "kept and added to");
    state.cancel();
    assert_eq!(
        state.selected(),
        [4, 5, 8, 9],
        "a cancelled marquee puts it back"
    );
}

#[test]
fn a_drag_reorders_once_on_release_and_a_cancelled_one_does_nothing() {
    let grid = layout();
    let mut state = OrganizeState::new(gpui::EntityId::from(1u64), 0);
    state.click(1, Held::Nothing);
    state.click(2, Held::Extend);
    state.press(centre(2), Held::Nothing, grid, 10);
    for step in 1..20 {
        let (x, y) = centre(2);
        // Many intermediate positions: none of them is a reorder.
        state.drag_to((x + step as f32 * 10.0, y + step as f32 * 5.0), grid, 10);
    }
    let (x, y) = grid.origin(7);
    let reorder = state.release((x + 80.0, y + 50.0), grid, 10);
    assert_eq!(
        reorder,
        Some(Reorder {
            pages: vec![1, 2],
            before: 8
        })
    );
    state.follow(reorder.as_ref().unwrap());
    assert_eq!(
        state.selected(),
        [6, 7],
        "the moved pages stay chosen where they went"
    );

    state.press(centre(6), Held::Nothing, grid, 10);
    state.drag_to(centre(0), grid, 10);
    state.cancel();
    assert_eq!(state.gesture, None);
    assert_eq!(
        state.release(centre(0), grid, 10),
        None,
        "nothing to release"
    );
}

#[test]
fn a_drop_that_leaves_the_pages_where_they_are_is_not_a_reorder() {
    let grid = layout();
    let mut state = OrganizeState::new(gpui::EntityId::from(1u64), 3);
    state.press(centre(3), Held::Nothing, grid, 10);
    let (x, y) = grid.origin(3);
    assert_eq!(state.release((x + 70.0, y + 60.0), grid, 10), None);
    // A click, not a drag, on one page of several narrows to it.
    state.click(4, Held::Extend);
    state.press(centre(4), Held::Nothing, grid, 10);
    assert_eq!(state.release(centre(4), grid, 10), None);
    assert_eq!(state.selected(), [4]);
}

#[test]
fn an_undo_that_removes_pages_clamps_the_selection_and_says_so() {
    let mut state = OrganizeState::new(gpui::EntityId::from(1u64), 0);
    state.select_all(6);
    state.clamp(4);
    assert_eq!(state.selected(), [0, 1, 2, 3]);
    assert_eq!(
        state.error.as_deref(),
        Some("2 selected pages are no longer in the document")
    );
    let mut lone = OrganizeState::new(gpui::EntityId::from(1u64), 5);
    lone.clamp(3);
    assert_eq!(
        lone.selected(),
        [2],
        "never an empty selection over a document"
    );
}

#[test]
fn window_points_become_grid_points_through_the_last_drawn_bounds() {
    let mut state = OrganizeState::new(gpui::EntityId::from(1u64), 0);
    assert_eq!(
        state.size(),
        (640.0, 480.0),
        "a guess before the first frame"
    );
    state.bounds.set(Some((100.0, 50.0, 470.0, 300.0)));
    state.scroll = 20.0;
    assert_eq!(state.to_grid((150.0, 90.0)), (50.0, 60.0));
    assert_eq!(state.size(), (470.0, 300.0));
    state.scroll_by(-1_000_000.0, layout(), 1000);
    assert_eq!(
        state.scroll,
        layout().height(1000) - 300.0,
        "to the last row, no further"
    );
    state.scroll_by(1_000_000.0, layout(), 1000);
    assert_eq!(state.scroll, 0.0);
}
