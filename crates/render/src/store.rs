//! The eviction policy over tile caches, so resident memory is proportional to
//! the pages being viewed rather than to every page ever visited.
//!
//! One [`TileCache`] per `(page, zoom)` costs its base raster plus its
//! composited tiles, roughly twice the page's pixels (M1 measurement), so a
//! scroll through a long document accumulates memory without a bound. The
//! store keeps the caches in least-recently-used order and, on every access,
//! evicts back to a byte budget.
//!
//! Three things it deliberately does not do. It never drops the entry the
//! caller just asked for, nor any page painted by the current frame or the one
//! before it (see [`TileStore::begin_frame`]). And eviction never invalidates
//! anything already handed out: a tile is an `Arc`, so a frame holding one
//! keeps it alive whatever the store does.

use crate::base::BaseRaster;
use crate::tile::{TileCache, TILE_BYTES};

/// A 3840x2160 viewport spans 15x9 tiles.
const VIEWPORT_TILES: usize = 15 * 9;
/// What one screenful costs: its composited tiles, plus the base rasters under
/// them, which the M1 spike measured at about the same again.
const VIEWPORT_BYTES: usize = 2 * VIEWPORT_TILES * TILE_BYTES;
/// The visible screen plus a screen of scroll slack above and below.
///
/// What that buys depends entirely on zoom, because a cache is sized by the
/// page's pixels and not by the screen's. Measured for US Letter: about twelve
/// pages at 2x, three at 4x, two at 5x, one at 7x, and not even one at 8x,
/// where a single page needs 243 MiB against this 202 MiB. The pages a frame
/// touches are exempt from eviction for exactly that reason - above 5x the
/// budget cannot hold a two-page spread, and without the exemption the pages
/// on screen would be the ones evicted.
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
    /// The pages the frame in progress has touched, which eviction may not
    /// take. `None` until a frame is declared: a caller that never declares
    /// one, a headless warmer filling caches say, gets plain recency
    /// eviction and nothing pinned forever.
    frame: Option<Vec<Key>>,
    /// The pages the previous frame touched, exempt for one more frame. A
    /// frame asks for its pages one at a time, so without this the second
    /// page of a spread is evicted in the moment between the first being
    /// pinned and the second being asked for.
    last_frame: Vec<Key>,
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
            frame: None,
            last_frame: Vec::new(),
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Start a frame, ageing out the exemption two frames back.
    ///
    /// Every page the frame asks for, through [`Self::get`] or
    /// [`Self::insert`], is exempt from eviction for this frame and the next,
    /// tiles and base raster both. Without an exemption the store protects
    /// only the page most recently asked for, which is enough while the
    /// visible set fits the budget and wrong as soon as it does not: at 6x a
    /// two-page spread needs 276 MiB against a 202 MiB budget, so fetching the
    /// second page would evict the first, every frame, forever.
    ///
    /// The extra frame is what lets a frame ask for its pages one at a time:
    /// the spread's second page has to survive the moment between the first
    /// being pinned and itself being asked for, and it is the previous frame's
    /// exemption that carries it there.
    ///
    /// Both come from the keys `get` and `insert` are called with, so there is
    /// no second value to disagree with them. A frame that declared its pages
    /// separately had one: a single ULP between the zoom it named and the zoom
    /// it painted with left every visible page unpinned and re-rendered on
    /// every frame, with nothing in the store able to notice.
    ///
    /// When one frame's pages exceed the budget the store stops evicting and
    /// stays over rather than thrashing. [`Self::over_budget`] reports by how
    /// much, so a caller that cares can lower the zoom or narrow the spread;
    /// nothing here silently drops what the frame is painting.
    pub fn begin_frame(&mut self) {
        self.last_frame = self.frame.replace(Vec::new()).unwrap_or_default();
        self.evict_to_budget();
    }

    /// Bytes the store is over its budget by, zero when it fits.
    ///
    /// Non-zero only while the visible set demands it, or momentarily while
    /// the page a caller is holding composites its tiles.
    pub fn over_budget(&self) -> usize {
        self.resident_bytes().saturating_sub(self.budget)
    }

    /// Bytes held by every cache in the store. See [`Self::over_budget`] for
    /// when this is allowed to exceed the budget.
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
        let key = Key::new(page, zoom);
        let at = self.position(key)?;
        self.touch(at);
        self.pin(key);
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

        self.pin(key);
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
        self.frame = None;
        self.last_frame.clear();
    }

    fn position(&self, key: Key) -> Option<usize> {
        self.entries.iter().position(|entry| entry.key == key)
    }

    fn touch(&mut self, at: usize) {
        let entry = self.entries.remove(at);
        self.entries.push(entry);
    }

    /// Evict until the store fits its budget, oldest evictable page first and,
    /// within that page, its composited tiles before its base raster.
    ///
    /// Recency outranks rebuild cost. Ranking the other way round - every
    /// other page's tiles before any page's base raster, because a tile costs
    /// ~0.1 ms and a base raster an interpreter run - makes a page one frame
    /// old pay for a page the user scrolled past minutes ago.
    ///
    /// Off limits: the page the caller just asked for, and everything
    /// [`Self::begin_frame`] declared visible. When only those are left the
    /// loop stops with the store over budget, which is the one state it
    /// reports rather than resolves.
    fn evict_to_budget(&mut self) {
        loop {
            let over = self.over_budget();
            if over == 0 {
                return;
            }
            let Some(at) = self.oldest_evictable() else {
                return;
            };
            if self.entries[at].cache.evict_tiles(over) < over {
                self.entries.remove(at);
            }
        }
    }

    /// Exempt the key from eviction for the rest of the frame, if one is in
    /// progress.
    fn pin(&mut self, key: Key) {
        if let Some(frame) = &mut self.frame {
            if !frame.contains(&key) {
                frame.push(key);
            }
        }
    }

    fn is_pinned(&self, key: Key) -> bool {
        self.frame.as_deref().unwrap_or(&[]).contains(&key) || self.last_frame.contains(&key)
    }

    /// The oldest entry eviction is allowed to take: not painted by this frame
    /// or the one before it, and not the entry just handed to the caller.
    fn oldest_evictable(&self) -> Option<usize> {
        let last = self.entries.len().checked_sub(1)?;

        (0..last).find(|&at| !self.is_pinned(self.entries[at].key))
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
