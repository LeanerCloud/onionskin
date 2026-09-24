//! Redaction marks: `/Redact` annotations, what they cover and how the
//! covered area looks once the redaction is applied.
//!
//! Marking is an ordinary edit, saved incrementally and undone like any
//! other: nothing is removed until the `redact` plugin applies the marks,
//! which is the one path that rewrites the file. A mark covers the quads of
//! the text it was made from, or its rectangle when it has none.

mod read;
mod write;

use onionskin_cos::ObjRef;

use crate::{PageIndex, PageQuad};

pub use read::{read_redactions, redaction_areas};
pub use write::{add_redaction, remove_redaction, set_redaction};

/// Acrobat's default: redacted areas are filled black.
pub const BLACK: [f64; 3] = [0.0, 0.0, 0.0];
/// A mark is outlined in red until it is applied.
pub const RED: [f64; 3] = [1.0, 0.0, 0.0];

/// Where overlay text sits in the area: `/Q`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

impl Align {
    pub const ALL: [Align; 3] = [Align::Left, Align::Centre, Align::Right];

    pub fn label(self) -> &'static str {
        match self {
            Align::Left => "Left",
            Align::Centre => "Centre",
            Align::Right => "Right",
        }
    }

    fn quadding(self) -> i64 {
        match self {
            Align::Left => 0,
            Align::Centre => 1,
            Align::Right => 2,
        }
    }

    fn from_quadding(value: i64) -> Align {
        match value {
            1 => Align::Centre,
            2 => Align::Right,
            _ => Align::Left,
        }
    }
}

/// Text written over the redacted area: `/OverlayText` with its `/DA`,
/// `/Q` and `/Repeat`. Set in Helvetica.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlay {
    pub text: String,
    /// In points; `0.0` fits the text to the area.
    pub size: f64,
    pub color: [f64; 3],
    pub align: Align,
    /// Repeat the text to fill the area.
    pub repeat: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay {
            text: String::new(),
            size: 0.0,
            color: [1.0, 1.0, 1.0],
            align: Align::Centre,
            repeat: false,
        }
    }
}

/// How a mark looks, and how its area looks once applied.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactionLook {
    /// `/IC`: the fill; `None` leaves the area empty.
    pub fill: Option<[f64; 3]>,
    /// `/OC`: the mark's outline before it is applied.
    pub outline: [f64; 3],
    pub overlay: Option<Overlay>,
}

impl Default for RedactionLook {
    fn default() -> Self {
        RedactionLook {
            fill: Some(BLACK),
            outline: RED,
            overlay: None,
        }
    }
}

/// A redaction mark as the document has it.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactionMark {
    pub objref: ObjRef,
    pub page: PageIndex,
    /// `/Rect`: `[x0 y0 x1 y1]`.
    pub rect: [f64; 4],
    /// `/QuadPoints`: the text it covers. Empty for a region.
    pub quads: Vec<PageQuad>,
    pub look: RedactionLook,
}

impl RedactionMark {
    /// Whether `point` is inside the mark.
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        let [x0, y0, x1, y1] = self.rect;
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }
}
