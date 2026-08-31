//! Decision 11's second budget: **something visible under 200 ms on any
//! page**, with the full raster completing on a background thread.
//!
//! The budget exists because the first one cannot be met by rendering. A
//! correct transparency-heavy page costs hundreds of milliseconds in a CPU
//! interpreter, and no amount of laziness changes that, so what a viewer owes
//! the reader in 200 ms is a correctly sized, correctly placed page, not its
//! pixels. `PagePlaceholder` is that promise and this is where it is measured:
//! on `external/hayro-corpus/0041790.pdf`, whose first page is the heaviest
//! the M1 spike found, and on the last page of the thousand-page bench file,
//! where nothing before it has been touched.
//!
//! Three things are asserted, because the clock alone would pass on a
//! placeholder that was blank, see-through, or in fact the raster:
//!
//! * the first response is a placeholder, not a raster, so what appears is
//!   what the reader gets to see before the interpreter has run;
//! * it is opaque and not empty, so there is something to look at;
//! * it is the size the raster turns out to be, so the canvas can paint it as
//!   the page rather than as a guess.
//!
//! Only the last of those can fail against a working renderer; the first two
//! fail against a change that hollowed the placeholder out, which is what they
//! are for. The cost of the raster itself is reported, not budgeted: a render
//! that got faster is good news and a bench that failed on it would be a trap.

mod harness;

use std::time::{Duration, Instant};

use onionskin_core::{Document, RenderRequest, RenderResponse};

/// PLAN.md decision 11: something visible, on any page, in this long.
const FIRST_PAINT: Duration = Duration::from_millis(200);

/// A raster that never arrives would leave the placeholder assertions vacuous,
/// so the wait is bounded. Not a budget: the whole point is that this can be
/// slow.
const RASTER_DEADLINE: Duration = Duration::from_secs(120);

/// Long enough not to spend a core the renderer wants, short enough not to
/// show up in the first-paint measurement, which is milliseconds.
const POLL_INTERVAL: Duration = Duration::from_micros(50);

/// Actual size on a retina display, which is what a viewer rasterizes at.
///
/// The zoom decides whether this bench measures anything. Rasterizing costs
/// roughly the square of it: the heavy page takes about 180 ms at 1.0, which
/// is inside the 200 ms budget, and about 230 ms here, which is not. Measured
/// at 1.0 the whole thing quietly became a claim that hayro meets the budget
/// on its own, and the placeholder it is supposed to be about would have been
/// beside the point.
const ZOOM: f32 = 2.0;

fn main() {
    if let Some(path) = harness::heavy_document() {
        first_paint(&path, "the heaviest page in the corpus", 0);
    }
    if let Some(path) = harness::bench_document() {
        // Nothing before page 1000 has been parsed, rendered or measured: the
        // first paint budget holds at the far end of a long document too.
        first_paint(&path, "the last page of a thousand", 999);
    }
}

/// Open the document and paint `page`, cold, the way the shell does.
fn first_paint(path: &std::path::Path, what: &str, page: usize) {
    harness::heading(&format!("first paint: {what}"));
    let started = Instant::now();
    let mut document = Document::open_path(path).expect("the document opens");
    assert!(
        page < document.page_count(),
        "{} has {} pages, so page {} is not in it",
        path.display(),
        document.page_count(),
        page + 1
    );
    document
        .request_render(
            RenderRequest {
                page,
                zoom: ZOOM,
                generation: 1,
            },
            None,
        )
        .expect("the render is queued");

    let (placeholder, visible_at) = loop {
        match document.try_render_response().expect("the worker is alive") {
            Some(RenderResponse::Placeholder(placeholder)) => {
                break (placeholder, started.elapsed())
            }
            Some(RenderResponse::Raster { .. }) => {
                panic!("the raster arrived before anything was visible")
            }
            Some(RenderResponse::Failed { error, .. }) => panic!("page {page}: {error}"),
            None => {
                assert!(
                    started.elapsed() < RASTER_DEADLINE,
                    "nothing became visible at all"
                );
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    };

    println!(
        "  placeholder {}x{} on {} background",
        placeholder.width,
        placeholder.height,
        if placeholder.background.a == 255 {
            "an opaque"
        } else {
            "a transparent"
        }
    );
    // Neither of these can fail against today's code, and both would against a
    // change that gutted the placeholder: an empty one is nothing visible, and
    // a transparent one shows the last document through the new one.
    assert!(
        placeholder.width > 0 && placeholder.height > 0,
        "an empty placeholder is nothing visible"
    );
    assert_eq!(
        placeholder.background.a, 255,
        "the placeholder is see-through"
    );
    harness::under_time("time to first paint", visible_at, FIRST_PAINT);

    let render = loop {
        match document.try_render_response().expect("the worker is alive") {
            Some(RenderResponse::Raster { render, .. }) => break render,
            Some(RenderResponse::Failed { error, .. }) => panic!("page {page}: {error}"),
            Some(RenderResponse::Placeholder(_)) => panic!("a second placeholder for one request"),
            None => {
                assert!(
                    started.elapsed() < RASTER_DEADLINE,
                    "the raster never arrived, so nothing proves it was backgrounded"
                );
                // Polling flat out would spend a core the renderer wants.
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    };
    let raster_at = started.elapsed();

    // The one assertion here that a slow renderer cannot satisfy by accident:
    // the placeholder has to be the page's size, or the canvas is painting a
    // rectangle that is not where the page will be, and the reader watches it
    // jump when the raster lands.
    assert_eq!(
        (render.raster.width(), render.raster.height()),
        (placeholder.width, placeholder.height),
        "the placeholder was not the size of the page it stood in for"
    );
    let ratio = raster_at.as_secs_f64() / visible_at.as_secs_f64().max(f64::EPSILON);
    println!(
        "  raster at {raster_at:?}, {ratio:.1}x the time to first paint; {} interpreter warnings",
        render.warnings.len()
    );
    // Not a budget: rendering getting faster is good news, and a bench that
    // failed on it would be a trap. But this ratio is the reason the budget is
    // stated about a placeholder at all, so a run where it approaches one is a
    // run where this bench has stopped measuring anything, and it is printed
    // rather than buried for exactly that reason.
    if ratio < 2.0 {
        println!(
            "  NOTE: the raster is barely slower than the placeholder, so this page no longer \
             shows why first paint is separate from rendering"
        );
    }
}
