//! Appearance stream generation: `/AP` `/N` as a Form XObject, per subtype.
//!
//! Onionskin writes `/AP` itself. A reader is allowed to synthesize one when a
//! markup annotation has none, and every reader synthesizes a different thing,
//! so a file without `/AP` looks different in Acrobat, Preview, Chrome and
//! hayro. Acrobat writes `/AP`; so do we.
//!
//! **The coordinate space, which is the thing to get right.** ISO 32000-1
//! 12.5.5 maps the form's `/BBox`, transformed by `/Matrix`, onto the
//! annotation's `/Rect`: the transformed bounding box is scaled and translated
//! to fit `/Rect` exactly. Writing a `/BBox` in page coordinates and an
//! identity `/Matrix` therefore produces an appearance that is *re-scaled* by
//! the ratio between the box and the rect, which looks right at the zoom where
//! the ratio happens to be 1 and drifts at every other.
//!
//! So every generator here writes **`/BBox [0 0 w h]` with an identity
//! `/Matrix`**, where `w` and `h` are the `/Rect`'s own width and height, and
//! draws in that box's coordinates with the rect's lower-left corner as the
//! origin. The mapping is then exactly the identity at every zoom, which is
//! what makes the drift impossible rather than merely absent from one test.

use std::fmt::Write as _;

use onionskin_cos::{Dict, Name, Object, Stream};

use super::model::{Annotation, Color, Quad, Rect, Subtype};

/// The `/AP` `/N` form for this annotation, in the annotation's own space.
pub(crate) fn normal_appearance(annotation: &Annotation) -> Stream {
    let rect = annotation.rect;
    let content = match annotation.subtype {
        Subtype::Highlight => highlight(annotation, rect),
        Subtype::Underline => rule(annotation, rect, RulePosition::Under),
        Subtype::StrikeOut => rule(annotation, rect, RulePosition::Through),
        Subtype::Squiggly => squiggly(annotation, rect),
        Subtype::Square => square(annotation, rect),
        Subtype::Circle => circle(annotation, rect),
        Subtype::Line => line(annotation, rect),
        Subtype::Ink => ink(annotation, rect),
        Subtype::Text | Subtype::FileAttachment => note_icon(annotation, rect),
        Subtype::FreeText => free_text(annotation, rect),
        Subtype::Stamp => stamp(annotation, rect),
    };
    form(rect, &content, annotation.opacity, blend_mode(annotation))
}

/// The blend mode a subtype's appearance needs, or `None` for the default.
///
/// Only Highlight has one. A highlighter lays ink over the text rather than
/// replacing it, and an opaque fill hides every glyph it covers: the mark is in
/// the right place and the words under it are gone. `/BM /Multiply` is how
/// Acrobat gets the ink to darken what is beneath instead of painting it out,
/// and it has to be in the appearance's own graphics state, because a reader
/// composites the form as the form asks.
fn blend_mode(annotation: &Annotation) -> Option<&'static str> {
    match annotation.subtype {
        Subtype::Highlight => Some("Multiply"),
        _ => None,
    }
}

/// Wrap a content stream as a Form XObject whose box is the rect at the origin.
fn form(rect: Rect, content: &str, opacity: Option<f64>, blend: Option<&str>) -> Stream {
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("XObject"));
    dict.set(Name::new("Subtype"), Object::name("Form"));
    dict.set(Name::new("FormType"), Object::Integer(1));
    dict.set(
        Name::new("BBox"),
        numbers(&[0.0, 0.0, rect.width(), rect.height()]),
    );
    // Stated rather than omitted. An absent /Matrix defaults to the identity,
    // but writing it makes the invariant this module rests on visible in the
    // file instead of implied by its absence.
    dict.set(
        Name::new("Matrix"),
        numbers(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
    );

    let state = graphics_state(opacity, blend);
    let mut resources = Dict::new();
    if let Some(state) = state {
        let mut states = Dict::new();
        states.set(Name::new("GS0"), Object::Dict(state));
        resources.set(Name::new("ExtGState"), Object::Dict(states));
    }
    let selects_state = resources.get(b"ExtGState").is_some();
    dict.set(Name::new("Resources"), Object::Dict(resources));

    let body = match selects_state {
        true => format!("q\n/GS0 gs\n{content}Q\n"),
        false => format!("q\n{content}Q\n"),
    };
    Stream {
        dict,
        raw: body.into_bytes(),
    }
}

