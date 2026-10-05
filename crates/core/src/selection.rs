//! What the user has picked out on a page.
//!
//! A [`Selection`] is one of three things, never two: a marquee region, a
//! run of text quads, or an image. Setting one clears the others for that
//! reason, so no consumer has to decide which of two populated fields
//! wins. Both are expressed in page user space, not device pixels, so a
//! selection survives a zoom, a rotation and a re-render without being
//! recomputed.
//!
//! What a find has matched lives in [`crate::search`] instead: a selection is
//! one thing the user pointed at, and a search result set is a walk over the
//! whole document.

use crate::{PageIndex, PageQuad, PageRect};

/// Selected text: the quads to draw it with, and the text they cover in
/// document order. The text travels with the quads because the clipboard
/// path needs it, and reading it back off the quads would have to guess
/// where the line breaks were.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextSelection {
    pub page: PageIndex,
    pub quads: Vec<PageQuad>,
    pub text: String,
    /// `text` cut where the face or size changes, for a writer that keeps
    /// formatting (Export Selection As RTF). Joined, the spans are `text`.
    pub spans: Vec<TextSpan>,
}

/// A stretch of selected text in one face and size.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpan {
    pub text: String,
    /// `/BaseFont`, subset prefix stripped.
    pub font: String,
    /// The `Tf` size, in text space units.
    pub size: f64,
}

/// A selected image: which of its page's placements, in drawing order,
/// and where it was when it was selected.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageSelection {
    pub page: PageIndex,
    pub index: usize,
    pub placement: onionskin_content::placements::ImagePlacement,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    region: Option<PageRect>,
    text: Option<TextSelection>,
    image: Option<ImageSelection>,
}

impl Selection {
    pub fn region(&self) -> Option<PageRect> {
        self.region
    }

    pub fn text(&self) -> Option<&TextSelection> {
        self.text.as_ref()
    }

    pub fn text_quads(&self) -> &[PageQuad] {
        match &self.text {
            Some(text) => &text.quads,
            None => &[],
        }
    }

    pub fn image(&self) -> Option<&ImageSelection> {
        self.image.as_ref()
    }

    pub fn set_region(&mut self, region: PageRect) {
        self.clear();
        self.region = Some(region);
    }

    pub fn set_text(&mut self, text: TextSelection) {
        self.clear();
        self.text = Some(text);
    }

    pub fn set_image(&mut self, image: ImageSelection) {
        self.clear();
        self.image = Some(image);
    }

    pub fn clear(&mut self) {
        self.region = None;
        self.text = None;
        self.image = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_and_text_selection_are_exclusive() {
        let mut selection = Selection::default();
        selection.set_region(PageRect {
            page: 0,
            x0: 1.0,
            y0: 2.0,
            x1: 3.0,
            y1: 4.0,
        });
        assert!(selection.region().is_some());

        selection.set_text(TextSelection {
            page: 0,
            quads: vec![PageQuad {
                page: 0,
                corners: [(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)],
            }],
            text: "a".into(),
            spans: Vec::new(),
        });
        assert!(selection.region().is_none());
        assert_eq!(selection.text_quads().len(), 1);
        assert_eq!(selection.text().map(|text| text.text.as_str()), Some("a"));

        selection.set_image(ImageSelection {
            page: 0,
            index: 1,
            placement: onionskin_content::placements::ImagePlacement {
                image: onionskin_cos::ObjRef::new(5, 0),
                name: "Im0".into(),
                ctm: onionskin_content::Matrix::IDENTITY,
                provenance: None,
                marked: None,
            },
        });
        assert!(selection.text().is_none(), "an image replaces the text");
        assert_eq!(selection.image().map(|image| image.index), Some(1));
        selection.set_region(PageRect {
            page: 0,
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
        });
        assert!(selection.image().is_none(), "a region replaces the image");

        selection.clear();
        assert!(selection.region().is_none());
        assert!(selection.text_quads().is_empty());
        assert!(selection.text().is_none());
    }
}
