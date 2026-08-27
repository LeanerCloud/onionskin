//! The 3x2 affine transform PDF writes as `[a b c d e f]` (ISO 32000-2 8.3.3).
//!
//! Points are row vectors: `(x y 1) * M`. `cm` premultiplies, so the operand
//! matrix is applied before whatever the CTM already held.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Matrix {
        Matrix { a, b, c, d, e, f }
    }

    pub fn translate(tx: f64, ty: f64) -> Matrix {
        Matrix::new(1.0, 0.0, 0.0, 1.0, tx, ty)
    }

    pub fn scale(sx: f64, sy: f64) -> Matrix {
        Matrix::new(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    /// `self` applied first, then `other`.
    pub fn then(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Length of the transformed unit x vector: how much one text-space unit
    /// spans on the page. Used to compare glyph gaps against the font size
    /// without caring how the text got rotated.
    pub fn x_scale(&self) -> f64 {
        (self.a * self.a + self.b * self.b).sqrt()
    }

    /// True when every component is finite. A content stream can set the CTM to
    /// NaN through a degenerate `cm`, and every quad downstream of that is
    /// meaningless rather than merely wrong.
    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }

    /// Reads six numbers out of an operator's operands. `None` unless all six
    /// are present and numeric, so a truncated `cm` leaves the CTM alone.
    pub fn from_operands(operands: &[onionskin_cos::Object]) -> Option<Matrix> {
        if operands.len() < 6 {
            return None;
        }
        let at = |i: usize| crate::tokenizer::number(&operands[operands.len() - 6 + i]);
        Some(Matrix::new(at(0)?, at(1)?, at(2)?, at(3)?, at(4)?, at(5)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn then_matches_the_pdf_composition_order() {
        // Scale by two, then move right by ten: the point lands at 2x + 10.
        let m = Matrix::scale(2.0, 2.0).then(&Matrix::translate(10.0, 0.0));
        assert_eq!(m.apply(3.0, 1.0), (16.0, 2.0));
        // The other order moves first, so the translation is scaled too.
        let m = Matrix::translate(10.0, 0.0).then(&Matrix::scale(2.0, 2.0));
        assert_eq!(m.apply(3.0, 1.0), (26.0, 2.0));
    }

    #[test]
    fn quarter_turn_rotates_the_axes() {
        let rot = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let (x, y) = rot.apply(1.0, 0.0);
        assert!((x - 0.0).abs() < 1e-12 && (y - 1.0).abs() < 1e-12);
        assert!((rot.x_scale() - 1.0).abs() < 1e-12);
    }
}
