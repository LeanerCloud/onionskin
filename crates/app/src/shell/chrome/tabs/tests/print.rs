//! P17 on a real window: File > Print opens from its keystroke, every
//! control is in the tree and leaves it when the dialog closes, the preview
//! is the printed file's sheets, the Pages box refuses a backwards range,
//! and Page Setup and the Print dialog share one paper.

use super::*;
use crate::shell::chrome::accessible::Activation;
#[cfg(feature = "commands-core")]
use crate::shell::chrome::global_bar::PageCommand;
use crate::shell::chrome::print_dialog::{PagesChoice, PrintAction};
use crate::shell::context_menu::CanvasContextCommand;
use crate::shell::dialog::ShellDialog;

fn window_over(
    seed: &str,
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::WindowHandle<ShellFrame>,
    Vec<crate::keymap::Binding>,
) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(seed);
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(seed),
        &path,
    )
    .expect("copies");
    window_on(dir, path, cx)
}

fn window_on(
    dir: tempfile::TempDir,
    path: PathBuf,
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::WindowHandle<ShellFrame>,
    Vec<crate::keymap::Binding>,
) {
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let (window, bindings) = bound_window_with_models(
        vec![(path, model)],
        crate::config::ConfigPaths::default(),
        cx,
    );
    (dir, window, bindings)
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: PrintAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Print(action), window, cx);
        })
        .unwrap();
}

fn page_text(path: &Path, page: usize) -> String {
    let mut document = onionskin_core::Document::open_path(path).expect("opens PDF");
    document
        .page_text(page)
        .expect("reads page text")
        .flatten()
        .text
}

#[cfg(feature = "tools-comment")]
fn add_comment(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().unwrap().clone();
            canvas.update(cx, |canvas, cx| {
                let mut document = canvas.model.document_mut();
                let page = document.structure().unwrap().page(0).unwrap().objref;
                document
                    .edit_annotations("Sticky Note", |tx, structure| {
                        let mut note = onionskin_core::Annotation::new(
                            onionskin_core::Subtype::Text,
                            onionskin_core::Rect::new(50.0, 50.0, 70.0, 70.0),
                        );
                        note.contents = Some("Check the totals".to_owned());
                        onionskin_core::add_annotation(tx, structure, page, &note, 0)
                    })
                    .expect("adds a note");
                drop(document);
                canvas.handle_change(Ok(true), cx);
            });
        })
        .unwrap();
}

/// Every id the Print dialog publishes.
fn print_ids(
    frame: &mut ShellFrame,
    window: &mut gpui::Window,
    cx: &mut Context<ShellFrame>,
) -> Vec<String> {
    let tree = frame.accessible(window, cx);
    tree.walk()
        .map(|element| format!("{:?}", element.key))
        .filter(|key| key.contains("print-"))
        .collect()
}

#[gpui::test]
fn the_print_keystroke_opens_the_dialog_with_every_control_described(cx: &mut TestAppContext) {
    let (_dir, window, bindings) = window_over("two-page.pdf", cx);
    cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "file.print"));
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::Print));
            let tree = frame.accessible(window, cx);
            for (group, label) in [
                ("print-printer", "Printer"),
                ("print-copies-group", "Copies"),
                ("print-pages-group", "Pages to Print"),
                ("print-sizing", "Page Sizing & Handling"),
                ("print-n-up", "Multiple Pages per Sheet"),
                ("print-paper", "Paper Size"),
                ("print-orientation", "Orientation"),
                ("print-comments", "Comments & Forms"),
                ("print-duplex", "Print on Both Sides of Paper"),
                ("print-advanced", "Advanced"),
            ] {
                let node = tree
                    .find(&group.into())
                    .unwrap_or_else(|| panic!("{group}"));
                assert_eq!(node.label, label);
                assert!(!node.children.is_empty(), "{group} has controls");
                for control in &node.children {
                    assert!(!control.label.is_empty(), "{group}: an unlabelled control");
                    assert!(
                        control.state.selected.is_some()
                            || control.state.toggled.is_some()
                            || control.value.is_some(),
                        "{group}: {} carries no state",
                        control.label
                    );
                }
            }
            let save = &tree.find(&"print-printer".into()).unwrap().children[0];
            assert_eq!(save.label, "Save as PDF");
            assert_eq!(save.state.selected, Some(true));
            let preview = tree.find(&"print-preview".into()).expect("a preview");
            // The first page is wider than tall, so Auto turns the sheet.
            assert_eq!(preview.label, "Preview: sheet 1 of 2, landscape, page 1");
            assert!(tree.find(&"print-submit".into()).is_some());
        })
        .unwrap();
}

