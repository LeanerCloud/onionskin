//! Overlay primitives and their tiny-skia rasterization.

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

impl Overlay {
    /// Bounding box in page render space, grown by half the stroke width so
    /// it covers what actually gets painted. `None` when there is nothing to
    /// paint, which for an ink stroke means fewer than two points.
    pub(crate) fn bounds(&self) -> Option<Bounds> {
        let (points, pad): (&[(f32, f32)], f32) = match self {
            Self::Highlight { corners, .. } => (corners, 0.0),
            Self::Ink { points, width, .. } => {
                if points.len() < 2 {
                    return None;
                }
                (points, width / 2.0)
            }
        };

        let mut b = Bounds {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        };
        for &(x, y) in points {
            b.min_x = b.min_x.min(x);
            b.min_y = b.min_y.min(y);
            b.max_x = b.max_x.max(x);
            b.max_y = b.max_y.max(y);
        }
        if !b.min_x.is_finite() || !b.min_y.is_finite() {
            return None;
        }

        b.min_x -= pad;
        b.min_y -= pad;
        b.max_x += pad;
        b.max_y += pad;
        Some(b)
    }
}

/// Paint every overlay onto `target`. `transform` maps page render space to
/// `target`'s pixels; for a tile that is zoom then the tile's origin.
pub(crate) fn draw(overlays: &[Overlay], target: &mut PixmapMut<'_>, transform: Transform) {
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
                if points.len() < 2 {
                    continue;
                }
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
