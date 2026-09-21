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

use super::model::{
    Annotation, BaseFont, BorderEffect, Color, Intent, LineEnding, Quad, Rect, Subtype, TextStyle,
};

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
        Subtype::Polygon => polygon(annotation, rect),
        Subtype::PolyLine => polyline(annotation, rect),
        Subtype::Ink => ink(annotation, rect),
        Subtype::Text => note_icon(annotation, rect),
        Subtype::FileAttachment => paperclip_icon(annotation, rect),
        Subtype::FreeText => free_text(annotation, rect),
        Subtype::Stamp => stamp(annotation, rect),
    };
    let fonts: Vec<BaseFont> = annotation
        .text_style
        .map(|style| style.font)
        .into_iter()
        .collect();
    form(
        rect,
        &content,
        annotation.opacity,
        blend_mode(annotation),
        &fonts,
    )
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
fn form(
    rect: Rect,
    content: &str,
    opacity: Option<f64>,
    blend: Option<&str>,
    fonts: &[BaseFont],
) -> Stream {
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
    // A direct dictionary rather than an indirect font object: it is one
    // standard font named by `/BaseFont` with no `/FontFile` of any kind, so
    // there is nothing to share between annotations and nothing shipped.
    if !fonts.is_empty() {
        let mut named = Dict::new();
        for font in fonts {
            let mut descriptor = Dict::new();
            descriptor.set(Name::new("Type"), Object::name("Font"));
            descriptor.set(Name::new("Subtype"), Object::name("Type1"));
            descriptor.set(Name::new("BaseFont"), Object::name(font.base_font()));
            named.set(Name::new(font.resource_name()), Object::Dict(descriptor));
        }
        resources.set(Name::new("Font"), Object::Dict(named));
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
    let (start, end) = ((sx - rect.x0, sy - rect.y0), (ex - rect.x0, ey - rect.y0));
    let mut out = format!(
        "{} {} {} RG {} {} {} rg {} w {} {} m {} {} l S\n",
        color.red,
        color.green,
        color.blue,
        color.red,
        color.green,
        color.blue,
        annotation.border_width,
        start.0,
        start.1,
        end.0,
        end.1,
    );
    // `/LE` is what a reader draws the endings from when it synthesizes an
    // appearance; with an `/AP` present it draws the stream, so an arrow whose
    // head lives only in `/LE` is a line.
    if let Some((first, last)) = annotation.endings {
        out.push_str(&arrow_head(first, end, start, annotation.border_width));
        out.push_str(&arrow_head(last, start, end, annotation.border_width));
    }
    out
}

/// One arrow head at `tip`, pointing away from `from`.
///
/// Sized from the border width the way Acrobat does, so a thicker line gets a
/// proportionally larger head rather than a pin-prick on a fat stroke.
fn arrow_head(ending: LineEnding, from: (f64, f64), tip: (f64, f64), width: f64) -> String {
    if ending == LineEnding::None {
        return String::new();
    }
    let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
    let length = dx.hypot(dy);
    if length == 0.0 {
        return String::new();
    }
    let (ux, uy) = (dx / length, dy / length);
    let size = (width * 4.0).max(6.0);
    // Half the head's width, which sets the angle: about 22 degrees a side.
    let spread = size * 0.4;
    let base = (tip.0 - ux * size, tip.1 - uy * size);
    let (px, py) = (-uy * spread, ux * spread);
    let wings = ((base.0 + px, base.1 + py), (base.0 - px, base.1 - py));
    match ending {
        LineEnding::ClosedArrow => format!(
            "{} {} m {} {} l {} {} l h f\n",
            tip.0, tip.1, wings.0 .0, wings.0 .1, wings.1 .0, wings.1 .1
        ),
        LineEnding::OpenArrow => format!(
            "{} {} m {} {} l {} {} l S\n",
            wings.0 .0, wings.0 .1, tip.0, tip.1, wings.1 .0, wings.1 .1
        ),
        LineEnding::None => String::new(),
    }
}

/// A closed shape through its vertices, cloudy when `/BE` says so.
fn polygon(annotation: &Annotation, rect: Rect) -> String {
    vertex_path(annotation, rect, true)
}

/// The same path left open.
fn polyline(annotation: &Annotation, rect: Rect) -> String {
    vertex_path(annotation, rect, false)
}

fn vertex_path(annotation: &Annotation, rect: Rect, closed: bool) -> String {
    if annotation.vertices.len() < 2 {
        return String::new();
    }
    let local: Vec<(f64, f64)> = annotation
        .vertices
        .iter()
        .map(|(x, y)| (x - rect.x0, y - rect.y0))
        .collect();
    let mut out = stroke_and_fill(annotation);
    let _ = writeln!(out, "{} w 1 J 1 j", annotation.border_width);

    if let (true, Some(BorderEffect::Cloudy { intensity })) = (closed, annotation.border_effect) {
        out.push_str(&cloudy(&local, intensity, annotation.border_width));
    } else {
        let _ = writeln!(out, "{} {} m", local[0].0, local[0].1);
        for (x, y) in &local[1..] {
            let _ = writeln!(out, "{x} {y} l");
        }
        if closed {
            out.push_str("h\n");
        }
    }
    out.push_str(paint_operator(annotation, closed));
    out
}

/// A scalloped edge: arcs bulging outward along each side.
///
/// Drawn rather than left to `/BE`, for the reason the arrow head is: a reader
/// with an `/AP` draws the stream, so a cloud whose scallops live only in the
/// border-effect dictionary renders as a plain polygon.
fn cloudy(points: &[(f64, f64)], intensity: f64, width: f64) -> String {
    // Acrobat's intensity is 0, 1 or 2; 0 means a flat edge.
    let bulge = intensity.max(0.0) * (width.max(1.0) * 3.0);
    if bulge == 0.0 {
        let mut out = format!("{} {} m\n", points[0].0, points[0].1);
        for (x, y) in &points[1..] {
            let _ = writeln!(out, "{x} {y} l");
        }
        out.push_str("h\n");
        return out;
    }
    // Which side is outward depends on the winding, and the user decides that
    // by the order they click. Taken from the signed area rather than assumed,
    // because a polygon clicked the other way round would otherwise scallop
    // inward and look like a gear.
    //
    // The sign: `scallops` lifts along the *left* of travel, and in PDF user
    // space - where y increases upward - the left of travel is the **inside**
    // of a counterclockwise polygon. So a positive signed area wants the lift
    // turned around.
    let outward = if signed_area(points) >= 0.0 {
        -1.0
    } else {
        1.0
    };
    let mut out = format!("{} {} m\n", points[0].0, points[0].1);
    for index in 0..points.len() {
        let from = points[index];
        let to = points[(index + 1) % points.len()];
        out.push_str(&scallops(from, to, bulge, outward));
    }
    out.push_str("h\n");
    out
}

/// Twice the signed area of a closed polygon: positive counterclockwise.
fn signed_area(points: &[(f64, f64)]) -> f64 {
    let mut total = 0.0;
    for index in 0..points.len() {
        let (x0, y0) = points[index];
        let (x1, y1) = points[(index + 1) % points.len()];
        total += x0 * y1 - x1 * y0;
    }
    total
}

/// One side of a cloud, as a run of outward arcs approximated by cubics.
fn scallops(from: (f64, f64), to: (f64, f64), bulge: f64, outward: f64) -> String {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    if length == 0.0 {
        return String::new();
    }
    let count = ((length / (bulge * 2.0)).round() as usize).max(1);
    let (ux, uy) = (dx / length, dy / length);
    // To the left of travel, turned around by `outward` when the caller found
    // the winding to be clockwise.
    let (nx, ny) = (-uy * outward, ux * outward);
    let step = length / count as f64;
    let mut out = String::new();
    for index in 0..count {
        let start = (
            from.0 + ux * step * index as f64,
            from.1 + uy * step * index as f64,
        );
        let end = (
            from.0 + ux * step * (index + 1) as f64,
            from.1 + uy * step * (index + 1) as f64,
        );
        let lift = bulge * 1.33;
        let _ = writeln!(
            out,
            "{} {} {} {} {} {} c",
            start.0 + ux * step * 0.25 + nx * lift,
            start.1 + uy * step * 0.25 + ny * lift,
            end.0 - ux * step * 0.25 + nx * lift,
            end.1 - uy * step * 0.25 + ny * lift,
            end.0,
            end.1
        );
    }
    out
}

/// Stroke, fill, or both, which is the one place that decision is made.
fn paint_operator(annotation: &Annotation, closed: bool) -> &'static str {
    match (annotation.interior_color.is_some(), closed) {
        (true, true) => "B\n",
        (false, true) => "s\n",
        (_, false) => "S\n",
    }
}

