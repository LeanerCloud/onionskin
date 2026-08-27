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
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let doc = onionskin_cos::Document::open_path(std::path::Path::new("in.pdf"))?;
//! let page = onionskin_content::extract_page(&doc, 0)?;
//! for run in &page.runs {
//!     println!("{:?} at {:?}", run.text, run.glyphs.first().map(|g| g.quad));
//! }
//! # Ok(()) }
//! ```

mod error;
mod filter;
mod font;
mod interpret;
mod matrix;
mod page;
mod run;
mod tokenizer;

pub use error::{Error, Result, Warning};
pub use font::{Code, Font, FontId};
pub use matrix::Matrix;
pub use page::{page_count, Content, ContentPart, Page};
pub use run::{ByteProvenance, Glyph, Mapping, PageText, TextRun};
pub use tokenizer::{Operation, Operator, Tokenizer};

use onionskin_cos::Document;
use onionskin_plugin_api::PageIndex;

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

/// The page's content streams, decoded and concatenated, with each part's
/// provenance. Exposed for redaction, which rewrites these bytes.
pub fn content(doc: &Document, page: &Page, warnings: &mut Vec<Warning>) -> Result<Content> {
    page::content(doc, page, warnings)
}
