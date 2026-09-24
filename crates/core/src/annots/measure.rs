//! Measurements: a scale, what it makes of a length or an area on the page,
//! and the annotation a measurement is kept as.
//!
//! **What Acrobat writes.** A measurement is a `/Line`, `/PolyLine` or
//! `/Polygon` whose `/IT` says it is a dimension, with a `/Measure`
//! dictionary of subtype `/RL` (ISO 32000-1 12.9): the scale as words in
//! `/R`, and number formats that turn a length in points into the scale's
//! units (`/X`), name the unit a distance is shown in (`/D`) and the one an
//! area is (`/A`). The measurement itself is in `/Contents` and drawn as the
//! shape's caption, so a reader that knows nothing of `/Measure` still shows
//! it.

use std::fmt::Write as _;

use onionskin_cos::{Dict, Name, Object};

use super::author::text_string;
use super::model::{Annotation, Intent, Rect, Subtype};

/// A unit of length, on the page or in the world the page draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Point,
    Inch,
    Millimetre,
    Centimetre,
    Metre,
    Kilometre,
    Foot,
    Yard,
    Mile,
}

impl Unit {
    pub const ALL: [Unit; 9] = [
        Unit::Point,
        Unit::Inch,
        Unit::Millimetre,
        Unit::Centimetre,
        Unit::Metre,
        Unit::Kilometre,
        Unit::Foot,
        Unit::Yard,
        Unit::Mile,
    ];

    /// The abbreviation Acrobat shows and writes as a number format's `/U`.
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Point => "pt",
            Unit::Inch => "in",
            Unit::Millimetre => "mm",
            Unit::Centimetre => "cm",
            Unit::Metre => "m",
            Unit::Kilometre => "km",
            Unit::Foot => "ft",
            Unit::Yard => "yd",
            Unit::Mile => "mi",
        }
    }

    pub fn from_abbreviation(text: &str) -> Option<Unit> {
        Unit::ALL.into_iter().find(|unit| unit.as_str() == text)
    }

    /// How many points one of this unit is.
    pub fn points(self) -> f64 {
        const INCH: f64 = 72.0;
        const MM: f64 = INCH / 25.4;
        match self {
            Unit::Point => 1.0,
            Unit::Inch => INCH,
            Unit::Millimetre => MM,
            Unit::Centimetre => MM * 10.0,
            Unit::Metre => MM * 1000.0,
            Unit::Kilometre => MM * 1_000_000.0,
            Unit::Foot => INCH * 12.0,
            Unit::Yard => INCH * 36.0,
            Unit::Mile => INCH * 63_360.0,
        }
    }
}

/// A scale ratio: `page` of `page_unit` on the page stands for `real` of
/// `real_unit` in the world, as in "1 in = 10 ft".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub page: f64,
    pub page_unit: Unit,
    pub real: f64,
    pub real_unit: Unit,
}

impl Default for Scale {
    /// Acrobat's own: the page measured as it is, in inches.
    fn default() -> Self {
        Scale::new(1.0, Unit::Inch, 1.0, Unit::Inch)
    }
}

impl Scale {
    pub fn new(page: f64, page_unit: Unit, real: f64, real_unit: Unit) -> Self {
        Scale {
            page,
            page_unit,
            real,
            real_unit,
        }
    }

    /// The scale as words, which is what `/R` holds: "1 in = 10 ft".
    pub fn label(&self) -> String {
        format!(
            "{} {} = {} {}",
            number(self.page),
            self.page_unit.as_str(),
            number(self.real),
            self.real_unit.as_str()
        )
    }

    /// A scale from its words, as [`Self::label`] writes them. `None` for
    /// anything else, or for a side that is not a positive number.
    pub fn parse(text: &str) -> Option<Scale> {
        let (page, real) = text.split_once('=')?;
        let side = |side: &str| -> Option<(f64, Unit)> {
            let mut parts = side.split_whitespace();
            let amount: f64 = parts.next()?.parse().ok()?;
            let unit = Unit::from_abbreviation(parts.next()?)?;
            (parts.next().is_none() && amount.is_finite() && amount > 0.0).then_some((amount, unit))
        };
        let ((page, page_unit), (real, real_unit)) = (side(page)?, side(real)?);
        Some(Scale::new(page, page_unit, real, real_unit))
    }

