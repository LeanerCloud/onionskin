//! Errors that stop extraction, and the warnings that do not.
//!
//! The split is deliberate. An `Error` means the page could not be read at
//! all: the object is missing, the filter chain is one this crate cannot
//! decode, the page tree does not lead anywhere. A [`Warning`] means the page
//! was read but something in it could not be interpreted faithfully - a glyph
//! with no Unicode mapping, a font whose widths are absent, a CMap this
//! milestone does not carry. Warnings ride along on the extraction result so a
//! caller can see exactly what it is missing instead of inferring it from
//! text that looks slightly wrong.

use std::fmt;

use onionskin_cos::ObjRef;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Cos(onionskin_cos::Error),
    /// A stream filter this crate does not implement, or a stream whose data
    /// does not decode through the filter it declares.
    Filter {
        filter: String,
        detail: String,
    },
    /// The content stream is not lexable at this offset.
    Syntax {
        offset: u64,
        detail: String,
    },
    /// The document's object graph does not have the shape a page needs.
    Structure(String),
    /// A page index past the end of the page tree.
    NoSuchPage {
        index: usize,
        count: usize,
    },
}

impl Error {
    /// Stable slug for grouping failures in a corpus run.
    pub fn category(&self) -> &'static str {
        match self {
            Error::Cos(e) => e.category(),
            Error::Filter { .. } => "filter",
            Error::Syntax { .. } => "syntax",
            Error::Structure(_) => "structure",
            Error::NoSuchPage { .. } => "no-such-page",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cos(e) => write!(f, "{e}"),
            Error::Filter { filter, detail } => write!(f, "filter {filter}: {detail}"),
            Error::Syntax { offset, detail } => {
                write!(f, "content syntax at {offset}: {detail}")
            }
            Error::Structure(detail) => write!(f, "{detail}"),
            Error::NoSuchPage { index, count } => {
                write!(f, "page {index} requested, the document has {count}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<onionskin_cos::Error> for Error {
    fn from(e: onionskin_cos::Error) -> Self {
        Error::Cos(e)
    }
}

/// Something the page said that this crate could not honour. Collected per
/// extraction rather than thrown, because a page with one broken font still
/// has the rest of its text worth returning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// Codes that produced a positioned glyph with no Unicode. The glyph is in
    /// the run with [`crate::Mapping::Unmapped`]; this is the tally.
    UnmappedGlyphs { font: String, count: usize },
    /// The font offered no width for these codes, so they advanced by zero and
    /// every later glyph on the line is to the left of where it belongs.
    MissingWidths { font: String, count: usize },
    /// A predefined CMap name that is not Identity-H or Identity-V. Codes are
    /// split on a two-byte codespace, which is right for the CJK CMaps and a
    /// guess for anything else.
    UnsupportedCMap { name: String },
    /// A font resource that could not be loaded at all. Its showing operators
    /// still produce runs, with no glyphs.
    FontLoadFailed { resource: String, detail: String },
    /// A `Do` naming a form XObject already on the stack.
    XObjectCycle { object: ObjRef },
    /// Recursion past the form XObject depth cap.
    XObjectDepthExceeded { object: ObjRef },
    /// A content stream part that could not be decoded. The remaining parts
    /// were still interpreted.
    ContentPartFailed { stream: ObjRef, detail: String },
    /// The lexer could not make sense of the bytes at this offset and skipped
    /// forward. Text after the skip is still extracted.
    LexerResync { offset: u64, detail: String },
    /// The page drew more glyphs than one page is allowed to keep, and the
    /// rest were dropped. The page is incomplete; nothing else says so.
    GlyphLimit { limit: usize },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::UnmappedGlyphs { font, count } => {
                write!(f, "{count} glyphs of {font} have no Unicode mapping")
            }
            Warning::MissingWidths { font, count } => {
                write!(f, "{count} glyphs of {font} have no width")
            }
            Warning::UnsupportedCMap { name } => {
                write!(f, "predefined CMap {name} is not carried by this build")
            }
            Warning::FontLoadFailed { resource, detail } => {
                write!(f, "font resource /{resource} failed to load: {detail}")
            }
            Warning::XObjectCycle { object } => {
                write!(f, "form XObject {} refers to itself", object.number)
            }
            Warning::XObjectDepthExceeded { object } => {
                write!(f, "form XObject {} nests too deeply", object.number)
            }
            Warning::ContentPartFailed { stream, detail } => {
                write!(f, "content stream {} failed: {detail}", stream.number)
            }
            Warning::LexerResync { offset, detail } => {
                write!(f, "resynchronised at {offset}: {detail}")
            }
            Warning::GlyphLimit { limit } => {
                write!(f, "the page was cut off at {limit} glyphs")
            }
        }
    }
}
