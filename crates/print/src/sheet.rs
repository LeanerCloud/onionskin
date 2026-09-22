//! What imposition produces: sheets of paper, and on each the pages placed
//! on it.

use onionskin_core::PageIndex;

/// One page on a sheet.
///
/// `transform` maps the page **as displayed**, its `/Rotate` already
/// applied, from its own box `[0, width] x [0, height]` in points onto the
/// sheet. A backend composes it with the page's rotation to draw the page's
/// content, or with the raster's size to draw Print as Image.
///
/// `clip`, when set, is the part of the sheet the page may draw in,
/// `[x0, y0, x1, y1]`. Only a poster tile has one: its page is larger than
/// the tile, and the margin around the tile is left for the cut marks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub source: PageIndex,
    pub transform: [f64; 6],
    pub clip: Option<[f64; 4]>,
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

impl Placement {
    /// The part of the footprint the page may draw in: the footprint cut to
    /// the clip, when there is one. What a preview outlines.
    pub fn visible(&self, width: f64, height: f64) -> [f64; 4] {
        let [x0, y0, x1, y1] = self.footprint(width, height);
        match self.clip {
            None => [x0, y0, x1, y1],
            Some([cx0, cy0, cx1, cy1]) => [x0.max(cx0), y0.max(cy0), x1.min(cx1), y1.min(cy1)],
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clipped_placement_shows_only_its_clip() {
        let tile = Placement {
            source: 0,
            transform: [2.0, 0.0, 0.0, 2.0, -100.0, -100.0],
            clip: Some([0.0, 0.0, 50.0, 60.0]),
        };
        assert_eq!(tile.footprint(100.0, 100.0), [-100.0, -100.0, 100.0, 100.0]);
        assert_eq!(tile.visible(100.0, 100.0), [0.0, 0.0, 50.0, 60.0]);
        let whole = Placement { clip: None, ..tile };
        assert_eq!(whole.visible(100.0, 100.0), whole.footprint(100.0, 100.0));
    }
}
