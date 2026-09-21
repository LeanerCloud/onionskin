//! P22 on a real window: Edit > Advanced Search opens from its keystroke,
//! finds a word only an attached PDF has when attachments are included,
//! lists the pages' hits in the Search Results pane, and holds a document
//! that does not meet its criterion out of the results.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::advanced_search::{AdvancedAction, NO_WORDS};
use crate::shell::dialog::ShellDialog;
use crate::shell::panes::NavigationPane;

fn window(cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let (window, bindings) = bound_window_from_bytes(
        vec![("cover.pdf", crate::shell::fixtures::attached_pdf_pdf())],
        cx,
    );
    cx.simulate_keystrokes(
        window.into(),
        &keystroke_for(&bindings, "edit.advanced-search"),
    );
    window
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: AdvancedAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::AdvancedSearch(action), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

fn type_into(
    window: gpui::WindowHandle<ShellFrame>,
    query: &str,
    value: Option<&str>,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |frame, _window, cx| {
            let state = frame.advanced_search.as_ref().expect("open");
            let (query_input, value_input) = (state.query.clone(), state.value.clone());
            query_input.update(cx, |input, cx| input.set_query(query, cx));
            if let Some(value) = value {
                value_input.update(cx, |input, cx| input.set_query(value, cx));
            }
        })
        .unwrap();
}

/// The dialog's outcome lines, as the tree reads them.
fn said(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, window, cx| {
            frame
                .accessible(window, cx)
                .walk()
                .filter(|element| format!("{:?}", element.key).contains("advanced-outcome"))
                .map(|element| element.label.clone())
                .collect()
        })
        .unwrap()
}

/// The words the page walk is searching, and whether the results pane shows.
fn page_search(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> (String, bool) {
    window
        .update(cx, |frame, _window, cx| {
            let needle = frame
                .active_canvas()
                .unwrap()
                .read(cx)
                .model
                .search()
                .needle()
                .to_owned();
            (
                needle,
                frame.navigation.active() == Some(NavigationPane::SearchResults),
            )
        })
        .unwrap()
}

#[gpui::test]
fn a_word_only_an_attachment_has_is_found_when_attachments_are_included(cx: &mut TestAppContext) {
    let window = window(cx);
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::AdvancedSearch));
            let tree = frame.accessible(window, cx);
            let attachments = tree.find(&"advanced-attachments".into()).expect("offered");
            assert_eq!(attachments.state.toggled, Some(false));
        })
        .unwrap();
    type_into(window, "heron", None, cx);
    act(window, AdvancedAction::Search, cx);
    let without = said(window, cx);
    assert_eq!(
        without,
        ["The pages' results are in the Search Results pane."],
        "attachments are not searched unless asked"
    );
    assert_eq!(page_search(window, cx), ("heron".to_owned(), true));

    act(window, AdvancedAction::IncludeAttachments, cx);
    act(window, AdvancedAction::Search, cx);
    let with = said(window, cx);
    assert!(
        with.contains(&"annex.pdf, page 1: heron".to_owned()),
        "{with:?}"
    );
    assert!(with.contains(&"1 result in the PDF attachments:".to_owned()));
}

#[gpui::test]
fn a_document_that_misses_the_criterion_is_not_searched(cx: &mut TestAppContext) {
    let window = window(cx);
    act(window, AdvancedAction::UseCriterion, cx);
    type_into(window, "cover", Some("Radu"), cx);
    act(window, AdvancedAction::Search, cx);
    assert_eq!(
        said(window, cx),
        ["The document does not meet the criteria; nothing was searched."]
    );
    assert_eq!(page_search(window, cx).0, "", "no page walk ran");

    type_into(window, "cover", Some("ana"), cx);
    act(window, AdvancedAction::Search, cx);
    assert_eq!(
        said(window, cx)[0],
        "The document meets the criteria.",
        "Author contains \"ana\", ignoring case"
    );
    assert_eq!(page_search(window, cx).0, "cover");

    // A date criterion with a value that is not a date says how to write it.
    act(window, AdvancedAction::NextField, cx);
    window
        .update(cx, |frame, _window, _cx| {
            let state = frame.advanced_search.as_mut().unwrap();
            while state.form.criterion.map(|(field, _)| field)
                != Some(onionskin_core::metadata::PropertyField::Created)
            {
                state.apply(AdvancedAction::NextField);
            }
        })
        .unwrap();
    type_into(window, "cover", Some("March"), cx);
    act(window, AdvancedAction::Search, cx);
    assert!(said(window, cx)[0].contains("YYYY-MM-DD"));
    type_into(window, "cover", Some("2025-03-02"), cx);
    act(window, AdvancedAction::Search, cx);
    assert_eq!(
        said(window, cx)[0],
        "The document meets the criteria.",
        "created before"
    );
}

#[gpui::test]
fn searching_for_nothing_says_to_type_words(cx: &mut TestAppContext) {
    let window = window(cx);
    type_into(window, "   ", None, cx);
    act(window, AdvancedAction::Search, cx);
    assert_eq!(said(window, cx), [NO_WORDS]);
    // Changing the form drops an outcome that no longer describes it.
    act(window, AdvancedAction::IncludeAttachments, cx);
    assert!(said(window, cx).is_empty());
}
