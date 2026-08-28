use onionskin_content as content;

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
}

impl From<&content::Page> for PageGeometry {
    fn from(page: &content::Page) -> Self {
        PageGeometry {
            index: page.index,
            media_box: page.media_box,
            crop_box: page.crop_box,
            rotate: page.rotate,
        }
    }
}
