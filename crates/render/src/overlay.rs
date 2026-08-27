//! Overlay primitives and their tiny-skia rasterization.

use std::error::Error;
use std::fmt;

use tiny_skia::{
    BlendMode, FillRule, LineCap, LineJoin, Paint, PathBuilder, PixmapMut, Stroke, Transform,
};

/// Straight (unpremultiplied) sRGB with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// A draw-list entry composited over the base page raster.
///
/// Coordinates are page render space: PDF points, origin at the top-left of
/// the page as hayro rasterizes it (crop box and `/Rotate` already applied),
/// y increasing downwards. That is exactly the base raster at zoom 1. Mapping
/// `plugin-api`'s user-space `PageQuad` and `PagePoint` into it is `app`'s job
/// in M2, alongside the same flip it already does for pointer input.
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    /// Filled quad, multiply-blended so the text beneath stays legible: a
    /// text highlight. Corners are in `/QuadPoints` order (upper-left,
    /// upper-right, lower-left, lower-right), matching `plugin-api::PageQuad`.
    Highlight {
        corners: [(f32, f32); 4],
        color: Rgba,
    },
    /// Stroked open polyline: an ink annotation.
    Ink {
        points: Vec<(f32, f32)>,
        color: Rgba,
        /// Stroke width in page points.
        width: f32,
    },
}

/// An axis-aligned bounding box in page render space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// Why an overlay was refused. Rejected at
/// [`TileCache::add_overlay`](crate::TileCache::add_overlay) rather than
/// dropped later: an overlay that paints nothing is a bug in whatever built
/// it, and a cache that silently kept one would re-filter it on every
/// composite for the life of the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlayError {
    /// An ink stroke needs two points to be a line.
    TooFewPoints { points: usize },
    /// A coordinate is NaN or infinite, so the overlay has no position.
    NotFinite { x: f32, y: f32 },
    /// A stroke width is NaN, infinite, or not positive.
    InvalidWidth { width: f32 },
}

impl fmt::Display for OverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewPoints { points } => {
                write!(f, "an ink stroke needs at least 2 points, got {points}")
            }
            Self::NotFinite { x, y } => write!(f, "overlay coordinate ({x}, {y}) is not finite"),
            Self::InvalidWidth { width } => {
                write!(f, "stroke width {width} is not a positive finite number")
            }
        }
    }
}

impl Error for OverlayError {}

impl Overlay {
    /// The points that make up the overlay, in page render space.
    fn points(&self) -> &[(f32, f32)] {
        match self {
            Self::Highlight { corners, .. } => corners,
            Self::Ink { points, .. } => points,
        }
    }

    /// Refuse anything that cannot be painted, so [`Self::bounds`] and
    /// [`draw`] can both assume a real shape.
    pub(crate) fn validate(&self) -> Result<(), OverlayError> {
        if let Self::Ink { points, width, .. } = self {
            if points.len() < 2 {
                return Err(OverlayError::TooFewPoints {
                    points: points.len(),
                });
            }
            if !width.is_finite() || *width <= 0.0 {
                return Err(OverlayError::InvalidWidth { width: *width });
            }
        }

        for &(x, y) in self.points() {
            if !x.is_finite() || !y.is_finite() {
                return Err(OverlayError::NotFinite { x, y });
            }
        }
        Ok(())
    }

    /// Bounding box in page render space, grown by half the stroke width so
    /// it covers what actually gets painted. Call [`Self::validate`] first:
    /// on an overlay that never passed it, this is meaningless.
    pub(crate) fn bounds(&self) -> Bounds {
        let pad = match self {
            Self::Highlight { .. } => 0.0,
            Self::Ink { width, .. } => width / 2.0,
        };

        let mut b = Bounds {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        };
        for &(x, y) in self.points() {
            b.min_x = b.min_x.min(x);
            b.min_y = b.min_y.min(y);
            b.max_x = b.max_x.max(x);
            b.max_y = b.max_y.max(y);
        }

        b.min_x -= pad;
        b.min_y -= pad;
        b.max_x += pad;
        b.max_y += pad;
        b
    }
}

/// Paint the given overlays onto `target`. `transform` maps page render space
/// to `target`'s pixels; for a tile that is zoom then the tile's origin.
///
/// Takes an iterator because a tile paints only the overlays its index lists,
/// which are not contiguous in the cache's overlay vector.
pub(crate) fn draw<'a>(
    overlays: impl IntoIterator<Item = &'a Overlay>,
    target: &mut PixmapMut<'_>,
    transform: Transform,
) {
    for overlay in overlays {
        match overlay {
            Overlay::Highlight { corners, color } => {
                // /QuadPoints order is UL, UR, LL, LR, so the perimeter is
                // 0, 1, 3, 2 rather than 0, 1, 2, 3.
                let mut pb = PathBuilder::new();
                pb.move_to(corners[0].0, corners[0].1);
                pb.line_to(corners[1].0, corners[1].1);
                pb.line_to(corners[3].0, corners[3].1);
                pb.line_to(corners[2].0, corners[2].1);
                pb.close();
                let Some(path) = pb.finish() else {
                    continue;
                };

                let mut paint = paint(*color);
                paint.blend_mode = BlendMode::Multiply;
                target.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
            Overlay::Ink {
                points,
                color,
                width,
            } => {
                let mut pb = PathBuilder::new();
                pb.move_to(points[0].0, points[0].1);
                for &(x, y) in &points[1..] {
                    pb.line_to(x, y);
                }
                let Some(path) = pb.finish() else {
                    continue;
                };

                let stroke = Stroke {
                    width: *width,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                target.stroke_path(&path, &paint(*color), &stroke, transform, None);
            }
        }
    }
}

fn paint(color: Rgba) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint
}