    /// How many of the real unit one point on the page stands for.
    pub fn per_point(&self) -> f64 {
        self.real / (self.page * self.page_unit.points())
    }
}

/// What is measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Between two points.
    Distance,
    /// Along a path of points.
    Perimeter,
    /// Inside a closed path of points.
    Area,
}

impl Kind {
    /// The annotation a measurement of this kind is kept as.
    pub fn subtype(self) -> Subtype {
        match self {
            Kind::Distance => Subtype::Line,
            Kind::Perimeter => Subtype::PolyLine,
            Kind::Area => Subtype::Polygon,
        }
    }

    pub fn intent(self) -> Intent {
        match self {
            Kind::Distance => Intent::LineDimension,
            Kind::Perimeter => Intent::PolyLineDimension,
            Kind::Area => Intent::PolygonDimension,
        }
    }

    /// The kind an intent says an annotation is measuring, if it is one.
    pub fn from_intent(intent: Intent) -> Option<Kind> {
        match intent {
            Intent::LineDimension => Some(Kind::Distance),
            Intent::PolyLineDimension => Some(Kind::Perimeter),
            Intent::PolygonDimension => Some(Kind::Area),
            Intent::FreeTextTypewriter | Intent::FreeTextCallout => None,
        }
    }

    /// The fewest points a measurement of this kind is made from.
    pub fn least_points(self) -> usize {
        match self {
            Kind::Distance | Kind::Perimeter => 2,
            Kind::Area => 3,
        }
    }
}

/// A measurement's scale and how it is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub kind: Kind,
    pub scale: Scale,
    /// Decimal places shown.
    pub precision: u8,
}

/// The size of a caption's lettering, in points.
pub const CAPTION_SIZE: f64 = 10.0;

/// The gap between a line and its caption, in points.
const CAPTION_GAP: f64 = 2.0;

impl Measure {
    pub fn new(kind: Kind, scale: Scale) -> Self {
        Measure {
            kind,
            scale,
            precision: 2,
        }
    }

    /// The measurement of `points`, in the scale's unit, or its square for an
    /// area. Distance is between the first two points.
    pub fn value(&self, points: &[(f64, f64)]) -> f64 {
        let per_point = self.scale.per_point();
        match self.kind {
            Kind::Distance => match points {
                [from, to, ..] => length(&[*from, *to]) * per_point,
                _ => 0.0,
            },
            Kind::Perimeter => length(points) * per_point,
            Kind::Area => area(points) * per_point * per_point,
        }
    }

    /// The unit a value of this kind is shown in: "ft", or "sq ft".
    pub fn unit_label(&self) -> String {
        let unit = self.scale.real_unit.as_str();
        match self.kind {
            Kind::Area => format!("sq {unit}"),
            Kind::Distance | Kind::Perimeter => unit.to_owned(),
        }
    }

    /// A value as shown: "12.50 ft".
    pub fn format(&self, value: f64) -> String {
        format!(
            "{value:.precision$} {}",
            self.unit_label(),
            precision = usize::from(self.precision)
        )
    }

    /// What `points` measures, as shown.
    pub fn label(&self, points: &[(f64, f64)]) -> String {
        self.format(self.value(points))
    }

