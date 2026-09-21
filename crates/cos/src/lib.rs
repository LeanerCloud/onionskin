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
//!   one. A document that opened clean and then meets an object its
//!   cross-reference cannot produce can be escalated to the same scan by the
//!   caller ([`Document::escalate_to_scan`]), never by itself.
//! - **Incremental save**: [`Document::save_to_path`] writes the original bytes
//!   plus at most one appended section, streaming the original through in
//!   bounded chunks rather than holding it. A no-op save on a clean document
//!   appends nothing at all.
//!
//! A stream whose `/Length` is wrong is recovered the way every real reader
//! recovers it, by finding `endstream`, and the recovery is recorded per
//! object as a [`RecoveredBoundary`] rather than being refused or passed over
//! in silence.
//!
//! Encryption is detected and refused with [`Error::Encrypted`] rather than
//! half-parsed. One decoder serves the whole workspace: the structural layer
//! uses it for cross-reference and object streams, and [`Document::decode_stream`]
//! exposes it for page descriptions, font programs and CMaps. It implements
//! Flate and LZW with the PNG and TIFF predictors, RunLength and the two ASCII
//! armours; image codecs fail loud as [`Error::UnsupportedFilter`].
//!
//! [`Document::page`] reaches page `n` in document order with the four
//! inheritable attributes resolved, parsing only the tree nodes on the path to
//! it and skipping whole subtrees by their `/Count`.
//!
//! # Running the guarantee tests
//!
//! `corpus/external/` and `corpus/malformed/` are gitignored, so the tests
//! find their inputs through `$ONIONSKIN_CORPUS` (defaulting to
//! `<workspace>/corpus`) and skip loudly when it is absent. Set
//! `ONIONSKIN_CORPUS_REQUIRED=1` in CI to turn that skip into a failure.

mod decrypt;
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

pub use document::{Dangling, Document, PendingEdit, Section};
pub use error::{Error, Result};
pub use object::{
    Dict, Holder, Name, ObjRef, Object, Origin, PageNode, Parsed, RecoveredBoundary, Span, Stream,
};
pub use repair::{Provenance, RepairReason, RepairReport};
pub use source::{BytesSource, CountingSource, FileSource, ReadStats, Source};
pub use xref::{Xref, XrefEntry};
