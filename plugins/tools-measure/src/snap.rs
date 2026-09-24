//! Snapping a point to the page's line art: the ends of its straight edges,
//! their middles, where two cross, and the nearest point along one.
//!
//! What Acrobat's 2D measuring snaps to. A point is snapped only to what is
//! within the sensitivity of it, and the kinds are tried in the order that
//! means most: an end or a crossing is a place someone measures from on
//! purpose, a point somewhere along an edge is only better than nothing.

use onionskin_content::shapes::Segment;
use onionskin_core::{Document, PageIndex};

/// What a point was snapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Endpoint,
    Intersection,
    Midpoint,
    Path,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Endpoint => "Endpoint",
            SnapKind::Intersection => "Intersection",
            SnapKind::Midpoint => "Midpoint",
            SnapKind::Path => "Path",
        }
    }
}

/// Which kinds of snapping are on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapOptions {
    pub endpoints: bool,
    pub midpoints: bool,
    pub intersections: bool,
    pub paths: bool,
}

impl Default for SnapOptions {
    fn default() -> Self {
        SnapOptions {
            endpoints: true,
            midpoints: true,
            intersections: true,
            paths: true,
        }
    }
}

impl SnapOptions {
    /// Each setting's id and what it is called.
    pub const NAMES: [(&'static str, &'static str); 4] = [
        ("endpoints", "Endpoints"),
        ("midpoints", "Midpoints"),
        ("intersections", "Intersections"),
        ("paths", "Paths"),
    ];

    fn setting(&mut self, name: &str) -> Option<&mut bool> {
        match name {
            "endpoints" => Some(&mut self.endpoints),
            "midpoints" => Some(&mut self.midpoints),
            "intersections" => Some(&mut self.intersections),
            "paths" => Some(&mut self.paths),
            _ => None,
        }
    }

    /// Turn setting `name` the other way. `false` when there is none.
    pub fn toggle(&mut self, name: &str) -> bool {
        match self.setting(name) {
            Some(on) => {
                *on = !*on;
                true
            }
            None => false,
        }
    }

    pub fn is_on(&self, name: &str) -> bool {
        let mut options = *self;
        options.setting(name).is_some_and(|on| *on)
    }
}

/// More edges than this near the pointer and crossings are not looked for:
/// the pairs grow as the square.
const MAX_NEAR: usize = 64;

/// A page's straight edges, read once and kept while the tool stays on it.
#[derive(Debug, Default)]
pub struct Snapper {
    page: Option<PageIndex>,
    segments: Vec<Segment>,
}

type Point = (f64, f64);

impl Snapper {
    /// Read `page`'s edges, unless they are the ones held. A page that will
    /// not read has none.
    pub fn load(&mut self, doc: &mut Document, page: PageIndex) {
        if self.page == Some(page) {
            return;
        }
        self.page = Some(page);
        self.segments = doc
            .structure()
            .ok()
            .and_then(|structure| onionskin_content::page_shapes(structure, page).ok())
            .map(|shapes| {
                shapes
                    .into_iter()
                    .flat_map(|shape| shape.segments)
                    .collect()
            })
            .unwrap_or_default();
    }

    /// Forget the page, so it is read again: the page may have changed.
    pub fn clear(&mut self) {
        self.page = None;
        self.segments.clear();
    }

