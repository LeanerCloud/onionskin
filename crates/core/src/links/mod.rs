//! Links: `/Link` annotations, what they go to, and how they look.
//!
//! Read for the viewer, which follows a link clicked with the Hand tool, and
//! for the Link tool, which edits one; written by the Link tool, Create Links
//! from URLs and Remove Web Links. A link goes to a page of this document, a
//! web page or a file; anything else a file says (a named action, a
//! JavaScript action) is read as [`LinkTarget::Other`] and kept as written.

mod read;
mod write;

use onionskin_cos::ObjRef;

use crate::PageIndex;

pub use read::{link_at, read_links};
pub use write::{add_link, remove_link, remove_web_links, set_link};

/// What a link goes to.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    /// A page of this document.
    Page(PageIndex),
    /// A web page: a `/URI` action.
    Web(String),
    /// Another file: a `/Launch` or `/GoToR` action's `/F`.
    File(String),
    /// An action this crate does not write, by its `/S` name. Kept as the
    /// file has it when the link's look is changed.
    Other(String),
}

/// How a link is highlighted while it is pressed: `/H`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    None,
    Invert,
    Outline,
    Push,
}

impl Highlight {
    pub const ALL: [Highlight; 4] = [
        Highlight::None,
        Highlight::Invert,
        Highlight::Outline,
        Highlight::Push,
    ];

    fn key(self) -> &'static str {
        match self {
            Highlight::None => "N",
            Highlight::Invert => "I",
            Highlight::Outline => "O",
            Highlight::Push => "P",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Highlight::None => "None",
            Highlight::Invert => "Invert",
            Highlight::Outline => "Outline",
            Highlight::Push => "Inset",
        }
    }
}

/// A visible link's rectangle's line: `/BS /S`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Dashed,
    Underline,
}

impl LineStyle {
    pub const ALL: [LineStyle; 3] = [LineStyle::Solid, LineStyle::Dashed, LineStyle::Underline];

    fn key(self) -> &'static str {
        match self {
            LineStyle::Solid => "S",
            LineStyle::Dashed => "D",
            LineStyle::Underline => "U",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineStyle::Solid => "Solid",
            LineStyle::Dashed => "Dashed",
            LineStyle::Underline => "Underline",
        }
    }
}

/// How a link looks: Acrobat's Link Properties Appearance tab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkLook {
    /// A visible rectangle, or none: Acrobat's Link Type.
    pub visible: bool,
    /// Points: thin 1, medium 2, thick 3.
    pub width: f64,
    pub color: [f64; 3],
    pub style: LineStyle,
    pub highlight: Highlight,
}

impl Default for LinkLook {
    /// Acrobat's defaults for a new link: an invisible rectangle, inverted
    /// when pressed; blue and thin if it is made visible.
    fn default() -> Self {
        Self {
            visible: false,
            width: 1.0,
            color: [0.0, 0.0, 1.0],
            style: LineStyle::Solid,
            highlight: Highlight::Invert,
        }
    }
}

/// One link, as a page carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub objref: ObjRef,
    pub page: PageIndex,
    /// `/Rect`, lower-left corner first.
    pub rect: [f64; 4],
    pub target: LinkTarget,
    pub look: LinkLook,
}

impl Link {
    /// Whether page point `(x, y)` is on the link.
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        let [x0, y0, x1, y1] = self.rect;
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }
}
