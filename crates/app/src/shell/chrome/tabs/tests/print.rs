//! P17 on a real window: File > Print opens from its keystroke, every
//! control is in the tree and leaves it when the dialog closes, the preview
//! is the printed file's sheets, the Pages box refuses a backwards range,
//! and Page Setup and the Print dialog share one paper.

use std::sync::Arc;

use super::*;
use crate::shell::chrome::accessible::Activation;
#[cfg(feature = "commands-core")]
use crate::shell::chrome::global_bar::PageCommand;
use crate::shell::chrome::print_dialog::{PagesChoice, PrintAction};
#[cfg(feature = "tools-comment")]
use crate::shell::chrome::summary_dialog::SummaryChoice;
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

fn window_letter_marked(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::WindowHandle<ShellFrame>,
    Vec<crate::keymap::Binding>,
) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("letter-marked.pdf");
    std::fs::write(&path, super::page_grid::letter_marked()).expect("writes Letter fixture");
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

fn saved_poster_geometry(path: &Path) -> Vec<([f64; 6], [f64; 4])> {
    let document = onionskin_cos::Document::open_path(path).expect("opens saved PDF");
    let count =
        usize::try_from(document.page_count().expect("page count")).expect("page count fits");
    (0..count)
        .map(|index| {
            let page = document.page(index).expect("saved sheet");
            assert_eq!(page.media_box, Some([0.0, 0.0, 612.0, 792.0]));
            let contents = document
                .resolve(page.dict.get(b"Contents").expect("contents"))
                .expect("resolves contents");
            let data = document
                .decode_stream(contents.as_stream().expect("content stream"))
                .expect("decodes contents");
            let tokens: Vec<_> = std::str::from_utf8(&data)
                .expect("ASCII content stream")
                .split_ascii_whitespace()
                .collect();
            assert_eq!(tokens.len(), 25, "one clipped placement stream");
            assert_eq!(tokens[0], "q");
            assert_eq!(tokens[5], "re");
            assert_eq!(tokens[6], "W");
            assert_eq!(tokens[7], "n");
            assert_eq!(tokens[14], "cm");
            assert_eq!(tokens[21], "cm");
            assert_eq!(tokens[22], "/P0");
            assert_eq!(tokens[23], "Do");
            assert_eq!(tokens[24], "Q");
            let rect: [f64; 4] = tokens[1..5]
                .iter()
                .map(|token| token.parse().expect("clip number"))
                .collect::<Vec<_>>()
                .try_into()
                .expect("four clip numbers");
            assert_eq!(rect, [0.0, 0.0, 612.0, 792.0]);
            let transform: [f64; 6] = tokens[8..14]
                .iter()
                .map(|token| token.parse().expect("placement number"))
                .collect::<Vec<_>>()
                .try_into()
                .expect("six placement numbers");
            let inner: [f64; 6] = tokens[15..21]
                .iter()
                .map(|token| token.parse().expect("form number"))
                .collect::<Vec<_>>()
                .try_into()
                .expect("six form numbers");
            assert_eq!(inner, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            (transform, [0.0, 0.0, 612.0, 792.0])
        })
        .collect()
}

fn assert_saved_mark(render: &onionskin_core::PageRender, point: (f64, f64), red: bool) {
    let width = render.raster.width() as usize;
    let height = render.raster.height() as usize;
    let (x, y) = point;
    let raster_y = height as f64 - y;
    let found = ((raster_y.floor() as isize - 2)..=(raster_y.ceil() as isize + 2)).any(|row| {
        ((x.floor() as isize - 2)..=(x.ceil() as isize + 2)).any(|column| {
            if row < 0 || column < 0 || row >= height as isize || column >= width as isize {
                return false;
            }
            let offset = (row as usize * width + column as usize) * 4;
            let pixel = &render.raster.rgba()[offset..offset + 4];
            pixel[3] > 200
                && if red {
                    pixel[0] > 200 && pixel[1] < 80 && pixel[2] < 80
                } else {
                    pixel[2] > 200 && pixel[0] < 80 && pixel[1] < 80
                }
        })
    });
    assert!(
        found,
        "missing {} mark near {point:?}",
        if red { "red" } else { "blue" }
    );
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

#[gpui::test]
fn poster_controls_editable_fields_save_fractional_letter(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_letter_marked(cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens print dialog");
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    act(window, PrintAction::CutMarks, cx);
    window
        .update(cx, |frame, _window, _cx| {
            assert!(
                !frame
                    .print_dialog()
                    .expect("dialog")
                    .settings
                    .poster
                    .cut_marks
            );
        })
        .unwrap();
    window
        .update(cx, |frame, window, cx| {
            let dialog = frame.print_dialog().expect("dialog");
            dialog
                .poster_scale
                .update(cx, |input, cx| input.set_query("", cx));
            dialog
                .poster_overlap
                .update(cx, |input, cx| input.set_query("", cx));
            frame.run_activation(
                Activation::Focus(crate::shell::chrome::accessible::TextField::PrintPosterScale),
                window,
                cx,
            );
            let tree = frame.accessible(window, cx);
            for (id, label) in [
                ("print-poster-scale", "Tile Scale (%)"),
                ("print-poster-overlap", "Overlap (in)"),
            ] {
                let field = tree.find(&id.into()).expect("Poster field");
                assert_eq!(field.label, label);
                assert_eq!(field.role, accesskit::Role::NumberInput);
            }
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "125.5%");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "tab");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), ".125");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some("print-poster-overlap".into())
            );
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintPosterOverlap)
                .expect("overlap field")
                .read(cx)
                .focus_handle(cx)
                .is_focused(window));
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-poster-scale".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("125.5%")
            );
            assert_eq!(
                tree.find(&"print-poster-overlap".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some(".125")
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some(gpui::ElementId::NamedInteger(
                    "print-poster-marks".into(),
                    0
                ))
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some("print-poster-overlap".into())
            );
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintPosterOverlap)
                .expect("overlap field")
                .read(cx)
                .focus_handle(cx)
                .is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some("print-poster-scale".into())
            );
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintPosterScale)
                .expect("scale field")
                .read(cx)
                .focus_handle(cx)
                .is_focused(window));
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-poster-scale".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("125.5%")
            );
            assert_eq!(
                tree.find(&"print-poster-overlap".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some(".125")
            );
        })
        .unwrap();
    for handling in [
        crate::shell::chrome::print_dialog::HandlingChoice::Pages,
        crate::shell::chrome::print_dialog::HandlingChoice::Booklet,
        crate::shell::chrome::print_dialog::HandlingChoice::Poster,
    ] {
        act(window, PrintAction::Handling(handling), cx);
    }
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-poster-scale".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("125.5%")
            );
            assert_eq!(
                tree.find(&"print-poster-overlap".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some(".125")
            );
        })
        .unwrap();
    let preview = window
        .update(cx, |frame, _window, cx| frame.print_preview_sheets(cx))
        .unwrap()
        .expect("fractional preview");
    assert_eq!(preview.len(), 4);
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(
                frame.print_dialog().expect("dialog").printed.page_sizes,
                vec![(612.0, 792.0)]
            );
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    let output = dir.path().join("poster-fractional.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let document = onionskin_core::Document::open_path(&output).expect("saved Poster output");
    assert_eq!(document.page_count(), 4);
    let geometries = saved_poster_geometry(&output);
    let expected_matrices = [
        [1.255, 0.0, 0.0, 1.255, 0.0, -201.96],
        [1.255, 0.0, 0.0, 1.255, -603.0, -201.96],
        [1.255, 0.0, 0.0, 1.255, 0.0, 581.04],
        [1.255, 0.0, 0.0, 1.255, -603.0, 581.04],
    ];
    assert_eq!(geometries.len(), expected_matrices.len());
    for ((matrix, clip), expected) in geometries.iter().zip(expected_matrices) {
        for (actual, expected) in matrix.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_eq!(*clip, [0.0, 0.0, 612.0, 792.0]);
    }
    let mut parsed = onionskin_core::Document::open_path(&output).expect("reopens saved PDF");
    for (sheet, point, red) in [
        (0, (607.42, 300.04), true),
        (1, (4.42, 300.04), true),
        (0, (251.0, 5.115), false),
        (2, (251.0, 788.115), false),
    ] {
        let render = parsed
            .render_page_now(sheet, 1.0)
            .expect("renders saved sheet");
        assert_saved_mark(&render, point, red);
    }
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("reopens print dialog");
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    act(window, PrintAction::CutMarks, cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-poster-scale".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("200")
            );
            assert_eq!(
                tree.find(&"print-poster-overlap".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("0.25")
            );
        })
        .unwrap();
}