#[gpui::test]
fn closing_the_dialog_takes_every_control_out_of_the_tree(cx: &mut TestAppContext) {
    let (_dir, window, _bindings) = window_over("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
            assert!(print_ids(frame, window, cx).len() > 20);
        })
        .unwrap();
    act(window, PrintAction::Cancel, cx);
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, None);
            assert_eq!(print_ids(frame, window, cx), Vec::<String>::new());
        })
        .unwrap();
}

/// The plan's assertion that makes "the preview cannot disagree" true:
/// the sheets the preview draws are the sheets in the printed file, one
/// for one, with the same size and the same number of pages on each.
#[gpui::test]
fn the_preview_is_the_printed_files_sheets(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_over("two-page.pdf", cx);
    for action in [
        PrintAction::PerSheet(2),
        PrintAction::Borders,
        PrintAction::Reverse,
        PrintAction::Duplex(onionskin_print::Duplex::LongEdge),
    ] {
        window
            .update(cx, |frame, window, cx| {
                if frame.print_dialog().is_none() {
                    frame
                        .run_main_menu_command(MenuCommand::Print, window, cx)
                        .expect("opens");
                }
            })
            .unwrap();
        act(window, action, cx);
    }
    let preview = window
        .update(cx, |frame, _window, cx| frame.print_preview_sheets(cx))
        .unwrap()
        .expect("a preview");
    assert_eq!(preview.len(), 2, "one two-up sheet and its blank back");
    act(window, PrintAction::Print, cx);
    let output = dir.path().join("printed.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();

    let printed = onionskin_cos::Document::open_path(&output).expect("printed");
    assert_eq!(printed.page_count().expect("pages") as usize, preview.len());
    for (index, sheet) in preview.iter().enumerate() {
        let page = printed.page(index).expect("a sheet");
        let [x0, y0, x1, y1] = page.media_box.expect("a box");
        assert_eq!((x1 - x0, y1 - y0), (sheet.width, sheet.height));
        let drawn = page
            .resources
            .as_ref()
            .and_then(|resources| resources.get(b"XObject"))
            .and_then(onionskin_cos::Object::as_dict)
            .map_or(0, |xobjects| xobjects.iter().count());
        assert_eq!(drawn, sheet.placements.len(), "sheet {index}");
    }
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.dialog, None, "a print that went closes the dialog");
            assert!(frame
                .notices
                .iter()
                .any(|notice| notice.starts_with("Printed to")));
        })
        .unwrap();
}

#[gpui::test]
fn a_typed_range_prints_those_pages_and_a_backwards_one_is_refused(cx: &mut TestAppContext) {
    let (_dir, window, _bindings) = window_over("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::Pages(PagesChoice::Custom), cx);
    let type_pages = |text: &'static str, cx: &mut TestAppContext| {
        window
            .update(cx, |frame, _window, cx| {
                let input = frame.print_dialog().unwrap().pages.clone();
                input.update(cx, |input, cx| input.set_query(text, cx));
            })
            .unwrap();
    };
    type_pages("2-1", cx);
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            let error = tree.find(&"print-error".into()).expect("refused");
            assert!(error.label.contains("backwards"), "{}", error.label);
        })
        .unwrap();
    assert!(!cx.did_prompt_for_new_path(), "nothing was printed");
    type_pages("2", cx);
    let sources: Vec<usize> = window
        .update(cx, |frame, _window, cx| frame.print_preview_sheets(cx))
        .unwrap()
        .expect("a preview")
        .iter()
        .flat_map(|sheet| sheet.placements.iter().map(|placement| placement.source))
        .collect();
    assert_eq!(sources, [1], "page 2 only");
}

#[gpui::test]
fn page_setup_and_the_print_dialog_share_one_paper(cx: &mut TestAppContext) {
    let (_dir, window, bindings) = window_over("two-page.pdf", cx);
    cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "file.page-setup"));
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::PageSetup));
        })
        .unwrap();
    act(window, PrintAction::Paper(2), cx);
    act(
        window,
        PrintAction::Orientation(onionskin_print::Orientation::Portrait),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::CloseDialog, window, cx);
            frame.run_activation(
                Activation::CanvasContext(CanvasContextCommand::Print),
                window,
                cx,
            );
            assert_eq!(
                frame.dialog,
                Some(ShellDialog::Print),
                "the context menu opens it too"
            );
            let tree = frame.accessible(window, cx);
            let a4 = tree
                .find(&"print-paper".into())
                .unwrap()
                .children
                .iter()
                .find(|choice| choice.label == "A4")
                .expect("A4 is offered");
            assert_eq!(a4.state.selected, Some(true));
            let portrait = tree
                .find(&"print-orientation".into())
                .unwrap()
                .children
                .iter()
                .find(|choice| choice.label == "Portrait")
                .expect("Portrait is offered");
            assert_eq!(portrait.state.selected, Some(true));
            let sheets = frame.print_preview_sheets(cx).expect("a preview");
            assert_eq!(sheets[0].width, onionskin_print::PaperSize::A4.width);
        })
        .unwrap();
}

