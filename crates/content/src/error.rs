//! Errors that stop extraction, and the warnings that do not.
//!
//! The split is deliberate. An `Error` means the page could not be read at
//! all: the object is missing, the filter chain is one this build does not
//! implement, the page tree does not lead anywhere. A [`Warning`] means the page
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
    /// Everything the structural layer refuses: a filter it does not
    /// implement, a payload that does not decode, a page the tree does not
    /// reach, an object graph with the wrong shape. `cos` owns the page walk
    /// and the filter chain, so it owns their errors too, and restating them
    /// here would only be a second taxonomy to keep in step.
    Cos(onionskin_cos::Error),
    /// The content stream is not lexable at this offset.
    Syntax { offset: u64, detail: String },
}

impl Error {
    /// Stable slug for grouping failures in a corpus run.
    pub fn category(&self) -> &'static str {
        match self {
            Error::Cos(e) => e.category(),
            Error::Syntax { .. } => "syntax",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cos(e) => write!(f, "{e}"),
            Error::Syntax { offset, detail } => {
                write!(f, "content syntax at {offset}: {detail}")
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
    /// A font resource that could not be loaded at all. `Tf` then selects no
    /// font, so the showing operators that follow it draw nothing and are
    /// counted in [`Warning::TextWithoutFont`] as well.
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
    /// Showing operators ran with no font selected. They drew nothing, so
    /// there is no text to extract, and a caller must be able to tell that
    /// from a page that simply has no text.
    TextWithoutFont { count: usize },
    /// An operation's operands and its operator landed in different
    /// `/Contents` parts, which ISO 32000-2 7.8.2 permits. The run's byte
    /// range is clamped to the part it starts in and does not reach its
    /// operator.
    ProvenanceClamped { stream: ObjRef },
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
            Warning::TextWithoutFont { count } => {
                write!(f, "{count} showing operators ran with no font selected")
            }
            Warning::ProvenanceClamped { stream } => write!(
                f,
                "an operation in stream {} runs past the end of its content part",
                stream.number
            ),
        }
    }
}