#[gpui::test]
fn poster_controls_invalid_text_keeps_chooser_closed_and_hides_when_switching_modes(
    cx: &mut TestAppContext,
) {
    let (_dir, window, _bindings) = window_letter_marked(cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens")
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    window
        .update(cx, |frame, _window, cx| {
            let dialog = frame.print_dialog().expect("dialog");
            dialog
                .poster_scale
                .update(cx, |input, cx| input.set_query("1e2", cx));
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    assert!(!cx.did_prompt_for_new_path());
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::Print));
            assert!(frame
                .accessible(window, cx)
                .find(&"print-error".into())
                .is_some());
        })
        .unwrap();
    window
        .update(cx, |frame, _window, cx| {
            let dialog = frame.print_dialog().expect("dialog");
            dialog
                .poster_scale
                .update(cx, |input, cx| input.set_query("125.5", cx));
            dialog
                .poster_overlap
                .update(cx, |input, cx| input.set_query("0.125in", cx));
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    assert!(!cx.did_prompt_for_new_path());
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::Print));
            assert!(frame
                .accessible(window, cx)
                .find(&"print-error".into())
                .is_some());
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Pages),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintPosterScale)
                .is_none());
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintPosterOverlap)
                .is_none());
            assert!(frame
                .print_dialog()
                .unwrap()
                .job(crate::shell::chrome::print_dialog::PageSetup::default(), cx)
                .is_ok());
            assert!(frame
                .accessible(window, cx)
                .find(&"print-poster-scale".into())
                .is_none());
            assert!(frame
                .accessible(window, cx)
                .find(&"print-poster-overlap".into())
                .is_none());
        })
        .unwrap();
}

