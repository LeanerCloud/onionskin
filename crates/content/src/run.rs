//! What extraction hands back: text, where it sits on the page, and which
//! bytes drew it.
//!
//! The three travel together on purpose. Decision 7 of the plan asks every
//! model object to know its source bytes; for text that is what makes
//! selection map to a source operator, and what lets redaction prove it
//! removed the right thing rather than the right-looking thing.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use onionskin_cos::{ObjRef, Origin, Span};

use crate::error::Warning;
use crate::font::FontId;
use crate::{PageIndex, PageQuad};

/// One immutable `/ActualText` occurrence. Equality is occurrence identity,
/// not replacement-string equality.
#[derive(Clone, Debug)]
pub struct ActualText(Arc<str>);

impl ActualText {
    pub(crate) fn new(text: String) -> Self {
        Self(Arc::from(text))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for ActualText {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ActualText {}

impl Hash for ActualText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// Where a run's operator lives in the file.
///
/// `decoded` indexes the stream's decoded bytes, which is where the operator
/// actually is; `origin` is the same stream's place in the file, which for an
/// unfiltered stream contains those bytes verbatim and for a filtered one is
/// the object that must be rewritten to change them.
///
/// One limitation, and it is reported rather than hidden: ISO 32000-2 7.8.2
/// lets a page divide its content between streams at any token boundary, so an
/// operator can sit in a later `/Contents` part than its operands. This names
/// one stream, so a run split that way reaches the end of the part it starts
/// in and does not include its own operator. The page carries
/// [`crate::Warning::ProvenanceClamped`] when it happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ByteProvenance {
    pub stream: ObjRef,
    pub origin: Origin,
    pub decoded: Span,
}

impl ByteProvenance {
    /// The byte range in the file that has to be preserved for this operator
    /// to survive. `None` for a stream that only exists as a pending edit.
    pub fn file_span(&self) -> Option<Span> {
        self.origin.file_span()
    }
}

/// What a glyph contributed to its run's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mapping {
    /// Byte range of [`TextRun::decoded_text`] this glyph produced. A ligature glyph
    /// covers several characters; a combining sequence covers several too.
    Text(Range<usize>),
    /// The font offered no `/ToUnicode`, no usable glyph name and no reverse
    /// cmap. The glyph is still positioned and its code is still here, so a
    /// caller can select it, redact it, or report it - but nothing may claim
    /// to know what character it is.
    Unmapped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// The character code as written in the shown string.
    pub code: u32,
    /// The CID the code selected. Equal to `code` for a simple font.
    pub cid: u32,
    pub quad: PageQuad,
    pub mapping: Mapping,
}

impl Glyph {
    pub fn is_unmapped(&self) -> bool {
        matches!(self.mapping, Mapping::Unmapped)
    }
}

/// One text-showing operator's output. `Tj`, `'` and `"` produce one run;
/// `TJ` produces one run for the whole array, because the array is one
/// operator and therefore one byte range.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub page: PageIndex,
    pub decoded_text: String,
    pub actual_text: Option<ActualText>,
    pub glyphs: Vec<Glyph>,
    pub provenance: ByteProvenance,
    pub font: FontId,
    /// `/BaseFont`, with any subset prefix stripped. For diagnostics; two runs
    /// are the same face when their [`FontId`]s match, not their names.
    pub font_name: String,
    /// The `Tf` operand. The page-space size is already in the quads, which
    /// carry every scale between text space and the page.
    pub size: f64,
    /// `Tr`. Mode 3 and mode 7 draw nothing; they are still extracted, because
    /// invisible text over a scan is exactly what a search has to find.
    pub render_mode: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedRun {
    pub run: usize,
    pub glyphs: Range<usize>,
}

impl TextRun {
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Quads for decoded characters covering `range`.
    pub fn quads_for_decoded(&self, range: Range<usize>) -> Vec<PageQuad> {
        self.glyphs
            .iter()
            .filter(|g| match &g.mapping {
                Mapping::Text(r) => r.start < range.end && range.start < r.end,
                Mapping::Unmapped => false,
            })
            .map(|g| g.quad)
            .collect()
    }
}

/// Every run on one page, in the order the content stream drew them.
#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub page: PageIndex,
    pub runs: Vec<TextRun>,
    /// Everything the page said that could not be honoured exactly.
    pub warnings: Vec<Warning>,
}

impl PageText {
    pub fn selection_members(&self, selected: &[(usize, usize)]) -> Vec<SelectedRun> {
        let mut selected_ranges = HashMap::new();
        let mut associations = HashSet::new();
        for &(run, glyph) in selected {
            selected_ranges
                .entry(run)
                .and_modify(|range: &mut Range<usize>| {
                    range.start = range.start.min(glyph);
                    range.end = range.end.max(glyph + 1);
                })
                .or_insert(glyph..glyph + 1);
            if let Some(actual_text) = &self.runs[run].actual_text {
                associations.insert(actual_text.clone());
            }
        }
        self.runs
            .iter()
            .enumerate()
            .filter_map(|(run, text)| {
                if text
                    .actual_text
                    .as_ref()
                    .is_some_and(|actual_text| associations.contains(actual_text))
                {
                    Some(SelectedRun {
                        run,
                        glyphs: 0..text.glyphs.len(),
                    })
                } else {
                    selected_ranges
                        .remove(&run)
                        .map(|glyphs| SelectedRun { run, glyphs })
                }
            })
            .filter(|selected| !selected.glyphs.is_empty())
            .collect()
    }

    pub fn has_unmapped(&self) -> bool {
        self.runs
            .iter()
            .any(|r| r.actual_text.is_none() && r.glyphs.iter().any(Glyph::is_unmapped))
    }
}
