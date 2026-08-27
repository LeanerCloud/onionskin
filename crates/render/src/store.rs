//! The eviction policy over tile caches, so resident memory is proportional to
//! the pages being viewed rather than to every page ever visited.
//!
//! One [`TileCache`] per `(page, zoom)` costs its base raster plus its
//! composited tiles, roughly twice the page's pixels (M1 measurement), so a
//! scroll through a long document accumulates memory without a bound. The
//! store keeps the caches in least-recently-used order and, on every access,
//! evicts back to a byte budget.
//!
//! Two things it deliberately does not do. It never drops the entry the caller
//! just asked for, which is the one about to be painted. And eviction never
//! invalidates anything already handed out: a tile is an `Arc`, so a frame
//! holding one keeps it alive whatever the store does.

use crate::base::BaseRaster;
use crate::tile::{TileCache, TILE_BYTES};

/// A 3840x2160 viewport spans 15x9 tiles.
const VIEWPORT_TILES: usize = 15 * 9;
/// What one screenful costs: its composited tiles, plus the base rasters under
/// them, which the M1 spike measured at about the same again.
const VIEWPORT_BYTES: usize = 2 * VIEWPORT_TILES * TILE_BYTES;
/// The visible screen plus a screen of scroll slack above and below, so a
/// continuous scroll evicts pages it has left rather than pages it is still
/// painting.
const VIEWPORTS_RESIDENT: usize = 3;

/// One page at one zoom. Zoom is keyed by its bits because a cache is only
/// reusable at the exact scale its base raster was rasterized at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    page: usize,
    zoom_bits: u32,
}

impl Key {
    fn new(page: usize, zoom: f32) -> Self {
        Self {
            page,
            zoom_bits: zoom.to_bits(),
        }
    }
}

struct Entry {
    key: Key,
    cache: TileCache,
}

/// Tile caches under a byte budget, least recently used evicted first.
///
/// `Send`, not `Sync`: a [`TileCache`] composites behind `&self` through a
/// `RefCell`. One thread owns the store, which is what the viewer does - the
/// render worker produces base rasters and the canvas caches and paints them.
pub struct TileStore {
    budget: usize,
    /// Least recently used first, most recently used last.
    entries: Vec<Entry>,
}

impl TileStore {
    /// Base rasters and tiles for three 4K viewports.
    pub const DEFAULT_BUDGET_BYTES: usize = VIEWPORTS_RESIDENT * VIEWPORT_BYTES;

    pub fn new() -> Self {
        Self::with_budget(Self::DEFAULT_BUDGET_BYTES)
    }

    /// Panics on a zero budget: a store that may hold nothing would evict
    /// every cache the moment it was built, which is a configuration mistake
    /// rather than a memory policy.
    pub fn with_budget(bytes: usize) -> Self {
        assert!(bytes > 0, "a tile store needs a budget above zero");
        Self {
            budget: bytes,
            entries: Vec::new(),
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Bytes held by every cache in the store.
    ///
    /// Above the budget only in the two cases [`Self::evict_to_budget`]
    /// documents: between accesses, while the page the caller is holding
    /// composites its tiles, and for as long as a single page is larger than
    /// the whole budget.
    pub fn resident_bytes(&self) -> usize {
        self.entries
            .iter()
            .map(|entry| entry.cache.resident_bytes())
            .sum()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The cache for `(page, zoom)`, marked most recently used, or `None` if
    /// it was never inserted or has been evicted.
    pub fn get(&mut self, page: usize, zoom: f32) -> Option<&mut TileCache> {
        let at = self.position(Key::new(page, zoom))?;
        self.touch(at);
        self.evict_to_budget();
        Some(&mut self.entries.last_mut().expect("just touched").cache)
    }

    /// Cache `base` as the render of `page` at its own zoom, replacing any
    /// cache already held for that pair, and evict back to the budget.
    ///
    /// A replaced cache is dropped whole, overlays included: the caller that
    /// re-rendered the page owns re-adding them, because only it knows which
    /// of them the new raster already contains.
    pub fn insert(&mut self, page: usize, base: BaseRaster) -> &mut TileCache {
        let key = Key::new(page, base.zoom());
        if let Some(at) = self.position(key) {
            self.entries.remove(at);
        }
        self.entries.push(Entry {
            key,
            cache: TileCache::new(base),
        });

        self.evict_to_budget();
        &mut self.entries.last_mut().expect("just pushed").cache
    }

    /// Drop every cache.
    ///
    /// The key is `(page, zoom)`, not the [`RenderOptions`](crate::RenderOptions)
    /// the raster was produced with, so a caller that changes those - toggling
    /// a layer, turning annotations off - has to say so: every cached raster
    /// predates the change and none of them would be rebuilt otherwise.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn position(&self, key: Key) -> Option<usize> {
        self.entries.iter().position(|entry| entry.key == key)
    }

    fn touch(&mut self, at: usize) {
        let entry = self.entries.remove(at);
        self.entries.push(entry);
    }

    /// Evict until the store fits its budget, oldest page first and, within
    /// that page, its composited tiles before its base raster.
    ///
    /// Recency outranks rebuild cost. Ranking the other way round - every
    /// other page's tiles before any page's base raster, because a tile costs
    /// ~0.1 ms and a base raster an interpreter run - makes a page one frame
    /// old pay for a page the user scrolled past minutes ago, and at a zoom
    /// where two pages do not fit it recomposites a visible page's whole grid
    /// on every frame.
    ///
    /// The most recently used entry is never evicted: it is the page the
    /// caller just asked for and is about to paint. A single page can
    /// therefore exceed the budget on its own at the zoom that was requested;
    /// the budget bounds what the store keeps around, not what a caller
    /// demands right now.
    fn evict_to_budget(&mut self) {
        loop {
            let over = self.over_budget();
            if over == 0 || self.entries.len() < 2 {
                return;
            }
            if self.entries[0].cache.evict_tiles(over) < over {
                self.entries.remove(0);
            }
        }
    }

    /// Bytes the store is over its budget by.
    fn over_budget(&self) -> usize {
        self.resident_bytes().saturating_sub(self.budget)
    }
}

impl Default for TileStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store has to be able to move to the thread that owns the canvas,
    /// and it deliberately cannot be shared with another one.
    #[test]
    fn a_store_moves_to_one_thread_and_stays_there() {
        fn assert_send<T: Send>() {}
        assert_send::<TileStore>();
    }
}