#[gpui::test]
fn poster_controls_shrinking_preview_uses_effective_index(cx: &mut TestAppContext) {
    let (_dir, window, _bindings) = window_letter_marked(cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens")
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    act(window, PrintAction::CutMarks, cx);
    window
        .update(cx, |frame, _window, cx| {
            let dialog = frame.print_dialog().expect("dialog");
            dialog
                .poster_scale
                .update(cx, |input, cx| input.set_query("125.5", cx));
            dialog
                .poster_overlap
                .update(cx, |input, cx| input.set_query(".125", cx));
        })
        .unwrap();
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(
                frame.print_dialog().expect("dialog").printed.page_sizes,
                vec![(612.0, 792.0)]
            );
        })
        .unwrap();
    assert_eq!(
        window
            .update(cx, |frame, _window, cx| frame
                .print_preview_sheets(cx)
                .unwrap()
                .len())
            .unwrap(),
        4
    );
    act(window, PrintAction::PreviewNext, cx);
    act(window, PrintAction::PreviewNext, cx);
    act(window, PrintAction::PreviewNext, cx);
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.print_dialog().expect("dialog").preview_index(4), 3);
        })
        .unwrap();
    window
        .update(cx, |frame, _window, cx| {
            let dialog = frame.print_dialog().expect("dialog");
            dialog
                .poster_scale
                .update(cx, |input, cx| input.set_query("100", cx));
            dialog
                .poster_overlap
                .update(cx, |input, cx| input.set_query("0", cx));
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw_window(&mut visual);
    window
        .update(&mut visual, |frame, window, cx| {
            assert_eq!(frame.print_dialog().expect("dialog").preview_index(1), 0);
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"print-preview-previous".into())
                    .unwrap()
                    .state
                    .disabled
            );
            assert!(
                tree.find(&"print-preview-next".into())
                    .unwrap()
                    .state
                    .disabled
            );
            assert!(tree
                .find(&"print-preview".into())
                .unwrap()
                .label
                .contains("sheet 1 of 1"));
        })
        .unwrap();
}

