//! Decision 11's first budget: **time to first page under 200 ms** on the
//! thousand-page bench file, guarding xref-driven laziness.
//!
//! Wall clock alone does not guard laziness: a parser that read the whole file
//! and indexed it would post a fine number on a fast machine with a warm page
//! cache, and would then fall over on the 66 MB file it was never measured
//! against. So both are measured, on `corpus/bench/pages-1000.pdf`, whose
//! shape is fixed by `corpus/make-bench.py`: a page tree branching by ten, a
//! thousand pages four levels down.
//!
//! The two measurements are of different layers, deliberately. The clock is
//! `core`'s, because decision 11's budget is about the viewer, and what a
//! reader waits for is the session: the file read, the cross-reference parsed,
//! the render worker up and hayro's document loaded. The byte counters are
//! `cos`'s, because that is the layer that does the reading;
//! `core::Document::open_path` reads the whole file into memory on purpose, so
//! that hayro and `cos` can share one buffer, and counting bytes there would
//! only ever report the file's size. Laziness in decision 11 is a claim about
//! parsing, and this is where parsing decides what to read.
//!
//! The byte budgets are absolute and derived from the file's structure.
//! `crates/cos/tests/pages.rs` already pins the *ratio* between one page and a
//! full walk; a ratio survives both sides growing together, which is exactly
//! the regression a budget is for.

mod harness;

use std::time::{Duration, Instant};

use onionskin_cos::{CountingSource, Document as CosDocument, FileSource, ReadStats};

/// PLAN.md decision 11: time to first page, on this file.
const TIME_TO_FIRST_PAGE: Duration = Duration::from_millis(200);

/// What one object costs to parse: a 64-byte probe for its `N G obj` header
/// and a 1 KiB window for its body, which `cos` grows only if the object does
/// not fit (`object_header_at` and `INITIAL_WINDOW`,
/// `crates/cos/src/reader.rs`). Every node of this file's page tree is a
/// two-line dictionary, so each one costs exactly one of each and a budget in
/// bytes is a budget in objects.
const OBJECT_READ: u64 = 1024 + 64;

/// Bytes in one classic cross-reference entry (ISO 32000-1, 7.5.4).
const XREF_ENTRY: u64 = 20;

/// The first window `cos` reads at a cross-reference section, quadrupled until
/// the section fits (`XREF_INITIAL_WINDOW`, `crates/cos/src/xref.rs`).
const XREF_WINDOW: u64 = 4096;

/// Bytes read looking for `%PDF-` (`MAX_HEADER_SEARCH`,
/// `crates/cos/src/reader.rs`) and for `startxref` (`TAIL_WINDOW`,
/// `crates/cos/src/document.rs`).
const HEADER_SEARCH: u64 = 4096;
const TAIL_WINDOW: u64 = 2048;

/// Objects an open resolves before it hands the document back: the catalog and
/// the page-tree root, checked to establish the file has a structure at all.
/// Doubled, as slack for one more indirection on either.
const OPEN_OBJECTS: u64 = 4;

/// An eighth on top of a derived count, so that these are budgets and not
/// equalities. The models below predict the measurements exactly, to the byte,
/// and a bench that fails on a parser reading one more probe would be a
/// tripwire rather than a budget. An eighth is still far inside the fifth the
/// plan mutation-tests against.
fn with_slack(derived: u64) -> u64 {
    derived + derived / 8
}

/// `corpus/make-bench.py` branches the page tree by ten, so a thousand pages
/// sit four levels below the catalog.
const BRANCH: usize = 10;
const PAGES: usize = 1000;

fn main() {
    let Some(path) = harness::bench_document() else {
        return;
    };

    time_to_first_page(&path);
    opening_reads_the_cross_reference_and_a_constant(&path);
    reaching_a_page_reads_its_path_and_nothing_else(&path, 0);
    reaching_a_page_reads_its_path_and_nothing_else(&path, PAGES - 1);
}

/// The budget as a user meets it: from a path on disk to geometry for page 1,
/// which is everything the canvas needs before it can place anything.
///
/// This is the whole session, not just `cos`: the bytes are read, the
/// cross-reference is parsed, the render worker is spawned and hayro has
/// loaded the document by the time `page_geometry` answers.
fn time_to_first_page(path: &std::path::Path) {
    harness::heading("lazy open: time to first page");
    let started = Instant::now();
    let mut document = onionskin_core::Document::open_path(path).expect("the bench file opens");
    let opened = started.elapsed();
    assert_eq!(document.page_count(), PAGES);
    let geometry = document.page_geometry(0).expect("page 0 geometry");
    assert_eq!(geometry.render_size, (612.0, 792.0));
    let elapsed = started.elapsed();

    println!(
        "  open {opened:?}, geometry for page 1 {:?}",
        elapsed - opened
    );
    harness::under_time("time to first page", elapsed, TIME_TO_FIRST_PAGE);
}

