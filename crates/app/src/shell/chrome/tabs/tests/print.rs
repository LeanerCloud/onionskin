//! P17 on a real window: File > Print opens from its keystroke, every
//! control is in the tree and leaves it when the dialog closes, the preview
//! is the printed file's sheets, the Pages box refuses a backwards range,
//! and Page Setup and the Print dialog share one paper.

use super::*;
use crate::shell::chrome::accessible::Activation;
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
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let error = window
        .update(cx, |frame, _, _| {
            frame.print_dialog().and_then(|state| state.error.clone())
        })
        .unwrap();
    match error {
        Some(error) => Err(error),
        None => Ok(onionskin_cos::Document::open_path(&output)
            .expect("printed")
            .page_count()
            .expect("pages") as usize),
    }
}

/// Summarize Comments puts the comment summary's pages after the
/// document's: two document sheets and at least one of the summary.
#[cfg(feature = "tools-comment")]
#[gpui::test]
fn summarize_comments_prints_the_summary_after_the_document(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_over("two-page.pdf", cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().unwrap().clone();
            canvas.update(cx, |canvas, cx| {
                {
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
                }
                canvas.handle_change(Ok(true), cx);
            });
        })
        .unwrap();
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