/// Freehand ink. A stroke whose points carry pen widths is drawn a segment
/// at a time, each at the mean of its ends' widths, which is how pressure
/// reaches the page; a stroke without is one path at `/BS /W`. A stroke of one
/// point is a dot, drawn as a zero-length segment with round caps.
fn ink(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::BLACK);
    let mut out = format!(
        "{} {} {} RG {} w 1 J 1 j\n",
        color.red, color.green, color.blue, annotation.border_width
    );
    for (index, stroke) in annotation.ink.iter().enumerate() {
        let local: Vec<(f64, f64)> = stroke
            .iter()
            .map(|(x, y)| (x - rect.x0, y - rect.y0))
            .collect();
        match annotation
            .ink_widths
            .get(index)
            .filter(|widths| widths.len() == stroke.len())
        {
            Some(widths) => pressured_stroke(&mut out, &local, widths),
            None => uniform_stroke(&mut out, &local),
        }
    }
    out
}

fn uniform_stroke(out: &mut String, points: &[(f64, f64)]) {
    let Some(&(x, y)) = points.first() else {
        return;
    };
    let _ = writeln!(out, "{x} {y} m");
    if points.len() == 1 {
        let _ = writeln!(out, "{x} {y} l");
    }
    for (x, y) in &points[1..] {
        let _ = writeln!(out, "{x} {y} l");
    }
    out.push_str("S\n");
}

