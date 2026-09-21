//! What imposition produces: sheets of paper, and on each the pages placed
//! on it.

use onionskin_core::PageIndex;

/// One page on a sheet.
///
/// `transform` maps the page **as displayed**, its `/Rotate` already
/// applied, from its own box `[0, width] x [0, height]` in points onto the
/// sheet. A backend composes it with the page's rotation to draw the page's
/// content, or with the raster's size to draw Print as Image. There is no
/// clip: nothing in M3 places a page partly off its space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub source: PageIndex,
    pub transform: [f64; 6],
}

impl Placement {
    /// Where the page's displayed box lands on the sheet:
    /// `[x0, y0, x1, y1]`, for a page `width` by `height`.
    pub fn footprint(&self, width: f64, height: f64) -> [f64; 4] {
        let [a, b, c, d, e, f] = self.transform;
        let corners = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
            .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
        let xs = corners.map(|p| p.0);
        let ys = corners.map(|p| p.1);
        [
            xs.iter().copied().fold(f64::INFINITY, f64::min),
            ys.iter().copied().fold(f64::INFINITY, f64::min),
            xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ]
    }
}

/// A sheet of paper and what is on it. `frames` are the page borders
/// Multiple's "Print Page Border" draws, one rectangle per cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub width: f64,
    pub height: f64,
    pub placements: Vec<Placement>,
    pub frames: Vec<[f64; 4]>,
}

impl Sheet {
    pub fn is_landscape(&self) -> bool {
        self.width > self.height
    }
}
