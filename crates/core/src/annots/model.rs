//! The typed annotation, and the conventions this crate commits to.
//!
//! One place authors annotations so that thirty-odd comment tools do not each
//! get a corner of it slightly wrong. The tool packages own geometry and
//! defaults; everything about how an annotation is written lives here.

use onionskin_cos::{Name, ObjRef};

/// The annotation subtypes M3 authors.
///
/// Every one of these has an appearance generator in
/// [`super::appearance`], because Onionskin writes `/AP` itself rather than
/// relying on a reader to synthesize one: Acrobat writes it, and readers that
/// synthesize disagree with each other about the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subtype {
    Highlight,
    Underline,
    StrikeOut,
    Squiggly,
    /// A sticky note. Its `/Rect` is the icon, not the text.
    Text,
    FreeText,
    Ink,
    Square,
    Circle,
    Line,
    Stamp,
    FileAttachment,
}

impl Subtype {
    pub fn as_name(self) -> Name {
        Name::new(self.as_str())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Subtype::Highlight => "Highlight",
            Subtype::Underline => "Underline",
            Subtype::StrikeOut => "StrikeOut",
            Subtype::Squiggly => "Squiggly",
            Subtype::Text => "Text",
            Subtype::FreeText => "FreeText",
            Subtype::Ink => "Ink",
            Subtype::Square => "Square",
            Subtype::Circle => "Circle",
            Subtype::Line => "Line",
            Subtype::Stamp => "Stamp",
            Subtype::FileAttachment => "FileAttachment",
        }
    }

    pub fn from_name(name: &Name) -> Option<Subtype> {
        Some(match name.as_bytes() {
            b"Highlight" => Subtype::Highlight,
            b"Underline" => Subtype::Underline,
            b"StrikeOut" => Subtype::StrikeOut,
            b"Squiggly" => Subtype::Squiggly,
            b"Text" => Subtype::Text,
            b"FreeText" => Subtype::FreeText,
            b"Ink" => Subtype::Ink,
            b"Square" => Subtype::Square,
            b"Circle" => Subtype::Circle,
            b"Line" => Subtype::Line,
            b"Stamp" => Subtype::Stamp,
            b"FileAttachment" => Subtype::FileAttachment,
            _ => return None,
        })
    }

    /// Markup annotations are the ones a reader can be asked to hide as a
    /// class. A `/Widget` is not one, which is what makes `FormFieldsOnly` a
    /// different question from `DocumentOnly`.
    pub fn is_markup(self) -> bool {
        // Every subtype M3 authors is a markup annotation. `/Popup` and
        // `/Widget` are the two that are not, and M3 authors neither.
        true
    }

    /// Whether this subtype takes `/QuadPoints`: the four text-markup types
    /// and nothing else.
    pub fn takes_quads(self) -> bool {
        matches!(
            self,
            Subtype::Highlight | Subtype::Underline | Subtype::StrikeOut | Subtype::Squiggly
        )
    }
}

/// A rectangle in PDF user space, `[x0 y0 x1 y1]` with `x0 <= x1` and
/// `y0 <= y1`. Normalized on construction, because a `/Rect` written the other
/// way round is legal in a file and painful everywhere else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// The smallest rectangle containing both.
    pub fn union(self, other: Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    pub fn contains(&self, other: &Rect) -> bool {
        self.x0 <= other.x0 && self.y0 <= other.y0 && self.x1 >= other.x1 && self.y1 >= other.y1
    }
}

/// One text-markup quadrilateral.
///
/// **Order.** `/QuadPoints` is written as upper-left, upper-right, lower-left,
/// lower-right. ISO 32000-1 12.5.6.10's prose says "counterclockwise", which
/// would be a different order, and essentially no producer follows the prose:
/// Acrobat writes the order above, every major reader expects it, and a file
/// written the other way renders as a rotated or empty markup. Onionskin writes
/// what readers read. This is stated here rather than in a comment at the call
/// site because it is the single most common way a text markup comes out wrong.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub upper_left: (f64, f64),
    pub upper_right: (f64, f64),
    pub lower_left: (f64, f64),
    pub lower_right: (f64, f64),
}

impl Quad {
    /// The axis-aligned quad for a rectangle, which is what a text run on an
    /// unrotated page produces.
    pub fn from_rect(rect: Rect) -> Self {
        Quad {
            upper_left: (rect.x0, rect.y1),
            upper_right: (rect.x1, rect.y1),
            lower_left: (rect.x0, rect.y0),
            lower_right: (rect.x1, rect.y0),
        }
    }

