//! What the user has picked out on a page.
//!
//! A [`Selection`] is one of two things, never both: a marquee region or a run
//! of text quads. `set_region` and `set_text_quads` each clear the other for
//! that reason, so no consumer has to decide which of two populated fields
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    region: Option<PageRect>,
    text: Option<TextSelection>,
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

    pub fn set_region(&mut self, region: PageRect) {
        self.region = Some(region);
        self.text = None;
    }

    pub fn set_text(&mut self, text: TextSelection) {
        self.region = None;
        self.text = Some(text);
    }

    pub fn clear(&mut self) {
        self.region = None;
        self.text = None;
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
        });
        assert!(selection.region().is_none());
        assert_eq!(selection.text_quads().len(), 1);
        assert_eq!(selection.text().map(|text| text.text.as_str()), Some("a"));

        selection.clear();
        assert!(selection.region().is_none());
        assert!(selection.text_quads().is_empty());
        assert!(selection.text().is_none());
    }
}