fn pressured_stroke(out: &mut String, points: &[(f64, f64)], widths: &[f64]) {
    if let ([(x, y)], [width]) = (points, widths) {
        let _ = writeln!(out, "{width} w {x} {y} m {x} {y} l S");
        return;
    }
    for (pair, width) in points.windows(2).zip(widths.windows(2)) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        let width = (width[0] + width[1]) / 2.0;
        let _ = writeln!(out, "{width} w {x0} {y0} m {x1} {y1} l S");
    }
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

/// A paperclip, drawn here rather than left to the reader: with no `/AP` a
/// reader draws its own icon, and every reader's is different.
fn paperclip_icon(annotation: &Annotation, rect: Rect) -> String {
    let color = annotation.color.unwrap_or(Color::new(0.2, 0.3, 0.6));
    let (w, h) = (rect.width(), rect.height());
    let x = |fraction: f64| w * fraction;
    let y = |fraction: f64| h * fraction;
    format!(
        "{} {} {} RG {} w 1 J 1 j\n\
         {} {} m {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l S\n",
        color.red,
        color.green,
        color.blue,
        (w.min(h) * 0.08).max(0.75),
        // Up the inner wire, over the top, down the outer wire, round the
        // bottom and back up: a clip seen from the front.
        x(0.45),
        y(0.35),
        x(0.45),
        y(0.8),
        x(0.45),
        y(0.95),
        x(0.7),
        y(0.95),
        x(0.7),
        y(0.8),
        x(0.7),
        y(0.2),
        x(0.7),
        y(0.02),
        x(0.3),
        y(0.02),
        x(0.3),
        y(0.2),
        x(0.3),
        y(0.7),
    )
}

/// The box and border of a free-text annotation. The text itself is not drawn
/// here: laying out glyphs needs a font resource, which P9a's free-text tool
/// owns along with the font it chose. This generator produces the frame that
/// tool draws into, and an empty frame is what an empty free text looks like.
/// A free text annotation: its box, its leader if it is a callout, and its
/// text drawn in the same style the `/DA` names.
///
/// A typewriter has no box: `/IT /FreeTextTypewriter` is text placed on the
/// page, and drawing a border around it would be a border the user never
/// asked for and cannot remove.
fn free_text(annotation: &Annotation, rect: Rect) -> String {
    let interior = annotation.interior_color;
    let border = annotation.color.unwrap_or(Color::BLACK);
    let width = annotation.border_width;
    let inset = width / 2.0;
    let mut out = String::new();
    if annotation.intent != Some(Intent::FreeTextTypewriter) {
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
    }
    // The leader, in the form's own space. `/CL` is in page coordinates, the
    // same as `/Rect`, so it shifts by the same origin everything else here
    // does. A reader draws `/CL` itself only when there is no `/AP`; with one,
    // what is in the stream is what is seen, so leaving it out here is how a
    // callout loses its leader in every viewer that trusts the appearance.
    if annotation.callout.len() >= 2 {
        let _ = writeln!(
            out,
            "{} {} {} RG {} w",
            border.red,
            border.green,
            border.blue,
            width.max(1.0)
        );
        for (index, (x, y)) in annotation.callout.iter().enumerate() {
            let (x, y) = (x - rect.x0, y - rect.y0);
            let _ = writeln!(out, "{x} {y} {}", if index == 0 { "m" } else { "l" });
        }
        let _ = writeln!(out, "S");
    }
    if let (Some(style), Some(text)) = (annotation.text_style, annotation.contents.as_deref()) {
        out.push_str(&text_lines(text, &style, rect, annotation.border_width));
    }
    out
}

