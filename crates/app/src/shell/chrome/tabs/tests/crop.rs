//! M5 Crop Pages on a real window: the Edit menu, the thumbnails' menu and
//! the Organize grid open one dialog on the pages chosen; its margins set the
//! box on those pages as one undo step, Remove White Margins fits each page
//! to what it draws, and a margin that is not a distance keeps the dialog
//! open saying so.

use super::page_grid::{letter_marked, numbered};
use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::crop_dialog::{CropAction, CropScope};
use crate::shell::dialog::ShellDialog;
use crate::shell::organize::OrganizeAction;
use crate::shell::panes::{PaneAction, ThumbnailAction, ThumbnailsCommand};
use onionskin_core::pages::PageBox;

fn window(bytes: Vec<u8>, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let window = bound_window_from_bytes(vec![("crop.pdf", bytes)], cx).0;
    window
        .update(cx, |frame, _window, cx| {
            frame.run_view_action(crate::shell::canvas::ViewAction::GoToPage(0), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

fn open_from_menu(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::CropPages, window, cx)
                .expect("opens");
            assert_eq!(frame.dialog, Some(ShellDialog::CropPages));
        })
        .unwrap();
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: CropAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Crop(action), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

/// Type the four margins, top, bottom, left, right.
fn type_margins(window: gpui::WindowHandle<ShellFrame>, typed: [&str; 4], cx: &mut TestAppContext) {
    window
        .update(cx, |frame, _, cx| {
            let state = frame.crop.as_ref().expect("open");
            let fields = [&state.top, &state.bottom, &state.left, &state.right].map(Clone::clone);
            for (field, text) in fields.iter().zip(typed) {
                field.update(cx, |input, cx| input.set_query(text, cx));
            }
        })
        .unwrap();
}

fn typed(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, _, cx| {
            let state = frame.crop.as_ref().expect("open");
            [&state.top, &state.bottom, &state.left, &state.right]
                .map(|field| field.read(cx).query().to_owned())
                .to_vec()
        })
        .unwrap()
}

fn crop_boxes(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> Vec<Option<[f64; 4]>> {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                let count = canvas.model.viewport().page_count();
                (0..count)
                    .map(|page| {
                        canvas
                            .model
                            .document_mut()
                            .page_geometry(page)
                            .expect("geometry")
                            .crop_box
                    })
                    .collect()
            })
        })
        .unwrap()
}

fn dialog(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Option<ShellDialog> {
    window.update(cx, |frame, _, _| frame.dialog).unwrap()
}

#[gpui::test]
fn the_edit_menu_crops_the_page_on_screen_as_one_undo_step(cx: &mut TestAppContext) {
    let window = window(numbered(3), cx);
    open_from_menu(window, cx);
    assert_eq!(typed(window, cx), ["0", "0", "0", "0"], "no crop yet");
    // The numbered pages are 200 by 100 points.
    type_margins(window, ["10", "20", "5", "15"], cx);
    act(window, CropAction::Submit, cx);
    assert_eq!(
        dialog(window, cx),
        None,
        "a crop that runs closes the dialog"
    );
    assert_eq!(
        crop_boxes(window, cx),
        [Some([5.0, 20.0, 185.0, 90.0]), None, None]
    );

    // Opened again, it shows the crop the page now has.
    open_from_menu(window, cx);
    assert_eq!(typed(window, cx), ["10", "20", "5", "15"]);
    act(window, CropAction::SetToZero, cx);
    assert_eq!(typed(window, cx), ["0", "0", "0", "0"]);
    window
        .update(cx, |frame, window, cx| frame.close_dialog(window, cx))
        .unwrap();

    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Undo, window, cx)
                .expect("undoes");
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(crop_boxes(window, cx), [None, None, None]);
}

#[gpui::test]
fn every_page_can_take_another_box(cx: &mut TestAppContext) {
    let window = window(numbered(2), cx);
    open_from_menu(window, cx);
    act(window, CropAction::SetScope(CropScope::All), cx);
    act(window, CropAction::SetBox(PageBox::Crop), cx);
    type_margins(window, ["10", "10", "10", "10"], cx);
    act(window, CropAction::Submit, cx);
    assert_eq!(crop_boxes(window, cx), [Some([10.0, 10.0, 190.0, 90.0]); 2]);
}

