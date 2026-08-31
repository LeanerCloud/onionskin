//! P9's document-level search: off the calling thread, incremental, agreeing
//! with the per-page search, and loud about a page it could not read.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use onionskin_core::{Document, SearchMatch, SearchOptions};

/// Enough pages that the walk cannot plausibly finish between the call
/// returning and the first poll, which is what makes the incremental
/// assertions meaningful rather than lucky.
const PAGES: usize = 200;
const DEADLINE: Duration = Duration::from_secs(60);

#[test]
fn the_call_returns_before_the_walk_does_and_results_arrive_page_by_page() {
    let mut doc = Document::open_bytes(multi_page_pdf(PAGES, false)).expect("fixture opens");

    let started = Instant::now();
    assert!(doc
        .start_search("alpha", SearchOptions::default(), 0)
        .expect("the search worker starts"));
    let returned = started.elapsed();
    // Nothing has been applied yet: the walk belongs to the worker, and the
    // session only learns about it when the caller asks.
    assert_eq!(doc.search().searched_pages(), 0);
    assert!(doc.search().is_running());

    let mut partial = false;
    let deadline = Instant::now() + DEADLINE;
    while doc.search().is_running() {
        assert!(Instant::now() < deadline, "the walk never finished");
        doc.poll_search();
        let seen = doc.search().searched_pages();
        partial |= seen > 0 && seen < PAGES;
    }
    let total = started.elapsed();

    assert!(
        partial,
        "every page arrived in one poll, so nothing proves the results stream"
    );
    assert!(
        returned * 4 < total,
        "starting the search cost {returned:?} of the walk's {total:?}, which is not off-thread"
    );
    assert_eq!(doc.search().searched_pages(), PAGES);
}

#[test]
fn every_page_agrees_with_the_per_page_search() {
    let mut doc = Document::open_bytes(multi_page_pdf(PAGES, false)).expect("fixture opens");
    let options = SearchOptions::default();

    let expected: Vec<Vec<SearchMatch>> = (0..PAGES)
        .map(|page| {
            doc.search_page(page, "alpha", options)
                .expect("per-page search succeeds")
        })
        .collect();

    drain(&mut doc, "alpha", options, 0);

    for (page, hits) in expected.iter().enumerate() {
        assert_eq!(
            doc.search().matches_on(page),
            hits.as_slice(),
            "page {page} disagrees with content::search"
        );
    }
    assert_eq!(
        doc.search().len(),
        expected.iter().map(Vec::len).sum::<usize>()
    );
    assert!(doc.search().failures().is_empty());
}

#[test]
fn the_walk_starts_at_the_page_being_viewed_and_wraps_to_cover_the_rest() {
    let mut doc = Document::open_bytes(multi_page_pdf(PAGES, false)).expect("fixture opens");
    let options = SearchOptions::default();

    // Page 7 carries a word no other page does, so a walk that started at page
    // 0 would put the cursor somewhere else first.
    doc.start_search("marker007", options, 7)
        .expect("the search worker starts");
    let deadline = Instant::now() + DEADLINE;
    while doc.search().current().is_none() {
        assert!(Instant::now() < deadline, "the first hit never arrived");
        doc.poll_search();
    }

    assert_eq!(doc.search().current().map(|hit| hit.page), Some(7));
    drain_running(&mut doc);
    assert_eq!(doc.search().searched_pages(), PAGES);
    assert_eq!(doc.search().len(), 1);
}

#[test]
fn a_page_that_cannot_be_read_is_reported_rather_than_skipped() {
    let held = 8;
    let mut doc = Document::open_bytes(multi_page_pdf(held, true)).expect("fixture opens");
    let counted = held + 1;
    assert_eq!(doc.page_count(), counted);
    assert!(
        doc.search_page(held, "alpha", SearchOptions::default())
            .is_err(),
        "the fixture's last page must be unreadable for this test to mean anything"
    );

    drain(&mut doc, "alpha", SearchOptions::default(), 0);

    assert_eq!(doc.search().searched_pages(), counted);
    assert_eq!(doc.search().failures().len(), 1);
    assert_eq!(doc.search().failures()[0].page, held);
    assert!(doc.search().failures()[0]
        .to_string()
        .starts_with("page 9:"));
    // The pages that do read still produced their hits.
    assert!(!doc.search().is_empty());
}