    /// The annotation that keeps this measurement of `points`: the shape, its
    /// intent, the measurement in `/Contents`, and a `/Rect` that holds the
    /// caption as well as the shape. `None` for too few points.
    pub fn annotation(&self, points: &[(f64, f64)]) -> Option<Annotation> {
        if points.len() < self.kind.least_points() {
            return None;
        }
        let points = match self.kind {
            Kind::Distance => points[..2].to_vec(),
            Kind::Perimeter | Kind::Area => points.to_vec(),
        };
        let label = self.label(&points);
        let caption = caption(self.kind, &points, &label);
        let mut bounds = bounds(points.iter().chain(caption.corners.iter()))?;
        bounds = Rect::new(
            bounds.x0 - CAPTION_GAP,
            bounds.y0 - CAPTION_GAP,
            bounds.x1 + CAPTION_GAP,
            bounds.y1 + CAPTION_GAP,
        );
        let mut annotation = Annotation::new(self.kind.subtype(), bounds);
        match self.kind {
            Kind::Distance => annotation.line = Some((points[0], points[1])),
            Kind::Perimeter | Kind::Area => annotation.vertices = points,
        }
        annotation.intent = Some(self.kind.intent());
        annotation.contents = Some(label);
        annotation.measure = Some(*self);
        Some(annotation)
    }

    /// The `/Measure` dictionary: the scale in words and the three number
    /// formats Acrobat reads it back from.
    pub(crate) fn dictionary(&self) -> Dict {
        let mut dict = Dict::new();
        dict.set(Name::new("Type"), Object::name("Measure"));
        dict.set(Name::new("Subtype"), Object::name("RL"));
        dict.set(Name::new("R"), text_string(&self.scale.label()));
        let unit = self.scale.real_unit.as_str();
        let format = |unit: &str, factor: f64| {
            Object::Array(vec![Object::Dict(self.number_format(unit, factor))])
        };
        dict.set(Name::new("X"), format(unit, self.scale.per_point()));
        dict.set(Name::new("D"), format(unit, 1.0));
        dict.set(Name::new("A"), format(&format!("sq {unit}"), 1.0));
        dict
    }

    fn number_format(&self, unit: &str, factor: f64) -> Dict {
        let mut format = Dict::new();
        format.set(Name::new("Type"), Object::name("NumberFormat"));
        format.set(Name::new("U"), text_string(unit));
        format.set(Name::new("C"), Object::Real(factor));
        format.set(Name::new("F"), Object::name("D"));
        format.set(
            Name::new("D"),
            Object::Integer(10_i64.pow(u32::from(self.precision))),
        );
        format
    }

    /// A measurement read back from its `/Measure` and intent. `None` when
    /// either is missing or the scale's words are not ones this reads.
    pub(crate) fn from_dict(dict: &Dict, intent: Option<Intent>) -> Option<Measure> {
        let kind = Kind::from_intent(intent?)?;
        let measure = dict.get(b"Measure")?.as_dict()?;
        let words = match measure.get(b"R")? {
            Object::String(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => return None,
        };
        let scale = Scale::parse(&words)?;
        let precision = match measure.get(b"D") {
            Some(Object::Array(formats)) => formats
                .first()
                .and_then(Object::as_dict)
                .and_then(|format| format.get(b"D"))
                .and_then(|denominator| match denominator {
                    Object::Integer(value) if *value > 0 => Some(*value),
                    _ => None,
                })
                .map_or(2, |value| value.ilog10() as u8),
            _ => 2,
        };
        Some(Measure {
            kind,
            scale,
            precision,
        })
    }
}

/// The length of the path through `points`.
pub fn length(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|pair| (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1))
        .sum()
}

/// The area inside the closed path through `points`.
pub fn area(points: &[(f64, f64)]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let twice: f64 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
        .sum();
    (twice / 2.0).abs()
}

fn bounds<'a>(points: impl Iterator<Item = &'a (f64, f64)>) -> Option<Rect> {
    points.fold(None, |bounds: Option<Rect>, &(x, y)| {
        let point = Rect::new(x, y, x, y);
        Some(bounds.map_or(point, |bounds| bounds.union(point)))
    })
}

/// Where a caption is set: its baseline origin, its direction, and the four
/// corners of the box it fills, in page space.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Caption {
    pub(crate) origin: (f64, f64),
    /// The cosine and sine of the angle the text runs at.
    pub(crate) direction: (f64, f64),
    pub(crate) corners: [(f64, f64); 4],
}