/// The annotation's text, laid out as lines from the top of the box down.
///
/// The font, size and colour come from the same [`TextStyle`] the `/DA`
/// string is built from, which is the whole reason that type exists: a stream
/// that picked its own would render one way in a reader that trusts `/AP` and
/// another in one that re-lays-out from `/DA`.
///
/// Line breaking is on the text's own newlines only. Wrapping to the box needs
/// the font's widths, and this crate has no font metrics; a wrap computed from
/// a guessed advance is worse than none, because it looks deliberate.
fn text_lines(text: &str, style: &TextStyle, rect: Rect, border: f64) -> String {
    let padding = border + 2.0;
    let leading = style.size * 1.2;
    let mut out = String::from("BT\n");
    let _ = writeln!(out, "{}", style.default_appearance());
    let _ = writeln!(out, "{leading} TL");
    let _ = writeln!(
        out,
        "{padding} {} Td",
        (rect.height() - padding - style.size).max(0.0)
    );
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            out.push_str("T*\n");
        }
        let _ = writeln!(out, "({}) Tj", escape(line));
    }
    out.push_str("ET\n");
    out
}

/// A literal string's escapes, which are the only three characters that end
/// one early or nest a parenthesis wrongly.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(character);
            }
            _ => out.push(character),
        }
    }
    out
}

/// A stamp's appearance from its artwork: the art scaled to fill the rect,
/// in the form's own `[0 0 w h]` box like every other appearance here.
///
/// A drawing is inlined with the fonts it names. A page is imported as a
/// Form XObject of its own - resources and all - and drawn through the
/// appearance, so the stamp is self-contained in the document it lands in.
pub(crate) fn stamp_appearance(
    tx: &mut crate::edit::Transaction<'_>,
    annotation: &Annotation,
    art: &super::model::StampArt,
) -> crate::Result<Stream> {
    use super::model::StampArt;

    let rect = annotation.rect;
    let (content, fonts, art_form) = match art {
        StampArt::Drawing {
            size,
            content,
            fonts,
        } => {
            let (sx, sy) = (rect.width() / size.0, rect.height() / size.1);
            (
                format!("{sx} 0 0 {sy} 0 0 cm\n{content}\n"),
                fonts.clone(),
                None,
            )
        }
        StampArt::Page(pdf) => {
            let source = onionskin_cos::Document::open(Box::new(
                onionskin_cos::BytesSource::from_shared(pdf.clone()),
            ))?;
            let (form, [x0, y0, x1, y1]) = crate::pages::import_page_as_form(tx, &source, 0)?;
            let (sx, sy) = (rect.width() / (x1 - x0), rect.height() / (y1 - y0));
            (
                format!("{sx} 0 0 {sy} {} {} cm\n/Art Do\n", -x0 * sx, -y0 * sy),
                Vec::new(),
                Some(form),
            )
        }
    };
    let mut stream = form(rect, &content, annotation.opacity, None, &fonts);
    if let Some(art_form) = art_form {
        let Some(Object::Dict(resources)) = stream.dict.get(b"Resources").cloned() else {
            unreachable!("every appearance has resources");
        };
        let mut resources = resources;
        let mut xobjects = Dict::new();
        xobjects.set(Name::new("Art"), Object::Ref(art_form));
        resources.set(Name::new("XObject"), Object::Dict(xobjects));
        stream
            .dict
            .set(Name::new("Resources"), Object::Dict(resources));
    }
    Ok(stream)
}

/// A stamp's frame, for a stamp with no artwork.
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