    pub fn bounds(&self) -> Rect {
        let xs = [
            self.upper_left.0,
            self.upper_right.0,
            self.lower_left.0,
            self.lower_right.0,
        ];
        let ys = [
            self.upper_left.1,
            self.upper_right.1,
            self.lower_left.1,
            self.lower_right.1,
        ];
        Rect {
            x0: xs.iter().copied().fold(f64::INFINITY, f64::min),
            y0: ys.iter().copied().fold(f64::INFINITY, f64::min),
            x1: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            y1: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        }
    }

    /// In `/QuadPoints` order.
    pub fn as_array(&self) -> [f64; 8] {
        [
            self.upper_left.0,
            self.upper_left.1,
            self.upper_right.0,
            self.upper_right.1,
            self.lower_left.0,
            self.lower_left.1,
            self.lower_right.0,
            self.lower_right.1,
        ]
    }
}

/// An RGB colour, each component in `0.0..=1.0`, which is what `/C` and `/IC`
/// take. M3 writes no other colour space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}

impl Color {
    pub const fn new(red: f64, green: f64, blue: f64) -> Self {
        Color { red, green, blue }
    }

    pub const YELLOW: Color = Color::new(1.0, 0.92, 0.23);
    pub const BLACK: Color = Color::new(0.0, 0.0, 0.0);
}

/// `/F`, the annotation flags. Only the bits M3 sets are named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags(pub i64);

impl Flags {
    /// Bit 2. The flag the render filter sets, and the only one it sets.
    pub const HIDDEN: i64 = 2;
    /// Bit 3. Set on everything M3 authors, because an annotation that renders
    /// on screen and vanishes in print is a surprise nobody asked for.
    pub const PRINT: i64 = 4;

    pub fn with_hidden(self, hidden: bool) -> Flags {
        if hidden {
            Flags(self.0 | Flags::HIDDEN)
        } else {
            Flags(self.0 & !Flags::HIDDEN)
        }
    }

    pub fn is_hidden(self) -> bool {
        self.0 & Flags::HIDDEN != 0
    }
}

/// An annotation, as a tool describes one.
///
/// **The author name is a field, never a lookup.** Nothing here reads the OS
/// user name: a comment tool passes what the person chose to be called, or
/// passes `None`. Filling it in from the system account would put a real name
/// into a file the person is about to send somewhere, without being asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub subtype: Subtype,
    pub rect: Rect,
    /// Text markup only. Empty for every other subtype.
    pub quads: Vec<Quad>,
    /// Ink only: one entry per stroke, each a list of points.
    pub ink: Vec<Vec<(f64, f64)>>,
    /// `/L`, for `Line`.
    pub line: Option<((f64, f64), (f64, f64))>,
    pub contents: Option<String>,
    /// `/T`. See the type comment: supplied, never discovered.
    pub author: Option<String>,
    pub subject: Option<String>,
    pub color: Option<Color>,
    /// `/IC`, the interior colour of a shape.
    pub interior_color: Option<Color>,
    /// `/CA`, in `0.0..=1.0`.
    pub opacity: Option<f64>,
    pub flags: Flags,
    /// `/Name`, the icon for a `Text` or `FileAttachment` annotation, and the
    /// stamp name for a `Stamp`.
    pub icon: Option<String>,
    /// `/BS` `/W`, the border width.
    pub border_width: f64,
    /// `/IRT`, the annotation this one replies to.
    pub in_reply_to: Option<ObjRef>,
    /// `/State` and `/StateModel`, for a review reply.
    pub state: Option<(String, String)>,
}

impl Annotation {
    /// A minimal annotation of one subtype, which every constructor in the tool
    /// packages starts from.
    pub fn new(subtype: Subtype, rect: Rect) -> Self {
        Annotation {
            subtype,
            rect,
            quads: Vec::new(),
            ink: Vec::new(),
            line: None,
            contents: None,
            author: None,
            subject: None,
            color: None,
            interior_color: None,
            opacity: None,
            flags: Flags(Flags::PRINT),
            icon: None,
            border_width: 1.0,
            in_reply_to: None,
            state: None,
        }
    }

    /// Text markup from a run of quads: the `/Rect` is their union, which is
    /// what a reader uses for hit-testing.
    pub fn markup(subtype: Subtype, quads: Vec<Quad>) -> Option<Self> {
        let mut bounds = quads.first()?.bounds();
        for quad in &quads[1..] {
            bounds = bounds.union(quad.bounds());
        }
        let mut annotation = Annotation::new(subtype, bounds);
        annotation.quads = quads;
        annotation.color = Some(Color::YELLOW);
        Some(annotation)
    }
}