/// Where a measurement's caption goes. A distance is labelled above the
/// middle of its line and along it, turned so it never reads upside down;
/// a perimeter above the middle of its last side, level; an area level at
/// the middle of its points.
pub(crate) fn caption(kind: Kind, points: &[(f64, f64)], text: &str) -> Caption {
    let width = caption_width(text);
    let level = (1.0, 0.0);
    let (anchor, direction) = match (kind, points) {
        (Kind::Distance, [from, to, ..]) => (midpoint(*from, *to), readable(*from, *to)),
        (Kind::Perimeter, [.., from, to]) => (midpoint(*from, *to), level),
        (Kind::Area, [_, ..]) => {
            let count = points.len() as f64;
            let (x, y) = points
                .iter()
                .fold((0.0, 0.0), |sum, point| (sum.0 + point.0, sum.1 + point.1));
            let centre = (x / count, y / count);
            // Centred on the middle rather than set above it.
            let drop = CAPTION_SIZE / 2.0 + CAPTION_GAP;
            ((centre.0, centre.1 - drop), level)
        }
        _ => ((0.0, 0.0), level),
    };
    let (cos, sin) = direction;
    // Up, square to the direction the text runs.
    let up = (-sin, cos);
    let along = |distance: f64, rise: f64| {
        (
            anchor.0 + cos * distance + up.0 * rise,
            anchor.1 + sin * distance + up.1 * rise,
        )
    };
    let base = CAPTION_GAP;
    let top = CAPTION_GAP + CAPTION_SIZE;
    Caption {
        origin: along(-width / 2.0, base),
        direction,
        corners: [
            along(-width / 2.0, base),
            along(width / 2.0, base),
            along(width / 2.0, top),
            along(-width / 2.0, top),
        ],
    }
}

/// How wide a caption sets in Helvetica at [`CAPTION_SIZE`].
pub(crate) fn caption_width(text: &str) -> f64 {
    let bytes = onionskin_content::encode_win_ansi(text);
    onionskin_content::standard_text_width("Helvetica", &bytes).unwrap_or(0.0) / 1000.0
        * CAPTION_SIZE
}

fn midpoint(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0)
}

/// The direction from `from` to `to`, turned half round when it points
/// left, so the text along it reads left to right.
fn readable(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    if length == 0.0 {
        return (1.0, 0.0);
    }
    let (cos, sin) = (dx / length, dy / length);
    if cos < 0.0 || (cos == 0.0 && sin < 0.0) {
        (-cos, -sin)
    } else {
        (cos, sin)
    }
}

/// The caption drawn into an appearance whose origin is `origin`, in
/// Helvetica, in `color`'s fill. Empty for a measurement with no text.
pub(crate) fn caption_stream(
    kind: Kind,
    points: &[(f64, f64)],
    text: &str,
    origin: (f64, f64),
    fill: (f64, f64, f64),
) -> String {
    if text.is_empty() {
        return String::new();
    }
    let caption = caption(kind, points, text);
    let (cos, sin) = caption.direction;
    let (x, y) = (caption.origin.0 - origin.0, caption.origin.1 - origin.1);
    let mut out = String::from("BT\n");
    let _ = writeln!(
        out,
        "/Helv {CAPTION_SIZE} Tf {} {} {} rg",
        fill.0, fill.1, fill.2
    );
    let _ = writeln!(out, "{cos} {sin} {} {cos} {x} {y} Tm", -sin);
    out.push('(');
    for byte in onionskin_content::encode_win_ansi(text) {
        match byte {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(char::from(byte));
            }
            0x20..=0x7e => out.push(char::from(byte)),
            _ => {
                let _ = write!(out, "\\{byte:03o}");
            }
        }
    }
    out.push_str(") Tj\nET\n");
    out
}

