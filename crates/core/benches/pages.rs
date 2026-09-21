//! What a page-tree rewrite costs on the thousand-page file, in time and in
//! the size of the section it appends.
//!
//! **The section size is the number this exists for.** T5's free-nothing rule
//! means a page-set change rewrites every surviving page dict - materialized,
//! re-parented - and appends all of them. The plan leaves open whether that is
//! small enough to live with or needs a fallback, and refuses to guess; this
//! is where the guess would have gone.
//!
//! Two operations, because they differ in what they touch: deleting one page
//! rewrites the other 999, and reversing the order rewrites all 1000 plus every
//! fix-up that walks a page list.
//!
//! Like the other benches, it asserts rather than prints.

mod harness;

use std::time::{Duration, Instant};

use onionskin_core::pages::{rewrite_page_tree, PageSource};
use onionskin_core::{read_structure, EditSession};
use onionskin_cos::{BytesSource, Document as CosDocument};

/// One rewrite of the thousand-page tree. Chosen with room for a slower
/// machine; the point is to fail when the rewrite stops being linear in pages.
const REWRITE_BUDGET: Duration = Duration::from_millis(1500);

/// The appended section per surviving page. A rewritten page dict is its
/// entries plus an xref row, a few hundred bytes; inlining a shared
/// `/Resources` into each would multiply this by the size of the resource
/// dictionary, which is precisely the bug the raw walk exists to avoid.
const BYTES_PER_PAGE: u64 = 1024;

fn main() {
    let Some(path) = harness::bench_document() else {
        return;
    };
    let bytes = std::fs::read(&path).expect("the bench file is readable");
    let base =
        CosDocument::open(Box::new(BytesSource::new(bytes.clone()))).expect("the bench file opens");
    let count = base.page_count().expect("pages") as usize;

    for (label, order) in [
        (
            "delete the first page",
            (1..count).map(PageSource::Existing).collect::<Vec<_>>(),
        ),
        (
            "reverse every page",
            (0..count).rev().map(PageSource::Existing).collect(),
        ),
    ] {
        let (elapsed, section) = rewrite(&base, &order);
        harness::heading(&format!("page-tree rewrite: {label}, {count} pages"));
        println!(
            "  {elapsed:?}, section {section} bytes ({} bytes per surviving page, on a {} byte file)",
            section / order.len() as u64,
            bytes.len()
        );
        harness::under_time(label, elapsed, REWRITE_BUDGET);
        harness::under_bytes(
            &format!("{label}: appended section"),
            section,
            BYTES_PER_PAGE * order.len() as u64,
        );
    }
}

fn rewrite(base: &CosDocument, order: &[PageSource]) -> (Duration, u64) {
    let structure = read_structure(base).expect("the structure reads");
    let mut edit = EditSession::for_base(base);
    let started = Instant::now();
    edit.transact(base, "Rewrite Pages", |tx| {
        rewrite_page_tree(tx, &structure, order)?;
        Ok(())
    })
    .expect("the rewrite commits");
    let section = base
        .section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
        .expect("a section");
    (started.elapsed(), section.len() as u64)
}
