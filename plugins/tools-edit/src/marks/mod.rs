//! Watermarks, backgrounds, headers and footers, and Bates numbers: what
//! each draws on a page, laid out here, and written by `core::pages` as page
//! marks that Update and Remove find again.
//!
//! Everything is laid out in the page's shown space, points from the
//! bottom-left of the page as it is displayed, which `core` maps onto the
//! page whatever its rotation.

mod art;
mod bates_files;
mod header_footer;
mod text;

use onionskin_core::pages::{self, MarkKind, PageMark};
use onionskin_core::{Document, PageIndex};
use onionskin_cos::{Dict, Name, Object};
use onionskin_plugin_api::CommandError;

pub use art::{add_background, add_watermark, Appearance, Art, HAlign, VAlign};
pub use bates_files::{number_files, output_name, Naming, Numbered};
pub use header_footer::{add_bates, add_header_footer, Bates, HeaderFooter, Numbering, POSITIONS};
pub use text::{Font, TextStyle};

/// The marks of each kind `page` carries.
pub fn page_marks(doc: &mut Document, page: PageIndex) -> Result<Vec<MarkKind>, CommandError> {
    doc.edit_pages("Read Page Marks", |tx, _| pages::page_marks(tx, page))
        .map_err(|source| CommandError::Page { page, source })
}

/// The settings the document's first mark of `kind` was made with, as they
/// were handed to the add function, for Update to open on.
pub fn saved_settings(doc: &mut Document, kind: MarkKind) -> Result<Option<String>, CommandError> {
    doc.edit_pages("Read Page Marks", |tx, _| pages::mark_settings(tx, kind))
        .map(|settings| settings.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
        .map_err(|source| CommandError::Failed {
            label: "Read Page Marks",
            reason: source.to_string(),
        })
}

/// The pages that carry a mark of `kind`.
pub fn marked_pages(doc: &mut Document, kind: MarkKind) -> Result<Vec<PageIndex>, CommandError> {
    doc.edit_pages("Read Page Marks", |tx, _| pages::marked_pages(tx, kind))
        .map_err(|source| CommandError::Failed {
            label: "Read Page Marks",
            reason: source.to_string(),
        })
}

/// Remove `kind`'s marks from `pages`, as one undo step named `label`. How
/// many pages had one.
pub fn remove_marks(
    doc: &mut Document,
    kind: MarkKind,
    pages: &[PageIndex],
) -> Result<usize, CommandError> {
    let label = remove_label(kind);
    doc.edit_pages(label, |tx, _| pages::remove_page_marks(tx, kind, pages))
        .map_err(|source| CommandError::Edit { label, source })
}

/// What Remove is called in the Edit menu and the undo list.
pub fn remove_label(kind: MarkKind) -> &'static str {
    match kind {
        MarkKind::Watermark => "Remove Watermark",
        MarkKind::Background => "Remove Background",
        MarkKind::HeaderFooter => "Remove Header & Footer",
        MarkKind::Bates => "Remove Bates Numbering",
    }
}

/// What Add, or with `replace` Update, is called.
pub fn add_label(kind: MarkKind, replace: bool) -> &'static str {
    match (kind, replace) {
        (MarkKind::Watermark, false) => "Add Watermark",
        (MarkKind::Watermark, true) => "Update Watermark",
        (MarkKind::Background, false) => "Add Background",
        (MarkKind::Background, true) => "Update Background",
        (MarkKind::HeaderFooter, false) => "Add Header & Footer",
        (MarkKind::HeaderFooter, true) => "Update Header & Footer",
        (MarkKind::Bates, _) => "Add Bates Numbering",
    }
}

/// A page and its shown width and height.
type Sized = (PageIndex, (f64, f64));

/// Each page's shown size, as `core` lays marks out in.
fn shown_sizes(doc: &mut Document, pages: &[PageIndex]) -> Result<Vec<Sized>, CommandError> {
    pages
        .iter()
        .map(|&page| {
            let geometry = doc
                .page_geometry(page)
                .map_err(|source| CommandError::Page { page, source })?;
            let crop = geometry.crop_box.unwrap_or(geometry.media_box);
            let space = pages::shown(crop, geometry.rotate);
            Ok((page, (space.width, space.height)))
        })
        .collect()
}

/// An `/ExtGState` making everything drawn `opacity` opaque, named `GS0`,
/// or nothing for a mark drawn fully opaque.
fn opacity_state(opacity: f64, resources: &mut Dict) -> &'static str {
    if opacity >= 1.0 {
        return "";
    }
    let mut state = Dict::new();
    state.set(Name::new("Type"), Object::name("ExtGState"));
    state.set(Name::new("ca"), Object::Real(opacity.max(0.0)));
    state.set(Name::new("CA"), Object::Real(opacity.max(0.0)));
    let mut states = Dict::new();
    states.set(Name::new("GS0"), Object::Dict(state));
    resources.set(Name::new("ExtGState"), Object::Dict(states));
    "/GS0 gs\n"
}

/// Write `marks` as one undo step.
fn write(
    doc: &mut Document,
    kind: MarkKind,
    replace: bool,
    build: impl FnOnce(
        &mut onionskin_core::Transaction<'_>,
    ) -> onionskin_core::Result<Vec<(PageIndex, PageMark)>>,
) -> Result<(), CommandError> {
    let label = add_label(kind, replace);
    doc.edit_pages(label, |tx, _| {
        let marks = build(tx)?;
        pages::add_page_marks(tx, kind, &marks, replace)
    })
    .map_err(|source| CommandError::Edit { label, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_names_its_add_update_and_remove() {
        for kind in MarkKind::ALL {
            assert!(remove_label(kind).starts_with("Remove "));
            assert!(add_label(kind, false).starts_with("Add "));
        }
        assert_eq!(add_label(MarkKind::Watermark, true), "Update Watermark");
        assert_eq!(add_label(MarkKind::Background, true), "Update Background");
        assert_eq!(
            add_label(MarkKind::HeaderFooter, true),
            "Update Header & Footer"
        );
        assert_eq!(add_label(MarkKind::Bates, true), "Add Bates Numbering");
    }

    #[test]
    fn full_opacity_needs_no_graphics_state() {
        let mut resources = Dict::new();
        assert_eq!(opacity_state(1.0, &mut resources), "");
        assert!(resources.get(b"ExtGState").is_none());
        assert_eq!(opacity_state(-1.0, &mut resources), "/GS0 gs\n");
    }
}