/// The `/GS0` state the form selects, or `None` when it needs none.
fn graphics_state(opacity: Option<f64>, blend: Option<&str>) -> Option<Dict> {
    if opacity.is_none() && blend.is_none() {
        return None;
    }
    let mut state = Dict::new();
    state.set(Name::new("Type"), Object::name("ExtGState"));
    if let Some(opacity) = opacity {
        state.set(Name::new("ca"), Object::Real(opacity));
        state.set(Name::new("CA"), Object::Real(opacity));
    }
    if let Some(blend) = blend {
        state.set(Name::new("BM"), Object::name(blend));
    }
    Some(state)
}

/// Quad coordinates moved into the form's space, whose origin is the rect's
/// lower-left corner.
fn local(quad: &Quad, rect: Rect) -> Quad {
    let shift = |(x, y): (f64, f64)| (x - rect.x0, y - rect.y0);
    Quad {
        upper_left: shift(quad.upper_left),
        upper_right: shift(quad.upper_right),
        lower_left: shift(quad.lower_left),
        lower_right: shift(quad.lower_right),
    }
}

fn highlight(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::YELLOW);
    let mut out = String::new();
    // Multiply is what a highlighter does: the text underneath stays readable.
    let _ = writeln!(out, "{} {} {} rg", color.red, color.green, color.blue);
    for quad in &annotation.quads {
        let q = local(quad, rect);
        let _ = writeln!(
            out,
            "{} {} m {} {} l {} {} l {} {} l h f",
            q.upper_left.0,
            q.upper_left.1,
            q.upper_right.0,
            q.upper_right.1,
            q.lower_right.0,
            q.lower_right.1,
            q.lower_left.0,
            q.lower_left.1,
        );
    }
    out
}

enum RulePosition {
    Under,
    Through,
}

/// Underline and strike-out are the same rule at different heights, which is
/// why they share a generator rather than drifting apart.
fn rule(annotation: &Annotation, rect: Rect, position: RulePosition) -> String {
    let color = annotation.color.unwrap_or(Color::BLACK);
    let mut out = String::new();
    let _ = writeln!(out, "{} {} {} RG", color.red, color.green, color.blue);
    for quad in &annotation.quads {
        let q = local(quad, rect);
        let height = (q.upper_left.1 - q.lower_left.1).abs();
        let thickness = (height * 0.06).max(0.5);
        let y = match position {
            RulePosition::Under => q.lower_left.1 + height * 0.06,
            RulePosition::Through => q.lower_left.1 + height * 0.42,
        };
        let _ = writeln!(
            out,
            "{thickness} w {} {y} m {} {y} l S",
            q.lower_left.0, q.lower_right.0
        );
    }
    out
}

fn squiggly(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::BLACK);
    let mut out = String::new();
    let _ = writeln!(out, "{} {} {} RG", color.red, color.green, color.blue);
    for quad in &annotation.quads {
        let q = local(quad, rect);
        let height = (q.upper_left.1 - q.lower_left.1).abs();
        let amplitude = (height * 0.08).max(0.6);
        let step = amplitude * 2.0;
        let base = q.lower_left.1 + amplitude;
        let _ = writeln!(out, "{} w {} {base} m", amplitude * 0.6, q.lower_left.0);
        let mut x = q.lower_left.0;
        let mut up = true;
        while x < q.lower_right.0 {
            x = (x + step).min(q.lower_right.0);
            let y = if up { base + amplitude } else { base };
            let _ = writeln!(out, "{x} {y} l");
            up = !up;
        }
        out.push_str("S\n");
    }
    out
}

fn square(annotation: &Annotation, rect: Rect) -> String {
    let width = annotation.border_width;
    let inset = width / 2.0;
    let mut out = stroke_and_fill(annotation);
    let _ = writeln!(
        out,
        "{width} w {inset} {inset} {} {} re {}",
        rect.width() - width,
        rect.height() - width,
        paint(annotation)
    );
    out
}

/// A circle inscribed in the rect, as four Bezier arcs. `0.5523` is the
/// standard circle-to-Bezier constant; a four-segment approximation is what
/// every producer writes and is accurate to about one part in a thousand.
fn circle(annotation: &Annotation, rect: Rect) -> String {
    const KAPPA: f64 = 0.552_284_749_8;
    let width = annotation.border_width;
    let inset = width / 2.0;
    let (x0, y0) = (inset, inset);
    let (x1, y1) = (rect.width() - inset, rect.height() - inset);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let (ox, oy) = (rx * KAPPA, ry * KAPPA);

    let mut out = stroke_and_fill(annotation);
    let _ = writeln!(
        out,
        "{width} w {} {cy} m \
         {} {} {} {} {cx} {y1} c \
         {} {} {} {} {x1} {cy} c \
         {} {} {} {} {cx} {y0} c \
         {} {} {} {} {x0} {cy} c {}",
        x0,
        x0,
        cy + oy,
        cx - ox,
        y1,
        cx + ox,
        y1,
        x1,
        cy + oy,
        x1,
        cy - oy,
        cx + ox,
        y0,
        cx - ox,
        y0,
        x0,
        cy - oy,
        paint(annotation)
    );
    out
}

