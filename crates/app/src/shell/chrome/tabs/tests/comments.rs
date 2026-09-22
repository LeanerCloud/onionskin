//! The Comments pane on a real window: reading, replying, setting a status,
//! checking, editing and deleting, each one undoable step.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::panes::{CommentAction, NavigationPane, PaneAction};
use onionskin_core::{Annotation, ObjRef, ReadAnnotation, Rect, Subtype};

const NOW: i64 = 1_790_000_000;

fn seed_copy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("hello.pdf");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf"),
        &path,
    )
    .expect("copies");
    (dir, path)
}

fn window_on(
    path: &Path,
    cx: &mut TestAppContext,
) -> (gpui::WindowHandle<ShellFrame>, Vec<crate::keymap::Binding>) {
    let model = CanvasModel::new(
        Document::open_path(path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    bound_window_with_models(
        vec![(path.to_path_buf(), model)],
        crate::config::ConfigPaths::default(),
        cx,
    )
}

/// Put a note with `text` on the first page, as another reviewer's file
/// would carry one.
fn add_note(frame: &mut ShellFrame, text: &str, cx: &mut Context<ShellFrame>) -> ObjRef {
    let canvas = frame.tabs.active().expect("a tab").canvas.clone();
    canvas.update(cx, |canvas, cx| {
        let placed = {
            let mut document = canvas.model.document_mut();
            let page = document.structure().unwrap().page(0).unwrap().objref;
            document
                .edit_annotations("Sticky Note", |tx, structure| {
                    let mut note =
                        Annotation::new(Subtype::Text, Rect::new(50.0, 50.0, 70.0, 70.0));
                    note.contents = Some(text.to_owned());
                    note.author = Some("Zoe".to_owned());
                    onionskin_core::add_annotation(tx, structure, page, &note, NOW)
                })
                .expect("the note is added")
        };
        canvas.handle_change(Ok(true), cx);
        placed
    })
}

fn annotations(frame: &ShellFrame, cx: &mut Context<ShellFrame>) -> Vec<ReadAnnotation> {
    let canvas = frame.tabs.active().expect("a tab").canvas.clone();
    canvas.update(cx, |canvas, _| canvas.model.annotations().expect("reads"))
}

fn comment(action: CommentAction) -> Activation {
    Activation::Pane(PaneAction::Comment(action))
}

fn row_description(
    frame: &mut ShellFrame,
    window: &mut Window,
    cx: &mut Context<ShellFrame>,
) -> String {
    let tree = frame.accessible(window, cx);
    tree.find(&("comment-row", 0usize).into())
        .expect("the comment is listed")
        .description
        .clone()
        .unwrap_or_default()
}

#[gpui::test]
fn a_comment_is_replied_to_given_a_status_checked_edited_and_deleted_from_the_pane(
    cx: &mut TestAppContext,
) {
    let (_dir, path) = seed_copy();
    let (window, bindings) = window_on(&path, cx);
    let note = window
        .update(cx, |frame, window, cx| {
            let note = add_note(frame, "Is this figure right?", cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            let tree = frame.accessible(window, cx);
            let row = tree
                .find(&("comment-row", 0usize).into())
                .expect("the note is listed");
            assert_eq!(row.label, "Is this figure right?");
            assert!(
                row.description.as_deref().unwrap().contains("Note · Zoe"),
                "{:?}",
                row.description
            );
            frame.run_activation(comment(CommentAction::Select(note)), window, cx);
            frame.run_activation(comment(CommentAction::Reply), window, cx);
            note
        })
        .unwrap();

    // Enter in the reply field saves it: the field's own binding, not the
    // focus ring's.
    window
        .update(cx, |frame, _window, cx| {
            let input = frame
                .navigation
                .comment_draft()
                .expect("the field opened")
                .clone();
            input.update(cx, |input, cx| input.set_query("Yes, checked", cx));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    window
        .update(cx, |frame, window, cx| {
            assert!(
                frame.navigation.comment_draft().is_none(),
                "Enter closed it"
            );
            let reply = annotations(frame, cx)
                .into_iter()
                .find(|annotation| annotation.in_reply_to == Some(note))
                .expect("the reply answers the note");
            assert_eq!(reply.contents.as_deref(), Some("Yes, checked"));
            let tree = frame.accessible(window, cx);
            let listed = tree
                .find(&("comment-reply", 0usize).into())
                .expect("the reply is listed under it");
            assert_eq!(listed.label, "Yes, checked");

            frame.run_activation(comment(CommentAction::SetStatus("Accepted")), window, cx);
            frame.run_activation(comment(CommentAction::ToggleMark), window, cx);
            let described = row_description(frame, window, cx);
            assert!(described.contains("Accepted"), "{described}");
            assert!(described.contains('✓'), "{described}");
            let note_dict = annotations(frame, cx)
                .into_iter()
                .find(|annotation| annotation.objref == note)
                .unwrap();
            assert_eq!(note_dict.state, None, "the status is an answer, not a key");
        })
        .unwrap();

    // Undo takes the checkmark back, and the list follows the undo without
    // being reopened.
    cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "edit.undo"));
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let described = row_description(frame, window, cx);
            assert!(!described.contains('✓'), "{described}");
            assert!(described.contains("Accepted"), "{described}");

            frame.run_activation(comment(CommentAction::Edit), window, cx);
            let input = frame
                .navigation
                .comment_draft()
                .expect("the field opened")
                .clone();
            assert_eq!(
                input.read(cx).query(),
                "Is this figure right?",
                "it starts from the text"
            );
            input.update(cx, |input, cx| input.set_query("Is figure 2 right?", cx));
            frame.run_activation(comment(CommentAction::SaveDraft), window, cx);
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&("comment-row", 0usize).into()).unwrap().label,
                "Is figure 2 right?"
            );

            frame.run_activation(comment(CommentAction::Delete), window, cx);
            assert!(
                annotations(frame, cx).is_empty(),
                "the note went with its reply and its status answers"
            );
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"comment-rows-empty".into()).is_some());
        })
        .unwrap();
}

