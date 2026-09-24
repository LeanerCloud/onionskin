//! Editing images: the one selected, turned, flipped, moved, resized,
//! replaced or deleted, and a new one added. Each is one undo step, and
//! the selection follows the image through it.
//!
//! The image to put in, when there is one, arrives as a PDF whose first
//! page is the picture: what every image import makes, and a PDF page
//! works as well.

use onionskin_content::Matrix;
use onionskin_core::image_edit::{
    add_image as add_to_page, edit_placement, image_at, import_image, transforms, PlacementEdit,
};
use onionskin_core::{Document, ImageSelection, PageIndex, PageRect};
use onionskin_plugin_api::CommandError;

fn failed(label: &'static str) -> impl Fn(onionskin_core::Error) -> CommandError {
    move |source| CommandError::Edit { label, source }
}

fn nothing_selected(label: &'static str) -> CommandError {
    CommandError::Failed {
        label,
        reason: "no image is selected: click one with the Edit Image tool".to_owned(),
    }
}

/// Select the image drawn topmost at `(x, y)` on `page`, or nothing.
/// Whether one was there.
pub fn select_image_at(doc: &mut Document, page: PageIndex, (x, y): (f64, f64)) -> bool {
    let placements = doc.page_images(page).unwrap_or_default();
    let found = image_at(&placements, x, y).cloned();
    let index = found
        .as_ref()
        .and_then(|found| placements.iter().position(|each| each == found));
    match (found, index) {
        (Some(placement), Some(index)) => {
            doc.selection_mut().set_image(ImageSelection {
                page,
                index,
                placement,
            });
            true
        }
        _ => {
            if doc.selection().image().is_some() {
                doc.selection_mut().clear();
            }
            false
        }
    }
}

/// The selected image, if one is.
pub fn selected(doc: &Document) -> Option<ImageSelection> {
    doc.selection().image().cloned()
}

/// Make `edit` to the selected image, then select it again where it is
/// now, or clear the selection when it went.
fn edit_selected(
    doc: &mut Document,
    label: &'static str,
    edit: impl FnOnce(
        &mut onionskin_core::Transaction<'_>,
        &ImageSelection,
    ) -> onionskin_core::Result<PlacementEdit>,
) -> Result<(), CommandError> {
    let chosen = selected(doc).ok_or_else(|| nothing_selected(label))?;
    doc.edit_document(label, |tx| {
        let change = edit(tx, &chosen)?;
        edit_placement(tx, chosen.page, &chosen.placement, &change)
    })
    .map_err(failed(label))?;
    let placements = doc.page_images(chosen.page).map_err(failed(label))?;
    match placements.get(chosen.index) {
        Some(placement) => doc.selection_mut().set_image(ImageSelection {
            placement: placement.clone(),
            ..chosen
        }),
        None => doc.selection_mut().clear(),
    }
    Ok(())
}

fn centre(selection: &ImageSelection) -> (f64, f64) {
    let [x0, y0, x1, y1] = selection.placement.bounds();
    ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

/// Carry the selected image through page-space `change`.
pub fn transform_selected(
    doc: &mut Document,
    label: &'static str,
    change: Matrix,
) -> Result<(), CommandError> {
    edit_selected(doc, label, |_, _| Ok(PlacementEdit::Transform(change)))
}

/// Turn the selected image a quarter clockwise, or anticlockwise, about
/// its centre.
pub fn rotate_selected(doc: &mut Document, clockwise: bool) -> Result<(), CommandError> {
    let (label, quarters) = if clockwise {
        ("Rotate Image Clockwise", 1)
    } else {
        ("Rotate Image Counterclockwise", 3)
    };
    let chosen = selected(doc).ok_or_else(|| nothing_selected(label))?;
    transform_selected(
        doc,
        label,
        transforms::rotate_about(quarters, centre(&chosen)),
    )
}

/// Mirror the selected image left to right, or top to bottom.
pub fn flip_selected(doc: &mut Document, horizontal: bool) -> Result<(), CommandError> {
    let label = if horizontal {
        "Flip Image Horizontal"
    } else {
        "Flip Image Vertical"
    };
    let chosen = selected(doc).ok_or_else(|| nothing_selected(label))?;
    transform_selected(
        doc,
        label,
        transforms::flip_about(horizontal, centre(&chosen)),
    )
}

/// Take the selected image off its page.
pub fn delete_selected(doc: &mut Document) -> Result<(), CommandError> {
    edit_selected(doc, "Delete Image", |_, _| Ok(PlacementEdit::Remove))
}

/// Put the picture on page 1 of `document` where the selected image is,
/// fitted and centred in its frame.
pub fn replace_selected(doc: &mut Document, document: Vec<u8>) -> Result<(), CommandError> {
    const LABEL: &str = "Replace Image";
    let mut picture = Document::open_bytes(document).map_err(unreadable(LABEL))?;
    let source = picture.structure().map_err(unreadable(LABEL))?;
    edit_selected(doc, LABEL, |tx, _| {
        let (form, bbox) = import_image(tx, source)?;
        Ok(PlacementEdit::Replace { form, bbox })
    })
}

fn unreadable(label: &'static str) -> impl Fn(onionskin_core::Error) -> CommandError {
    move |error| CommandError::Failed {
        label,
        reason: format!("the image could not be read: {error}"),
    }
}

/// Draw the picture on page 1 of `document` over `rect`, fitted and
/// centred, after everything its page draws, and select it.
pub fn add_image(
    doc: &mut Document,
    rect: PageRect,
    document: Vec<u8>,
) -> Result<(), CommandError> {
    const LABEL: &str = "Add Image";
    let mut picture = Document::open_bytes(document).map_err(unreadable(LABEL))?;
    let source = picture.structure().map_err(unreadable(LABEL))?;
    doc.edit_document(LABEL, |tx| {
        add_to_page(tx, rect.page, source, [rect.x0, rect.y0, rect.x1, rect.y1])
    })
    .map_err(failed(LABEL))?;
    let placements = doc.page_images(rect.page).map_err(failed(LABEL))?;
    if let Some(placement) = placements.last() {
        doc.selection_mut().set_image(ImageSelection {
            page: rect.page,
            index: placements.len() - 1,
            placement: placement.clone(),
        });
    }
    Ok(())
}

/// A picture's size in points: its first page's.
pub fn picture_size(document: &[u8]) -> Option<(f64, f64)> {
    let mut picture = Document::open_bytes(document.to_vec()).ok()?;
    let geometry = picture.page_geometry(0).ok()?;
    let [x0, y0, x1, y1] = geometry.crop_box.unwrap_or(geometry.media_box);
    Some(((x1 - x0).abs(), (y1 - y0).abs()))
}
