//! Editing images through the plugin: selecting, turning, flipping,
//! replacing and deleting the selected one, the Edit Image tool's moves
//! and resizes, the Add Image tool, and the commands the Edit menu runs.

mod common;

use common::{content, pdf, Page};
use onionskin_core::{Document, Modifiers, PagePoint};
use onionskin_plugin_api::command_ids::{
    FLIP_IMAGE_HORIZONTAL, FLIP_IMAGE_VERTICAL, ROTATE_IMAGE_CLOCKWISE,
    ROTATE_IMAGE_COUNTERCLOCKWISE,
};
use onionskin_plugin_api::{
    CommandCtx, EditVerb, PluginManifest, PluginRegistry, PointerInput, ToolCapability, ToolPlugin,
};
use onionskin_tools_edit::images::{
    add_image, delete_selected, flip_selected, picture_size, replace_selected, rotate_selected,
    select_image_at, selected,
};
use onionskin_tools_edit::{AddImageTool, EditImageTool, EditToolsPlugin};

fn image() -> Vec<u8> {
    let mut out = b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
/BitsPerComponent 8 /Length 1 >>\nstream\n"
        .to_vec();
    out.push(0x80);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A Letter page drawing the image 100 by 50 at (100, 600).
fn document() -> Document {
    Document::open_bytes(pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im0 5 0 R >> >> >>"
            .to_vec(),
        content("q 100 0 0 50 100 600 cm /Im0 Do Q"),
        image(),
    ]))
    .expect("opens")
}

/// A 40 by 20 picture, as an image import makes one.
fn picture() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 40 20] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /P 5 0 R >> >> >>"
            .to_vec(),
        content("q 40 0 0 20 0 0 cm /P Do Q"),
        image(),
    ])
}

fn bounds(doc: &mut Document, index: usize) -> [f64; 4] {
    doc.page_images(0).expect("reads")[index].bounds()
}

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn the_selected_image_is_turned_flipped_replaced_and_deleted() {
    let mut doc = document();
    assert!(rotate_selected(&mut doc, true)
        .unwrap_err()
        .to_string()
        .contains("no image is selected"));
    assert!(!select_image_at(&mut doc, 0, (10.0, 10.0)));
    assert!(select_image_at(&mut doc, 0, (150.0, 625.0)));
    assert_eq!(selected(&doc).map(|chosen| chosen.index), Some(0));

    rotate_selected(&mut doc, true).expect("turns");
    assert!(close(bounds(&mut doc, 0), [125.0, 575.0, 175.0, 675.0]));
    assert_eq!(
        doc.edit().history().undo_label(),
        Some("Rotate Image Clockwise")
    );
    assert!(
        close(
            selected(&doc).expect("still selected").placement.bounds(),
            [125.0, 575.0, 175.0, 675.0]
        ),
        "the selection follows the image"
    );
    rotate_selected(&mut doc, false).expect("turns back");
    assert!(close(bounds(&mut doc, 0), [100.0, 600.0, 200.0, 650.0]));

    flip_selected(&mut doc, true).expect("flips");
    assert!(doc.page_images(0).expect("reads")[0].ctm.a < 0.0);
    flip_selected(&mut doc, false).expect("flips");
    assert_eq!(
        doc.edit().history().undo_label(),
        Some("Flip Image Vertical")
    );

    replace_selected(&mut doc, picture()).expect("replaces");
    assert!(
        close(bounds(&mut doc, 0), [100.0, 600.0, 200.0, 650.0]),
        "2:1 fits 2:1 exactly"
    );
    assert!(replace_selected(&mut doc, b"not a pdf".to_vec())
        .unwrap_err()
        .to_string()
        .contains("could not be read"));

    delete_selected(&mut doc).expect("deletes");
    assert!(doc.page_images(0).expect("reads").is_empty());
    assert!(selected(&doc).is_none(), "gone with it");
}

#[test]
fn an_image_is_added_fitted_and_selected() {
    let mut doc = document();
    assert_eq!(picture_size(&picture()), Some((40.0, 20.0)));
    assert_eq!(picture_size(b"nope"), None);
    let rect = onionskin_core::PageRect {
        page: 0,
        x0: 300.0,
        y0: 300.0,
        x1: 400.0,
        y1: 400.0,
    };
    add_image(&mut doc, rect, picture()).expect("adds");
    assert!(close(bounds(&mut doc, 1), [300.0, 325.0, 400.0, 375.0]));
    assert_eq!(selected(&doc).map(|chosen| chosen.index), Some(1));
    assert!(add_image(&mut doc, rect, Vec::new()).is_err());
}