/// A comment placed on the page while the pane is open is in the list at
/// once: the pane follows the document's edits, not only its own.
#[gpui::test]
fn a_comment_made_while_the_pane_is_open_is_listed(cx: &mut TestAppContext) {
    let (_dir, path) = seed_copy();
    let (window, _) = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"comment-rows-empty".into()).is_some());
            add_note(frame, "Late note", cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            let row = tree
                .find(&("comment-row", 0usize).into())
                .expect("listed without reopening the pane");
            assert_eq!(row.label, "Late note");
        })
        .unwrap();
}

#[gpui::test]
fn sorting_and_filtering_step_through_their_values(cx: &mut TestAppContext) {
    let (_dir, path) = seed_copy();
    let (window, _) = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            add_note(frame, "First", cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            let control = |frame: &mut ShellFrame,
                           window: &mut Window,
                           cx: &mut Context<ShellFrame>,
                           index: usize| {
                frame
                    .accessible(window, cx)
                    .find(&("comment-control", index).into())
                    .expect("the control is there")
                    .label
                    .clone()
            };
            assert_eq!(control(frame, window, cx, 0), "Sort: Page");
            frame.run_activation(comment(CommentAction::CycleSort), window, cx);
            assert_eq!(control(frame, window, cx, 0), "Sort: Author");
            assert_eq!(control(frame, window, cx, 2), "Author: All");
            frame.run_activation(
                comment(CommentAction::CycleFilter(
                    crate::shell::panes::FilterField::Author,
                )),
                window,
                cx,
            );
            assert_eq!(control(frame, window, cx, 2), "Author: Zoe");
        })
        .unwrap();
}

/// Someone else's comment is unread until it is opened, and the user can
/// mark it unread again. The marks are the reader's own: nothing is written
/// to the document, and they survive switching tabs.
#[gpui::test]
fn a_comment_is_unread_until_opened_and_marking_it_writes_nothing(cx: &mut TestAppContext) {
    let (_dir, path) = seed_copy();
    let (window, _) = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            let note = add_note(frame, "Please look", cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            assert!(row_description(frame, window, cx).starts_with("Unread."));

            let canvas = frame.tabs.active().unwrap().canvas.clone();
            let epoch = canvas.read(cx).model.edit_epoch();
            frame.run_activation(comment(CommentAction::Select(note)), window, cx);
            assert!(!row_description(frame, window, cx).starts_with("Unread."));

            frame.run_activation(comment(CommentAction::ToggleRead), window, cx);
            assert!(row_description(frame, window, cx).starts_with("Unread."));
            assert_eq!(
                canvas.read(cx).model.edit_epoch(),
                epoch,
                "reading is not an edit"
            );
        })
        .unwrap();
}

