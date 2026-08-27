//! The tile cache: Schist's 256x256 copy-on-write tiles and damage tracking,
//! moved from the document substrate to the render cache (PLAN.md, "What does
//! not transfer", item 1). A tile holds the base page raster with the overlay
//! draw list composited on top; damaging a rectangle drops exactly the tiles
//! it intersects, so the next paint recomposites only those.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use tiny_skia::{Pixmap, PixmapMut, Transform};

use crate::base::BaseRaster;
use crate::overlay::{self, Overlay, OverlayError};

/// Schist's tile size, kept: 256 KiB of RGBA8 per tile.
pub const TILE_SIZE: u32 = 256;

pub(crate) const TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE * 4) as usize;

/// An axis-aligned rectangle in device pixels, i.e. base-raster space at the
/// cache's zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One composited tile, always `TILE_SIZE` square. Pixels outside the page
/// are left transparent. Handing one to the canvas is an `Arc` clone, which
/// is the copy-on-write part: a tile is never mutated, only replaced.
pub struct Tile {
    rgba: Vec<u8>,
}

impl Tile {
    /// Premultiplied RGBA8, row-major.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// The composited tiles of one page at one zoom, over one base raster.
///
/// Compositing happens behind `&self`: a paint pass walks the visible tiles of
/// several pages and must not need a `&mut` on each of them. Nothing observable
/// changes, only which tiles are cached.
pub struct TileCache {
    base: BaseRaster,
    overlays: Vec<Overlay>,
    /// For each tile, the overlays that paint into it, so a page carrying 500
    /// annotations does not walk all 500 to composite one tile.
    tile_overlays: Vec<Vec<usize>>,
    cols: u32,
    rows: u32,
    tiles: RefCell<Vec<Option<Arc<Tile>>>>,
    composites: Cell<u64>,
}

impl TileCache {
    pub fn new(base: BaseRaster) -> Self {
        let cols = base.width().div_ceil(TILE_SIZE);
        let rows = base.height().div_ceil(TILE_SIZE);
        let count = (cols * rows) as usize;
        Self {
            base,
            overlays: Vec::new(),
            tile_overlays: vec![Vec::new(); count],
            cols,
            rows,
            tiles: RefCell::new(vec![None; count]),
            composites: Cell::new(0),
        }
    }

    pub fn cols(&self) -> u32 {
        self.cols
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// How many tiles have been composited since construction. The damage
    /// tracking is only worth anything if this stays small, so it is part of
    /// the public surface rather than a debug counter.
    pub fn composites(&self) -> u64 {
        self.composites.get()
    }

    /// How many overlays paint into the tile at `(col, row)`. The per-tile
    /// index is only worth anything if this stays far below the page's overlay
    /// count, so it is public for the same reason [`Self::composites`] is.
    pub fn overlays_in_tile(&self, col: u32, row: u32) -> usize {
        self.tile_overlays[(row * self.cols + col) as usize].len()
    }

    /// Add an overlay, index it into the tiles it paints, and damage those
    /// tiles. Returns the number of cached tiles dropped.
    ///
    /// An overlay that paints nothing is refused here rather than skipped on
    /// every composite for the life of the page.
    pub fn add_overlay(&mut self, overlay: Overlay) -> Result<usize, OverlayError> {
        overlay.validate()?;

        let b = overlay.bounds();
        let zoom = self.base.zoom();
        // One pixel of slack on each side for the antialiased edge.
        let painted = DeviceRect {
            x: b.min_x * zoom - 1.0,
            y: b.min_y * zoom - 1.0,
            width: (b.max_x - b.min_x) * zoom + 2.0,
            height: (b.max_y - b.min_y) * zoom + 2.0,
        };

        let index = self.overlays.len();
        self.overlays.push(overlay);
        if let Some((col0, row0, col1, row1)) = self.tile_span(painted) {
            for row in row0..=row1 {
                for col in col0..=col1 {
                    self.tile_overlays[(row * self.cols + col) as usize].push(index);
                }
            }
        }

        Ok(self.damage(painted))
    }

    /// Drop every cached tile intersecting `rect`. Returns how many were
    /// dropped, which is what the damage tracking has to be judged on.
    pub fn damage(&mut self, rect: DeviceRect) -> usize {
        let Some((col0, row0, col1, row1)) = self.tile_span(rect) else {
            return 0;
        };

        let tiles = self.tiles.get_mut();
        let mut dropped = 0;
        for row in row0..=row1 {
            for col in col0..=col1 {
                if tiles[(row * self.cols + col) as usize].take().is_some() {
                    dropped += 1;
                }
            }
        }
        dropped
    }

    /// The tile at `(col, row)`, compositing it if it is not cached.
    ///
    /// Panics when out of range: the caller derives the range from
    /// [`Self::cols`] and [`Self::rows`], so a miss is a bug, not input.
    pub fn tile(&self, col: u32, row: u32) -> Arc<Tile> {
        assert!(
            col < self.cols && row < self.rows,
            "tile ({col}, {row}) is outside the {}x{} grid",
            self.cols,
            self.rows
        );

        let index = (row * self.cols + col) as usize;
        if let Some(tile) = self.cached(index) {
            return tile;
        }

        // Composited with no borrow of `tiles` outstanding: `page_image` calls
        // this in a loop, and a borrow held across the composite would be a
        // re-entrancy panic waiting for the first caller that nests them.
        let tile = Arc::new(self.composite(col, row));
        self.composites.set(self.composites.get() + 1);
        self.tiles.borrow_mut()[index] = Some(tile.clone());
        tile
    }

    fn cached(&self, index: usize) -> Option<Arc<Tile>> {
        self.tiles.borrow()[index].clone()
    }

    /// Every tile composited and blitted back into one page-sized image.
    /// Evidence and tests only; the canvas paints tiles.
    pub fn page_image(&self) -> Pixmap {
        let mut page = Pixmap::new(self.base.width(), self.base.height())
            .expect("a base raster always has a non-zero, in-range size");
        let stride = self.base.width() as usize * 4;

        for row in 0..self.rows {
            for col in 0..self.cols {
                let tile = self.tile(col, row);
                let (ox, oy) = (col * TILE_SIZE, row * TILE_SIZE);
                let copy_w = TILE_SIZE.min(self.base.width() - ox) as usize * 4;
                let copy_h = TILE_SIZE.min(self.base.height() - oy);

                for y in 0..copy_h {
                    let dst = (oy + y) as usize * stride + ox as usize * 4;
                    let src = y as usize * TILE_SIZE as usize * 4;
                    page.data_mut()[dst..dst + copy_w]
                        .copy_from_slice(&tile.rgba()[src..src + copy_w]);
                }
            }
        }
        page
    }

    /// Inclusive tile range covering `rect`, or `None` when it misses the page.
    fn tile_span(&self, rect: DeviceRect) -> Option<(u32, u32, u32, u32)> {
        let (x0, y0) = (rect.x.max(0.0), rect.y.max(0.0));
        let x1 = (rect.x + rect.width).min(self.base.width() as f32);
        let y1 = (rect.y + rect.height).min(self.base.height() as f32);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }

        // The far edge is exclusive, so a rect ending exactly on a tile
        // boundary must not claim the tile beyond it.
        let last =
            |v: f32, count: u32| ((v.ceil() as u32).saturating_sub(1) / TILE_SIZE).min(count - 1);
        Some((
            x0.floor() as u32 / TILE_SIZE,
            y0.floor() as u32 / TILE_SIZE,
            last(x1, self.cols),
            last(y1, self.rows),
        ))
    }