/// Opening reads the cross-reference and a constant, and no page.
///
/// The cross-reference is the one part of a file that has to be read whole, so
/// the budget is written around it: the table itself at 20 bytes an entry,
/// plus the windows `cos` grew through to find its extent (see
/// [`xref_reads`]). Everything else is fixed, and in particular nothing here
/// is a function of the page count, which is the property "opens instantly
/// whatever the size" rests on.
///
/// An earlier version of this budget called everything but the table a
/// constant. It was not: three quarters of that supposed constant was the
/// cross-reference being re-read by the window-growing loop, a term that grows
/// with the entry count. The budget passed on this file and would have drifted
/// out of true on a larger one.
fn opening_reads_the_cross_reference_and_a_constant(path: &std::path::Path) {
    harness::heading("lazy open: what opening reads");
    let (document, stats) = counted(path);
    let read = stats.total();
    let entries = document.xref().iter().count() as u64;
    let table = XREF_ENTRY * entries;
    let file = std::fs::metadata(path).expect("the bench file stats").len();

    println!(
        "  {entries} cross-reference entries, so a {table} byte table read in {} bytes of \
         windows; {read} bytes read of a {file} byte file ({:.1}%)",
        xref_reads(table),
        read as f64 / file as f64 * 100.0
    );
    harness::under_bytes(
        "bytes to open",
        read,
        with_slack(xref_reads(table) + HEADER_SEARCH + TAIL_WINDOW + OPEN_OBJECTS * OBJECT_READ),
    );
    // The largest single read bounds what the open held in memory at once. The
    // cross-reference is the biggest thing an open reads, and nothing may hold
    // more of the file than it at one time.
    harness::under_bytes(
        "largest single read while opening",
        stats.largest_read(),
        with_slack(table + OBJECT_READ),
    );
}

/// Bytes spent reading a cross-reference section whose table is `table` bytes.
///
/// `cos` does not know the section's extent before it parses it, so it reads a
/// window at the section's offset and quadruples it until the table fits. The
/// windows that were too small are paid for as well as the one that was not,
/// which for any table is under a third again.
fn xref_reads(table: u64) -> u64 {
    let mut window = XREF_WINDOW;
    let mut total = table;
    while window < table {
        total += window;
        window *= 4;
    }
    total
}

/// Reaching one page reads the objects on its path, and no others.
///
/// Page `p` costs the catalog, the four nodes from the root down to the leaf,
/// and the `/Count` of every sibling skipped on the way (see
/// [`objects_to_reach`]). Thirty-two objects for the last page of a thousand,
/// five for the first, and neither number moves if the document grows.
fn reaching_a_page_reads_its_path_and_nothing_else(path: &std::path::Path, page: usize) {
    harness::heading(&format!("lazy open: what reaching page {} reads", page + 1));
    let (document, stats) = counted(path);
    // Opening is its own budget, above.
    stats.reset();
    let started = Instant::now();
    let node = document.page(page).expect("the page resolves");
    let elapsed = started.elapsed();

    let objects = objects_to_reach(page);
    println!(
        "  page {} is object {}, reached in {elapsed:?} through {objects} objects",
        page + 1,
        node.objref.number
    );
    harness::under_bytes(
        &format!("bytes to reach page {}", page + 1),
        stats.total(),
        with_slack(objects * OBJECT_READ),
    );
}

/// Objects a correct lazy reader touches to answer `page(index)` on this file.
///
/// The catalog, then the four nodes on the path from the root to the leaf,
/// then the siblings skipped at each level: reaching page 999 skips nine
/// subtrees at the root, nine below that and nine leaves, so 1 + 4 + 27. The
/// digits of the index in base `BRANCH` are exactly those skip counts, because
/// the tree branches by ten and the leaves are in order.
fn objects_to_reach(index: usize) -> u64 {
    let mut levels = 0u64;
    let mut span = 1usize;
    while span < PAGES {
        span *= BRANCH;
        levels += 1;
    }
    let mut skipped = 0u64;
    let mut rest = index;
    for _ in 0..levels {
        skipped += (rest % BRANCH) as u64;
        rest /= BRANCH;
    }
    let path_nodes = levels + 1;
    1 + path_nodes + skipped
}

/// The bench file opened through a byte counter, from a file on disk rather
/// than from a buffer: what the counter sees is what the parser asked the
/// operating system for.
fn counted(path: &std::path::Path) -> (CosDocument, std::sync::Arc<ReadStats>) {
    let file = FileSource::open(path).expect("the bench file opens");
    let (counting, stats) = CountingSource::new(Box::new(file));
    let document = CosDocument::open(Box::new(counting)).expect("the bench file opens clean");
    (document, stats)
}
