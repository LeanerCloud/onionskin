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

/// US Letter at an arbitrary zoom, for the probes that vary it.
fn page_at(zoom: f32) -> BaseRaster {
    let (w, h) = page_size(zoom);
    BaseRaster::new(w, h, zoom, vec![255; (w * h * 4) as usize])
}

fn page_size(zoom: f32) -> (u32, u32) {
    ((612.0 * zoom) as u32, (792.0 * zoom) as u32)
}

/// Base raster plus every tile, which is what one page costs the store.
fn page_bytes(zoom: f32) -> usize {
    let (w, h) = page_size(zoom);
    let tiles = (w.div_ceil(TILE_SIZE) * h.div_ceil(TILE_SIZE)) as usize;
    (w * h * 4) as usize + tiles * TILE_BYTES
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
fn scrolling_at_a_zoom_that_fits_never_recomposites_a_visible_page() {
    // A scrolling regression guard, not a test of the visible-set exemption:
    // at 2x, twelve pages fit the budget, so three visible pages would survive
    // on recency alone. It pins that a scroll does not evict its own working
    // set at a zoom where nothing forces it to.
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

/// Paint a whole frame the way the canvas does: open a frame, render anything
/// missing, composite every visible page's tiles, and close it. Returns the
/// number of pages that had to be rendered again this frame.
fn frame(store: &mut TileStore, visible: &[usize], zoom: f32) -> usize {
    store.begin_frame();

    let mut rerendered = 0;
    for &index in visible {
        if store.get(index, zoom).is_none() {
            store.insert(index, page_at(zoom));
            rerendered += 1;
        }
        let cache = store
            .get(index, zoom)
            .expect("just inserted or already held");
        for row in 0..cache.rows() {
            for col in 0..cache.cols() {
                let _ = cache.tile(col, row);
            }
        }
    }
    store.end_frame();

    rerendered
}

#[test]
fn two_visible_pages_survive_a_zoom_where_both_do_not_fit() {
    // 6x: one page costs 138 MiB, so a two-page spread needs 276 MiB against a
    // 202 MiB budget. Without the visible-set exemption the store protects
    // only the page most recently asked for, so fetching the second evicts the
    // first and both are rendered again on every frame.
    const ZOOM_6: f32 = 6.0;
    assert!(
        2 * page_bytes(ZOOM_6) > TileStore::DEFAULT_BUDGET_BYTES,
        "the probe is pointless unless the spread really does not fit"
    );

    let mut store = TileStore::new();
    let tiles_per_page = {
        let (w, h) = page_size(ZOOM_6);
        (w.div_ceil(TILE_SIZE) * h.div_ceil(TILE_SIZE)) as u64
    };

    assert_eq!(frame(&mut store, &[0, 1], ZOOM_6), 2, "the first frame");
    for _ in 0..5 {
        assert_eq!(
            frame(&mut store, &[0, 1], ZOOM_6),
            0,
            "a visible page was evicted and had to be rendered again"
        );
        for index in 0..2 {
            let cache = store.get(index, ZOOM_6).expect("visible and pinned");
            assert_eq!(
                cache.composites(),
                tiles_per_page,
                "page {index} recomposited"
            );
        }
    }

    // The spread does not fit, and the store says so instead of thrashing.
    assert!(store.over_budget() > 0);
    assert_eq!(store.len(), 2);
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
fn a_framed_scroll_still_evicts_what_it_has_scrolled_past() {
    // The exemption must not become a leak: a page is exempt for the frame it
    // was painted in and the one after, and evictable once the view has moved
    // on. Three visible pages at 2x, scrolled the length of a 200-page book.
    let mut store = TileStore::new();

    for top in 0..198 {
        let visible: Vec<usize> = (top..top + 3).collect();
        frame(&mut store, &visible, ZOOM);

        // Tiles are composited after the frame's last sweep, so the peak is
        // the budget plus the tiles of the page painted last.
        assert!(
            store.over_budget() <= TILES_PER_PAGE * TILE_BYTES,
            "frame {top}: {} bytes over budget",
            store.over_budget()
        );
        // The budget holds twelve pages at this zoom, and eviction runs
        // before the frame's last page composites its tiles, so one more can
        // be resident at the moment the frame ends.
        assert!(
            store.len() <= 13,
            "frame {top}: {} caches resident, the exemption is leaking",
            store.len()
        );
    }
    assert!(store.get(0, ZOOM).is_none(), "page 0 is long out of view");
}

#[test]
fn caches_arriving_between_frames_do_not_join_the_last_one() {
    // A render worker delivers pages whenever it finishes, not when the canvas
    // paints. Those inserts land outside any frame, so they must be evictable:
    // while the frame stayed open they exempted themselves into it, and thirty
    // of them at 6x held 4 GB that eviction was not allowed to touch.
    const ZOOM_6: f32 = 6.0;
    let mut store = TileStore::new();
    frame(&mut store, &[0, 1], ZOOM_6);

    for index in 2..32 {
        store.insert(index, page_at(ZOOM_6));
    }

    // The two pages the last frame painted keep their grace exemption; nothing
    // that arrived after it does.
    assert!(
        store.resident_bytes() <= 2 * page_bytes(ZOOM_6) + page_bytes(ZOOM_6),
        "{} bytes resident after thirty unframed inserts",
        store.resident_bytes()
    );
    assert!(store.len() <= 3, "{} caches resident", store.len());
}

#[test]
fn clearing_mid_frame_keeps_the_frame_open() {
    // A layers toggle clears the store while the canvas is painting. The pages
    // it is about to insert again are still the frame's, so re-inserting the
    // second must not evict the first.
    const ZOOM_6: f32 = 6.0;
    let mut store = TileStore::new();

    store.begin_frame();
    for index in 0..2 {
        store.insert(index, page_at(ZOOM_6));
    }

    store.clear();
    assert!(store.is_empty());

    for index in 0..2 {
        store.insert(index, page_at(ZOOM_6));
    }
    assert_eq!(store.len(), 2, "the frame's own pages evicted each other");
    assert!(store.get(0, ZOOM_6).is_some(), "the first page survived");
}

#[test]
fn the_exemption_follows_the_zoom_the_frame_painted_with() {
    // 6.0000005 is a different f32 from 6.0, and a viewport that computes its
    // zoom twice can land either side of that. The exemption is built from the
    // keys `get` and `insert` are called with, so there is no second value to
    // disagree with: a frame declaring 6.0 while painting this left both pages
    // unpinned and re-rendered them on every one of five steady frames.
    const ZOOM_ULP: f32 = 6.000_000_5;
    assert_ne!(ZOOM_ULP.to_bits(), 6.0f32.to_bits(), "same f32, no probe");
    assert!(
        2 * page_bytes(ZOOM_ULP) > TileStore::DEFAULT_BUDGET_BYTES,
        "the probe is pointless unless the spread really does not fit"
    );

    let mut store = TileStore::new();
    assert_eq!(frame(&mut store, &[0, 1], ZOOM_ULP), 2, "the first frame");
    for _ in 0..5 {
        assert_eq!(
            frame(&mut store, &[0, 1], ZOOM_ULP),
            0,
            "a visible page was evicted and had to be rendered again"
        );
    }
    assert!(store.over_budget() > 0, "and the overage is reported");
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

/// The probe the review asked for: hold a visible set steady for six frames at
/// each zoom and count what the store made the caller redo. Run it with
/// `--nocapture` to read the table.
#[test]
fn a_steady_visible_set_costs_nothing_to_repaint_at_any_zoom() {
    println!(
        "\n pages  zoom  page MiB  visible MiB  budget MiB  over MiB  re-renders  recomposites"
    );

    for visible_count in [2usize, 3] {
        for zoom in [2.0f32, 4.0, 5.0, 6.0, 8.0] {
            let visible: Vec<usize> = (0..visible_count).collect();
            let tiles_per_page = {
                let (w, h) = page_size(zoom);
                (w.div_ceil(TILE_SIZE) * h.div_ceil(TILE_SIZE)) as u64
            };

            let mut store = TileStore::new();
            frame(&mut store, &visible, zoom);

            let mut rerendered = 0;
            for _ in 0..5 {
                rerendered += frame(&mut store, &visible, zoom);
            }
            let recomposited: u64 = visible
                .iter()
                .map(|&index| {
                    store
                        .get(index, zoom)
                        .expect("visible and pinned")
                        .composites()
                        - tiles_per_page
                })
                .sum();

            let mib = |bytes: usize| bytes as f64 / (1024.0 * 1024.0);
            println!(
                "{visible_count:6}  {zoom:4.0}  {:8.1}  {:11.1}  {:10.1}  {:8.1}  {rerendered:10}  {recomposited:12}",
                mib(page_bytes(zoom)),
                mib(visible_count * page_bytes(zoom)),
                mib(TileStore::DEFAULT_BUDGET_BYTES),
                mib(store.over_budget()),
            );

            // Whether or not the visible set fits, nothing on screen may be
            // rendered or composited twice. What varies is the overage.
            assert_eq!(
                rerendered, 0,
                "{visible_count} pages at {zoom}x re-rendered"
            );
            assert_eq!(
                recomposited, 0,
                "{visible_count} pages at {zoom}x recomposited"
            );
            let fits = visible_count * page_bytes(zoom) <= TileStore::DEFAULT_BUDGET_BYTES;
            assert_eq!(
                store.over_budget() == 0,
                fits,
                "{visible_count} pages at {zoom}x: overage does not match what fits"
            );
        }
    }
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
