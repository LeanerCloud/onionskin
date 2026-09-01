//! Decision 11's remaining two budgets: **60 fps scroll** and **memory
//! proportional to the pages being viewed**.
//!
//! A scripted continuous scroll across two hundred pages of the thousand-page
//! bench file, headless: `core`'s layout and viewport place the pages, the
//! render worker rasterizes them on its own thread, and `render`'s `TileStore`
//! caches and evicts what comes back. There is no window and no `app`; P6a put
//! the layout and viewport maths in `core` so this could be driven without
//! one.
//!
//! What is timed is the frame path: laying the visible pages out, asking for
//! what is missing, compositing the tiles under the viewport, and taking
//! delivery of the rasters that arrived since the last frame, which is where
//! eviction runs. All of that is on the thread that would be painting.
//! Rasterizing is not in it, by decision 11: the worker owns that and the
//! canvas paints a placeholder until it lands (see `benches/paint.rs`).
//! Between frames the run waits for the worker to catch up, untimed, standing
//! in for the frames a shell would spend painting placeholders; without it the
//! loop outruns the renderer by three orders of magnitude and the memory
//! budget would be measured against an empty store.
//!
//! Two of the four budgets here are counts rather than clocks, because a
//! shared CI runner has an opinion about clocks and none about arithmetic: the
//! pages a frame lays out, and the tiles it composites.
//!
//! `crates/render/tests/eviction.rs` already pins the eviction policy and the
//! composite cache in isolation, over two hundred hand-fed rasters at a fixed
//! zoom. This is the same two budgets with nothing hand-fed: the pages come
//! from a real document, the zoom is the one fit-width chose, the visible set
//! is whatever `core`'s layout says it is, and the rasters are hayro's. A
//! policy that is correct against a fabricated working set and wrong against
//! the real one is exactly what a unit test cannot see.

mod harness;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use onionskin_core::{
    Document, FitMode, PageGeometryResponse, PageIndex, PagePlacement, RenderRequest,
    RenderResponse, ViewPoint, ViewSize, Viewport,
};
use onionskin_render::{TileStore, TILE_SIZE};

use harness::CompositeTally;

/// A window a document would be read in. The budgets are stated against this
/// size because the visible page count and the tile count both follow from it.
const VIEWPORT: ViewSize = ViewSize {
    width: 1400.0,
    height: 900.0,
};
const PAGE_GAP: f32 = 12.0;

/// Pages crossed, from the plan's memory budget: "resident tile bytes after
/// scrolling 200 pages".
const PAGES_TO_CROSS: usize = 200;
/// Frames the crossing is spread over. Three per page is a fast fling, which
/// is the hard case for both budgets: every page enters the layout, the
/// worker and the store, and only a few frames later leaves them again.
const FRAMES: usize = 600;

/// 60 fps, as decision 11 states it, applied to the 99th percentile frame.
///
/// The percentile is where the budget belongs and the maximum is not: this
/// runs on shared CI runners, where any single frame can be taken away by the
/// scheduler, and a gate that a stolen timeslice can fail is a gate that gets
/// turned off. Six of six hundred frames are allowed to miss.
const FRAME_BUDGET: Duration = Duration::from_micros(16_667);
/// The worst single frame, held to a stall a reader would see rather than to
/// the frame rate: six frames' worth. This catches a frame that went and did
/// something O(document) while the percentile above catches the frame rate.
const WORST_FRAME_BUDGET: Duration = Duration::from_millis(100);

/// How long the whole scroll may take before it is called stuck. Two hundred
/// pages of this file rasterize in a couple of seconds here and should have an
/// order of magnitude of room on a slower runner; anything past this is a
/// worker that stopped, not a worker that is behind.
const RUN_DEADLINE: Duration = Duration::from_secs(300);

/// Rows laid out on each side of the visible band (`core::layout::GUARD_ROWS`),
/// so a scroll of less than a row still has a placed page to paint.
const GUARD_ROWS: usize = 1;

/// US Letter, the size of every page in the bench file.
const PAGE_POINTS: (f32, f32) = (612.0, 792.0);

fn main() {
    let Some(path) = harness::bench_document() else {
        return;
    };
    let mut run = Run::start(&path);
    run.scroll();
    run.report();
}

