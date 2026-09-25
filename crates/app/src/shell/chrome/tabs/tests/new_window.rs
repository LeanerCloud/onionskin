//! P22 on real windows: View > New Window is a second window on the same
//! session. An edit in one appears in the other, one Undo in either takes
//! it back once, each window keeps its own zoom, and closing one of the two
//! loses nothing.

use super::*;
use crate::shell::canvas::ViewAction;

/// Two windows on `two-page.pdf`: the first, and the one New Window opened.
fn two_windows(
    cx: &mut TestAppContext,
) -> (
    gpui::WindowHandle<ShellFrame>,
    gpui::WindowHandle<ShellFrame>,
) {
    let (first, _bindings) =
        bound_window_in(&["two-page.pdf"], crate::config::ConfigPaths::default(), cx);
    first
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::NewWindow, window, cx)
                .expect("New Window runs");
        })
        .unwrap();
    cx.run_until_parked();
    let second = cx
        .windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<ShellFrame>())
        .find(|handle| *handle != first)
        .expect("a second window opened");
    (first, second)
}

fn canvas_of(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Entity<Canvas> {
    window
        .update(cx, |frame, _, _| {
            frame.active_canvas().expect("a tab").clone()
        })
        .unwrap()
}

/// `(pages laid out, undo label)` as `window` sees them.
fn seen(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> (usize, Option<&'static str>) {
    let canvas = canvas_of(window, cx);
    canvas.read_with(cx, |canvas, _| {
        (
            canvas.model.viewport().page_count(),
            canvas.model.history_facts().undo,
        )
    })
}

/// Delete page 2 through `window`'s own canvas, as a page command would.
fn delete_second_page(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) {
    let canvas = canvas_of(window, cx);
    canvas.update(cx, |canvas, cx| {
        let result = canvas
            .model
            .edit_pages(|document| {
                document
                    .edit_pages("Delete Pages", |tx, structure| {
                        onionskin_core::pages::delete_pages(tx, structure, &[1]).map(|_| ())
                    })
                    .map_err(|source| onionskin_plugin_api::CommandError::Page { page: 1, source })
            })
            .map(|()| true);
        canvas.handle_change(result, cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn an_edit_in_one_window_shows_in_the_other_and_one_undo_takes_it_back(cx: &mut TestAppContext) {
    let (first, second) = two_windows(cx);
    assert_eq!(seen(first, cx), (2, None));
    assert_eq!(seen(second, cx), (2, None));

    delete_second_page(first, cx);
    assert_eq!(seen(first, cx), (1, Some("Delete Pages")));
    assert_eq!(
        seen(second, cx),
        (1, Some("Delete Pages")),
        "the second window follows the first's edit"
    );

    second
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Undo, window, cx)
                .expect("undoes");
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(seen(second, cx), (2, None), "one undo, and nothing left");
    assert_eq!(
        seen(first, cx),
        (2, None),
        "the first window's document is the one the undo took back"
    );
}

#[gpui::test]
fn each_window_keeps_its_own_view(cx: &mut TestAppContext) {
    let (first, second) = two_windows(cx);
    let zoom = |window, cx: &mut TestAppContext| {
        canvas_of(window, cx).read_with(cx, |canvas, _| canvas.model.viewport().zoom())
    };
    let (before, second_before) = (zoom(first, cx), zoom(second, cx));
    second
        .update(cx, |frame, _, cx| {
            frame.run_view_action(ViewAction::ZoomIn, cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        zoom(second, cx) > second_before,
        "{second_before} to {}",
        zoom(second, cx)
    );
    assert_eq!(
        zoom(first, cx),
        before,
        "the first window's zoom is its own"
    );
}

#[gpui::test]
fn save_as_updates_the_shared_new_window_title_and_path(cx: &mut TestAppContext) {
    let (first, second) = two_windows(cx);
    let dir = tempfile::tempdir().expect("dir");
    let destination = dir.path().join("shared-copy.pdf");
    let expected = destination.clone();
    first
        .update(cx, |frame, _window, cx| frame.save_active_as(cx))
        .unwrap();
    cx.simulate_new_path_selection(move |_| Some(destination));
    cx.run_until_parked();
    second
        .update(cx, |frame, _window, cx| {
            let tab = frame.tabs.active().expect("shared tab");
            assert_eq!(tab.title(), "shared-copy.pdf");
            assert_eq!(tab.canvas.read(cx).model.path(), Some(expected.clone()));
        })
        .unwrap();
}

#[gpui::test]
fn closing_one_of_two_windows_asks_nothing_and_the_last_one_asks(cx: &mut TestAppContext) {
    let (first, second) = two_windows(cx);
    delete_second_page(first, cx);
    let loses = |window, cx: &mut TestAppContext| {
        let canvas = canvas_of(window, cx);
        cx.update(|cx| ShellFrame::close_loses_changes(&canvas, cx))
    };
    assert!(!loses(first, cx), "the second window still has the edit");
    assert!(!loses(second, cx));

    second
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert!(
        loses(first, cx),
        "now the only window on the edited document"
    );
}