/// A number as a scale's words write it: no trailing zeros.
fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_is_its_length_in_points_and_its_abbreviation() {
        assert_eq!(Unit::Inch.points(), 72.0);
        assert!((Unit::Centimetre.points() - 28.346_456).abs() < 1e-5);
        assert_eq!(Unit::Foot.points(), 864.0);
        assert_eq!(Unit::Yard.points(), 2592.0);
        assert!((Unit::Kilometre.points() / Unit::Metre.points() - 1000.0).abs() < 1e-9);
        assert!((Unit::Mile.points() / Unit::Foot.points() - 5280.0).abs() < 1e-9);
        for unit in Unit::ALL {
            assert_eq!(Unit::from_abbreviation(unit.as_str()), Some(unit));
        }
        assert_eq!(Unit::from_abbreviation("furlong"), None);
    }

    #[test]
    fn a_scale_reads_back_from_its_words() {
        let scale = Scale::new(1.0, Unit::Inch, 10.0, Unit::Foot);
        assert_eq!(scale.label(), "1 in = 10 ft");
        assert_eq!(Scale::parse(&scale.label()), Some(scale));
        assert_eq!(
            Scale::parse(" 2.5 cm = 1 km "),
            Some(Scale::new(2.5, Unit::Centimetre, 1.0, Unit::Kilometre))
        );
        assert_eq!(
            Scale::new(0.125, Unit::Inch, 1.0, Unit::Foot).label(),
            "0.125 in = 1 ft"
        );
        for bad in [
            "1 in",
            "1 in = ft",
            "0 in = 1 ft",
            "1 in = -2 ft",
            "1 in = 1 ft x",
            "a in = 1 ft",
        ] {
            assert_eq!(Scale::parse(bad), None, "{bad}");
        }
        assert_eq!(Scale::default().label(), "1 in = 1 in");
    }

    #[test]
    fn lengths_and_areas_are_read_against_the_scale() {
        let scale = Scale::new(1.0, Unit::Inch, 10.0, Unit::Foot);
        let distance = Measure::new(Kind::Distance, scale);
        // Three inches across, four up: five inches, fifty feet.
        let points = [(0.0, 0.0), (216.0, 288.0), (0.0, 288.0)];
        assert!((distance.value(&points) - 50.0).abs() < 1e-9);
        assert_eq!(distance.label(&points), "50.00 ft");
        assert_eq!(distance.value(&points[..1]), 0.0);

        let perimeter = Measure::new(Kind::Perimeter, scale);
        assert!((perimeter.value(&points) - 80.0).abs() < 1e-9);

        let area = Measure::new(Kind::Area, scale);
        // A right triangle, 3 in by 4 in: 6 sq in, 600 sq ft.
        assert!((area.value(&points) - 600.0).abs() < 1e-9);
        assert_eq!(area.format(600.0), "600.00 sq ft");
        assert_eq!(super::area(&points[..2]), 0.0);

        let mut precise = Measure::new(Kind::Distance, Scale::default());
        precise.precision = 0;
        assert_eq!(precise.label(&[(0.0, 0.0), (144.0, 0.0)]), "2 in");
    }

    #[test]
    fn a_measurement_is_kept_as_the_shape_its_kind_names() {
        let scale = Scale::new(1.0, Unit::Inch, 1.0, Unit::Foot);
        let line = Measure::new(Kind::Distance, scale)
            .annotation(&[(72.0, 72.0), (144.0, 72.0), (1.0, 1.0)])
            .expect("a line");
        assert_eq!(line.subtype, Subtype::Line);
        assert_eq!(line.line, Some(((72.0, 72.0), (144.0, 72.0))));
        assert_eq!(line.intent, Some(Intent::LineDimension));
        assert_eq!(line.contents.as_deref(), Some("1.00 ft"));
        assert!(
            line.rect.y1 >= 72.0 + CAPTION_GAP + CAPTION_SIZE,
            "the caption is inside"
        );

        let polygon = Measure::new(Kind::Area, scale)
            .annotation(&[(0.0, 0.0), (72.0, 0.0), (72.0, 72.0)])
            .expect("a polygon");
        assert_eq!(polygon.subtype, Subtype::Polygon);
        assert_eq!(polygon.vertices.len(), 3);
        assert_eq!(polygon.intent, Some(Intent::PolygonDimension));

        let polyline = Measure::new(Kind::Perimeter, scale)
            .annotation(&[(0.0, 0.0), (72.0, 0.0)])
            .expect("a polyline");
        assert_eq!(polyline.intent, Some(Intent::PolyLineDimension));
        assert!(Measure::new(Kind::Area, scale)
            .annotation(&[(0.0, 0.0), (1.0, 1.0)])
            .is_none());
    }

    #[test]
    fn the_measure_dictionary_reads_back() {
        let mut measure = Measure::new(
            Kind::Area,
            Scale::new(1.0, Unit::Centimetre, 2.0, Unit::Metre),
        );
        measure.precision = 3;
        let mut annotation = Dict::new();
        annotation.set(Name::new("Measure"), Object::Dict(measure.dictionary()));
        assert_eq!(
            Measure::from_dict(&annotation, Some(Intent::PolygonDimension)),
            Some(measure)
        );
        assert_eq!(
            Measure::from_dict(&annotation, Some(Intent::FreeTextCallout)),
            None
        );
        assert_eq!(Measure::from_dict(&annotation, None), None);
        let dict = measure.dictionary();
        let x = dict.get(b"X").and_then(|x| match x {
            Object::Array(formats) => formats[0].as_dict().cloned(),
            _ => None,
        });
        let factor = x.and_then(|x| x.get(b"C").cloned());
        let Some(Object::Real(factor)) = factor else {
            panic!("a factor: {factor:?}");
        };
        assert!((factor - 2.0 / Unit::Centimetre.points()).abs() < 1e-12);
    }

    #[test]
    fn a_caption_reads_left_to_right_whichever_way_its_line_was_drawn() {
        let rightward = caption(Kind::Distance, &[(0.0, 0.0), (100.0, 0.0)], "1 in");
        let leftward = caption(Kind::Distance, &[(100.0, 0.0), (0.0, 0.0)], "1 in");
        assert_eq!(rightward, leftward);
        assert_eq!(rightward.direction, (1.0, 0.0));
        assert!(rightward.origin.1 > 0.0, "above the line");
        let down = caption(Kind::Distance, &[(0.0, 100.0), (0.0, 0.0)], "x");
        assert_eq!(down.direction, (0.0, 1.0), "a vertical line reads upward");
        let still = caption(Kind::Distance, &[(5.0, 5.0), (5.0, 5.0)], "x");
        assert_eq!(still.direction, (1.0, 0.0));
        let area = caption(
            Kind::Area,
            &[(0.0, 0.0), (60.0, 0.0), (60.0, 60.0), (0.0, 60.0)],
            "x",
        );
        assert!(
            (area.corners[0].1 + area.corners[3].1) / 2.0 - 30.0 < 1e-9,
            "centred"
        );
        assert_eq!(caption(Kind::Area, &[], "x").direction, (1.0, 0.0));
        assert!(caption_width("12.00 ft") > 30.0);
    }

    #[test]
    fn a_caption_is_drawn_in_helvetica_along_its_line() {
        let stream = caption_stream(
            Kind::Distance,
            &[(0.0, 0.0), (0.0, 100.0)],
            "(3) é",
            (-20.0, 0.0),
            (1.0, 0.0, 0.0),
        );
        assert!(stream.contains("/Helv 10 Tf 1 0 0 rg"), "{stream}");
        assert!(stream.contains("0 1 -1 0 "), "turned a quarter: {stream}");
        assert!(stream.contains("(\\(3\\) \\351) Tj"), "{stream}");
        assert!(caption_stream(Kind::Distance, &[], "", (0.0, 0.0), (0.0, 0.0, 0.0)).is_empty());
    }
}