/// The right-click menu offers the same commands as the row, on the comment
/// it was opened on, and running one closes it.
#[gpui::test]
fn the_context_menu_runs_the_rows_commands_on_the_comment_it_was_opened_on(
    cx: &mut TestAppContext,
) {
    let (_dir, path) = seed_copy();
    let (window, _) = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            let note = add_note(frame, "Menu me", cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            frame.run_pane_action(
                PaneAction::Comment(CommentAction::OpenMenu {
                    comment: note,
                    at: gpui::point(px(10.0), px(40.0)),
                }),
                cx,
            );
            let tree = frame.accessible(window, cx);
            let menu = tree
                .find(&"comments-context-menu".into())
                .expect("the menu is described");
            let labels: Vec<&str> = menu
                .children
                .iter()
                .map(|entry| entry.label.as_str())
                .collect();
            assert!(
                labels.contains(&"Reply") && labels.contains(&"Delete"),
                "{labels:?}"
            );
            let accepted = menu
                .children
                .iter()
                .find(|entry| entry.label == "Accepted")
                .and_then(|entry| entry.activation.clone())
                .expect("Accepted runs something");
            frame.run_activation(accepted, window, cx);
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"comments-context-menu".into()).is_none(),
                "it closed"
            );
            assert!(row_description(frame, window, cx).contains("Accepted"));
        })
        .unwrap();
}

/// Commenting preferences: the name typed and saved with Enter is written to
/// the preferences file and handed to the open tab's tools, so the next
/// comment and the next reply are signed with it.
#[gpui::test]
fn the_author_name_saved_in_preferences_signs_the_next_comment(cx: &mut TestAppContext) {
    let (dir, path) = seed_copy();
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let config = crate::config::ConfigPaths::in_dir(&dir.path().join("config"));
    let preferences_file = config.preferences.clone().expect("a preferences path");
    let (window, _) = bound_window_with_models(vec![(path.clone(), model)], config, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.show_preferences(
                crate::preferences::PreferenceCategory::Commenting,
                window,
                cx,
            );
            let tree = frame.accessible(window, cx);
            assert!(tree
                .find(&crate::shell::preferences_dialog::AUTHOR_FIELD_ID.into())
                .is_some());
            let input = frame.commenting_author_input().clone();
            window.focus(&input.read(cx).focus_handle(cx));
            input.update(cx, |input, cx| input.set_query("  Ana Pop ", cx));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(
                frame.preferences().commenting_author.as_deref(),
                Some("Ana Pop")
            );
            let written = std::fs::read_to_string(&preferences_file).expect("saved");
            assert!(written.contains("\"Ana Pop\""), "{written}");

            let canvas = frame.tabs.active().unwrap().canvas.clone();
            assert_eq!(canvas.read(cx).model.author(), Some("Ana Pop"));
            let note = add_note(frame, "Unsigned by the fixture", cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            frame.run_activation(comment(CommentAction::Select(note)), window, cx);
            frame.run_activation(comment(CommentAction::SetStatus("Completed")), window, cx);
            let status = annotations(frame, cx)
                .into_iter()
                .find(|annotation| annotation.state.is_some())
                .expect("the status answer");
            assert_eq!(status.author.as_deref(), Some("Ana Pop"));
        })
        .unwrap();
}

/// The properties inspector: Properties opens it on the chosen comment,
/// colour and opacity apply at once as undoable edits, author and subject
/// wait for Save, and "Make Current Properties Default" makes the look the
/// next comment of that kind's.
#[gpui::test]
fn the_inspector_changes_a_comment_and_makes_its_look_the_default(cx: &mut TestAppContext) {
    use crate::shell::chrome::inspector::InspectorAction;

    let (_dir, path) = seed_copy();
    let (window, _) = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            let note = add_note(frame, "Colour me", cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            frame.run_activation(comment(CommentAction::Select(note)), window, cx);
            frame.run_activation(Activation::Inspector(InspectorAction::Show), window, cx);
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"inspector".into()).is_some(),
                "the panel shows it"
            );
            let author = tree
                .find(&crate::shell::chrome::inspector::AUTHOR_ID.into())
                .expect("the author field");
            assert_eq!(
                author.value.as_deref(),
                Some("Zoe"),
                "filled from the comment"
            );

            frame.run_activation(Activation::Inspector(InspectorAction::Color(0)), window, cx);
            frame.run_activation(
                Activation::Inspector(InspectorAction::Opacity(50)),
                window,
                cx,
            );
            let placed = annotations(frame, cx).remove(0);
            assert_eq!(
                placed.color.map(crate::shell::chrome::inspector::to_rgb),
                Some(crate::shell::chrome::inspector::PALETTE[0].1)
            );
            assert_eq!(placed.opacity, Some(0.5));
            let tree = frame.accessible(window, cx);
            let red = tree.find(&("inspector-color", 0usize).into()).unwrap();
            assert_eq!(red.state.selected, Some(true));

            frame
                .inspector
                .author
                .update(cx, |input, cx| input.set_query("Max", cx));
            frame
                .inspector
                .subject
                .update(cx, |input, cx| input.set_query("Layout", cx));
            frame.run_activation(Activation::Inspector(InspectorAction::SaveText), window, cx);
            let placed = annotations(frame, cx).remove(0);
            assert_eq!(placed.author.as_deref(), Some("Max"));
            assert_eq!(placed.subject.as_deref(), Some("Layout"));

            frame.run_activation(
                Activation::Inspector(InspectorAction::MakeDefault),
                window,
                cx,
            );
            let default = frame.preferences().comment_defaults["Text"];
            assert_eq!(
                default.color,
                Some(crate::shell::chrome::inspector::PALETTE[0].1)
            );
            assert_eq!(default.opacity_percent, 50);
        })
        .unwrap();
}