    fn composite(&self, col: u32, row: u32) -> Tile {
        let mut rgba = vec![0u8; TILE_BYTES];
        let (ox, oy) = (col * TILE_SIZE, row * TILE_SIZE);
        let copy_w = TILE_SIZE.min(self.base.width() - ox) as usize * 4;
        let copy_h = TILE_SIZE.min(self.base.height() - oy);
        let src_stride = self.base.width() as usize * 4;

        for y in 0..copy_h {
            let src = (oy + y) as usize * src_stride + ox as usize * 4;
            let dst = y as usize * TILE_SIZE as usize * 4;
            rgba[dst..dst + copy_w].copy_from_slice(&self.base.rgba()[src..src + copy_w]);
        }

        let mut target = PixmapMut::from_bytes(&mut rgba, TILE_SIZE, TILE_SIZE)
            .expect("a tile buffer is TILE_SIZE square by construction");
        let zoom = self.base.zoom();
        let transform = Transform::from_translate(-(ox as f32), -(oy as f32)).pre_scale(zoom, zoom);
        let painting = self.tile_overlays[(row * self.cols + col) as usize]
            .iter()
            .map(|&i| &self.overlays[i]);
        overlay::draw(painting, &mut target, transform);

        Tile { rgba }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::Rgba;

    /// Schist's canvas grew a dark line at every tile edge once it painted one
    /// quad per tile, so the equivalent has to be nailed down here: an
    /// antialiased diagonal composited tile by tile must land on exactly the
    /// pixels a single whole-page pass produces.
    #[test]
    fn tiled_compositing_matches_a_single_pass() {
        const W: u32 = 600;
        const H: u32 = 400;

        let base = BaseRaster::new(W, H, 1.0, vec![255; (W * H * 4) as usize]);
        let ink = Overlay::Ink {
            points: vec![(40.0, 30.0), (560.0, 370.0)],
            color: Rgba {
                r: 20,
                g: 20,
                b: 20,
                a: 255,
            },
            width: 3.0,
        };

        let mut cache = TileCache::new(base.clone());
        cache.add_overlay(ink.clone()).expect("a two-point stroke");
        let tiled = cache.page_image();

        let mut single = Pixmap::new(W, H).expect("600x400 is a valid pixmap size");
        single.data_mut().copy_from_slice(base.rgba());
        overlay::draw([&ink], &mut single.as_mut(), Transform::identity());

        let mut worst = 0u8;
        let mut worst_at = (0u32, 0u32);
        let mut on_a_seam = Vec::new();
        for (i, (a, b)) in tiled.data().iter().zip(single.data()).enumerate() {
            let delta = a.abs_diff(*b);
            if delta == 0 {
                continue;
            }
            let (x, y) = ((i / 4) as u32 % W, (i / 4) as u32 / W);
            if delta > worst {
                worst = delta;
                worst_at = (x, y);
            }
            if [TILE_SIZE - 1, 0].contains(&(x % TILE_SIZE))
                || [TILE_SIZE - 1, 0].contains(&(y % TILE_SIZE))
            {
                on_a_seam.push((x, y));
            }
        }

        // Not bit-identical. tiny-skia clips a path to the pixmap it is
        // drawing into, so an edge crossing a tile is scan-converted from
        // slightly different endpoints than the same edge crossing the whole
        // page, and coverage on antialiased pixels shifts a little. What
        // matters is that it stays small and never lands on a tile edge,
        // which is where Schist's canvas showed a line.
        assert!(
            worst <= 16,
            "tiled compositing is off by {worst} at pixel {worst_at:?}"
        );
        assert!(
            on_a_seam.is_empty(),
            "{} differing pixels sit on a tile boundary, first at {:?}",
            on_a_seam.len(),
            on_a_seam.first()
        );
    }
}
