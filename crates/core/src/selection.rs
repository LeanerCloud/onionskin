use onionskin_content as content;

use crate::{PageIndex, PageQuad, PageRect, SearchOptions};

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