/// End to end through the tool, as the plan asks: after "Make Current
/// Properties Default" on a sticky note, the next sticky note the tool
/// places has that colour and opacity.
#[cfg(feature = "tools-comment")]
#[gpui::test]
fn the_next_sticky_note_takes_the_default_the_inspector_made(cx: &mut TestAppContext) {
    use crate::shell::chrome::inspector::{InspectorAction, PALETTE};

    let (_dir, path) = seed_copy();
    let (window, _) = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            let note = add_note(frame, "The model", cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Comments), cx);
            frame.run_activation(comment(CommentAction::Select(note)), window, cx);
            frame.run_activation(Activation::Inspector(InspectorAction::Color(4)), window, cx);
            frame.run_activation(
                Activation::Inspector(InspectorAction::Opacity(75)),
                window,
                cx,
            );
            frame.run_activation(
                Activation::Inspector(InspectorAction::MakeDefault),
                window,
                cx,
            );

            let canvas = frame.tabs.active().unwrap().canvas.clone();
            canvas.update(cx, |canvas, _| {
                let index = canvas
                    .model
                    .registry()
                    .tools()
                    .position(|tool| tool.id() == "sticky-note")
                    .expect("installed");
                canvas.model.activate_tool(index).expect("activates");
                let page = canvas.model.viewport().visible_pages().unwrap()[0].rect;
                let at = gpui::point(
                    px(page.origin.x + page.size.width / 2.0),
                    px(page.origin.y + page.size.height / 2.0),
                );
                canvas
                    .model
                    .pointer_down(at, 1.0, gpui::Modifiers::default())
                    .unwrap();
                canvas
                    .model
                    .pointer_up(at, 1.0, gpui::Modifiers::default())
                    .unwrap();
            });
            let placed = annotations(frame, cx)
                .into_iter()
                .find(|annotation| annotation.objref != note && annotation.in_reply_to.is_none())
                .expect("the tool placed a note");
            assert_eq!(
                placed.color.map(crate::shell::chrome::inspector::to_rgb),
                Some(PALETTE[4].1)
            );
            assert_eq!(placed.opacity, Some(0.75));
        })
        .unwrap();
}

/// Find with Include Comments on finds text that is only in a comment, and
/// with it off does not.
#[gpui::test]
fn find_with_include_comments_finds_a_comments_text(cx: &mut TestAppContext) {
    use crate::shell::find_bar::FindOption;

    let (_dir, path) = seed_copy();
    let (window, _) = window_on(&path, cx);
    let found = |cx: &mut TestAppContext| {
        window
            .update(cx, |frame, _window, cx| {
                let canvas = frame.tabs.active().unwrap().canvas.clone();
                canvas.update(cx, |canvas, _| {
                    let mut document = canvas.model.document_mut();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                    while document.search().is_running() {
                        assert!(std::time::Instant::now() < deadline, "the walk never ended");
                        document.poll_search();
                    }
                    document.search().len()
                })
            })
            .unwrap()
    };
    window
        .update(cx, |frame, window, cx| {
            add_note(frame, "mind the zebracorn", cx);
            frame.open_find_bar(Some("zebracorn".to_owned()), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(found(cx), 0, "the page text has no zebracorn");
    window
        .update(cx, |frame, _window, cx| {
            frame.apply_find_option(FindOption::IncludeComments, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(found(cx), 1, "the note's text is found");
}
