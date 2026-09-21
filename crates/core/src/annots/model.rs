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
    /// A closed shape of three or more vertices. A cloud is one of these with
    /// a `/BE` border effect, not a subtype of its own.
    Polygon,
    /// The open form of the same: connected lines.
    PolyLine,
    Stamp,
    FileAttachment,
}

impl Subtype {
    /// Every subtype `core::annots` authors. The filter and the test suite both
    /// read this rather than keeping their own copies, so a new subtype cannot
    /// be added to one and forgotten in the other.
    pub const ALL: &'static [Subtype] = &[
        Subtype::Highlight,
        Subtype::Underline,
        Subtype::StrikeOut,
        Subtype::Squiggly,
        Subtype::Text,
        Subtype::FreeText,
        Subtype::Ink,
        Subtype::Square,
        Subtype::Circle,
        Subtype::Line,
        Subtype::Polygon,
        Subtype::PolyLine,
        Subtype::Stamp,
        Subtype::FileAttachment,
    ];

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
            Subtype::Polygon => "Polygon",
            Subtype::PolyLine => "PolyLine",
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
            b"Polygon" => Subtype::Polygon,
            b"PolyLine" => Subtype::PolyLine,
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
    /// `/DA`, how a `FreeText`'s text is drawn. `None` for every other
    /// subtype, and for a `FreeText` that carries no text of its own.
    pub text_style: Option<TextStyle>,
    /// `/IT`, what a `FreeText` is being used as.
    pub intent: Option<Intent>,
    /// `/CL`, a callout's leader: two or three points, the tail - what the
    /// callout points at - first, the end at the text box last. Empty for
    /// everything else.
    pub callout: Vec<(f64, f64)>,
    /// `/Vertices`, for `Polygon` and `PolyLine`. A polygon's closing edge is
    /// implied rather than written, which is what the subtype means.
    pub vertices: Vec<(f64, f64)>,
    /// `/LE`, the endings a `Line` is drawn with, first then last. `None` for
    /// a plain line; an arrow is this and nothing else, because Acrobat has
    /// no arrow subtype and a `/Polygon` drawn as one renders in Acrobat as a
    /// line with no head.
    pub endings: Option<(LineEnding, LineEnding)>,
    /// `/BE`, the border effect. `Cloudy` is what makes a polygon a cloud.
    pub border_effect: Option<BorderEffect>,
}

/// `/LE` entries. Only the three M3 draws; a reader that meets a name it does
/// not know draws nothing, so writing one we cannot draw would be worse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    None,
    OpenArrow,
    ClosedArrow,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::None => "None",
            LineEnding::OpenArrow => "OpenArrow",
            LineEnding::ClosedArrow => "ClosedArrow",
        }
    }
}

/// `/BE`, a border effect: `/S` and, for a cloud, `/I` its intensity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderEffect {
    /// A scalloped edge. The intensity is 0, 1 or 2 in Acrobat's UI; a
    /// `/BE` with `/S /C` and no `/I` is a cloud a reader draws flat.
    Cloudy { intensity: f64 },
}

/// The fonts a `FreeText` may name.
///
/// Named, never embedded. Onionskin ships no font files (`PLAN.md` legal
/// posture rule 6), and it does not need to: every one of these is a standard
/// Type 1 font every reader substitutes for, and Acrobat itself writes
/// `/Helv` by name into a `/DA`. A `/FontFile` would be the thing rule 6
/// forbids, and there is none anywhere in this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseFont {
    Helvetica,
    TimesRoman,
    Courier,
}

impl BaseFont {
    /// The key a `/DA` string and the appearance's `/Resources` `/Font` both
    /// use. Acrobat's own short names, which is what a reader that patches a
    /// `/DA` expects to find.
    pub fn resource_name(self) -> &'static str {
        match self {
            BaseFont::Helvetica => "Helv",
            BaseFont::TimesRoman => "TiRo",
            BaseFont::Courier => "Cour",
        }
    }

    /// The `/BaseFont` name, which is the standard font this stands for.
    pub fn base_font(self) -> &'static str {
        match self {
            BaseFont::Helvetica => "Helvetica",
            BaseFont::TimesRoman => "Times-Roman",
            BaseFont::Courier => "Courier",
        }
    }
}

/// How a `FreeText`'s text is drawn.
///
/// **One source for both places the font appears.** A `/DA` string and the
/// appearance stream that draws the text can disagree about font, size or
/// colour, and when they do the annotation renders one way in a reader that
/// trusts `/AP` and another in one that regenerates from `/DA` - which is what
/// makes a text box look different in Acrobat. Both come from here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub font: BaseFont,
    pub size: f64,
    pub color: Color,
}

impl TextStyle {
    pub fn new(font: BaseFont, size: f64, color: Color) -> Self {
        TextStyle { font, size, color }
    }

    /// The `/DA` string: select the font at its size, then set the fill colour
    /// the text is painted with.
    pub fn default_appearance(&self) -> String {
        format!(
            "/{} {} Tf {} {} {} rg",
            self.font.resource_name(),
            self.size,
            self.color.red,
            self.color.green,
            self.color.blue
        )
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle::new(BaseFont::Helvetica, 12.0, Color::BLACK)
    }
}

/// `/IT`: what a `FreeText` is being used as.
///
/// A reader uses it to decide which handles to show and how to re-lay-out the
/// annotation when its text changes, so a typewriter that says it is a plain
/// text box gains a border it never had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Text on the page with no box around it.
    FreeTextTypewriter,
    /// A box with a leader line to what it points at.
    FreeTextCallout,
}

impl Intent {
    pub fn as_str(self) -> &'static str {
        match self {
            Intent::FreeTextTypewriter => "FreeTextTypewriter",
            Intent::FreeTextCallout => "FreeTextCallout",
        }
    }
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
            text_style: None,
            intent: None,
            callout: Vec::new(),
            vertices: Vec::new(),
            endings: None,
            border_effect: None,
        }
    }

    /// A polygon or polyline from its vertices, with the `/Rect` their bounds.
    ///
    /// `None` for fewer than two vertices, and for fewer than three when the
    /// shape is closed: a polygon of two points is a line drawn twice.
    pub fn vertices(subtype: Subtype, vertices: Vec<(f64, f64)>) -> Option<Self> {
        let least = match subtype {
            Subtype::Polygon => 3,
            Subtype::PolyLine => 2,
            _ => return None,
        };
        if vertices.len() < least {
            return None;
        }
        let mut bounds = Rect::new(vertices[0].0, vertices[0].1, vertices[0].0, vertices[0].1);
        for (x, y) in &vertices[1..] {
            bounds = bounds.union(Rect::new(*x, *y, *x, *y));
        }
        let mut annotation = Annotation::new(subtype, bounds);
        annotation.vertices = vertices;
        Some(annotation)
    }

    /// A `FreeText` carrying text, which is the only subtype that needs a
    /// `/DA`. The intent decides what a reader thinks it is looking at.
    pub fn free_text(rect: Rect, style: TextStyle, intent: Option<Intent>) -> Self {
        let mut annotation = Annotation::new(Subtype::FreeText, rect);
        annotation.text_style = Some(style);
        annotation.intent = intent;
        annotation
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
