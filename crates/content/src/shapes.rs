//! Line art, as a page paints it: each painted path's points and the
//! rectangles it is built from, in the page's default user space. What form
//! field detection looks for: the rules a form is filled in on, and the
//! small boxes that are check boxes.

use crate::matrix::Matrix;
use crate::tokenizer::Operation;

/// One painted path.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Every point the path was built through, curves' control points
    /// included, in page space.
    pub points: Vec<(f64, f64)>,
    /// The bounds of each `re` it was built from, in page space.
    pub rects: Vec<[f64; 4]>,
    pub stroked: bool,
    pub filled: bool,
}

impl Shape {
    /// The smallest box holding every point, as `[x0, y0, x1, y1]`.
    pub fn bounds(&self) -> [f64; 4] {
        bounds(&self.points)
    }
}

fn bounds(points: &[(f64, f64)]) -> [f64; 4] {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for &(x, y) in points {
        out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
    }
    out
}

/// The path being built, and the shapes painted so far.
#[derive(Debug, Default)]
pub(crate) struct Shapes {
    points: Vec<(f64, f64)>,
    rects: Vec<[f64; 4]>,
    pub(crate) found: Vec<Shape>,
}

/// More than this many shapes on a page and the rest are not kept: a map
/// or a chart has thousands, and none of them is a form's.
const MAX_SHAPES: usize = 20_000;

impl Shapes {
    /// Follow operator `op`, with the current transformation `ctm`: a
    /// building operator grows the path, a painting one keeps it.
    pub(crate) fn follow(&mut self, op: &Operation, ctm: &Matrix) {
        let operator = op.operator.as_bytes();
        match operator {
            b"m" | b"l" => self.add(op.numbers::<2>().map(|[x, y]| vec![(x, y)]), ctm),
            b"c" => self.add(
                op.numbers::<6>()
                    .map(|[a, b, c, d, e, f]| vec![(a, b), (c, d), (e, f)]),
                ctm,
            ),
            b"v" | b"y" => self.add(
                op.numbers::<4>().map(|[a, b, c, d]| vec![(a, b), (c, d)]),
                ctm,
            ),
            b"re" => {
                if let Some([x, y, w, h]) = op.numbers::<4>() {
                    let corners: Vec<(f64, f64)> = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                        .into_iter()
                        .map(|(x, y)| ctm.apply(x, y))
                        .collect();
                    self.rects.push(bounds(&corners));
                    self.points.extend(corners);
                }
            }
            b"S" | b"s" => self.paint(true, false),
            b"f" | b"F" | b"f*" => self.paint(false, true),
            b"B" | b"B*" | b"b" | b"b*" => self.paint(true, true),
            b"n" => self.clear(),
            _ => {}
        }
    }

    fn add(&mut self, points: Option<Vec<(f64, f64)>>, ctm: &Matrix) {
        self.points.extend(
            points
                .unwrap_or_default()
                .into_iter()
                .map(|(x, y)| ctm.apply(x, y)),
        );
    }

    fn paint(&mut self, stroked: bool, filled: bool) {
        let points = std::mem::take(&mut self.points);
        let rects = std::mem::take(&mut self.rects);
        if !points.is_empty() && self.found.len() < MAX_SHAPES {
            self.found.push(Shape {
                points,
                rects,
                stroked,
                filled,
            });
        }
    }

    fn clear(&mut self) {
        self.points.clear();
        self.rects.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::Tokenizer;

    fn follow(stream: &[u8], ctm: Matrix) -> Vec<Shape> {
        let mut shapes = Shapes::default();
        let mut lexer = Tokenizer::new(stream);
        while let Some(op) = lexer.next_operation() {
            shapes.follow(&op, &ctm);
        }
        shapes.found
    }

    #[test]
    fn painted_paths_are_kept_and_unpainted_ones_dropped() {
        let found = follow(
            b"10 20 m 110 20 l S 0 0 5 5 re f 1 1 m 2 2 l n 0 0 m 1 1 2 2 3 3 c 4 4 5 5 v B q",
            Matrix::IDENTITY,
        );
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].points, [(10.0, 20.0), (110.0, 20.0)]);
        assert!(found[0].stroked && !found[0].filled);
        assert_eq!(found[0].bounds(), [10.0, 20.0, 110.0, 20.0]);
        assert_eq!(found[1].rects, [[0.0, 0.0, 5.0, 5.0]]);
        assert!(found[1].filled && !found[1].stroked);
        assert!(found[2].stroked && found[2].filled);
        assert_eq!(found[2].points.len(), 6);
    }

    #[test]
    fn points_are_put_on_the_page_by_the_matrix() {
        let found = follow(
            b"0 0 10 10 re s 0 0 m 0 5 l 5 5 5 5 y f*",
            Matrix::new(2.0, 0.0, 0.0, 2.0, 100.0, 50.0),
        );
        assert_eq!(found[0].rects, [[100.0, 50.0, 120.0, 70.0]]);
        assert_eq!(found[1].bounds(), [100.0, 50.0, 110.0, 60.0]);
        assert!(follow(b"S f", Matrix::IDENTITY).is_empty(), "nothing built");
    }
}
