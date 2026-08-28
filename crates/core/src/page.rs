use onionskin_content as content;
use std::fmt;

pub use content::{PageIndex, PageQuad};

/// Keyboard modifiers accompanying a pointer event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl_or_cmd: bool,
}

/// A point in a page's default user space: origin at the lower-left corner,
/// y increasing upwards, units of 1/72 inch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePoint {
    pub page: PageIndex,
    pub x: f64,
    pub y: f64,
}

/// An axis-aligned rectangle in a page's user space, in PDF `/Rect` order:
/// lower-left, then upper-right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRect {
    pub page: PageIndex,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// Geometry loaded lazily from the page tree.
#[derive(Clone, Debug, PartialEq)]
pub struct PageGeometry {
    pub index: PageIndex,
    pub media_box: [f64; 4],
    pub crop_box: Option<[f64; 4]>,
    pub rotate: i32,
    pub render_size: (f64, f64),
    transform: onionskin_render::PageTransform,
}

impl PageGeometry {
    pub(crate) fn new(
        page: &content::Page,
        rendered: onionskin_render::PageRenderGeometry,
    ) -> Self {
        PageGeometry {
            index: page.index,
            media_box: page.media_box,
            crop_box: page.crop_box,
            rotate: page.rotate,
            render_size: rendered.render_size,
            transform: rendered.transform,
        }
    }

    pub fn user_to_device(&self, quad: PageQuad, zoom: f32) -> Result<DeviceQuad, GeometryError> {
        if quad.page != self.index {
            return Err(GeometryError::WrongPage {
                geometry: self.index,
                quad: quad.page,
            });
        }
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(GeometryError::InvalidZoom(zoom));
        }
        let scale = f64::from(zoom);
        Ok(DeviceQuad {
            page: quad.page,
            corners: quad.corners.map(|(x, y)| {
                let (x, y) = self
                    .transform
                    .apply(x + self.media_box[0], y + self.media_box[1]);
                (x * scale, y * scale)
            }),
        })
    }

    pub fn device_to_user(&self, x: f64, y: f64, zoom: f32) -> Result<PagePoint, GeometryError> {
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(GeometryError::InvalidZoom(zoom));
        }
        let scale = f64::from(zoom);
        let (x, y) = self.transform.apply_inverse(x / scale, y / scale)?;
        Ok(PagePoint {
            page: self.index,
            x: x - self.media_box[0],
            y: y - self.media_box[1],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceQuad {
    pub page: PageIndex,
    pub corners: [(f64, f64); 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeometryError {
    WrongPage {
        geometry: PageIndex,
        quad: PageIndex,
    },
    InvalidZoom(f32),
    Transform(onionskin_render::TransformError),
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongPage { geometry, quad } => {
                write!(f, "page {quad} quad cannot use page {geometry} geometry")
            }
            Self::InvalidZoom(zoom) => write!(f, "zoom must be positive and finite, got {zoom}"),
            Self::Transform(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for GeometryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform(error) => Some(error),
            Self::WrongPage { .. } | Self::InvalidZoom(_) => None,
        }
    }
}

impl From<onionskin_render::TransformError> for GeometryError {
    fn from(error: onionskin_render::TransformError) -> Self {
        Self::Transform(error)
    }
}