#[gpui::test]
fn an_encrypted_document_prints_as_images_and_the_box_says_why(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("locked.pdf");
    std::fs::copy(
        onionskin_corpus_testing::encrypted_fixture("r4-aes-128.pdf"),
        &path,
    )
    .expect("copies");
    let (dir, window, _bindings) = window_on(dir, path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
            let tree = frame.accessible(window, cx);
            let image = &tree.find(&"print-advanced".into()).unwrap().children[0];
            assert_eq!(image.label, "Print as Image");
            assert_eq!(image.state.toggled, Some(true));
            assert!(image.state.disabled);
            assert_eq!(
                image.description.as_deref(),
                Some(super::super::print::IMAGE_ONLY)
            );
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    let output = dir.path().join("locked printed.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let printed = onionskin_cos::Document::open_path(&output).expect("printed as images");
    assert!(!printed.is_encrypted());
}

/// Print one page of `window`'s document to a file with Summarize Comments
/// on, and read back how many sheets came out, or the dialog's error.
#[cfg(feature = "tools-comment")]
fn print_summarized(
    dir: &tempfile::TempDir,
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> Result<usize, String> {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::SummarizeComments, cx);
    act(window, PrintAction::Print, cx);
    let output = dir.path().join("summarized.pdf");
    let preparation_error = window
        .update(cx, |frame, _, _| {
            frame.print_dialog().and_then(|state| state.error.clone())
        })
        .unwrap();
    if let Some(error) = preparation_error {
        assert!(!cx.did_prompt_for_new_path());
        return Err(error);
    }
    assert!(cx.did_prompt_for_new_path());
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    Ok(onionskin_cos::Document::open_path(&output)
        .expect("printed")
        .page_count()
        .expect("pages") as usize)
}

/// Summarize Comments puts the comment summary's pages after the
/// document's: two document sheets and at least one of the summary.
#[cfg(feature = "tools-comment")]
#[gpui::test]
fn summarize_comments_prints_the_summary_after_the_document(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_over("two-page.pdf", cx);
    add_comment(window, cx);
    let sheets = print_summarized(&dir, window, cx).expect("prints");
    assert!(sheets > 2, "the summary follows the two pages: {sheets}");
}

/// A document with no comments has no summary to print: the dialog says so
/// and prints nothing, rather than printing without what was asked for.
#[cfg(feature = "tools-comment")]
#[gpui::test]
fn summarize_comments_with_nothing_to_summarize_says_so(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_over("two-page.pdf", cx);
    let error = print_summarized(&dir, window, cx).expect_err("refused");
    assert!(
        error.starts_with("The comments cannot be summarized"),
        "{error}"
    );
    assert!(!dir.path().join("summarized.pdf").exists());
}

/// Print freezes the submitted document before the destination chooser can
/// let another tab become active.
#[gpui::test]
fn a_pending_print_uses_the_document_that_was_submitted(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let (window, _bindings) = bound_window_in(
        &["two-page.pdf", "hello.pdf"],
        crate::config::ConfigPaths::default(),
        cx,
    );
    let mut submitted = onionskin_core::Document::open_path(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf"),
    )
    .expect("submitted document");
    let submitted_text = submitted
        .page_text(0)
        .expect("submitted page text")
        .flatten()
        .text;
    window
        .update(cx, |frame, window, cx| {
            frame.activate(0, cx);
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    assert!(cx.did_prompt_for_new_path());
    window
        .update(cx, |frame, _, cx| frame.activate(1, cx))
        .unwrap();
    let output = dir.path().join("pending.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();

    let mut printed = onionskin_core::Document::open_path(&output).expect("printed");
    let printed_text = printed
        .page_text(0)
        .expect("printed page text")
        .flatten()
        .text;
    assert_eq!(printed_text, submitted_text);
}

#[gpui::test]
fn cancelling_a_pending_print_writes_nothing(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_over("two-page.pdf", cx);
    let source = dir.path().join("two-page.pdf");
    let before_bytes = std::fs::read(&source).expect("reads source");
    let mut before_entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("reads directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    before_entries.sort();
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    window
        .update(cx, |frame, _, _| {
            let mut after_entries: Vec<_> = std::fs::read_dir(dir.path())
                .expect("reads directory")
                .map(|entry| entry.expect("entry").file_name())
                .collect();
            after_entries.sort();
            assert_eq!(after_entries, before_entries);
            assert_eq!(std::fs::read(&source).expect("reads source"), before_bytes);
            assert!(frame.notices.is_empty());
            assert_eq!(frame.dialog, None);
        })
        .unwrap();
}

#[gpui::test]
fn a_new_print_dialog_survives_an_older_chooser_callback(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let (window, _bindings) = bound_window(&["two-page.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens newer dialog");
        })
        .unwrap();
    let output = dir.path().join("older.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    window
        .update(cx, |frame, _, _| {
            assert_eq!(frame.dialog, Some(ShellDialog::Print));
            assert!(output.exists());
        })
        .unwrap();
}

#[gpui::test]
fn closing_the_source_tab_does_not_change_a_pending_print(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let (window, _bindings) = bound_window(&["two-page.pdf", "hello.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, _, cx| {
            frame
                .run_tab_command(TabCommand::Close, 0, cx)
                .expect("closes");
        })
        .unwrap();
    let output = dir.path().join("closed-source.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    assert_eq!(page_text(&output, 0), "Page one");
}

#[cfg(feature = "commands-core")]
#[gpui::test]
fn editing_the_source_after_submit_does_not_change_a_pending_print(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let (window, _bindings) = bound_window(&["two-page.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(
                Activation::MainMenu(MenuCommand::Page(PageCommand::Delete)),
                window,
                cx,
            );
            assert_eq!(
                frame
                    .tabs
                    .active()
                    .unwrap()
                    .canvas
                    .read(cx)
                    .model
                    .view_state()
                    .page_count,
                1
            );
        })
        .unwrap();
    let output = dir.path().join("edited-source.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let printed = onionskin_core::Document::open_path(&output).expect("printed");
    assert_eq!(printed.page_count(), 2);
    assert_eq!(page_text(&output, 0), "Page one");
}

#[cfg(feature = "tools-comment")]
#[gpui::test]
fn summary_setting_is_frozen_for_a_pending_print(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let (window, _bindings) = bound_window(&["two-page.pdf"], cx);
    add_comment(window, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::SummarizeComments, cx);
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens newer dialog");
            assert!(!frame.print_dialog().unwrap().settings.summarize_comments);
        })
        .unwrap();
    let output = dir.path().join("summary-snapshot.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let printed = onionskin_cos::Document::open_path(&output).expect("printed");
    assert!(printed.page_count().expect("pages") > 2);
    assert!(page_text(&output, 2).contains("Check the totals"));
    window
        .update(cx, |frame, _, _| {
            let state = frame.print_dialog().expect("newer dialog");
            assert_eq!(frame.dialog, Some(ShellDialog::Print));
            assert!(!state.settings.summarize_comments);
            assert!(state.error.is_none());
        })
        .unwrap();
}

#[gpui::test]
fn a_failed_pending_write_leaves_a_new_dialog_unchanged(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let (window, _bindings) = bound_window(&["two-page.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens newer dialog");
        })
        .unwrap();
    let output = dir.path().join("missing-parent").join("failed.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    assert!(!output.exists());
    window
        .update(cx, |frame, _, _| {
            assert_eq!(frame.dialog, Some(ShellDialog::Print));
            assert!(frame.print_dialog().unwrap().error.is_none());
            assert!(frame
                .notices
                .iter()
                .any(|notice| notice.contains("was not written")));
        })
        .unwrap();
}

/// Booklet from the dialog: its own controls replace Size and Multiple, and
/// the printed file is the booklet's landscape sides.
#[gpui::test]
fn choosing_booklet_shows_its_controls_and_prints_its_sides(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_over("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Booklet),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            let ids = print_ids(frame, window, cx);
            assert!(ids.iter().any(|id| id.contains("print-booklet-sides")));
            assert!(ids.iter().any(|id| id.contains("print-binding")));
            assert!(!ids.iter().any(|id| id.contains("print-n-up")), "{ids:?}");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    let output = dir.path().join("booklet.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let printed = onionskin_cos::Document::open_path(&output).expect("printed");
    // Two pages pad to four: one sheet, front and back.
    assert_eq!(printed.page_count().expect("pages"), 2);
    let [x0, y0, x1, y1] = printed.page(0).expect("a side").media_box.expect("a box");
    assert!(x1 - x0 > y1 - y0, "a booklet side is landscape");
}
