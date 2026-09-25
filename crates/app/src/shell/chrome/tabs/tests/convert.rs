//! The Convert panel, File > Create's single sources and Export All Images,
//! on a real window.
//!
//! The file pickers are the platform's and the test platform does not
//! implement them, so the tests start from what a picker hands back: the
//! chosen file or folder. The save prompt is simulated, as the export tests
//! do.

use gpui::{ClipboardItem, Image, ImageFormat};
use image::ImageEncoder as _;

use super::*;
use crate::shell::chrome::global_bar::MenuSectionId;

fn png(width: u32, height: u32) -> Vec<u8> {
    let pixels: Vec<u8> = (0..width * height)
        .map(|index| (index * 37) as u8)
        .collect();
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&pixels, width, height, image::ExtendedColorType::L8)
        .expect("encodes");
    bytes
}

fn activations(panel: &crate::shell::chrome::accessible::Element) -> Vec<MenuCommand> {
    panel
        .children
        .iter()
        .filter_map(|row| match row.activation {
            Some(Activation::MainMenu(command)) => Some(command),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn the_convert_button_opens_its_own_panel_of_create_and_export_entries(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::ToggleConvertMenu, window, cx);
            let tree = frame.accessible(window, cx);
            let button = tree.find(&"convert-button".into()).expect("in the bar");
            assert_eq!(button.state.toggled, Some(true));
            let panel = tree.find(&"main-menu-panel".into()).expect("open");
            assert_eq!(panel.label, "Convert");
            assert_eq!(panel.children[0].label, MenuSectionId::Convert.label());
            let mut expected = vec![
                MenuCommand::CreateFromFile,
                MenuCommand::CreateFromClipboard,
                MenuCommand::CreateFromFiles,
            ];
            expected.extend(ExportTarget::ALL.map(MenuCommand::Export));
            expected.push(MenuCommand::ExportAllImages);
            assert_eq!(activations(panel), expected);
            // Create From Multiple Files is `commands-core`'s combine, which
            // this build may have compiled out; everything else is the codecs'.
            let needs_core_commands = |row: &&crate::shell::chrome::accessible::Element| {
                row.activation == Some(Activation::MainMenu(MenuCommand::CreateFromFiles))
            };
            assert!(
                panel.children[1..]
                    .iter()
                    .filter(|row| cfg!(feature = "commands-core") || !needs_core_commands(row))
                    .all(|row| !row.state.disabled),
                "a build with the codecs and a document open has every entry live"
            );

            // The main menu button switches the panel rather than stacking a
            // second one; Convert again closes it.
            frame.run_activation(Activation::ToggleMainMenu, window, cx);
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"main-menu-panel".into()).expect("open").label,
                "Main Menu"
            );
            assert_eq!(
                tree.find(&"convert-button".into()).unwrap().state.toggled,
                Some(false)
            );
            frame.run_activation(Activation::ToggleConvertMenu, window, cx);
            frame.run_activation(Activation::ToggleConvertMenu, window, cx);
            assert!(frame
                .accessible(window, cx)
                .find(&"main-menu-panel".into())
                .is_none());
        })
        .unwrap();
}

/// The one file the plan names for the clipboard: a PNG a screenshot put
/// there becomes a one-page document, written where the user says and opened.
#[gpui::test]
fn the_clipboards_image_becomes_a_document_in_a_new_tab(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    let dir = tempfile::tempdir().expect("dir");
    let output = dir.path().join("From Clipboard.pdf");
    cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
        ImageFormat::Png,
        png(30, 20),
    )));
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::CreateFromClipboard, window, cx)
                .expect("live");
        })
        .unwrap();
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    let chosen = output.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();

    window
        .update(cx, |frame, _, cx| {
            assert!(frame.notices.is_empty(), "{:?}", frame.notices);
            assert_eq!(frame.tabs.tabs().len(), 2, "the new document opened");
            let tab = frame.tabs.active().expect("active");
            assert_eq!(tab.canvas.read(cx).model.path(), Some(output.clone()));
            assert_eq!(tab.canvas.read(cx).model.view_state().page_count, 1);
        })
        .unwrap();
    let mut written = Document::open_path(&output).expect("a PDF");
    let geometry = written.page_geometry(0).expect("measures");
    assert_eq!(
        geometry.media_box,
        [0.0, 0.0, 30.0, 20.0],
        "a PNG with no pHYs is 72 DPI"
    );
}

#[gpui::test]
fn a_clipboard_without_a_usable_image_says_why_and_asks_nothing(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    for (item, expected) in [
        (None, "The clipboard is empty"),
        (
            Some(ClipboardItem::new_string("words".into())),
            "The clipboard holds no image",
        ),
        (
            Some(ClipboardItem::new_image(&Image::from_bytes(
                ImageFormat::Gif,
                b"GIF89a\x01\x00\x01\x00".to_vec(),
            ))),
            "PNG, JPEG and TIFF",
        ),
    ] {
        if let Some(item) = item {
            cx.write_to_clipboard(item);
        }
        window
            .update(cx, |frame, window, cx| {
                frame.notices.clear();
                frame
                    .run_main_menu_command(MenuCommand::CreateFromClipboard, window, cx)
                    .expect("live");
                assert_eq!(frame.notices.len(), 1);
                assert!(frame.notices[0].contains(expected), "{:?}", frame.notices);
            })
            .unwrap();
        cx.run_until_parked();
        assert!(!cx.did_prompt_for_new_path());
    }
}

#[gpui::test]
fn a_chosen_image_file_becomes_a_document_named_after_it(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    let dir = tempfile::tempdir().expect("dir");
    let source = dir.path().join("scan.png");
    std::fs::write(&source, png(8, 8)).expect("writes");
    window
        .update(cx, |frame, _, cx| frame.create_from_source(&source, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    let output = dir.path().join("scan.pdf");
    let chosen = output.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();
    let actual_path = window
        .update(cx, |frame, _, cx| {
            frame
                .tabs
                .active()
                .expect("active")
                .canvas
                .update(cx, |canvas, _| canvas.model.path())
        })
        .unwrap();
    assert_eq!(actual_path, Some(output.clone()));

    let missing = dir.path().join("gone.png");
    window
        .update(cx, |frame, _, cx| {
            frame.create_from_source(&missing, cx);
            assert!(frame.notices.last().unwrap().contains("could not be read"));
        })
        .unwrap();
}

#[gpui::test]
fn export_all_images_writes_the_documents_images_into_the_folder(cx: &mut TestAppContext) {
    use onionskin_core::images::{image_document, ImageColor, ImageData, ImagePage};

    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pictures.pdf");
    let pdf = image_document(&ImagePage {
        width: 3,
        height: 2,
        dpi: (72.0, 72.0),
        color: ImageColor::Rgb,
        data: ImageData::Samples((0..18).collect()),
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
        crate::config::ConfigPaths::default(),
        cx,
    )
    .0;
    let folder = dir.path().join("images");
    std::fs::create_dir(&folder).expect("dir");

    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.tabs.active().expect("active").canvas.clone();
            frame.export_images_to(&canvas, &folder, cx);
            assert!(
                frame.notices.last().unwrap().starts_with("Wrote 1 image"),
                "{:?}",
                frame.notices
            );
        })
        .unwrap();
    let written: Vec<_> = std::fs::read_dir(&folder)
        .expect("lists")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(written.len(), 1);
    let bytes = std::fs::read(&written[0]).expect("reads");
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .expect("a PNG")
        .to_rgb8();
    assert_eq!(decoded.into_raw(), (0..18).collect::<Vec<u8>>());
}