#[test]
fn a_new_query_abandons_the_walk_in_flight() {
    let mut doc = Document::open_bytes(multi_page_pdf(PAGES, false)).expect("fixture opens");
    let options = SearchOptions::default();

    doc.start_search("alpha", options, 0)
        .expect("the first search starts");
    assert!(doc
        .start_search("marker007", options, 0)
        .expect("the second search starts"));
    drain_running(&mut doc);

    assert_eq!(doc.search().needle(), "marker007");
    assert_eq!(doc.search().len(), 1);
    assert_eq!(doc.search().searched_pages(), PAGES);
    // Repeating the query it already answered does not restart the walk.
    assert!(!doc
        .start_search("marker007", options, 0)
        .expect("an unchanged query is not an error"));
    assert!(!doc.search().is_running());
}

/// The same supersede, but with the first walk demonstrably streaming before
/// the second one starts.
///
/// `a_new_query_abandons_the_walk_in_flight` sends both queries back to back
/// and can win by a race: the worker often has not emitted a page before the
/// second request lands, so the generation filter it means to exercise is
/// never asked to drop anything. Here the first walk has already reported
/// pages, and its results are in the channel when the second query supersedes
/// it.
#[test]
fn results_from_a_superseded_walk_never_reach_the_state_that_replaced_it() {
    let mut doc = Document::open_bytes(multi_page_pdf(PAGES, false)).expect("fixture opens");
    let options = SearchOptions::default();

    doc.start_search("alpha", options, 0)
        .expect("the first search starts");
    let deadline = Instant::now() + DEADLINE;
    while doc.search().searched_pages() == 0 {
        assert!(Instant::now() < deadline, "the first walk reported nothing");
        doc.poll_search();
    }
    let abandoned_at = doc.search().searched_pages();
    assert!(
        abandoned_at < PAGES,
        "the first walk finished too early to supersede"
    );

    assert!(doc
        .start_search("marker007", options, 0)
        .expect("the second search starts"));
    // The count restarts with the walk. A page of the abandoned query counted
    // here would mean its results were applied under the new needle.
    assert_eq!(doc.search().searched_pages(), 0);

    let mut seen = 0;
    let deadline = Instant::now() + DEADLINE;
    while doc.search().is_running() {
        assert!(Instant::now() < deadline, "the walk never finished");
        doc.poll_search();
        let now = doc.search().searched_pages();
        assert!(
            now >= seen,
            "the searched count went backwards: {seen} then {now}"
        );
        assert!(
            now <= PAGES,
            "{now} pages searched in a {PAGES} page document"
        );
        seen = now;
    }

    assert_eq!(doc.search().needle(), "marker007");
    assert_eq!(doc.search().searched_pages(), PAGES);
    assert_eq!(doc.search().len(), 1, "marker007 is on exactly one page");
    assert_eq!(doc.search().current().map(|hit| hit.page), Some(7));
}

#[test]
fn closing_the_find_bar_clears_the_query_and_its_results() {
    let mut doc = Document::open_bytes(multi_page_pdf(16, false)).expect("fixture opens");
    drain(&mut doc, "alpha", SearchOptions::default(), 0);
    assert!(!doc.search().is_empty());

    doc.cancel_search();

    assert_eq!(doc.search().needle(), "");
    assert_eq!(doc.search().len(), 0);
    assert!(doc.search().current().is_none());
    assert!(!doc.search().is_running());
    // Polling after a cancel picks nothing up from the abandoned walk.
    doc.poll_search();
    assert_eq!(doc.search().searched_pages(), 0);
}

