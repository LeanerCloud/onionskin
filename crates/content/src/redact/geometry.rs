//! Where a redaction reaches: convex four-sided areas in page space, and how
//! much of a glyph, a path or an image falls inside them.

use crate::PageQuad;

pub(crate) type Point = (f64, f64);

/// How much of a glyph's box has to be covered before the glyph goes. Text
/// marked for redaction covers its glyphs whole; a neighbour kerned into the
/// mark overlaps it by a few percent and stays.
const GLYPH_COVERED: f64 = 0.2;

/// A convex quadrilateral on the page, in default user space: a rectangle
/// drawn over a region, or one glyph's quad of marked text.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    /// The corners in order round the edge.
    polygon: [Point; 4],
}

impl Area {
    pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Area {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        Area {
            polygon: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
        }
    }

    /// A quad in `/QuadPoints` order (upper-left, upper-right, lower-left,
    /// lower-right), turned into an edge walk.
    pub fn quad(quad: &PageQuad) -> Area {
        let [upper_left, upper_right, lower_left, lower_right] = quad.corners;
        Area {
            polygon: [upper_left, upper_right, lower_right, lower_left],
        }
    }

    pub fn corners(&self) -> [Point; 4] {
        self.polygon
    }

    /// The smallest upright rectangle holding the area.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        bounds(&self.polygon)
    }

    /// Whether `point` is inside or on the edge.
    pub(crate) fn contains(&self, point: Point) -> bool {
        let sign = orientation(&self.polygon);
        edges(&self.polygon).all(|(a, b)| cross(a, b, point) * sign >= -1e-9)
    }

    /// The area of `polygon` that falls inside this one.
    pub(crate) fn overlap(&self, polygon: &[Point]) -> f64 {
        polygon_area(&clip(polygon.to_vec(), &self.polygon))
    }
}

/// Whether a glyph with this quad is redacted: enough of its box is covered,
/// or, for a glyph with no width, its origin is.
pub(crate) fn covers_glyph(areas: &[Area], quad: &PageQuad) -> bool {
    let polygon = Area::quad(quad).polygon;
    let whole = polygon_area(&polygon);
    if whole < 1e-9 {
        let centre = centre(&polygon);
        return areas.iter().any(|area| area.contains(centre));
    }
    let covered: f64 = areas.iter().map(|area| area.overlap(&polygon)).sum();
    covered / whole >= GLYPH_COVERED
}

/// Whether anything of `polygon` is inside an area: an image or a form
/// touched at all has to be looked into.
pub(crate) fn touches(areas: &[Area], polygon: &[Point]) -> bool {
    let (x0, y0, x1, y1) = bounds(polygon);
    areas.iter().any(|area| {
        let (ax0, ay0, ax1, ay1) = area.bounds();
        let apart = ax1 < x0 || x1 < ax0 || ay1 < y0 || y1 < ay0;
        !apart && (area.overlap(polygon) > 1e-9 || polygon.iter().any(|p| area.contains(*p)))
    })
}

/// Whether the upright box `(x0, y0, x1, y1)` lies wholly inside one area:
/// a path that does is removed.
pub(crate) fn encloses(areas: &[Area], (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    areas
        .iter()
        .any(|area| corners.iter().all(|corner| area.contains(*corner)))
}

pub(crate) fn bounds(points: &[Point]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(x0, y0, x1, y1), (x, y)| (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)),
    )
}

fn centre(points: &[Point]) -> Point {
    let n = points.len() as f64;
    let (x, y) = points
        .iter()
        .fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
    (x / n, y / n)
}

fn edges(polygon: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    (0..polygon.len()).map(move |i| (polygon[i], polygon[(i + 1) % polygon.len()]))
}

/// Which side of the line from `a` to `b` the point `p` is on.
fn cross(a: Point, b: Point, p: Point) -> f64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// `1.0` for a counter-clockwise walk, `-1.0` for a clockwise one.
fn orientation(polygon: &[Point]) -> f64 {
    let signed: f64 = edges(polygon).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum();
    if signed < 0.0 {
        -1.0
    } else {
        1.0
    }
}

pub(crate) fn polygon_area(polygon: &[Point]) -> f64 {
    if polygon.len() < 3 {
        return 0.0;
    }
    (edges(polygon)
        .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
        .sum::<f64>()
        / 2.0)
        .abs()
}