#[cfg(feature = "commands-core")]
#[gpui::test]
fn poster_controls_pending_chooser_freezes_source_and_new_dialog(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let hello =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf"))
            .expect("reads second tab");
    let (window, _bindings) = bound_window_from_bytes(
        vec![
            ("letter-marked.pdf", super::page_grid::letter_marked()),
            ("hello.pdf", hello),
        ],
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.activate(0, cx);
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens Poster source dialog");
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    act(window, PrintAction::CutMarks, cx);
    window
        .update(cx, |frame, _window, cx| {
            let dialog = frame.print_dialog().expect("dialog");
            assert!(!dialog.settings.poster.cut_marks);
            dialog
                .poster_scale
                .update(cx, |input, cx| input.set_query("125.5%", cx));
            dialog
                .poster_overlap
                .update(cx, |input, cx| input.set_query(".125", cx));
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    assert!(cx.did_prompt_for_new_path());
    window
        .update(cx, |frame, window, cx| {
            frame.activate(0, cx);
            frame.run_activation(
                Activation::MainMenu(MenuCommand::Page(PageCommand::RotateClockwise)),
                window,
                cx,
            );
            let canvas = frame.tabs.active().expect("source tab").canvas.clone();
            canvas.update(cx, |canvas, _| {
                assert_eq!(
                    canvas
                        .model
                        .document_mut()
                        .page_geometry(0)
                        .expect("rotated geometry")
                        .render_size,
                    (792.0, 612.0)
                );
            });
            frame.activate(1, cx);
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens newer dialog");
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            let dialog = frame.print_dialog().expect("newer dialog");
            dialog
                .poster_scale
                .update(cx, |input, cx| input.set_query("100", cx));
            dialog
                .poster_overlap
                .update(cx, |input, cx| input.set_query("0", cx));
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-poster-scale".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("100")
            );
            assert_eq!(
                tree.find(&"print-poster-overlap".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("0")
            );
        })
        .unwrap();
    let output = dir.path().join("pending-poster.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let printed = onionskin_core::Document::open_path(&output).expect("saved pending Poster");
    assert_eq!(printed.page_count(), 4);
    let geometries = saved_poster_geometry(&output);
    let expected = [
        [1.255, 0.0, 0.0, 1.255, 0.0, -201.96],
        [1.255, 0.0, 0.0, 1.255, -603.0, -201.96],
        [1.255, 0.0, 0.0, 1.255, 0.0, 581.04],
        [1.255, 0.0, 0.0, 1.255, -603.0, 581.04],
    ];
    for ((matrix, clip), expected) in geometries.iter().zip(expected) {
        for (actual, expected) in matrix.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_eq!(*clip, [0.0, 0.0, 612.0, 792.0]);
    }
    let mut parsed = onionskin_core::Document::open_path(&output).expect("reopens pending Poster");
    for (sheet, point, red) in [
        (0, (607.42, 300.04), true),
        (1, (4.42, 300.04), true),
        (0, (251.0, 5.115), false),
        (2, (251.0, 788.115), false),
    ] {
        let render = parsed
            .render_page_now(sheet, 1.0)
            .expect("renders pending sheet");
        assert_saved_mark(&render, point, red);
    }
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::Print));
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-poster-scale".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("100")
            );
            assert_eq!(
                tree.find(&"print-poster-overlap".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("0")
            );
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
fn booklet_sheet_range_prints_the_selected_physical_sheet(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("numbered.pdf");
    std::fs::write(&source, super::page_grid::numbered(8)).expect("writes source");
    let (_dir, window, _bindings) = window_on(dir, source.clone(), cx);
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
        .update(cx, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("2", cx));
            state
                .booklet_to
                .update(cx, |input, cx| input.set_query("2", cx));
        })
        .unwrap();
    let preview = window
        .update(cx, |frame, _window, cx| frame.print_preview_sheets(cx))
        .unwrap()
        .expect("preview");
    assert_eq!(preview.len(), 2);
    assert_eq!(
        preview
            .iter()
            .flat_map(|sheet| sheet.placements.iter().map(|placement| placement.source))
            .collect::<Vec<_>>(),
        [5, 2, 3, 4]
    );
    act(window, PrintAction::Print, cx);
    let output = source.with_file_name("printed.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let printed = onionskin_cos::Document::open_path(&output).expect("printed");
    assert_eq!(printed.page_count().expect("pages"), 2);
    for page in 0..2 {
        let [x0, y0, x1, y1] = printed.page(page).expect("page").media_box.expect("box");
        assert_eq!((x1 - x0, y1 - y0), (792.0, 612.0));
    }
    let text = (0..2)
        .map(|page| page_text(&output, page))
        .collect::<Vec<_>>();
    assert!(text[0].contains("Page 6") && text[0].contains("Page 3"));
    assert!(text[1].contains("Page 4") && text[1].contains("Page 5"));
    assert!(text
        .iter()
        .all(|page| !page.contains("Page 1") && !page.contains("Page 2")));
    let mut parsed = onionskin_core::Document::open_path(&output).expect("opens output");
    for (page, expected) in [
        (0, [("Page 6", true), ("Page 3", false)]),
        (1, [("Page 4", true), ("Page 5", false)]),
    ] {
        let page_text = parsed.page_text(page).expect("text");
        let labels = page_text
            .runs
            .iter()
            .map(|run| run.text.trim())
            .filter(|text| text.starts_with("Page "))
            .collect::<Vec<_>>();
        let expected_labels = if page == 0 {
            vec!["Page 6", "Page 3"]
        } else {
            vec!["Page 4", "Page 5"]
        };
        assert_eq!(labels, expected_labels, "exact labels on side {page}");
        for (needle, left) in expected {
            let run = page_text
                .runs
                .iter()
                .find(|run| run.text.contains(needle))
                .expect("text run");
            let x = run
                .glyphs
                .iter()
                .flat_map(|glyph| glyph.quad.corners.into_iter().map(|(x, _)| x))
                .sum::<f64>()
                / (run.glyphs.len() * 4) as f64;
            assert_eq!(x < 396.0, left, "{needle} side");
        }
        let render = parsed.render_page_now(page, 1.0).expect("renders");
        let width = render.raster.width() as usize;
        let height = render.raster.height() as usize;
        for (start, end) in [(0, width / 2), (width / 2, width)] {
            assert!((0..height).any(|row| {
                (start..end).any(|column| {
                    let offset = (row * width + column) * 4;
                    let px = &render.raster.rgba()[offset..offset + 4];
                    px[3] > 200 && px[0] < 80 && px[1] < 80 && px[2] < 80
                })
            }));
        }
    }
}

#[cfg(feature = "commands-core")]
#[gpui::test]
fn booklet_sheet_range_pending_chooser_keeps_submitted_composition(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("numbered.pdf");
    std::fs::write(&source, super::page_grid::numbered(8)).expect("writes source");
    let second = dir.path().join("hello.pdf");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf"),
        &second,
    )
    .expect("copies second source");
    let model = |path: &Path| {
        CanvasModel::new(
            onionskin_core::Document::open_path(path).expect("opens"),
            crate::build_registry(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("model")
    };
    let (window, _bindings) = bound_window_with_models(
        vec![
            (source.clone(), model(&source)),
            (second, model(&dir.path().join("hello.pdf"))),
        ],
        crate::config::ConfigPaths::default(),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.activate(0, cx);
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
        .update(cx, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("2", cx));
            state
                .booklet_to
                .update(cx, |input, cx| input.set_query("2", cx));
        })
        .unwrap();
    window
        .update(cx, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            assert_eq!(state.booklet_from.read(cx).query(), "2");
            assert_eq!(state.booklet_to.read(cx).query(), "2");
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    assert!(cx.did_prompt_for_new_path());
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
                    .expect("source tab")
                    .canvas
                    .read(cx)
                    .model
                    .view_state()
                    .page_count,
                7
            );
            frame.activate(1, cx);
        })
        .unwrap();
    let output = dir.path().join("pending-booklet.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    let mut printed = onionskin_core::Document::open_path(&output).expect("printed");
    assert_eq!(printed.page_count(), 2);
    for (page, expected, excluded) in [
        (
            0,
            ["Page 6", "Page 3"],
            ["Page 1", "Page 2", "Page 4", "Page 5"],
        ),
        (
            1,
            ["Page 4", "Page 5"],
            ["Page 1", "Page 2", "Page 3", "Page 6"],
        ),
    ] {
        let text = printed.page_text(page).expect("text").flatten().text;
        for needle in expected {
            assert!(text.contains(needle), "{needle} missing from side {page}");
        }
        for needle in excluded {
            assert!(!text.contains(needle), "{needle} leaked onto side {page}");
        }
    }
    for (page, expected) in [(0, ["Page 6", "Page 3"]), (1, ["Page 4", "Page 5"])] {
        let parsed_page = printed.page_text(page).expect("parsed text");
        let labels = parsed_page
            .runs
            .iter()
            .map(|run| run.text.trim())
            .filter(|text| text.starts_with("Page "))
            .collect::<Vec<_>>();
        assert_eq!(labels, expected.to_vec(), "exact labels on side {page}");
        for (index, needle) in expected.into_iter().enumerate() {
            let run = parsed_page
                .runs
                .iter()
                .find(|run| run.text.contains(needle))
                .expect("text run");
            let x = run
                .glyphs
                .iter()
                .flat_map(|glyph| glyph.quad.corners.into_iter().map(|(x, _)| x))
                .sum::<f64>()
                / (run.glyphs.len() * 4) as f64;
            assert_eq!(x < 396.0, index == 0, "{needle} side");
        }
    }
}

#[gpui::test]
fn booklet_sheet_range_narrowing_keeps_preview_and_navigation_in_bounds(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("numbered.pdf");
    std::fs::write(&source, super::page_grid::numbered(8)).expect("writes source");
    let (_dir, window, _bindings) = window_on(dir, source, cx);
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
    for _ in 0..3 {
        act(window, PrintAction::PreviewNext, cx);
    }
    window
        .update(cx, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            assert_eq!(state.preview_sheet, 3);
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("2", cx));
        })
        .unwrap();
    window
        .update(cx, |frame, _window, _cx| {
            let state = frame.print_dialog().expect("print dialog");
            assert_eq!(state.preview_sheet, 3);
            assert_eq!(state.preview_index(2), 1);
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw_window(&mut visual);
    draw_window(&mut visual);
    window
        .update(&mut visual, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-preview".into()).expect("preview").label,
                "Preview: sheet 2 of 2, landscape, pages 4, 5"
            );
            assert!(
                !tree
                    .find(&"print-preview-previous".into())
                    .unwrap()
                    .state
                    .disabled
            );
            assert!(
                tree.find(&"print-preview-next".into())
                    .unwrap()
                    .state
                    .disabled
            );
        })
        .unwrap();
    visual.run_until_parked();
    act(window, PrintAction::PreviewPrevious, cx);
    draw_window(&mut visual);
    window
        .update(&mut visual, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-preview".into()).expect("preview").label,
                "Preview: sheet 1 of 2, landscape, pages 6, 3"
            );
            assert!(
                tree.find(&"print-preview-previous".into())
                    .unwrap()
                    .state
                    .disabled
            );
            assert!(
                !tree
                    .find(&"print-preview-next".into())
                    .unwrap()
                    .state
                    .disabled
            );
        })
        .unwrap();
    act(window, PrintAction::PreviewNext, cx);
    draw_window(&mut visual);
    window
        .update(&mut visual, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-preview".into()).expect("preview").label,
                "Preview: sheet 2 of 2, landscape, pages 4, 5"
            );
        })
        .unwrap();
    window
        .update(&mut visual, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("3", cx));
        })
        .unwrap();
    draw_window(&mut visual);
    window
        .update(&mut visual, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-preview".into()).unwrap().label,
                "No preview until the settings are fixed"
            );
            assert!(
                tree.find(&"print-preview-previous".into())
                    .unwrap()
                    .state
                    .disabled
            );
            assert!(
                tree.find(&"print-preview-next".into())
                    .unwrap()
                    .state
                    .disabled
            );
        })
        .unwrap();
    window
        .update(&mut visual, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("2", cx));
        })
        .unwrap();
    draw_window(&mut visual);
    window
        .update(&mut visual, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-preview".into()).unwrap().label,
                "Preview: sheet 2 of 2, landscape, pages 4, 5"
            );
        })
        .unwrap();
}

