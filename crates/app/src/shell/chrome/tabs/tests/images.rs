//! The Edit menu's image entries and the Add Image tool's picture, on a
//! real window.
//!
//! The file picker is the platform's, which the test platform does not
//! implement, so Replace Image starts from the path a picker would hand
//! back. The save prompt is simulated, as the export tests do.

use image::ImageEncoder as _;
use onionskin_plugin_api::{tool_with, ToolCapability};

use super::*;
use crate::shell::chrome::image_commands::ImageCommand;
use crate::shell::chrome::tabs::images::NO_IMAGE_SELECTED;

fn png(width: u32, height: u32) -> Vec<u8> {
    let pixels: Vec<u8> = (0..width * height).map(|index| index as u8).collect();
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&pixels, width, height, image::ExtendedColorType::L8)
        .expect("encodes");
    bytes
}

/// A window on a document whose one page is a 100 by 50 point picture,
/// its tools keeping their files under `data`.
fn picture_window(
    data: &Path,
    cx: &mut TestAppContext,
) -> (gpui::WindowHandle<ShellFrame>, PathBuf) {
    use onionskin_core::images::{image_document, ImageColor, ImageData, ImagePage};

    let path = data.join("picture.pdf");
    let pdf = image_document(&ImagePage {
        width: 100,
        height: 50,
        dpi: (72.0, 72.0),
        color: ImageColor::Gray,
        data: ImageData::Samples((0..5000).map(|index| index as u8).collect()),
        alpha: None,
        inverted_cmyk: false,
        icc: None,
    })
    .expect("writes");
    std::fs::write(&path, pdf).expect("writes");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let window = bound_window_with_models(
        vec![(path.clone(), model)],
        crate::config::ConfigPaths::in_dir(data),
        cx,
    )
    .0;
    (window, path)
}

fn placements(frame: &ShellFrame, cx: &App) -> Vec<onionskin_core::ImagePlacement> {
    let canvas = frame.tabs.active().expect("a tab").canvas.read(cx);
    let mut document = canvas.model.document_mut();
    document.page_images(0).expect("reads")
}

fn select_image(frame: &ShellFrame, cx: &App) {
    let canvas = frame.tabs.active().expect("a tab").canvas.read(cx);
    let mut document = canvas.model.document_mut();
    assert!(onionskin_tools_edit::images::select_image_at(
        &mut document,
        0,
        (50.0, 25.0)
    ));
}

fn entry(frame: &ShellFrame, image: ImageCommand, cx: &App) -> MenuAvailability {
    crate::shell::chrome::global_bar::main_menu_schema(frame.menu_state(cx))
        .iter()
        .flat_map(|section| section.entries.iter())
        .find(|entry| entry.command == MenuCommand::Image(image))
        .expect("the entry")
        .availability
}

#[gpui::test]
fn the_image_entries_turn_and_flip_the_selected_image(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = picture_window(data.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            for image in ImageCommand::ALL {
                assert_eq!(entry(frame, image, cx), MenuAvailability::Enabled);
            }
            let before = placements(frame, cx)[0].bounds();
            assert_eq!(before, [0.0, 0.0, 100.0, 50.0]);

            // Nothing selected: the command says so and nothing moves.
            frame
                .run_main_menu_command(
                    MenuCommand::Image(ImageCommand::RotateClockwise),
                    window,
                    cx,
                )
                .expect("live");
            assert!(frame
                .notices
                .last()
                .unwrap()
                .contains("no image is selected"));

            select_image(frame, cx);
            frame
                .run_main_menu_command(
                    MenuCommand::Image(ImageCommand::RotateClockwise),
                    window,
                    cx,
                )
                .expect("live");
            let turned = placements(frame, cx)[0].bounds();
            let size = (turned[2] - turned[0], turned[3] - turned[1]);
            assert!((size.0 - 50.0).abs() < 1e-6 && (size.1 - 100.0).abs() < 1e-6);

            frame
                .run_main_menu_command(MenuCommand::Image(ImageCommand::FlipHorizontal), window, cx)
                .expect("live");
            let flipped = placements(frame, cx)[0].bounds();
            assert!(flipped
                .iter()
                .zip(turned)
                .all(|(a, b)| (a - b).abs() < 1e-6));
            let canvas = frame.tabs.active().expect("a tab").canvas.read(cx);
            assert!(
                canvas.model.image_selection().is_some(),
                "the selection follows the image"
            );
        })
        .unwrap();
}

