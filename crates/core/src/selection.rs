//! What the user has picked out on a page, and what a find has matched.
//!
//! A [`Selection`] is one of two things, never both: a marquee region or a run
//! of text quads. `set_region` and `set_text_quads` each clear the other for
//! that reason, so no consumer has to decide which of two populated fields
//! wins. Both are expressed in page user space, not device pixels, so a
//! selection survives a zoom, a rotation and a re-render without being
//! recomputed.
//!
//! Search state lives here rather than in the find UI because the same match
//! set feeds the canvas highlight, the MCP verbs and the CLI.

use onionskin_content as content;

use crate::{PageIndex, PageQuad, PageRect, SearchOptions};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    region: Option<PageRect>,
    text: Vec<PageQuad>,
}

impl Selection {
    pub fn region(&self) -> Option<PageRect> {
        self.region
    }

    pub fn text_quads(&self) -> &[PageQuad] {
        &self.text
    }

    pub fn set_region(&mut self, region: PageRect) {
        self.region = Some(region);
        self.text.clear();
    }

    pub fn set_text_quads(&mut self, quads: Vec<PageQuad>) {
        self.region = None;
        self.text = quads;
    }

    pub fn clear(&mut self) {
        self.region = None;
        self.text.clear();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchMatch {
    pub page: PageIndex,
    pub text: String,
    pub quads: Vec<PageQuad>,
}

impl From<content::Match> for SearchMatch {
    fn from(hit: content::Match) -> Self {
        SearchMatch {
            page: hit.page,
            text: hit.text,
            quads: hit.quads,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchState {
    needle: String,
    options: SearchOptions,
    matches: Vec<SearchMatch>,
    current: Option<usize>,
}

impl SearchState {
    pub fn needle(&self) -> &str {
        &self.needle
    }

    pub fn options(&self) -> SearchOptions {
        self.options
    }

    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    pub fn current(&self) -> Option<&SearchMatch> {
        self.current.and_then(|index| self.matches.get(index))
    }

    pub fn set_query(&mut self, needle: impl Into<String>, options: SearchOptions) {
        let needle = needle.into();
        if self.needle == needle && self.options == options {
            return;
        }
        self.needle = needle;
        self.options = options;
        self.matches.clear();
        self.current = None;
    }

    pub fn replace_matches(&mut self, matches: Vec<SearchMatch>) {
        self.current = (!matches.is_empty()).then_some(0);
        self.matches = matches;
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

        selection.set_text_quads(vec![PageQuad {
            page: 0,
            corners: [(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)],
        }]);
        assert!(selection.region().is_none());
        assert_eq!(selection.text_quads().len(), 1);

        selection.clear();
        assert!(selection.region().is_none());
        assert!(selection.text_quads().is_empty());
    }

    #[test]
    fn changing_the_search_query_clears_stale_matches() {
        let mut state = SearchState::default();
        state.replace_matches(vec![SearchMatch {
            page: 0,
            text: "needle".into(),
            quads: Vec::new(),
        }]);
        state.set_query("other", SearchOptions::default());

        assert_eq!(state.needle(), "other");
        assert!(state.matches().is_empty());
        assert!(state.current().is_none());
    }
}