#[gpui::test]
fn booklet_sheet_range_validation_and_focus_switching(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("numbered.pdf");
    std::fs::write(&source, super::page_grid::numbered(8)).expect("writes source");
    let (_dir, window, _bindings) = window_on(dir, source, cx);
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
            let tree = frame.accessible(window, cx);
            for (id, label, value) in [
                ("print-booklet-from", "Sheets from", "1"),
                ("print-booklet-to", "To", "2"),
            ] {
                let field = tree.find(&id.into()).expect("booklet field");
                assert_eq!(field.label, label);
                assert_eq!(field.role, accesskit::Role::NumberInput);
                assert_eq!(field.value.as_deref(), Some(value));
            }
            assert!(tree.find(&"print-pages-group".into()).is_some());
        })
        .unwrap();
    window
        .update(cx, |frame, window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("", cx));
            state
                .booklet_to
                .update(cx, |input, cx| input.set_query("", cx));
            frame.run_activation(
                Activation::Focus(crate::shell::chrome::accessible::TextField::PrintBookletFrom),
                window,
                cx,
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "3");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some("print-booklet-to".into())
            );
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletTo)
                .expect("to field")
                .read(cx)
                .focus_handle(cx)
                .is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "3");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some(gpui::ElementId::NamedInteger("print-binding".into(), 0))
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some("print-booklet-to".into())
            );
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletTo)
                .expect("to field")
                .read(cx)
                .focus_handle(cx)
                .is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some("print-booklet-from".into())
            );
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletFrom)
                .expect("from field")
                .read(cx)
                .focus_handle(cx)
                .is_focused(window));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert!(matches!(
                frame.a11y.published_focus(),
                Some(gpui::ElementId::NamedInteger(name, 0)) if name == "print-booklet-sides"
            ));
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "tab");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            assert_eq!(
                frame.a11y.published_focus(),
                Some("print-booklet-from".into())
            );
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletFrom)
                .expect("from field")
                .read(cx)
                .focus_handle(cx)
                .is_focused(window));
        })
        .unwrap();
    window
        .update(cx, |frame, window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            assert_eq!(state.booklet_from.read(cx).query(), "3");
            assert_eq!(state.booklet_to.read(cx).query(), "3");
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"print-booklet-from".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("3")
            );
            assert_eq!(
                tree.find(&"print-booklet-to".into())
                    .unwrap()
                    .value
                    .as_deref(),
                Some("3")
            );
        })
        .unwrap();
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"print-error".into()).is_some());
        })
        .unwrap();
    assert!(
        !cx.did_prompt_for_new_path(),
        "invalid interval does not open chooser"
    );
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Pages),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"print-booklet-sheets".into()).is_none());
            assert!(tree.find(&"print-n-up".into()).is_some());
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletFrom)
                .is_none());
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletTo)
                .is_none());
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"print-booklet-sheets".into()).is_none());
            assert!(tree.find(&"print-poster".into()).is_some());
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletFrom)
                .is_none());
            assert!(frame
                .text_field(crate::shell::chrome::accessible::TextField::PrintBookletTo)
                .is_none());
        })
        .unwrap();
    window
        .update(cx, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("2", cx));
            state
                .booklet_to
                .update(cx, |input, cx| input.set_query("2", cx));
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Booklet),
        cx,
    );
    let valid_preview = window
        .update(cx, |frame, _window, cx| frame.print_preview_sheets(cx))
        .unwrap()
        .expect("valid booklet interval preview");
    assert_eq!(valid_preview.len(), 2);
    assert_eq!(
        valid_preview
            .iter()
            .flat_map(|sheet| sheet.placements.iter().map(|placement| placement.source))
            .collect::<Vec<_>>(),
        [5, 2, 3, 4]
    );
    act(window, PrintAction::Pages(PagesChoice::Current), cx);
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Booklet),
        cx,
    );
    act(window, PrintAction::Print, cx);
    window
        .update(cx, |frame, window, cx| {
            assert!(frame
                .accessible(window, cx)
                .find(&"print-error".into())
                .is_some());
        })
        .unwrap();
    window
        .update(cx, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            state
                .booklet_from
                .update(cx, |input, cx| input.set_query("1", cx));
            state
                .booklet_to
                .update(cx, |input, cx| input.set_query("1", cx));
        })
        .unwrap();
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Booklet),
        cx,
    );
    act(window, PrintAction::Print, cx);
    let output = _dir.path().join("fixed-booklet.pdf");
    let answer = output.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();
    assert!(output.exists(), "corrected interval prints successfully");
}

