//! Content-stream interpretation: operators, graphics state and text runs,
//! with glyphs mapped back to the byte spans they came from so selection,
//! search and redaction all know their source. Full-text search lives here,
//! serving both the viewer's find bar and search-and-redact.
//!
//! # What this milestone extracts
//!
//! Text, and where it is. [`extract_page`] interprets a page's content
//! streams and returns one [`TextRun`] per text-showing operator, in the order
//! the page drew them. Each run carries its characters, a [`PageQuad`] per
//! glyph in the page's default user space, and the [`ByteProvenance`] of the
//! operator that drew it: which stream object, and which bytes of that
//! stream's decoded data.
//!
//! Reading order is document order. Column detection, table reconstruction and
//! the rest of layout analysis are later work; a producer that draws its
//! footnotes first will have them extracted first.
//!
//! # What it refuses to guess
//!
//! A glyph whose font offers no `/ToUnicode`, no usable glyph name and no
//! reverse cmap comes back as [`Mapping::Unmapped`]. It keeps its position and
//! its character code, so it can still be selected and still be redacted, but
//! nothing in the API will claim to know what letter it is. The alternative -
//! a question mark, or a dropped character - is the failure mode that makes a
//! redaction verifier lie.
//!
//! Everything else that could not be honoured exactly arrives as a
//! [`Warning`] on the page, counted by font: unmapped glyphs, missing widths,
//! predefined CMaps this build does not carry, fonts that would not load.
//!
//! # Example
//!
//! ```no_run
//! use onionskin_content::{extract_page, search, SearchOptions};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let doc = onionskin_cos::Document::open_path(std::path::Path::new("in.pdf"))?;
//! let page = extract_page(&doc, 0)?;
//! for hit in search(&page, "invoice", SearchOptions::default()) {
//!     println!("{:?} at {:?}", hit.text, hit.quads.first());
//! }
//! # Ok(()) }
//! ```

pub mod edit_text;
mod error;
mod font;
mod geometry;
mod interpret;
mod lines;
mod marked;
mod matrix;
mod page;
pub mod placements;
pub mod redact;
mod run;
mod search;
pub mod shapes;
mod tokenizer;

pub use error::{Error, Result, Warning};
/// `pdf_text_string` is exported for `core`'s navigation-pane readers: a
/// bookmark title, an attachment name and a signer name are all PDF text
/// strings, and decoding them a second time in `core` would be a second set
/// of rules about UTF-16 byte order marks and PDFDoc encoding.
pub use font::{encode_win_ansi, pdf_text_string, standard_text_width, Code, Font, FontId};
pub use geometry::{PageIndex, PageQuad};
pub use lines::{text_lines, LineGlyph, TextLine};
pub use marked::{MarkedPage, MarkedRef};
pub use matrix::Matrix;
pub use page::{page_count, Content, ContentPart, Page};
pub use run::{ActualText, ByteProvenance, Glyph, Mapping, PageText, SelectedRun, TextRun};
pub use search::{
    flatten, search, search_flattened, text_matches, CoveredRun, FlatPiece, Flattened, Match,
    MatchMode, RunCoverage, SearchOptions,
};

/// The lexer, exposed for M5's redaction plugin.
///
/// Removing text means rewriting the operators that drew it, which means
/// re-lexing the stream a [`TextRun`]'s [`ByteProvenance`] points into and
/// editing operations in place. Extraction itself has no need of these types;
/// they are public because that consumer is the reason the lexer records byte
/// ranges at all.
pub use tokenizer::{Operation, Operator, Tokenizer};

use onionskin_cos::Document;
/// Loads page `index`, parsing only the page-tree nodes on the path to it.
pub fn page(doc: &Document, index: PageIndex) -> Result<Page> {
    page::page(doc, index)
}

/// Extracts one page's text runs.
pub fn extract_page(doc: &Document, index: PageIndex) -> Result<PageText> {
    let page = page::page(doc, index)?;
    interpret::page_text(doc, &page)
}

/// Extracts the text of an already-loaded page, for a caller that needs the
/// page's geometry as well.
pub fn extract(doc: &Document, page: &Page) -> Result<PageText> {
    interpret::page_text(doc, page)
}

/// Every image page `index` draws, and where: what saving, replacing and
/// moving an image start from. See [`placements`].
pub fn page_images(doc: &Document, index: PageIndex) -> Result<Vec<placements::ImagePlacement>> {
    let page = page::page(doc, index)?;
    interpret::page_images(doc, &page)
}

/// The marked-content ids page `index`'s own content opens: what the
/// structure tree's elements refer to its content by.
pub fn page_mcids(doc: &Document, index: PageIndex) -> Result<std::collections::BTreeSet<i64>> {
    let page = page::page(doc, index)?;
    interpret::page_mcids(doc, &page)
}

/// Every run, image and path on page `index`, each with the marked-content
/// sequences it was drawn in: what a structure element's `/MCID` is matched
/// against. See [`MarkedRef`].
pub fn page_marked(doc: &Document, index: PageIndex) -> Result<MarkedPage> {
    let page = page::page(doc, index)?;
    interpret::page_marked(doc, &page)
}

/// Every path page `index` paints, with its points in page space: what
/// form field detection looks for. See [`shapes`].
pub fn page_shapes(doc: &Document, index: PageIndex) -> Result<Vec<shapes::Shape>> {
    let page = page::page(doc, index)?;
    interpret::page_shapes(doc, &page)
}

/// Rewrites page `index`'s content streams without what `areas` cover. See
/// [`redact`].
pub fn redact_page(
    doc: &Document,
    index: PageIndex,
    areas: &[redact::Area],
) -> Result<redact::PageRedaction> {
    let page = page::page(doc, index)?;
    interpret::redact_page(doc, &page, areas, &[])
}

/// The same, also removing everything drawn in the optional content groups
/// `hidden`.
pub fn redact_page_with_hidden(
    doc: &Document,
    index: PageIndex,
    areas: &[redact::Area],
    hidden: &[onionskin_cos::ObjRef],
) -> Result<redact::PageRedaction> {
    let page = page::page(doc, index)?;
    interpret::redact_page(doc, &page, areas, hidden)
}

/// The page's content streams, decoded and concatenated, with each part's
/// provenance. Exposed for redaction, which rewrites these bytes.
pub fn content(doc: &Document, page: &Page, warnings: &mut Vec<Warning>) -> Result<Content> {
    page::content(doc, page, warnings)
}
