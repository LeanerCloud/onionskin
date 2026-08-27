//! What extraction hands back: text, where it sits on the page, and which
//! bytes drew it.
//!
//! The three travel together on purpose. Decision 7 of the plan asks every
//! model object to know its source bytes; for text that is what makes
//! selection map to a source operator, and what lets redaction prove it
//! removed the right thing rather than the right-looking thing.

use std::ops::Range;

use onionskin_cos::{ObjRef, Origin, Span};
use onionskin_plugin_api::{PageIndex, PageQuad};

use crate::error::Warning;
use crate::font::FontId;

/// Where a run's operator lives in the file.
///
/// `decoded` indexes the stream's decoded bytes, which is where the operator
/// actually is; `origin` is the same stream's place in the file, which for an
/// unfiltered stream contains those bytes verbatim and for a filtered one is
/// the object that must be rewritten to change them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Byte range of [`TextRun::text`] this glyph produced. A ligature glyph
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
    pub text: String,
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

impl TextRun {
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Quads for the characters covering `range` of [`TextRun::text`]. This is
    /// what turns a search hit into a highlight.
    pub fn quads_for(&self, range: Range<usize>) -> Vec<PageQuad> {
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
    pub fn has_unmapped(&self) -> bool {
        self.runs
            .iter()
            .any(|r| r.glyphs.iter().any(Glyph::is_unmapped))
    }
}