/// Baseline-compatible behavioral proof: before the interval feature these
/// existing Print/Booklet actions ran but the two physical-sheet controls were
/// absent from the accessible tree, so this exact test would fail at runtime.
#[gpui::test]
fn booklet_sheet_range_publishes_physical_sheet_inputs(cx: &mut TestAppContext) {
    let (_dir, window, _bindings) = window_over("two-page.pdf", cx);
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
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"print-booklet-from".into()).is_some());
            assert!(tree.find(&"print-booklet-to".into()).is_some());
        })
        .unwrap();
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

/// A combined Poster operation is rejected before the Save as PDF chooser
/// when its main document plus generated comment appendix exceeds the cap.
#[cfg(feature = "tools-comment")]
#[gpui::test]
fn poster_controls_summarize_comments_over_cap_keeps_chooser_closed(cx: &mut TestAppContext) {
    let (dir, window, _bindings) = window_over("two-page.pdf", cx);
    add_comment(window, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
        })
        .unwrap();
    act(window, PrintAction::SummarizeComments, cx);
    act(
        window,
        PrintAction::Handling(crate::shell::chrome::print_dialog::HandlingChoice::Poster),
        cx,
    );
    act(window, PrintAction::Pages(PagesChoice::Custom), cx);
    window
        .update(cx, |frame, _window, cx| {
            let pages = frame.print_dialog().expect("print dialog").pages.clone();
            pages.update(cx, |input, cx| input.set_query("1-2,2", cx));
        })
        .unwrap();
    // At 2800%, the three selected main pages fit under 1024, while the
    // one-page comment appendix pushes the combined operation over the cap.
    window
        .update(cx, |frame, _window, cx| {
            let scale = frame
                .print_dialog()
                .expect("print dialog")
                .poster_scale
                .clone();
            scale.update(cx, |input, cx| input.set_query("2800", cx));
        })
        .unwrap();
    let main_sheets = window
        .update(cx, |frame, _window, cx| frame.print_preview_sheets(cx))
        .unwrap()
        .expect("main preview");
    assert!(main_sheets.len() <= onionskin_print::MAX_POSTER_SHEETS);
    let (main_count, appendix_count) = window
        .update(cx, |frame, _window, cx| {
            let state = frame.print_dialog().expect("print dialog");
            let job = state.job(frame.page_setup(), cx).expect("poster job");
            let onionskin_print::Handling::Poster(poster) = job.handling else {
                panic!("poster handling");
            };
            let canvas = frame.active_canvas().expect("canvas").clone();
            canvas.update(cx, |canvas, _| {
                let mut document = canvas.model.document_mut();
                let main_bytes = document.preview_bytes(job.comments).expect("main bytes");
                let summary_bytes =
                    super::super::summary::summarize(&mut document, SummaryChoice::CommentsOnly)
                        .expect("summary bytes");
                let mut main_backend =
                    onionskin_print::FileBackend::new(main_bytes).expect("main backend");
                let mut appendix_backend =
                    onionskin_print::FileBackend::new(Arc::new(summary_bytes))
                        .expect("appendix backend");
                let main_sizes = main_backend.page_sizes().expect("main sizes");
                let appendix_sizes = appendix_backend.page_sizes().expect("appendix sizes");
                (
                    onionskin_print::poster_sheet_count(&job, poster, &main_sizes)
                        .expect("main count"),
                    onionskin_print::poster_sheet_count(
                        &onionskin_print::appendix_job(&job),
                        poster,
                        &appendix_sizes,
                    )
                    .expect("appendix count"),
                )
            })
        })
        .unwrap();
    assert!(main_count <= onionskin_print::MAX_POSTER_SHEETS);
    assert!(appendix_count <= onionskin_print::MAX_POSTER_SHEETS);
    assert!(main_count + appendix_count > onionskin_print::MAX_POSTER_SHEETS);
    act(window, PrintAction::Print, cx);
    let error = window
        .update(cx, |frame, _, _| {
            frame.print_dialog().and_then(|state| state.error.clone())
        })
        .unwrap()
        .expect("over-cap error");
    assert!(error.contains("limit of 1024"), "{error}");
    assert!(!cx.did_prompt_for_new_path());
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
