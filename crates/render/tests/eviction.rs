//! The eviction policy, tested against synthetic base rasters: what is being
//! bounded is bytes per `(page, zoom)`, which has nothing to do with what the
//! interpreter drew.

use onionskin_render::{BaseRaster, TileStore, TILE_SIZE};

/// US Letter at 2x, the size a retina viewer actually holds.
const ZOOM: f32 = 2.0;
const PAGE_WIDTH: u32 = 612 * ZOOM as u32;
const PAGE_HEIGHT: u32 = 792 * ZOOM as u32;

const TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE * 4) as usize;
const BASE_BYTES: usize = (PAGE_WIDTH * PAGE_HEIGHT * 4) as usize;
const TILES_PER_PAGE: usize =
    (PAGE_WIDTH.div_ceil(TILE_SIZE) * PAGE_HEIGHT.div_ceil(TILE_SIZE)) as usize;

fn page() -> BaseRaster {
    BaseRaster::new(PAGE_WIDTH, PAGE_HEIGHT, ZOOM, vec![255; BASE_BYTES])
}

/// What the canvas does with a page it has scrolled into view.
fn paint(store: &mut TileStore, page: usize) {
    let cache = store
        .get(page, ZOOM)
        .expect("the page was just inserted, and the store never evicts that one");
    for row in 0..cache.rows() {
        for col in 0..cache.cols() {
            let _ = cache.tile(col, row);
        }
    }
}

#[test]
fn scrolling_a_200_page_document_stays_under_the_budget() {
    let mut store = TileStore::new();

    let mut peak = 0;
    for index in 0..200 {
        store.insert(index, page());
        assert!(
            store.resident_bytes() <= TileStore::DEFAULT_BUDGET_BYTES,
            "page {index}: {} bytes resident against a {} byte budget",
            store.resident_bytes(),
            TileStore::DEFAULT_BUDGET_BYTES
        );

        paint(&mut store, index);
        peak = peak.max(store.resident_bytes());
    }

    // Compositing grows the cache the caller is holding, after the store's own
    // check, so the peak is the budget plus the tiles of the one page being
    // painted. Every access brings it back down.
    assert!(
        peak <= TileStore::DEFAULT_BUDGET_BYTES + TILES_PER_PAGE * TILE_BYTES,
        "peaked at {peak} bytes"
    );
    assert!(
        store.len() < 200,
        "nothing was evicted: {} caches resident",
        store.len()
    );
    assert!(store.get(0, ZOOM).is_none(), "page 0 is long out of view");
}

#[test]
fn a_page_that_stays_in_view_is_never_recomposited() {
    // Three pages visible, scrolled one page at a time under the real budget:
    // a page in view for three frames must composite its tiles once, not once
    // per frame, or eviction is dropping what the next frame paints.
    let mut store = TileStore::new();

    for top in 0..20 {
        for index in top..top + 3 {
            if store.get(index, ZOOM).is_none() {
                store.insert(index, page());
            }
            paint(&mut store, index);
        }

        for index in top..top + 3 {
            let cache = store.get(index, ZOOM).expect("still in view");
            assert_eq!(
                cache.composites(),
                TILES_PER_PAGE as u64,
                "page {index} recomposited during frame {top}"
            );
        }
    }
}

#[test]
fn the_oldest_page_goes_whole_before_a_newer_one_loses_a_tile() {
    // Room for two pages: page 0 has to go entirely, rather than page 0 and
    // page 1 each being stripped of the tiles they would need again next
    // frame.
    let budget = 2 * (BASE_BYTES + TILES_PER_PAGE * TILE_BYTES);
    let mut store = TileStore::with_budget(budget);

    for index in 0..3 {
        store.insert(index, page());
        paint(&mut store, index);
    }
    store.get(2, ZOOM).expect("the page just painted");

    assert!(store.get(0, ZOOM).is_none(), "the oldest page went first");
    let newer = store.get(1, ZOOM).expect("a page one frame old");
    assert_eq!(
        newer.resident_bytes(),
        BASE_BYTES + TILES_PER_PAGE * TILE_BYTES,
        "and it kept every tile"
    );
}

#[test]
fn tiles_go_before_base_rasters_and_only_as_many_as_the_budget_needs() {
    // Room for both pages' base rasters and 40 of their 70 tiles: a base raster
    // costs an interpreter run to rebuild, a tile costs a memcpy and a few
    // paths, so the tiles are what has to go.
    const KEPT: usize = 5;
    let budget = 2 * BASE_BYTES + (TILES_PER_PAGE + KEPT) * TILE_BYTES;
    let mut store = TileStore::with_budget(budget);

    for index in 0..2 {
        store.insert(index, page());
        paint(&mut store, index);
    }

    // Page 1 was painted last, so the sweep this access runs has to take page
    // 0's tiles, and only as many of them as the budget is short.
    let painting = store.get(1, ZOOM).expect("the page just painted");
    assert_eq!(
        painting.resident_bytes(),
        BASE_BYTES + TILES_PER_PAGE * TILE_BYTES
    );

    assert_eq!(store.len(), 2, "no cache may be dropped while tiles remain");
    let evicted = store
        .get(0, ZOOM)
        .expect("its base raster is still resident");
    assert_eq!(evicted.resident_bytes(), BASE_BYTES + KEPT * TILE_BYTES);
    assert_eq!(store.resident_bytes(), budget);
}

#[test]
fn a_tile_already_handed_out_survives_the_eviction_of_its_cache() {
    // Room for exactly one page's base raster: painting page 0 puts the store
    // over, and inserting page 1 evicts page 0 entirely.
    let mut store = TileStore::with_budget(BASE_BYTES);
    let tile = store.insert(0, page()).tile(0, 0);

    store.insert(1, page());
    assert!(store.get(0, ZOOM).is_none(), "page 0 was evicted");

    // The frame that asked for it still holds it: a tile is an `Arc`, so
    // eviction drops the store's reference and nothing else.
    assert_eq!(tile.rgba().len(), TILE_BYTES);
    assert_eq!(&tile.rgba()[0..4], [255, 255, 255, 255]);
}

#[test]
fn the_page_being_painted_is_never_evicted() {
    let mut store = TileStore::with_budget(1);
    let _ = store.insert(0, page()).tile(0, 0);

    assert!(
        store.get(0, ZOOM).is_some(),
        "the page the caller just asked for outranks the budget"
    );
    assert!(
        store.resident_bytes() > store.budget(),
        "so one page can exceed the budget on its own"
    );
}

#[test]
fn a_cache_serves_only_the_zoom_it_was_rasterized_at() {
    let mut store = TileStore::new();
    store.insert(3, page());

    assert!(store.get(3, ZOOM).is_some());
    assert!(store.get(3, ZOOM * 2.0).is_none(), "a different raster");
    assert!(store.get(4, ZOOM).is_none(), "a different page");
}

#[test]
fn clearing_drops_rasters_a_changed_render_option_has_invalidated() {
    // The key is (page, zoom), so nothing about a layer toggle or an
    // annotations switch reaches the store on its own.
    let mut store = TileStore::new();
    for index in 0..3 {
        store.insert(index, page());
        paint(&mut store, index);
    }

    store.clear();
    assert!(store.is_empty());
    assert_eq!(store.resident_bytes(), 0);
    assert!(store.get(1, ZOOM).is_none());
}