struct Run {
    document: Document,
    viewport: Viewport,
    store: TileStore,
    zoom: f32,
    step: f32,
    /// When the whole run gives up, rather than a deadline per frame: six
    /// hundred frames each allowed their own patience is no bound at all.
    deadline: Instant,
    /// Pages asked for and not yet delivered. A page evicted after delivery
    /// leaves this set, so it is asked for again the next time it is on
    /// screen, which is what a canvas does and what keeps the wait below from
    /// blocking forever on a raster nobody will send twice.
    outstanding: BTreeSet<PageIndex>,
    /// Tiles composited per page across every cache incarnation.
    composited: CompositeTally<PageIndex>,
    /// Cache incarnation currently installed for each page.
    cache_incarnations: BTreeMap<PageIndex, u64>,
    /// Tiles of the pages that were painted at all: the ceiling the composite
    /// count is judged against.
    tiles_painted: BTreeMap<PageIndex, u64>,
    fetches: u64,
    frames: Vec<Duration>,
    widest_frame: usize,
    inserted_pages: usize,
    inserted_bytes: usize,
    peak_resident: usize,
    waiting: Duration,
}

impl Run {
    fn start(path: &std::path::Path) -> Self {
        let mut document = Document::open_path(path).expect("the bench file opens");
        let mut viewport = Viewport::new(document.page_count(), VIEWPORT, PAGE_GAP)
            .expect("the viewport is valid");
        let geometry = document.page_geometry(0).expect("page 0 geometry").clone();
        viewport.measure_page(geometry).expect("page 0 is measured");
        viewport.fit(FitMode::Width).expect("fit width");
        let zoom = viewport.zoom();

        let stride = PAGE_POINTS.1 * zoom + PAGE_GAP;
        Run {
            document,
            viewport,
            store: TileStore::new(),
            zoom,
            step: stride * PAGES_TO_CROSS as f32 / FRAMES as f32,
            deadline: Instant::now() + RUN_DEADLINE,
            outstanding: BTreeSet::new(),
            composited: CompositeTally::default(),
            cache_incarnations: BTreeMap::new(),
            tiles_painted: BTreeMap::new(),
            fetches: 0,
            frames: Vec::with_capacity(FRAMES),
            widest_frame: 0,
            inserted_pages: 0,
            inserted_bytes: 0,
            peak_resident: 0,
            waiting: Duration::ZERO,
        }
    }

    fn scroll(&mut self) {
        harness::heading("scroll: a continuous scroll across two hundred pages");
        println!(
            "  {FRAMES} frames of {:.0} px at zoom {:.3}, viewport {}x{}",
            self.step, self.zoom, VIEWPORT.width, VIEWPORT.height
        );
        let run = Instant::now();
        for _ in 0..FRAMES {
            let started = Instant::now();
            self.frame();
            self.frames.push(started.elapsed());

            self.wait_for_the_worker();
            // Every page is US Letter, so fitting the width of a newly
            // measured one must land on the zoom the rasters were made at. A
            // zoom that moved would miss every cache in the store silently and
            // leave this bench measuring an empty scroll.
            assert_eq!(
                self.viewport.zoom(),
                self.zoom,
                "fit width moved the zoom mid-scroll"
            );
            self.viewport
                .scroll(
                    ViewPoint {
                        x: 0.0,
                        y: -self.step,
                    },
                    false,
                    ViewPoint::default(),
                )
                .expect("the scroll is valid");
        }
        println!(
            "  crossed to page {} in {:?}, of which {:?} waiting for the worker",
            self.viewport.current_page() + 1,
            run.elapsed(),
            self.waiting
        );
    }

    /// One frame: place the visible pages, ask for what is missing, paint the
    /// tiles under the viewport, take in whatever the worker has finished.
    fn frame(&mut self) {
        self.store.begin_frame();
        let visible = self
            .viewport
            .visible_pages()
            .expect("the layout places pages");
        self.widest_frame = self.widest_frame.max(visible.len());
        let _ = self.ask(&visible);

        for placement in &visible {
            let page = placement.page;
            let Some(cache) = self.store.get(page, self.zoom) else {
                continue;
            };
            let (cols, rows) = (cache.cols(), cache.rows());
            let raster = (cache.base().width() as f32, cache.base().height() as f32);
            assert!(
                (raster.0 - placement.rect.size.width).abs() <= 1.5
                    && (raster.1 - placement.rect.size.height).abs() <= 1.5,
                "page {page} is laid out {:?} and rasterized {raster:?}",
                placement.rect.size
            );
            let Some((col0, row0, col1, row1)) = visible_tiles(*placement, cols, rows) else {
                continue;
            };
            for row in row0..=row1 {
                for col in col0..=col1 {
                    let _ = cache.tile(col, row);
                    self.fetches += 1;
                }
            }
            let incarnation = *self
                .cache_incarnations
                .get(&page)
                .expect("a resident cache has an incarnation");
            self.composited
                .observe(page, incarnation, cache.composites());
            self.tiles_painted
                .insert(page, u64::from(cols) * u64::from(rows));
        }

        self.take_responses();
        // Sampled before the frame closes, which is the only moment the store
        // is allowed to be over its budget: `end_frame` evicts back under it,
        // so a reading taken afterwards is the budget restated rather than a
        // measurement. What a process actually has to hold is this.
        self.peak_resident = self.peak_resident.max(self.store.resident_bytes());
        self.store.end_frame();
    }