#[gpui::test]
fn a_margin_that_is_not_a_distance_keeps_the_dialog_open(cx: &mut TestAppContext) {
    let window = window(numbered(1), cx);
    open_from_menu(window, cx);
    type_margins(window, ["10", "ten", "0", "0"], cx);
    act(window, CropAction::Submit, cx);
    assert_eq!(dialog(window, cx), Some(ShellDialog::CropPages));
    let alert = window
        .update(cx, |frame, window, cx| {
            frame
                .accessible(window, cx)
                .find(&"crop-error".into())
                .map(|element| element.label.clone())
        })
        .unwrap();
    assert!(alert.expect("said").contains("\"ten\""));

    // And one core refuses is said the same way, nothing written.
    type_margins(window, ["50", "50", "0", "0"], cx);
    act(window, CropAction::Submit, cx);
    let error = window
        .update(cx, |frame, _, _| {
            frame.crop_dialog().and_then(|s| s.error.clone())
        })
        .unwrap();
    assert!(error.expect("refused").contains("page 1"));
    assert_eq!(crop_boxes(window, cx), [None]);
}

#[gpui::test]
fn remove_white_margins_fits_the_page_to_its_marks(cx: &mut TestAppContext) {
    let window = window(letter_marked(), cx);
    open_from_menu(window, cx);
    act(window, CropAction::RemoveWhiteMargins, cx);
    let fields = window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            let described = [
                "crop-top",
                "crop-bottom",
                "crop-left",
                "crop-right",
                "crop-width",
                "crop-height",
            ]
            .map(|id| tree.find(&id.into()).is_some());
            let focusable = crate::shell::chrome::crop_dialog::TEXT_FIELDS
                .map(|field| frame.text_field(field).is_some());
            assert_eq!(described, focusable);
            focusable
        })
        .unwrap();
    assert_eq!(fields, [false; 6], "the margins are the page's to decide");
    act(window, CropAction::Submit, cx);
    assert_eq!(crop_boxes(window, cx), [Some([199.0, 164.0, 485.0, 401.0])]);
}

#[gpui::test]
fn the_thumbnails_menu_and_the_grid_open_it_on_their_pages(cx: &mut TestAppContext) {
    let window = window(numbered(3), cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(
                Activation::Pane(PaneAction::Thumbnail(ThumbnailAction::Run(
                    ThumbnailsCommand::CropPages,
                ))),
                window,
                cx,
            );
            assert_eq!(frame.dialog, Some(ShellDialog::CropPages));
            assert_eq!(frame.crop_dialog().expect("open").chosen, [0]);
            let tree = frame.accessible(window, cx);
            let top = tree.find(&"crop-top".into()).expect("described");
            assert_eq!(top.role, accesskit::Role::NumberInput);
            assert!(tree.find(&"crop-scope-chosen".into()).is_some());
            frame.close_dialog(window, cx);

            frame
                .run_main_menu_command(MenuCommand::OrganizePages, window, cx)
                .expect("opens the grid");
            frame.run_activation(Activation::Organize(OrganizeAction::Choose(2)), window, cx);
            frame.run_activation(Activation::Organize(OrganizeAction::Crop), window, cx);
            let state = frame.crop_dialog().expect("open");
            assert_eq!(state.chosen, [2]);
            assert_eq!(state.chosen_label(), "Page 3");
        })
        .unwrap();
    type_margins(window, ["1", "1", "1", "1"], cx);
    act(window, CropAction::Submit, cx);
    assert_eq!(
        crop_boxes(window, cx),
        [None, None, Some([1.0, 1.0, 199.0, 99.0])]
    );
}

#[gpui::test]
fn change_page_size_resizes_about_the_centre_before_the_crop(cx: &mut TestAppContext) {
    let window = window(numbered(2), cx);
    open_from_menu(window, cx);
    act(window, CropAction::ChangePageSize, cx);
    let size = window
        .update(cx, |frame, _, cx| {
            let state = frame.crop.as_ref().expect("open");
            let (width, height) = (state.width.clone(), state.height.clone());
            let shown = (
                width.read(cx).query().to_owned(),
                height.read(cx).query().to_owned(),
            );
            width.update(cx, |input, cx| input.set_query("300", cx));
            height.update(cx, |input, cx| input.set_query("200", cx));
            shown
        })
        .unwrap();
    assert_eq!(
        size,
        ("200".to_owned(), "100".to_owned()),
        "the page's own size"
    );
    act(window, CropAction::Submit, cx);
    assert_eq!(dialog(window, cx), None);
    let media = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas
                    .model
                    .document_mut()
                    .page_geometry(0)
                    .expect("geometry")
                    .media_box
            })
        })
        .unwrap();
    assert_eq!(media, [-50.0, -50.0, 250.0, 150.0]);
    assert_eq!(
        crop_boxes(window, cx),
        [Some([-50.0, -50.0, 250.0, 150.0]), None],
        "the whole new page shows"
    );
}
