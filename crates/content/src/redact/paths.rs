//! Paths: held while they are built, and removed when the operator that
//! paints one finds it wholly inside a redaction area.
//!
//! A path only partly inside is kept whole. Line art crossing a redaction
//! is a rule or a box, not something that spells; a vector glyph is small
//! and falls inside.

use super::geometry::{bounds, encloses, Area, Point};
use super::output::Emit;
use crate::matrix::Matrix;
use crate::tokenizer::Operation;

/// The operators that build a path.
pub(crate) fn is_construction(operator: &[u8]) -> bool {
    matches!(operator, b"m" | b"l" | b"c" | b"v" | b"y" | b"h" | b"re")
}

/// The operators that end one, painting it or not.
pub(crate) fn is_painting(operator: &[u8]) -> bool {
    matches!(
        operator,
        b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n"
    )
}

pub(crate) fn is_clip(operator: &[u8]) -> bool {
    matches!(operator, b"W" | b"W*")
}

#[derive(Default)]
pub(crate) struct PathBuffer {
    bytes: Vec<u8>,
    points: Vec<Point>,
    clip: Option<Vec<u8>>,
}

impl PathBuffer {
    /// A building operator, with its points put on the page by `ctm`.
    pub(crate) fn construct(&mut self, op: &Operation, original: &[u8], ctm: &Matrix) {
        self.hold(original);
        let corners =
            |x: f64, y: f64, w: f64, h: f64| [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        let points: Vec<Point> = match op.operator.as_bytes() {
            b"m" | b"l" => op.numbers::<2>().map(|[x, y]| vec![(x, y)]),
            b"c" => op
                .numbers::<6>()
                .map(|[a, b, c, d, e, f]| vec![(a, b), (c, d), (e, f)]),
            b"v" | b"y" => op.numbers::<4>().map(|[a, b, c, d]| vec![(a, b), (c, d)]),
            b"re" => op
                .numbers::<4>()
                .map(|[x, y, w, h]| corners(x, y, w, h).to_vec()),
            _ => None,
        }
        .unwrap_or_default();
        self.points
            .extend(points.into_iter().map(|(x, y)| ctm.apply(x, y)));
    }

    pub(crate) fn clip(&mut self, original: &[u8]) {
        self.clip = Some(original.to_vec());
    }

    /// The painting operator: the path as written, or nothing when it is
    /// wholly redacted. A clip it set is kept, drawn with `n`, so what the
    /// page draws next is clipped as it was. `true` when it was removed.
    pub(crate) fn paint(&mut self, original: &[u8], areas: &[Area]) -> (Emit, bool) {
        let mut bytes = std::mem::take(&mut self.bytes);
        let points = std::mem::take(&mut self.points);
        let clip = self.clip.take();
        let removable = original != b"n" && !points.is_empty();
        if removable && encloses(areas, bounds(&points)) {
            return match clip {
                Some(clip) => {
                    bytes.push(b'\n');
                    bytes.extend_from_slice(&clip);
                    bytes.extend_from_slice(b"\nn");
                    (Emit::Replace(bytes), true)
                }
                None => (Emit::Drop, true),
            };
        }
        if let Some(clip) = clip {
            bytes.push(b'\n');
            bytes.extend_from_slice(&clip);
        }
        if !bytes.is_empty() {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(original);
        (Emit::Same(bytes), false)
    }

    /// A path left unpainted when something else came along, written back
    /// as it was.
    pub(crate) fn take(&mut self) -> Option<Vec<u8>> {
        let mut bytes = std::mem::take(&mut self.bytes);
        self.points.clear();
        if let Some(clip) = self.clip.take() {
            if !bytes.is_empty() {
                bytes.push(b'\n');
            }
            bytes.extend_from_slice(&clip);
        }
        (!bytes.is_empty()).then_some(bytes)
    }

    fn hold(&mut self, original: &[u8]) {
        if !self.bytes.is_empty() {
            self.bytes.push(b'\n');
        }
        self.bytes.extend_from_slice(original);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::Tokenizer;

    fn ops(stream: &[u8]) -> Vec<Operation> {
        let mut lexer = Tokenizer::new(stream);
        std::iter::from_fn(|| lexer.next_operation()).collect()
    }

    fn build(buffer: &mut PathBuffer, stream: &[u8], ctm: &Matrix) {
        for op in ops(stream) {
            let original = &stream[op.span.start as usize..op.span.end as usize];
            if is_clip(op.operator.as_bytes()) {
                buffer.clip(original);
            } else {
                buffer.construct(&op, original, ctm);
            }
        }
    }

    const AREA: [Area; 0] = [];

    fn areas() -> Vec<Area> {
        vec![Area::rect(0.0, 0.0, 100.0, 100.0)]
    }

    #[test]
    fn a_path_inside_an_area_is_removed_and_its_clip_kept() {
        let mut buffer = PathBuffer::default();
        build(
            &mut buffer,
            b"10 10 m 20 20 l 30 10 20 30 40 40 c",
            &Matrix::IDENTITY,
        );
        assert_eq!(buffer.paint(b"S", &areas()), (Emit::Drop, true));

        build(&mut buffer, b"10 10 20 20 re W", &Matrix::IDENTITY);
        assert_eq!(
            buffer.paint(b"f", &areas()),
            (Emit::Replace(b"10 10 20 20 re\nW\nn".to_vec()), true)
        );
    }

    #[test]
    fn a_path_reaching_outside_is_kept_as_written() {
        let mut buffer = PathBuffer::default();
        build(&mut buffer, b"10 10 m 150 10 l h", &Matrix::IDENTITY);
        assert_eq!(
            buffer.paint(b"S", &areas()),
            (Emit::Same(b"10 10 m\n150 10 l\nh\nS".to_vec()), false)
        );
        // The CTM carries a small path out of the area.
        build(
            &mut buffer,
            b"10 10 m 20 20 v 30 30 y",
            &Matrix::translate(95.0, 0.0),
        );
        assert!(!buffer.paint(b"B", &areas()).1);
        build(&mut buffer, b"10 10 m 20 20 l", &Matrix::IDENTITY);
        assert!(
            !buffer.paint(b"n", &areas()).1,
            "n paints nothing to remove"
        );
        assert!(!buffer.paint(b"f", &AREA).1);
    }

    #[test]
    fn an_unpainted_path_is_given_back() {
        let mut buffer = PathBuffer::default();
        assert_eq!(buffer.take(), None);
        build(&mut buffer, b"1 2 m W", &Matrix::IDENTITY);
        assert_eq!(buffer.take(), Some(b"1 2 m\nW".to_vec()));
        assert!(is_construction(b"re") && is_painting(b"b*") && !is_painting(b"Tj"));
    }
}