    /// Ask the worker for what the visible pages still lack: geometry while
    /// the layout is still estimating them, then a raster once it is in.
    /// Answers whether they now have both, which is what the wait between
    /// frames is waiting for.
    ///
    /// Asking and checking are one pass because the check is
    /// [`TileStore::get`], the only presence test the store exposes, and it is
    /// `&mut self`: it touches recency and evicts to budget on every call.
    /// Doing that twice per page per poll would have the bench spending its
    /// wait rearranging the store it is measuring.
    fn ask(&mut self, visible: &[PagePlacement]) -> bool {
        let mut ready = true;
        for placement in visible {
            let page = placement.page;
            let Some(geometry) = self.viewport.page_geometry(page) else {
                // `core` drops a request for a page already in flight, so
                // asking again on the next frame costs nothing and needs no
                // bookkeeping here.
                self.document
                    .request_page_geometry(page)
                    .expect("the geometry request is queued");
                ready = false;
                continue;
            };
            if self.store.get(page, self.zoom).is_some() {
                continue;
            }
            ready = false;
            if self.outstanding.insert(page) {
                let geometry = geometry.clone();
                self.document
                    .request_render_with_geometry(
                        RenderRequest {
                            page,
                            zoom: self.zoom,
                            generation: 1,
                        },
                        &geometry,
                        None,
                    )
                    .expect("the render request is queued");
            }
        }
        ready
    }

    fn take_responses(&mut self) {
        while let Some(response) = self
            .document
            .try_page_geometry_response()
            .expect("the worker is alive")
        {
            match response {
                PageGeometryResponse::Ready(geometry) => {
                    self.viewport.measure_page(geometry).expect("measured");
                }
                PageGeometryResponse::Failed { page, error } => {
                    panic!("page {page} geometry: {error}")
                }
            }
        }
        while let Some(response) = self
            .document
            .try_render_response()
            .expect("the worker is alive")
        {
            match response {
                RenderResponse::Raster { request, render } => {
                    self.outstanding.remove(&request.page);
                    self.inserted_bytes += render.raster.rgba().len();
                    self.inserted_pages += 1;
                    let incarnation = self.cache_incarnations.entry(request.page).or_default();
                    *incarnation = incarnation
                        .checked_add(1)
                        .expect("cache incarnation overflowed");
                    self.store.insert(request.page, render.raster);
                }
                RenderResponse::Failed { request, error } => {
                    panic!("page {} render: {error}", request.page)
                }
                RenderResponse::Placeholder(_) => {}
            }
        }
    }

    /// Untimed: let the worker catch up with the pages this frame showed.
    fn wait_for_the_worker(&mut self) {
        let started = Instant::now();
        loop {
            let visible = self
                .viewport
                .visible_pages()
                .expect("the layout places pages");
            self.take_responses();
            if self.ask(&visible) {
                break;
            }
            assert!(
                Instant::now() < self.deadline,
                "the render worker never caught up with the scroll"
            );
            // Polling flat out would spend a core the renderer wants, which on
            // a four-core shared runner is the difference between waiting and
            // competing.
            std::thread::sleep(Duration::from_micros(250));
        }
        self.waiting += started.elapsed();
    }