#[test]
fn the_edit_image_tool_selects_moves_and_resizes() {
    let mut page = Page::new(document());
    let mut tool = EditImageTool::new();
    assert_eq!(tool.capabilities(), [ToolCapability::EditImages]);
    assert!(tool.hint().is_some() && tool.claims(EditVerb::Delete));
    assert_eq!(
        (tool.id(), tool.name(), tool.group()),
        ("edit-image", "Edit Image", "images")
    );
    page.drag(&mut tool, (150.0, 625.0), (150.0, 625.0));
    assert!(selected(&page.doc).is_some(), "a click selects");
    assert_eq!(tool.overlays(&page.doc).len(), 1);

    page.drag(&mut tool, (150.0, 625.0), (160.0, 605.0));
    assert!(close(
        bounds(&mut page.doc, 0),
        [110.0, 580.0, 210.0, 630.0]
    ));
    assert_eq!(page.doc.edit().history().undo_label(), Some("Move Image"));

    // The top right corner, dragged out: twice the size about the bottom left.
    page.drag(&mut tool, (210.0, 630.0), (310.0, 680.0));
    assert!(
        close(bounds(&mut page.doc, 0), [110.0, 580.0, 310.0, 680.0]),
        "{:?}",
        bounds(&mut page.doc, 0)
    );
    assert_eq!(page.doc.edit().history().undo_label(), Some("Resize Image"));

    tool.edit(&mut page.ctx(), EditVerb::Copy, None);
    assert_eq!(
        page.doc.page_images(0).expect("reads").len(),
        1,
        "only Delete deletes"
    );
    tool.edit(&mut page.ctx(), EditVerb::Delete, None);
    assert!(page.doc.page_images(0).expect("reads").is_empty());

    page.drag(&mut tool, (10.0, 10.0), (50.0, 50.0));
    assert!(selected(&page.doc).is_none(), "nothing there to move");
    tool.on_cancel(&mut page.ctx());
    tool.on_deactivate(&mut page.ctx());
    assert!(tool.overlays(&page.doc).is_empty());
}

#[test]
fn a_selection_is_let_go_by_escape_and_by_another_tool() {
    let mut page = Page::new(document());
    let mut tool = EditImageTool::new();
    page.drag(&mut tool, (150.0, 625.0), (150.0, 625.0));
    tool.on_cancel(&mut page.ctx());
    assert!(selected(&page.doc).is_none());
    page.drag(&mut tool, (150.0, 625.0), (150.0, 625.0));
    let at = |x, y| PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    };
    tool.on_pointer_down(&mut page.ctx(), at(150.0, 625.0));
    tool.on_pointer_move(&mut page.ctx(), at(170.0, 625.0));
    assert_eq!(
        tool.overlays(&page.doc).len(),
        2,
        "the image and where it is going"
    );
    tool.on_cancel(&mut page.ctx());
    assert!(
        selected(&page.doc).is_some(),
        "Escape mid-drag drops the drag only"
    );
    tool.on_deactivate(&mut page.ctx());
    assert!(selected(&page.doc).is_none());
}

#[test]
fn the_add_image_tool_places_the_picture_it_was_given() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("picture.pdf");
    std::fs::write(&path, picture()).expect("writes");
    let mut page = Page::new(document());
    let mut tool = AddImageTool::new();
    assert!(tool.capabilities().contains(&ToolCapability::PlacesImage));
    assert!(!tool.choose("/no/such/file.pdf"));
    assert_eq!(tool.chosen(), None);
    page.drag(&mut tool, (300.0, 300.0), (300.0, 300.0));
    assert_eq!(
        page.doc.page_images(0).expect("reads").len(),
        1,
        "nothing chosen"
    );
    assert!(tool.choose(&path.to_string_lossy()));
    assert_eq!(tool.chosen(), Some(path.to_string_lossy().into_owned()));
    page.drag(&mut tool, (300.0, 300.0), (300.0, 300.0));
    assert!(
        close(bounds(&mut page.doc, 1), [300.0, 280.0, 340.0, 300.0]),
        "its own size"
    );
    page.drag(&mut tool, (100.0, 100.0), (300.0, 200.0));
    assert!(
        close(bounds(&mut page.doc, 2), [100.0, 100.0, 300.0, 200.0]),
        "fitted"
    );
    tool.on_pointer_down(
        &mut page.ctx(),
        PointerInput {
            at: PagePoint {
                page: 0,
                x: 0.0,
                y: 0.0,
            },
            pressure: 1.0,
            modifiers: Modifiers::default(),
            clicks: 1,
        },
    );
    tool.on_cancel(&mut page.ctx());
    assert!(tool.overlays(&page.doc).is_empty());
    assert_eq!((tool.id(), tool.name()), ("add-image", "Add Image"));
    assert!(tool.hint().is_some());
}

#[test]
fn the_edit_menu_turns_and_flips_through_the_registry() {
    let mut registry = PluginRegistry::new();
    EditToolsPlugin.register(&mut registry);
    let mut doc = document();
    select_image_at(&mut doc, 0, (150.0, 625.0));
    for id in [
        ROTATE_IMAGE_CLOCKWISE,
        ROTATE_IMAGE_COUNTERCLOCKWISE,
        FLIP_IMAGE_HORIZONTAL,
        FLIP_IMAGE_VERTICAL,
    ] {
        let command = registry
            .commands()
            .iter()
            .find(|command| command.id == id)
            .expect("registered");
        (command.run)(&mut CommandCtx {
            doc: &mut doc,
            page: 0,
        })
        .expect("runs");
    }
    assert!(close(bounds(&mut doc, 0), [100.0, 600.0, 200.0, 650.0]));
    let ctm = doc.page_images(0).expect("reads")[0].ctm;
    assert!(ctm.a < 0.0 && ctm.d < 0.0, "flipped both ways: {ctm:?}");
}