#[test]
fn an_empty_needle_searches_nothing() {
    let mut doc = Document::open_bytes(multi_page_pdf(4, false)).expect("fixture opens");

    assert!(!doc
        .start_search("", SearchOptions::default(), 0)
        .expect("an empty needle is not an error"));
    assert!(!doc.search().is_running());
    assert_eq!(doc.search().len(), 0);
}

#[test]
fn a_start_page_outside_the_document_is_refused() {
    let mut doc = Document::open_bytes(multi_page_pdf(4, false)).expect("fixture opens");

    assert!(doc
        .start_search("alpha", SearchOptions::default(), 4)
        .is_err());
}

/// The check the synthetic fixtures cannot make: real files, whose text comes
/// out of real fonts, agree page for page with the per-page search.
#[test]
fn corpus_documents_agree_with_the_per_page_search() {
    let Some(dir) = corpus_dir("external/hayro-corpus") else {
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the corpus directory is readable")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "pdf"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "{} holds no PDFs", dir.display());

    let options = SearchOptions::default();
    let mut checked = 0usize;
    for path in paths.iter().take(12) {
        let Ok(mut doc) = Document::open_path(path) else {
            continue;
        };
        let pages = doc.page_count().min(20);
        if pages == 0 {
            continue;
        }
        let expected: Vec<Vec<SearchMatch>> = (0..pages)
            .map(|page| doc.search_page(page, "e", options).unwrap_or_default())
            .collect();

        drain(&mut doc, "e", options, 0);

        for (page, hits) in expected.iter().enumerate() {
            assert_eq!(
                doc.search().matches_on(page),
                hits.as_slice(),
                "{} page {page} disagrees with content::search",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "no corpus file opened");
}

fn drain(doc: &mut Document, needle: &str, options: SearchOptions, start: usize) {
    doc.start_search(needle, options, start)
        .expect("the search worker starts");
    drain_running(doc);
}

fn drain_running(doc: &mut Document) {
    let deadline = Instant::now() + DEADLINE;
    while doc.search().is_running() {
        assert!(Instant::now() < deadline, "the walk never finished");
        doc.poll_search();
    }
}

/// `corpus/external` is gitignored, so it is absent from a fresh clone and from
/// CI. Say so loudly rather than reporting a pass that was never earned.
fn corpus_dir(relative: &str) -> Option<PathBuf> {
    let root = match std::env::var_os("ONIONSKIN_CORPUS") {
        Some(from_env) => PathBuf::from(from_env),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()?
            .join("corpus"),
    };
    let dir = root.join(relative);
    if dir.is_dir() {
        return Some(dir);
    }
    if std::env::var_os("ONIONSKIN_CORPUS_REQUIRED").is_some() {
        panic!("corpus required but {} is absent", dir.display());
    }
    eprintln!("SKIPPED: {} is absent (it is gitignored)", dir.display());
    None
}

/// `pages` pages of Helvetica text. Page `i` carries `i % 3` occurrences of
/// "alpha" and one zero-padded "marker{i}", so both the per-page counts and the page a hit
/// lands on are known without asking the code under test.
///
/// With `overcount`, the page tree's `/Count` claims one page more than the
/// tree holds, which is how a real file loses a page: the last index fails to
/// load and every other page still reads.
fn multi_page_pdf(pages: usize, overcount: bool) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = vec![
        Vec::new(), // 1: catalog, filled in below
        Vec::new(), // 2: page tree, filled in below
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    let mut kids = Vec::new();
    for page in 0..pages {
        let page_number = objects.len() + 1;
        let content_number = objects.len() + 2;
        kids.push(format!("{page_number} 0 R"));
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents {content_number} 0 R >>"
            )
            .into_bytes(),
        );
        objects.push(stream(
            format!(
                "BT /F1 12 Tf 20 100 Td ({}marker{page:03}) Tj ET",
                "alpha ".repeat(page % 3)
            )
            .as_bytes(),
        ));
    }
    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages + usize::from(overcount)
    )
    .into_bytes();

    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

fn stream(data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}