    fn report(&mut self) {
        self.frames.sort_unstable();
        let total: Duration = self.frames.iter().sum();
        let mean = total / self.frames.len() as u32;
        let p99 = self.frames[self.frames.len() * 99 / 100];
        let worst = self.frames[self.frames.len() - 1];
        println!("  mean frame {mean:?}");
        harness::under_time("99th percentile frame", p99, FRAME_BUDGET);
        harness::under_time("worst frame", worst, WORST_FRAME_BUDGET);

        // A row of pages plus the guard row on each side, and no more: the
        // layout must place what is on screen, not what is in the document.
        let stride = PAGE_POINTS.1 * self.zoom + PAGE_GAP;
        let rows_on_screen = (VIEWPORT.height / stride).ceil() as usize + 1;
        harness::under_count(
            "pages laid out in one frame",
            self.widest_frame,
            rows_on_screen + 2 * GUARD_ROWS,
            "pages",
        );

        // Every tile is composited at most once per page visited. The store
        // pins the pages of the frame in progress, so a page cannot be evicted
        // while it is being painted and composited again on the next frame; the
        // frame-pinning bug that `TileStore::begin_frame` documents having had
        // is exactly what pushes this number towards the fetch count, and this
        // is the integration-level tripwire for it coming back.
        let composited = self.composited.total();
        let ceiling: u64 = self.tiles_painted.values().sum();
        println!(
            "  {} tile fetches over {} pages, {:.2} per composite",
            self.fetches,
            self.composited.pages(),
            self.fetches as f64 / composited.max(1) as f64
        );
        harness::under_count(
            "tiles composited",
            composited as usize,
            ceiling as usize,
            "tiles",
        );

        assert!(
            self.viewport.current_page() + 1 >= PAGES_TO_CROSS,
            "the scroll stopped at page {}, short of the {PAGES_TO_CROSS} the memory budget is \
             stated over",
            self.viewport.current_page() + 1
        );
        // Without this the budget below would pass on a store nothing reached.
        assert!(
            self.inserted_bytes > 4 * self.store.budget(),
            "only {} MiB of rasters went through a {} MiB budget, so nothing was evicted",
            self.inserted_bytes / (1024 * 1024),
            self.store.budget() / (1024 * 1024)
        );
        println!(
            "  {} pages rasterized, {} MiB of them; {} caches resident at the end",
            self.inserted_pages,
            self.inserted_bytes / (1024 * 1024),
            self.store.len()
        );

        // Two hundred pages went through a store that ends holding a dozen, so
        // the memory a reader pays is the pages being viewed and not the pages
        // that have been viewed. `crates/render/tests/eviction.rs` pins the
        // policy itself; what it cannot do is drive it from a real layout at a
        // zoom the layout chose, which is where a working set that does not fit
        // the assumptions would show up.
        //
        // The budget is what a frame is allowed to hold *while painting*, and
        // that is deliberately not `TileStore::budget()`: the store evicts back
        // under it as each frame closes, so measuring after the fact would only
        // restate its loop condition. A frame may exceed it by its own pages,
        // which the store documents and refuses to evict, and that is the
        // allowance stated here.
        let frame_pages = self.widest_frame * self.page_bytes();
        harness::under_bytes(
            "peak resident tile bytes while painting",
            self.peak_resident as u64,
            (self.store.budget() + frame_pages) as u64,
        );
        // The plan's wording, and the store's own postcondition seen from
        // outside: whatever a frame needed while it painted, what is left
        // between frames is inside the budget.
        harness::under_bytes(
            "resident tile bytes after two hundred pages",
            self.store.resident_bytes() as u64,
            self.store.budget() as u64,
        );
    }

    /// What one page of this document costs the store at this zoom: its base
    /// raster, plus a composited tile for every tile of its grid.
    fn page_bytes(&self) -> usize {
        let (width, height) = (
            (PAGE_POINTS.0 * self.zoom) as u32,
            (PAGE_POINTS.1 * self.zoom) as u32,
        );
        let tiles = (width.div_ceil(TILE_SIZE) * height.div_ceil(TILE_SIZE)) as usize;
        let tile_bytes = (TILE_SIZE * TILE_SIZE) as usize * 4;
        width as usize * height as usize * 4 + tiles * tile_bytes
    }
}

/// The tiles of `placement` that overlap the viewport, as `(col0, row0, col1,
/// row1)`. What a canvas paints: never the whole page, only the band on
/// screen. `None` for a guard row, which is laid out and painted by nobody.
fn visible_tiles(placement: PagePlacement, cols: u32, rows: u32) -> Option<(u32, u32, u32, u32)> {
    let rect = placement.rect;
    let (col0, col1) = tile_span(
        -rect.origin.x,
        VIEWPORT.width - rect.origin.x,
        rect.size.width,
        cols,
    )?;
    let (row0, row1) = tile_span(
        -rect.origin.y,
        VIEWPORT.height - rect.origin.y,
        rect.size.height,
        rows,
    )?;
    Some((col0, row0, col1, row1))
}

/// The tile indices covering `[low, high]` of a page axis `extent` long.
fn tile_span(low: f32, high: f32, extent: f32, tiles: u32) -> Option<(u32, u32)> {
    let tile = TILE_SIZE as f32;
    let low = low.clamp(0.0, extent);
    let high = high.clamp(0.0, extent);
    if high <= low {
        return None;
    }
    let first = ((low / tile).floor() as u32).min(tiles - 1);
    let last = (((high / tile).ceil() as u32).max(1) - 1).clamp(first, tiles - 1);
    Some((first, last))
}
