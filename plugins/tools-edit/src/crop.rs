//! Crop Pages: a page box set from margins, as one undo step.
//!
//! The margins are measured in from the media box as the page is shown, the
//! way Acrobat's dialog measures them (`core::pages::set_page_box` has the
//! rules). Fitting a page to what it draws renders it once, the way the
//! screen does, and reads the margins off the picture.

use onionskin_core::pages::{self, shown_margins, Margins, PageBox};
use onionskin_core::{Document, PageIndex, PageRect};
use onionskin_plugin_api::CommandError;

/// A crop the dialog asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct CropPages {
    pub pages: Vec<PageIndex>,
    pub which: PageBox,
    pub margins: Margins,
    /// Change Page Size first: width and height in points, as shown. The
    /// margins are then measured from the new media box.
    pub page_size: Option<(f64, f64)>,
}

/// The undo label every crop has.
const LABEL: &str = "Crop Pages";

/// Resize `crop.pages` if asked, then set `crop.which` on them from
/// `crop.margins`, as one undo step.
pub fn crop_pages(doc: &mut Document, crop: &CropPages) -> Result<(), CommandError> {
    doc.edit_pages(LABEL, |tx, _| {
        if let Some((width, height)) = crop.page_size {
            pages::set_media_size(tx, &crop.pages, width, height)?;
        }
        pages::set_page_box(tx, &crop.pages, crop.which, crop.margins)
    })
    .map_err(edit_error)
}

/// Set `which` on each of `pages` to what the page draws, as one undo step:
/// Acrobat's Remove White Margins. A page that draws nothing keeps its box.
pub fn crop_to_content(
    doc: &mut Document,
    pages: &[PageIndex],
    which: PageBox,
) -> Result<(), CommandError> {
    let mut measured = Vec::with_capacity(pages.len());
    for &page in pages {
        if let Some(margins) = white_margins(doc, page)? {
            measured.push((page, margins));
        }
    }
    doc.edit_pages(LABEL, |tx, _| {
        measured
            .iter()
            .try_for_each(|(page, margins)| pages::set_page_box(tx, &[*page], which, *margins))
    })
    .map_err(edit_error)
}

/// The margins, measured from the media box as `page` is shown, that crop it
/// to the marks it draws; `None` for a page that draws nothing.
///
/// The page is rendered at one pixel a point, so the margins are whole
/// points, rounded outwards: a crop never cuts into a mark.
pub fn white_margins(doc: &mut Document, page: PageIndex) -> Result<Option<Margins>, CommandError> {
    let read = |source| CommandError::Page { page, source };
    let geometry = doc.page_geometry(page).map_err(read)?.clone();
    let render = doc.render_page_now(page, 1.0).map_err(read)?;
    let raster = &render.raster;
    let Some(ink) = raster.content_bounds() else {
        return Ok(None);
    };
    let shown = geometry.crop_box.unwrap_or(geometry.media_box);
    let offset = shown_margins(geometry.media_box, shown, geometry.rotate);
    let (width, height) = (raster.width(), raster.height());
    Ok(Some(Margins {
        top: offset.top + f64::from(ink.y),
        left: offset.left + f64::from(ink.x),
        bottom: offset.bottom + f64::from(height - ink.y - ink.height),
        right: offset.right + f64::from(width - ink.x - ink.width),
    }))
}

/// Crop `rect.page` to `rect`, in the page's own coordinates, as one undo
/// step. What lies outside the media box is not the page's: the crop stops
/// at its edges.
pub fn crop_to_rect(doc: &mut Document, rect: PageRect) -> Result<(), CommandError> {
    let page = rect.page;
    let geometry = doc
        .page_geometry(page)
        .map_err(|source| CommandError::Page { page, source })?
        .clone();
    let shown = shown_margins(
        geometry.media_box,
        [rect.x0, rect.y0, rect.x1, rect.y1],
        geometry.rotate,
    );
    let inside = |margin: f64| margin.max(0.0);
    crop_pages(
        doc,
        &CropPages {
            pages: vec![page],
            which: PageBox::Crop,
            margins: Margins {
                top: inside(shown.top),
                bottom: inside(shown.bottom),
                left: inside(shown.left),
                right: inside(shown.right),
            },
            page_size: None,
        },
    )
}

fn edit_error(source: onionskin_core::Error) -> CommandError {
    CommandError::Edit {
        label: LABEL,
        source,
    }
}