#[gpui::test]
fn replace_image_puts_the_picked_picture_where_the_image_was(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = picture_window(data.path(), cx);
    let picture = data.path().join("square.png");
    std::fs::write(&picture, png(4, 4)).expect("writes");
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Image(ImageCommand::Replace), window, cx)
                .expect("live");
            assert_eq!(
                frame.notices.last().map(String::as_str),
                Some(NO_IMAGE_SELECTED)
            );

            select_image(frame, cx);
            let before = placements(frame, cx);
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            frame.replace_image(&canvas, &picture, cx);
            let after = placements(frame, cx);
            assert_eq!(after.len(), 1);
            assert_ne!(after[0].image, before[0].image, "the new picture");
            let [x0, y0, x1, y1] = after[0].bounds();
            assert!(
                (x1 - x0 - 50.0).abs() < 1e-6 && (y1 - y0 - 50.0).abs() < 1e-6,
                "the square fitted in the frame: {:?}",
                after[0].bounds()
            );

            frame.replace_image(&canvas, &data.path().join("missing.png"), cx);
            assert!(frame.notices.last().unwrap().contains("could not be read"));
        })
        .unwrap();
}

#[gpui::test]
fn save_image_as_writes_the_selected_image(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = picture_window(data.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Image(ImageCommand::SaveAs), window, cx)
                .expect("live");
            assert_eq!(
                frame.notices.last().map(String::as_str),
                Some(NO_IMAGE_SELECTED)
            );
            select_image(frame, cx);
            frame
                .run_main_menu_command(MenuCommand::Image(ImageCommand::SaveAs), window, cx)
                .expect("live");
        })
        .unwrap();
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    let output = data.path().join("saved.png");
    let chosen = output.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();
    let notices = window
        .update(cx, |frame, _, _| frame.notices.clone())
        .unwrap();
    assert!(
        notices.last().unwrap().starts_with("Saved the image"),
        "{notices:?}"
    );
    let decoded = image::load_from_memory_with_format(
        &std::fs::read(&output).expect("written"),
        image::ImageFormat::Png,
    )
    .expect("a PNG")
    .to_luma8();
    assert_eq!(decoded.dimensions(), (100, 50));

    window
        .update(cx, |frame, _, _| {
            frame.write_image(&data.path().join("no/such/folder/x.png"), b"bytes");
            assert!(frame.notices.last().unwrap().contains("was not written"));
        })
        .unwrap();
}

#[gpui::test]
fn the_add_image_tool_is_handed_a_pdf_of_the_picked_picture(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, path) = picture_window(data.path(), cx);
    let picture = data.path().join("logo.png");
    std::fs::write(&picture, png(8, 8)).expect("writes");
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            let index = tool_with(
                canvas.read(cx).model.registry(),
                ToolCapability::PlacesImage,
            )
            .expect("add image is installed");
            let chosen = |cx: &App| {
                canvas
                    .read(cx)
                    .model
                    .registry()
                    .tools()
                    .nth(index)
                    .and_then(|tool| tool.chosen())
            };

            frame.give_tool_file(&canvas, index, &picture, cx);
            let made = chosen(cx).expect("chosen");
            assert!(made.ends_with("placed-images/logo.pdf"), "{made}");
            assert!(std::fs::read(&made).expect("written").starts_with(b"%PDF"));

            frame.give_tool_file(&canvas, index, &path, cx);
            assert_eq!(chosen(cx).as_deref(), path.to_str(), "a PDF as it is");

            let text = data.path().join("notes.txt");
            std::fs::write(&text, b"notes").expect("writes");
            frame.give_tool_file(&canvas, index, &text, cx);
            assert_eq!(chosen(cx).as_deref(), path.to_str(), "kept what it had");
            assert!(!frame.notices.is_empty());
        })
        .unwrap();
}
