//! M5 page marks on a real window: Edit > Header & Footer, Watermark,
//! Background and Bates Numbering open one dialog that adds a mark to the
//! pages, then updates or removes it, and says what it did.

use super::page_grid::numbered;
use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::marks_dialog::{MarkAction, MarkField, Source};
use crate::shell::dialog::ShellDialog;
use onionskin_core::pages::MarkKind;

fn window(pages: usize, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let window = bound_window_from_bytes(vec![("marks.pdf", numbered(pages))], cx).0;
    cx.run_until_parked();
    window
}

fn open(window: gpui::WindowHandle<ShellFrame>, kind: MarkKind, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::PageMarks(kind), window, cx)
                .expect("opens");
            assert_eq!(frame.dialog, Some(ShellDialog::Marks(kind)));
        })
        .unwrap();
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: MarkAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Marks(action), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

fn type_into(
    window: gpui::WindowHandle<ShellFrame>,
    field: MarkField,
    text: &str,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |frame, _, cx| {
            let input = frame.marks.as_ref().expect("open").inputs[&field].clone();
            input.update(cx, |input, cx| input.set_query(text, cx));
        })
        .unwrap();
}

/// Every page's extracted text, joined per page.
fn page_texts(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                let count = canvas.model.viewport().page_count();
                let mut document = canvas.model.document_mut();
                (0..count)
                    .map(|page| {
                        let mut runs: Vec<String> = document
                            .page_text(page)
                            .expect("extracts")
                            .runs
                            .iter()
                            .map(|run| run.text.clone())
                            .collect();
                        runs.sort();
                        runs.join(" | ")
                    })
                    .collect()
            })
        })
        .unwrap()
}

fn notices(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, _, _| frame.notices.clone())
        .unwrap()
}

fn labels(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, window, cx| {
            frame
                .accessible(window, cx)
                .walk()
                .filter(|element| format!("{:?}", element.key).contains("mark-"))
                .map(|element| element.label.clone())
                .collect()
        })
        .unwrap()
}

#[gpui::test]
fn a_header_and_footer_is_added_updated_and_removed(cx: &mut TestAppContext) {
    let window = window(2, cx);
    open(window, MarkKind::HeaderFooter, cx);
    assert!(labels(window, cx).contains(&"Add".to_owned()));
    act(window, MarkAction::Submit, cx);
    assert_eq!(
        window.update(cx, |frame, _, _| frame.dialog).unwrap(),
        None,
        "done, so closed"
    );
    assert_eq!(
        page_texts(window, cx),
        ["Page 1 | Page 1 of 2", "Page 2 | Page 2 of 2"]
    );
    assert!(notices(window, cx).contains(&"Added the header and footer on 2 pages.".to_owned()));

    // Opened again, it offers Update and Remove, on what it was made with.
    open(window, MarkKind::HeaderFooter, cx);
    let offered = labels(window, cx);
    assert!(offered.contains(&"Update".to_owned()) && offered.contains(&"Remove".to_owned()));
    type_into(window, MarkField::Size, "14", cx);
    act(window, MarkAction::NextFont, cx);
    act(window, MarkAction::Submit, cx);
    open(window, MarkKind::HeaderFooter, cx);
    let reopened = window
        .update(cx, |frame, _, cx| {
            let state = frame.marks.as_ref().expect("open");
            (
                state.inputs[&MarkField::Size].read(cx).query().to_owned(),
                state.inputs[&MarkField::CenterFooter]
                    .read(cx)
                    .query()
                    .to_owned(),
                state.form.font,
            )
        })
        .unwrap();
    assert_eq!(
        reopened,
        (
            "14".to_owned(),
            "Page [page] of [pages]".to_owned(),
            onionskin_tools_edit::marks::Font::HelveticaBold
        )
    );
    type_into(window, MarkField::CenterFooter, "", cx);
    type_into(window, MarkField::RightHeader, "Draft [date]", cx);
    act(window, MarkAction::Submit, cx);
    let texts = page_texts(window, cx);
    assert!(texts[0].contains("Draft 20"), "{texts:?}");
    assert!(!texts[0].contains("of 2"), "replaced: {texts:?}");

    open(window, MarkKind::HeaderFooter, cx);
    act(window, MarkAction::Remove, cx);
    assert_eq!(page_texts(window, cx), ["Page 1", "Page 2"]);
    assert!(notices(window, cx).contains(&"Removed the header and footer from 2 pages.".to_owned()));
}