    /// `at`, snapped to what is within `radius` of it, and what it snapped
    /// to. `None` when nothing is near enough.
    pub fn snap(&self, at: Point, radius: f64, options: SnapOptions) -> Option<(Point, SnapKind)> {
        let near: Vec<&Segment> = self
            .segments
            .iter()
            .filter(|segment| distance(at, closest(at, segment)) <= radius)
            .collect();
        let within = |point: &Point| distance(at, *point) <= radius;
        [
            (options.endpoints, SnapKind::Endpoint),
            (options.intersections, SnapKind::Intersection),
            (options.midpoints, SnapKind::Midpoint),
            (options.paths, SnapKind::Path),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .find_map(|(_, kind)| {
            candidates(kind, at, &near)
                .into_iter()
                .filter(within)
                .min_by(|a, b| distance(at, *a).total_cmp(&distance(at, *b)))
                .map(|point| (point, kind))
        })
    }
}

/// The points of `kind` the edges near `at` offer.
fn candidates(kind: SnapKind, at: Point, near: &[&Segment]) -> Vec<Point> {
    match kind {
        SnapKind::Endpoint => near
            .iter()
            .flat_map(|segment| segment.iter().copied())
            .collect(),
        SnapKind::Intersection => crossings(near),
        SnapKind::Midpoint => near.iter().map(|segment| middle(segment)).collect(),
        SnapKind::Path => near.iter().map(|segment| closest(at, segment)).collect(),
    }
}

fn distance(a: Point, b: Point) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn middle(segment: &Segment) -> Point {
    let [a, b] = segment;
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

/// The point of `segment` nearest `at`.
fn closest(at: Point, segment: &Segment) -> Point {
    let [a, b] = *segment;
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    if length == 0.0 {
        return a;
    }
    let t = (((at.0 - a.0) * dx + (at.1 - a.1) * dy) / length).clamp(0.0, 1.0);
    (a.0 + t * dx, a.1 + t * dy)
}

/// Where each pair of `segments` crosses, when it does.
fn crossings(segments: &[&Segment]) -> Vec<Point> {
    if segments.len() > MAX_NEAR {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (index, first) in segments.iter().enumerate() {
        for second in &segments[index + 1..] {
            out.extend(crossing(first, second));
        }
    }
    out
}

fn crossing(first: &Segment, second: &Segment) -> Option<Point> {
    let [a, b] = *first;
    let [c, d] = *second;
    let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    let across = r.0 * s.1 - r.1 * s.0;
    if across.abs() < f64::EPSILON {
        return None;
    }
    let (qx, qy) = (c.0 - a.0, c.1 - a.1);
    let t = (qx * s.1 - qy * s.0) / across;
    let u = (qx * r.1 - qy * r.0) / across;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((a.0 + t * r.0, a.1 + t * r.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapper(segments: Vec<Segment>) -> Snapper {
        Snapper {
            page: Some(0),
            segments,
        }
    }

    #[test]
    fn an_end_wins_over_a_crossing_a_middle_and_the_path() {
        // A cross, its arms 100 long, and a lone edge further off.
        let snapper = snapper(vec![
            [(0.0, 50.0), (100.0, 50.0)],
            [(50.0, 0.0), (50.0, 100.0)],
            [(200.0, 0.0), (300.0, 0.0)],
        ]);
        let all = SnapOptions::default();
        assert_eq!(
            snapper.snap((2.0, 49.0), 5.0, all),
            Some(((0.0, 50.0), SnapKind::Endpoint))
        );
        assert_eq!(
            snapper.snap((52.0, 51.0), 5.0, all),
            Some(((50.0, 50.0), SnapKind::Intersection))
        );
        assert_eq!(
            snapper.snap((251.0, 3.0), 5.0, all),
            Some(((250.0, 0.0), SnapKind::Midpoint))
        );
        assert_eq!(
            snapper.snap((220.0, 3.0), 5.0, all),
            Some(((220.0, 0.0), SnapKind::Path))
        );
        assert_eq!(snapper.snap((150.0, 150.0), 5.0, all), None, "nothing near");
    }

    #[test]
    fn a_kind_turned_off_is_passed_over() {
        let snapper = snapper(vec![
            [(0.0, 50.0), (100.0, 50.0)],
            [(50.0, 0.0), (50.0, 100.0)],
        ]);
        let mut options = SnapOptions::default();
        assert!(options.toggle("intersections"));
        assert!(!options.is_on("intersections") && options.is_on("paths"));
        assert_eq!(
            snapper.snap((52.0, 51.0), 5.0, options),
            Some(((50.0, 50.0), SnapKind::Midpoint)),
            "the middle of both arms is where they cross"
        );
        let none = SnapOptions {
            endpoints: false,
            midpoints: false,
            intersections: false,
            paths: false,
        };
        assert_eq!(snapper.snap((52.0, 51.0), 5.0, none), None);
        assert!(!options.toggle("corners") && !options.is_on("corners"));
        let labels: Vec<_> = [
            SnapKind::Endpoint,
            SnapKind::Intersection,
            SnapKind::Midpoint,
            SnapKind::Path,
        ]
        .map(SnapKind::label)
        .to_vec();
        assert_eq!(labels, ["Endpoint", "Intersection", "Midpoint", "Path"]);
    }

    #[test]
    fn parallel_and_apart_edges_do_not_cross_and_a_dot_is_its_own_nearest() {
        let parallel = [(0.0, 0.0), (10.0, 0.0)];
        let above = [(0.0, 1.0), (10.0, 1.0)];
        let apart = [(20.0, -5.0), (20.0, 5.0)];
        assert_eq!(crossing(&parallel, &above), None);
        assert_eq!(crossing(&parallel, &apart), None);
        let dot = [(3.0, 3.0), (3.0, 3.0)];
        assert_eq!(closest((9.0, 9.0), &dot), (3.0, 3.0));
        let many: Vec<Segment> = (0..=MAX_NEAR).map(|_| parallel).collect();
        let near: Vec<&Segment> = many.iter().collect();
        assert!(crossings(&near).is_empty(), "too many to pair");
    }

    #[test]
    fn a_page_is_read_once_and_again_after_clearing() {
        let mut snapper = Snapper::default();
        let mut doc = Document::open_bytes(page_with_a_rule()).expect("opens");
        snapper.load(&mut doc, 0);
        assert_eq!(snapper.segments, [[(72.0, 100.0), (300.0, 100.0)]]);
        snapper.segments.clear();
        snapper.load(&mut doc, 0);
        assert!(snapper.segments.is_empty(), "held, not read again");
        snapper.clear();
        snapper.load(&mut doc, 0);
        assert_eq!(snapper.segments.len(), 1);
        snapper.load(&mut doc, 7);
        assert!(
            snapper.segments.is_empty(),
            "a page that is not there has none"
        );
    }

    fn page_with_a_rule() -> Vec<u8> {
        let content = "72 100 m 300 100 l S";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R >>".to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ];
        crate::testing::pdf(&objects)
    }
}