/// Sutherland-Hodgman: `subject` cut down to the part inside the convex
/// `window`.
fn clip(mut subject: Vec<Point>, window: &[Point; 4]) -> Vec<Point> {
    let sign = orientation(window);
    for (a, b) in edges(window) {
        if subject.is_empty() {
            break;
        }
        let inside = |p: Point| cross(a, b, p) * sign >= 0.0;
        let input = std::mem::take(&mut subject);
        for (i, current) in input.iter().enumerate() {
            let previous = input[(i + input.len() - 1) % input.len()];
            match (inside(previous), inside(*current)) {
                (true, true) => subject.push(*current),
                (true, false) => subject.push(crossing(previous, *current, a, b)),
                (false, true) => {
                    subject.push(crossing(previous, *current, a, b));
                    subject.push(*current);
                }
                (false, false) => {}
            }
        }
    }
    subject
}

/// Where the segment from `p` to `q` crosses the line through `a` and `b`.
fn crossing(p: Point, q: Point, a: Point, b: Point) -> Point {
    let (dp, dq) = (cross(a, b, p), cross(a, b, q));
    let t = if (dp - dq).abs() < 1e-12 {
        0.0
    } else {
        dp / (dp - dq)
    };
    (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(x0: f64, y0: f64, x1: f64, y1: f64) -> PageQuad {
        PageQuad {
            page: 0,
            corners: [(x0, y1), (x1, y1), (x0, y0), (x1, y0)],
        }
    }

    #[test]
    fn a_rectangle_contains_its_inside_and_edge_in_either_direction() {
        let area = Area::rect(10.0, 10.0, 0.0, 0.0);
        assert!(area.contains((5.0, 5.0)));
        assert!(area.contains((10.0, 0.0)));
        assert!(!area.contains((10.5, 5.0)));
        let turned = Area::quad(&glyph(0.0, 0.0, 10.0, 10.0));
        assert!(turned.contains((5.0, 5.0)));
        assert_eq!(turned.bounds(), (0.0, 0.0, 10.0, 10.0));
        assert_eq!(turned.corners().len(), 4);
    }

    #[test]
    fn overlap_is_the_shared_area() {
        let area = Area::rect(0.0, 0.0, 10.0, 10.0);
        let half = [(5.0, 0.0), (15.0, 0.0), (15.0, 10.0), (5.0, 10.0)];
        assert!((area.overlap(&half) - 50.0).abs() < 1e-9);
        let apart = [(20.0, 0.0), (30.0, 0.0), (30.0, 10.0), (20.0, 10.0)];
        assert_eq!(area.overlap(&apart), 0.0);
        let diamond = [(5.0, -5.0), (15.0, 5.0), (5.0, 15.0), (-5.0, 5.0)];
        assert!((area.overlap(&diamond) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_glyph_goes_when_a_fifth_of_it_is_covered() {
        let areas = [Area::rect(0.0, 0.0, 10.0, 10.0)];
        assert!(covers_glyph(&areas, &glyph(2.0, 2.0, 8.0, 8.0)));
        assert!(covers_glyph(&areas, &glyph(8.0, 0.0, 18.0, 10.0)));
        assert!(!covers_glyph(&areas, &glyph(9.5, 0.0, 19.5, 10.0)));
        assert!(
            covers_glyph(&areas, &glyph(5.0, 5.0, 5.0, 5.0)),
            "no width, origin inside"
        );
        assert!(!covers_glyph(&areas, &glyph(50.0, 5.0, 50.0, 5.0)));
    }

    #[test]
    fn touching_and_enclosing() {
        let areas = [Area::rect(0.0, 0.0, 10.0, 10.0)];
        let big = [(-5.0, -5.0), (50.0, -5.0), (50.0, 50.0), (-5.0, 50.0)];
        assert!(touches(&areas, &big));
        let far = [(20.0, 20.0), (30.0, 20.0), (30.0, 30.0), (20.0, 30.0)];
        assert!(!touches(&areas, &far));
        let line = [(1.0, 5.0), (9.0, 5.0), (9.0, 5.0), (1.0, 5.0)];
        assert!(touches(&areas, &line), "a flat shape inside still touches");
        assert!(encloses(&areas, (1.0, 1.0, 9.0, 9.0)));
        assert!(!encloses(&areas, (1.0, 1.0, 11.0, 9.0)));
        assert_eq!(polygon_area(&[(0.0, 0.0), (1.0, 1.0)]), 0.0);
    }
}