fn line(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::BLACK);
    let Some(((sx, sy), (ex, ey))) = annotation.line else {
        return String::new();
    };
    format!(
        "{} {} {} RG {} w {} {} m {} {} l S\n",
        color.red,
        color.green,
        color.blue,
        annotation.border_width,
        sx - rect.x0,
        sy - rect.y0,
        ex - rect.x0,
        ey - rect.y0,
    )
}

fn ink(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::BLACK);
    let mut out = format!(
        "{} {} {} RG {} w 1 J 1 j\n",
        color.red, color.green, color.blue, annotation.border_width
    );
    for stroke in &annotation.ink {
        let mut points = stroke.iter();
        let Some((x, y)) = points.next() else {
            continue;
        };
        let _ = writeln!(out, "{} {} m", x - rect.x0, y - rect.y0);
        for (x, y) in points {
            let _ = writeln!(out, "{} {} l", x - rect.x0, y - rect.y0);
        }
        out.push_str("S\n");
    }
    out
}

/// A sticky note or attachment icon: a filled rounded square with a fold.
/// Deliberately simple and drawn from primitives, because a bitmap or an
/// embedded font would make the appearance depend on resources this module
/// would then have to own.
fn note_icon(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::YELLOW);
    let (w, h) = (rect.width(), rect.height());
    let inset = (w.min(h) * 0.08).max(0.5);
    format!(
        "{} {} {} rg 0 0 0 RG 0.5 w\n\
         {inset} {inset} {} {} re B\n\
         {} {} m {} {} l S\n\
         {} {} m {} {} l S\n\
         {} {} m {} {} l S\n",
        color.red,
        color.green,
        color.blue,
        w - inset * 2.0,
        h - inset * 2.0,
        w * 0.25,
        h * 0.68,
        w * 0.75,
        h * 0.68,
        w * 0.25,
        h * 0.5,
        w * 0.75,
        h * 0.5,
        w * 0.25,
        h * 0.32,
        w * 0.55,
        h * 0.32,
    )
}

/// The box and border of a free-text annotation. The text itself is not drawn
/// here: laying out glyphs needs a font resource, which P9a's free-text tool
/// owns along with the font it chose. This generator produces the frame that
/// tool draws into, and an empty frame is what an empty free text looks like.
fn free_text(annotation: &Annotation, rect: Rect) -> String {
    let interior = annotation.interior_color;
    let border = annotation.color.unwrap_or(Color::BLACK);
    let width = annotation.border_width;
    let inset = width / 2.0;
    let mut out = String::new();
    if let Some(fill) = interior {
        let _ = writeln!(out, "{} {} {} rg", fill.red, fill.green, fill.blue);
    }
    let _ = writeln!(
        out,
        "{} {} {} RG {width} w {inset} {inset} {} {} re {}",
        border.red,
        border.green,
        border.blue,
        rect.width() - width,
        rect.height() - width,
        if interior.is_some() { "B" } else { "S" }
    );
    out
}

/// A stamp's frame. The stamp artwork itself comes from P10, which owns the
/// stamp library and the images in it.
fn stamp(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::new(0.8, 0.1, 0.1));
    let width = annotation.border_width.max(1.5);
    let inset = width / 2.0;
    format!(
        "{} {} {} RG {width} w {inset} {inset} {} {} re S\n",
        color.red,
        color.green,
        color.blue,
        rect.width() - width,
        rect.height() - width,
    )
}

fn stroke_and_fill(annotation: &Annotation) -> String {
    let stroke = annotation.color.unwrap_or(Color::BLACK);
    let mut out = format!("{} {} {} RG\n", stroke.red, stroke.green, stroke.blue);
    if let Some(fill) = annotation.interior_color {
        let _ = writeln!(out, "{} {} {} rg", fill.red, fill.green, fill.blue);
    }
    out
}

/// `B` fills and strokes, `S` only strokes. A shape with no `/IC` is an
/// outline, which is what an absent interior colour means.
fn paint(annotation: &Annotation) -> &'static str {
    if annotation.interior_color.is_some() {
        "B"
    } else {
        "S"
    }
}

pub(crate) fn numbers(values: &[f64]) -> Object {
    Object::Array(values.iter().copied().map(Object::Real).collect())
}
