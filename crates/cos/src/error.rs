//! Typed errors. Nothing in `cos` degrades quietly: an unreadable file, an
//! unsupported filter and a file that needed repair are three different
//! outcomes and each has its own variant.

use std::fmt;

use crate::object::ObjRef;
use crate::repair::RepairReport;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// No `%PDF-` header anywhere in the file.
    NotAPdf,
    /// The file has a `/Encrypt` entry. The spike parses no security handler,
    /// so this is a refusal rather than a partial open.
    Encrypted,
    /// A lexical or grammatical failure at a known byte offset.
    Syntax {
        offset: u64,
        detail: String,
    },
    /// The xref could not be read and the scan-and-rebuild path did not
    /// recover a usable document either.
    Unrecoverable {
        detail: String,
    },
    /// `Document::open` refuses a file that needed repair. Call
    /// `Document::open_repairing` to take it with the report attached.
    RepairRequired(Box<RepairReport>),
    /// A referenced object is absent from the xref, or the bytes at its
    /// recorded offset are some other object.
    MissingObject(ObjRef),
    /// A stream filter this spike does not implement.
    UnsupportedFilter(String),
    /// A filter's payload did not decode.
    Filter {
        filter: String,
        detail: String,
    },
    /// A write to an object number the file has already marked free. Taking
    /// one back needs the free list re-linked in a section that is already
    /// written, which an append-only save cannot do.
    FreedObject(ObjRef),
    /// A reference cycle, or nesting past the depth limit.
    DepthExceeded {
        detail: String,
    },
    /// A page index the page tree does not reach. `count` is how many pages the
    /// walk actually found, which is the page tree's own answer rather than the
    /// `/Count` the root claims.
    NoSuchPage {
        index: usize,
        count: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::NotAPdf => write!(f, "no %PDF- header found"),
            Error::Encrypted => write!(f, "document is encrypted"),
            Error::Syntax { offset, detail } => {
                write!(f, "syntax error at byte {offset}: {detail}")
            }
            Error::Unrecoverable { detail } => write!(f, "unrecoverable: {detail}"),
            Error::RepairRequired(report) => {
                write!(f, "document needs repair: {}", report.summary())
            }
            Error::MissingObject(r) => {
                write!(f, "object {} {} not found", r.number, r.generation)
            }
            Error::FreedObject(r) => {
                write!(
                    f,
                    "object {} is marked free and cannot be rewritten",
                    r.number
                )
            }
            Error::UnsupportedFilter(name) => write!(f, "unsupported filter /{name}"),
            Error::Filter { filter, detail } => write!(f, "filter /{filter} failed: {detail}"),
            Error::DepthExceeded { detail } => write!(f, "depth limit exceeded: {detail}"),
            Error::NoSuchPage { index, count } => {
                write!(f, "page {index} requested, the page tree reaches {count}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl Error {
    /// Stable short name for tallying corpus runs by failure category.
    pub fn category(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::NotAPdf => "not-a-pdf",
            Error::Encrypted => "encrypted",
            Error::Syntax { .. } => "syntax",
            Error::Unrecoverable { .. } => "unrecoverable",
            Error::RepairRequired(_) => "repair-required",
            Error::MissingObject(_) => "missing-object",
            Error::FreedObject(_) => "freed-object",
            Error::UnsupportedFilter(_) => "unsupported-filter",
            Error::Filter { .. } => "filter-failed",
            Error::DepthExceeded { .. } => "depth-exceeded",
            Error::NoSuchPage { .. } => "no-such-page",
        }
    }
}