#[gpui::test]
fn a_watermark_needs_its_text_and_marks_the_chosen_page(cx: &mut TestAppContext) {
    let window = window(2, cx);
    open(window, MarkKind::Watermark, cx);
    type_into(window, MarkField::Text, " ", cx);
    act(window, MarkAction::Submit, cx);
    let error = window
        .update(cx, |frame, _, _| {
            frame.marks_dialog().and_then(|s| s.error.clone())
        })
        .unwrap();
    assert!(error.expect("said").contains("text"));

    type_into(window, MarkField::Text, "SECRET", cx);
    type_into(window, MarkField::Size, "20", cx);
    act(
        window,
        MarkAction::SetScope(crate::shell::chrome::crop_dialog::CropScope::Chosen),
        cx,
    );
    act(window, MarkAction::Behind, cx);
    act(window, MarkAction::Submit, cx);
    let texts = page_texts(window, cx);
    assert_eq!(
        texts.iter().filter(|text| text.contains("SECRET")).count(),
        1,
        "only the page on screen: {texts:?}"
    );
}

#[gpui::test]
fn a_background_from_a_file_and_a_missing_file(cx: &mut TestAppContext) {
    let window = window(1, cx);
    open(window, MarkKind::Background, cx);
    act(window, MarkAction::SetSource(Source::File), cx);
    act(window, MarkAction::Submit, cx);
    let error = window
        .update(cx, |frame, _, _| {
            frame.marks_dialog().and_then(|s| s.error.clone())
        })
        .unwrap();
    assert!(error.expect("said").contains("Choose a PDF"));

    let art = std::env::temp_dir().join(format!("background-art-{}.pdf", std::process::id()));
    std::fs::write(&art, numbered(1)).expect("writes");
    window
        .update(cx, |frame, _, _| {
            frame.take_mark_files(vec![art.clone()], false)
        })
        .unwrap();
    assert!(labels(window, cx)
        .iter()
        .any(|label| label.contains("background-art")));
    act(window, MarkAction::Submit, cx);
    assert_eq!(
        page_texts(window, cx),
        ["Page 1 | Page 1"],
        "the art's own page text, behind this page's"
    );

    // A file that has gone by the time Add is pressed is named.
    open(window, MarkKind::Background, cx);
    act(window, MarkAction::SetSource(Source::File), cx);
    let gone = art.with_extension("gone.pdf");
    window
        .update(cx, |frame, _, _| frame.take_mark_files(vec![gone], false))
        .unwrap();
    act(window, MarkAction::Submit, cx);
    let error = window
        .update(cx, |frame, _, _| {
            frame.marks_dialog().and_then(|s| s.error.clone())
        })
        .unwrap();
    assert!(error.expect("said").contains("gone.pdf"));
    let _ = std::fs::remove_file(art);
}

#[gpui::test]
fn bates_numbers_this_document_and_the_files_added_after_it(cx: &mut TestAppContext) {
    let window = window(2, cx);
    let folder = std::env::temp_dir().join(format!("bates-window-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder");
    let other = folder.join("exhibit.pdf");
    std::fs::write(&other, numbered(3)).expect("writes");

    open(window, MarkKind::Bates, cx);
    type_into(window, MarkField::Prefix, "EX", cx);
    window
        .update(cx, |frame, _, _| {
            frame.take_mark_files(vec![other.clone()], true)
        })
        .unwrap();
    assert!(labels(window, cx).contains(&"exhibit.pdf".to_owned()));
    type_into(window, MarkField::After, "-bates", cx);
    act(window, MarkAction::Submit, cx);
    assert_eq!(
        page_texts(window, cx),
        ["EX000001 | Page 1", "EX000002 | Page 2"]
    );
    let written = folder.join("exhibit-bates_EX000003-EX000005.pdf");
    assert!(written.exists(), "{:?}", notices(window, cx));
    assert!(notices(window, cx)
        .iter()
        .any(|notice| notice.starts_with("Numbered 2 pages from EX000001 to EX000002.")));
    let _ = std::fs::remove_dir_all(&folder);
}
