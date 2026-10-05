//! Where a page draws its images: each image XObject's placement, and the
//! `Do` operator that placed it, so an image can be found under a point,
//! saved, replaced, moved or taken away.

use onionskin_cos::ObjRef;

use crate::marked::MarkedRef;
use crate::matrix::Matrix;
use crate::run::ByteProvenance;

/// One image drawn on a page. An image drawn twice is two placements of
/// the same XObject.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlacement {
    /// The image XObject.
    pub image: ObjRef,
    /// The resource name it was drawn by.
    pub name: String,
    /// What maps the image's unit square onto the page.
    pub ctm: Matrix,
    /// The `Do` that drew it: the stream, a page's or a form's, and where
    /// in its decoded bytes. `None` when it could not be located.
    pub provenance: Option<ByteProvenance>,
    /// The marked-content sequences it was drawn in. `Some` only from
    /// [`crate::page_marked`].
    pub marked: Option<MarkedRef>,
}

impl ImagePlacement {
    /// The image's corners on the page: its unit square through the CTM,
    /// lower left first, anticlockwise.
    pub fn corners(&self) -> [(f64, f64); 4] {
        [
            self.ctm.apply(0.0, 0.0),
            self.ctm.apply(1.0, 0.0),
            self.ctm.apply(1.0, 1.0),
            self.ctm.apply(0.0, 1.0),
        ]
    }

    /// The smallest page rectangle holding it, `[x0, y0, x1, y1]`.
    pub fn bounds(&self) -> [f64; 4] {
        let mut out = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (x, y) in self.corners() {
            out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
        }
        out
    }

    /// Whether `(x, y)` on the page is on the image, rotated or not.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        let Some(inverse) = self.ctm.inverse() else {
            return false;
        };
        let (u, v) = inverse.apply(x, y);
        (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
    }
}
