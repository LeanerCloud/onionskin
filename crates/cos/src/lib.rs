//! COS object layer. The crate's charter: lexer and parser, xref tables and
//! streams, object streams, stream filters and encryption. Parsing is lazy by
//! construction - objects resolve on demand through the xref, never the
//! whole file up front - and repairing like Acrobat, so a broken xref,
//! junk before the header or a truncated tail all recover through a
//! scan-and-rebuild path. Every object keeps its source byte span. The
//! writer emits incremental-update sections; a full rewrite exists only
//! behind an explicit flatten API.
//!
//! # M1 spike scope
//!
//! What is implemented today is the spike that de-risks three of those bets.
//! Encryption and the flatten API belong to the charter above, not to this
//! crate as it stands.
//!
//! - **Lazy**: [`Document::open`] reads the header, the tail and the
//!   cross-reference sections. Object bodies parse only when [`Document::get`]
//!   asks for them, through a read window that grows until the object is
//!   complete. [`source::CountingSource`] makes the claim measurable.
//! - **Repair**: [`Document::open_repairing`] recovers junk headers, missing
//!   `%%EOF`, absent or wrong `startxref`, wrong xref offsets and overstated
//!   subsection counts, and reports every one of them. [`Document::open`]
//!   refuses such a file outright, so a repaired open cannot pass for a clean
//!   one.
//! - **Incremental save**: [`Document::save_to_vec`] returns the original bytes
//!   plus at most one appended section. A no-op save on a clean document
//!   appends nothing at all.
//!
//! Encryption is detected and refused with [`Error::Encrypted`] rather than
//! half-parsed, and only the filters the structural layer needs are
//! implemented (Flate with predictors, ASCIIHex, ASCII85).
//!
//! # Running the guarantee tests
//!
//! `corpus/external/` and `corpus/malformed/` are gitignored, so the tests
//! find their inputs through `$ONIONSKIN_CORPUS` (defaulting to
//! `<workspace>/corpus`) and skip loudly when it is absent. Set
//! `ONIONSKIN_CORPUS_REQUIRED=1` in CI to turn that skip into a failure.

mod document;
mod error;
mod filters;
mod object;
mod parse;
mod reader;
mod repair;
mod writer;
mod xref;

pub mod source;

pub use document::Document;
pub use error::{Error, Result};
pub use object::{Dict, Name, ObjRef, Object, Origin, Parsed, Span, Stream};
pub use repair::{Provenance, RepairReason, RepairReport};
pub use source::{BytesSource, CountingSource, FileSource, Source};
pub use xref::{Xref, XrefEntry};
