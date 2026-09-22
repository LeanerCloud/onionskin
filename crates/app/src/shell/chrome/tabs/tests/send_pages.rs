//! P22 on a real window: Organize Pages' Copy To and Move To Document send
//! the chosen pages to the end of another open document, as one undo step
//! in each document they change.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::dialog::ShellDialog;
use crate::shell::organize::OrganizeAction;

/// `two-page.pdf` in front with its grid open and page 2 chosen, and
/// `hello.pdf` in a second tab.
fn grid_with_two_documents(cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let (window, _bindings) = bound_window_in(
        &["two-page.pdf", "hello.pdf"],
        crate::config::ConfigPaths::default(),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.activate(0, cx);
            frame
                .run_main_menu_command(MenuCommand::OrganizePages, window, cx)
                .expect("opens the grid");
            frame.run_activation(Activation::Organize(OrganizeAction::Choose(1)), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

/// Page counts of the two tabs, in tab order.
fn counts(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<usize> {
    window
        .update(cx, |frame, _, cx| {
            frame
                .tabs
                .tabs()
                .iter()
                .map(|tab| tab.canvas.read(cx).model.viewport().page_count())
                .collect()
        })
        .unwrap()
}

fn send(
    window: gpui::WindowHandle<ShellFrame>,
    action: OrganizeAction,
    cx: &mut TestAppContext,
) -> Vec<String> {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Organize(action), window, cx);
            assert!(matches!(frame.dialog, Some(ShellDialog::SendPages { .. })));
            let tree = frame.accessible(window, cx);
            let target = tree
                .find(&("send-pages-target", 0usize).into())
                .expect("the other document is offered");
            assert_eq!(target.label, "hello.pdf");
            frame.run_activation(Activation::SendPages(0), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, _, _| frame.notices.clone())
        .unwrap()
}

#[gpui::test]
fn copy_to_adds_the_pages_to_the_other_document_only(cx: &mut TestAppContext) {
    let window = grid_with_two_documents(cx);
    assert_eq!(counts(window, cx), [2, 1]);
    let notices = send(window, OrganizeAction::CopyTo, cx);
    assert_eq!(counts(window, cx), [2, 2]);
    assert!(
        notices.contains(&"Copied 1 page to hello.pdf".to_owned()),
        "{notices:?}"
    );
    let undo = window
        .update(cx, |frame, _, cx| {
            (
                frame.tabs.tabs()[0]
                    .canvas
                    .read(cx)
                    .model
                    .history_facts()
                    .undo,
                frame.tabs.tabs()[1]
                    .canvas
                    .read(cx)
                    .model
                    .history_facts()
                    .undo,
            )
        })
        .unwrap();
    assert_eq!(
        undo,
        (None, Some("Insert Pages")),
        "one step, in the target"
    );
}

#[gpui::test]
fn move_to_takes_the_pages_out_of_this_document(cx: &mut TestAppContext) {
    let window = grid_with_two_documents(cx);
    let notices = send(window, OrganizeAction::MoveTo, cx);
    assert_eq!(counts(window, cx), [1, 2]);
    assert!(
        notices.contains(&"Moved 1 page to hello.pdf".to_owned()),
        "{notices:?}"
    );
}

/// With one document open there is nowhere to send pages, and the dialog
/// says so rather than offering nothing.
#[gpui::test]
fn with_one_document_the_dialog_says_there_is_nowhere_to_send(cx: &mut TestAppContext) {
    let (window, _bindings) =
        bound_window_in(&["two-page.pdf"], crate::config::ConfigPaths::default(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::OrganizePages, window, cx)
                .expect("opens the grid");
            frame.run_activation(Activation::Organize(OrganizeAction::CopyTo), window, cx);
            let tree = frame.accessible(window, cx);
            let summary = tree.find(&"send-pages-summary".into()).expect("says");
            assert_eq!(
                summary.label,
                crate::shell::chrome::send_pages::NO_OTHER_DOCUMENT
            );
        })
        .unwrap();
}
