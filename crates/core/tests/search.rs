//! P9's document-level search: off the calling thread, incremental, agreeing
//! with the per-page search, and loud about a page it could not read.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use onionskin_core::pages::{delete_pages, move_pages};
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
fn actual_text_worker_search_covers_every_member() {
    let content =
        "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (AB) Tj 1 0 0 1 100 50 Tm (CD) Tj EMC ET";
    let mut doc = Document::open_bytes(common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        common::stream(content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
    ]))
    .expect("fixture opens");
    let page = doc.page_text(0).expect("page text reads").clone();
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs.len(), 2);
    assert_eq!(page.runs[1].glyphs.len(), 2);
    assert_eq!(page.runs[0].decoded_text, "AB");
    assert_eq!(page.runs[1].decoded_text, "CD");
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        vec![65, 66, 67, 68]
    );
    assert_eq!(page.runs[0].font_name, "Courier");
    assert_eq!(page.runs[1].font_name, "Courier");
    assert_eq!(page.runs[0].size, 10.0);
    assert_eq!(page.runs[1].size, 10.0);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    for (run, expected) in [
        (&page.runs[0], b"(AB) Tj" as &[u8]),
        (&page.runs[1], b"(CD) Tj"),
    ] {
        assert_eq!(run.provenance.stream, onionskin_cos::ObjRef::new(4, 0));
        let start = run.provenance.decoded.start as usize;
        let end = run.provenance.decoded.end as usize;
        assert_eq!(&content.as_bytes()[start..end], expected);
    }
    let source_quads: Vec<_> = page
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect();
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (22.0, 107.5), (16.0, 97.5), (22.0, 97.5)],
        [(100.0, 57.5), (106.0, 57.5), (100.0, 47.5), (106.0, 47.5)],
        [(106.0, 57.5), (112.0, 57.5), (106.0, 47.5), (112.0, 47.5)],
    ];
    assert_eq!(source_quads.len(), expected_corners.len());
    for (quad, expected) in source_quads.iter().zip(expected_corners) {
        for (&actual, &expected) in quad.corners.iter().zip(expected.iter()) {
            assert!((actual.0 - expected.0).abs() <= 1e-9);
            assert!((actual.1 - expected.1).abs() <= 1e-9);
        }
    }

    for needle in ["XY", "X", "Y"] {
        drain(&mut doc, needle, SearchOptions::default(), 0);
        assert!(!doc.search().is_running());
        assert_eq!(doc.search().searched_pages(), 1);
        assert!(doc.search().failures().is_empty());
        assert_eq!(doc.search().len(), 1);
        let matches = doc.search().matches_on(0);
        assert_eq!(matches.len(), 1);
        let hit = &matches[0];
        assert_eq!(hit.page, 0);
        assert_eq!(hit.text, needle);
        assert_eq!(hit.quads, source_quads);
    }
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

#[test]
fn edited_search_tracks_delete_move_undo_and_redo() {
    let mut doc = Document::open_bytes(multi_page_pdf(4, false)).expect("fixture opens");
    let options = SearchOptions::default();

    drain(&mut doc, "marker002", options, 0);
    assert_eq!(doc.search().current().map(|hit| hit.page), Some(2));
    assert_eq!(doc.search().len(), 1);
    assert_eq!(doc.search().searched_pages(), 4);
    let expected = doc.search_page(2, "marker002", options).unwrap();
    assert_eq!(doc.search().matches_on(2), expected.as_slice());
    assert!(!doc.start_search("marker002", options, 0).unwrap());

    doc.edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
        .expect("delete succeeds");
    assert!(doc
        .start_search("marker002", options, 0)
        .expect("edited search starts"));
    drain_running(&mut doc);
    assert_eq!(doc.search().current().map(|hit| hit.page), Some(1));
    assert_eq!(doc.search().len(), 1);
    assert_eq!(doc.search().searched_pages(), 3);
    let expected = doc.search_page(1, "marker002", options).unwrap();
    assert_eq!(doc.search().matches_on(1), expected.as_slice());
    assert!(!doc.start_search("marker002", options, 0).unwrap());

    doc.undo().expect("undo succeeds");
    assert!(doc
        .start_search("marker002", options, 0)
        .expect("undo search starts"));
    drain_running(&mut doc);
    assert_eq!(doc.search().current().map(|hit| hit.page), Some(2));
    assert_eq!(doc.search().searched_pages(), 4);

    doc.redo().expect("redo succeeds");
    assert!(doc
        .start_search("marker002", options, 0)
        .expect("redo search starts"));
    drain_running(&mut doc);
    assert_eq!(doc.search().current().map(|hit| hit.page), Some(1));
    assert_eq!(doc.search().searched_pages(), 3);

    doc.edit_pages("Move", |tx, structure| move_pages(tx, structure, &[1], 0))
        .expect("move succeeds");
    assert!(doc
        .start_search("marker002", options, 0)
        .expect("moved search starts"));
    drain_running(&mut doc);
    assert_eq!(doc.search().current().map(|hit| hit.page), Some(0));
    assert_eq!(doc.search().searched_pages(), 3);
}

#[test]
fn edited_search_uses_saved_bytes_after_an_existing_worker() {
    let mut file = onionskin_core::DocumentFile::from_document(
        Document::open_bytes(multi_page_pdf(4, false)).expect("fixture opens"),
    );
    let options = SearchOptions::default();
    drain(file.document_mut(), "marker002", options, 0);

    file.document_mut()
        .edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
        .expect("delete succeeds");
    let dir = tempfile::tempdir().expect("temporary directory");
    file.save_as(&dir.path().join("saved.pdf"))
        .expect("save succeeds");

    assert!(file
        .document_mut()
        .start_search("marker002", options, 0)
        .expect("saved search starts"));
    drain_running(file.document_mut());
    assert_eq!(file.search().current().map(|hit| hit.page), Some(1));
    assert_eq!(file.search().len(), 1);
    assert_eq!(file.search().searched_pages(), 3);
}

#[test]
fn edited_search_invalidates_once_and_stays_cancelled() {
    let mut doc = Document::open_bytes(multi_page_pdf(4, false)).expect("fixture opens");
    let options = SearchOptions::default();
    drain(&mut doc, "marker002", options, 0);
    doc.edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
        .expect("delete succeeds");

    assert!(doc.poll_search(), "the stale search needs one repaint");
    assert!(!doc.poll_search(), "stale state is not retried every frame");
    assert_eq!(doc.search().needle(), "marker002");
    assert_eq!(doc.search().len(), 0);
    assert!(!doc.search().is_running());

    assert!(doc.start_search("marker002", options, 0).unwrap());
    doc.poll_search();
    doc.cancel_search();
    doc.undo().expect("undo after cancel succeeds");
    assert!(!doc.poll_search());
    doc.cancel_search();
    assert_eq!(doc.search().needle(), "");
    assert!(!doc.poll_search());
    assert!(!doc
        .start_search("", options, usize::MAX)
        .expect("empty search cancels before page validation"));

    let mut running = Document::open_bytes(multi_page_pdf(PAGES, false)).expect("fixture opens");
    assert!(running.start_search("alpha", options, 0).unwrap());
    running
        .edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
        .expect("delete while running succeeds");
    assert!(running.poll_search());
    assert_eq!(running.search().len(), 0);
    assert!(!running.poll_search());
    assert!(running.start_search("alpha", options, 0).unwrap());
    drain_running(&mut running);
}

#[test]
fn edited_search_include_comments_rejects_overcounted_annotations() {
    let mut doc = Document::open_bytes(multi_page_pdf(4, true)).expect("fixture opens");
    let options = SearchOptions {
        include_comments: true,
        ..SearchOptions::default()
    };
    assert!(doc.annotations().is_err());
    assert!(!doc
        .start_search("marker000", options, 0)
        .expect("annotation preparation failure is visible"));
    assert!(doc.search().is_empty());
    assert!(doc.search().stopped().is_some());
    assert!(!doc.search().is_running());
    assert!(!doc.poll_search());
}

#[test]
fn edited_search_reads_changed_page_text() {
    let mut doc = Document::open_bytes(multi_page_pdf(4, false)).expect("fixture opens");
    let options = SearchOptions::default();
    drain(&mut doc, "marker000", options, 0);
    assert_eq!(doc.search().len(), 1);

    let (edit, base) = doc.edit_mut();
    edit.transact(base, "Replace Text", |tx| {
        tx.put_object(
            5,
            0,
            onionskin_cos::Object::Stream(onionskin_cos::Stream {
                dict: onionskin_cos::Dict::new(),
                raw: b"BT /F1 12 Tf 70 120 Td (replacement) Tj ET".to_vec(),
            }),
        )
    })
    .expect("text edit succeeds");

    drain(&mut doc, "replacement", options, 0);
    assert_eq!(doc.search().len(), 1);
    assert_eq!(doc.search().current().map(|hit| hit.page), Some(0));
    let expected = doc.search_page(0, "replacement", options).unwrap();
    assert_eq!(doc.search().matches_on(0), expected.as_slice());
    assert_eq!(doc.search().current().unwrap().quads[0].corners[0].0, 70.0);
    drain(&mut doc, "marker000", options, 0);
    assert_eq!(doc.search().len(), 0, "the original text is gone");
}

#[test]
fn edited_search_reports_preparation_failure_and_retries() {
    let mut doc = Document::open_bytes(multi_page_pdf(4, false)).expect("fixture opens");
    let options = SearchOptions::default();
    drain(&mut doc, "marker000", options, 0);
    assert_eq!(doc.search().len(), 1);
    doc.edit_document("Invalid real", |tx| {
        tx.set_trailer(
            onionskin_cos::Name::new("SearchTest"),
            Some(onionskin_cos::Object::Real(f64::NAN)),
        )
    })
    .expect("invalid trailer edit succeeds");
    assert!(doc
        .preview_bytes(onionskin_core::AnnotationFilter::DocumentAndMarkups)
        .is_err());

    assert!(!doc
        .start_search("marker000", options, 0)
        .expect("preparation failure is visible in state"));
    assert!(doc.search().is_empty());
    assert_eq!(doc.search().needle(), "marker000");
    assert!(doc.search().stopped().is_some());
    assert!(!doc.search().is_running());
    assert!(!doc.poll_search());
    assert!(!doc
        .start_search("marker000", options, 0)
        .expect("retry remains a visible failure"));
    assert!(doc.search().is_empty());
    assert!(doc.search().stopped().is_some());

    doc.undo().expect("undo succeeds");
    assert!(doc
        .start_search("marker000", options, 0)
        .expect("retry after undo starts"));
    drain_running(&mut doc);
    assert_eq!(doc.search().len(), 1);
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

/// The search results pane picks a hit by the page it sits on and its
/// position among that page's hits, and the cursor it moves is the one the
/// find bar's next and previous move.
///
/// Both coordinates are asserted away from zero. A `select` that ignored
/// them and put the cursor on the first hit would satisfy any test whose
/// only successful selection was the first hit, which is how this went
/// uncovered.
#[test]
fn a_hit_can_be_made_current_by_where_it_sits() {
    let options = SearchOptions::default();
    // Page `i` of the fixture carries `i % 3` occurrences of "alpha", so
    // page 2 has two of them and page 1 has one.
    let mut doc = Document::open_bytes(multi_page_pdf(6, false)).expect("fixture opens");
    doc.start_search("alpha", options, 0)
        .expect("the search worker starts");
    drain_running(&mut doc);
    assert_eq!(doc.search().matches_on(2).len(), 2);

    assert!(doc.select_match(2, 1));

    assert_eq!(doc.search().cursor(), Some((2, 1)));
    let current = doc.search().current().expect("the hit is there");
    assert_eq!(
        current.page, 2,
        "the cursor names the hit the pane clicked, not the first one"
    );
    // Page 1 has one hit and page 2 has two, so this is the third overall.
    assert_eq!(doc.search().current_ordinal(), Some(3));

    // A row clicked after a newer walk replaced the results names a hit that
    // no longer exists. Moving nothing is the answer; moving to the first hit
    // would send the reader somewhere they did not ask for.
    assert!(!doc.select_match(2, 7), "page 2 has two hits, not eight");
    assert!(!doc.select_match(600, 0), "there is no page 601");
    assert_eq!(
        doc.search().cursor(),
        Some((2, 1)),
        "a miss leaves the cursor where it was"
    );
}

/// The pane's cursor and the find bar's are one cursor: stepping from a hit
/// the pane chose continues from there rather than from wherever the walk
/// had left it.
#[test]
fn stepping_from_a_hit_the_pane_chose_continues_from_it() {
    let mut doc = Document::open_bytes(multi_page_pdf(6, false)).expect("fixture opens");
    doc.start_search("alpha", SearchOptions::default(), 0)
        .expect("the search worker starts");
    drain_running(&mut doc);

    assert!(doc.select_match(2, 0));
    assert!(doc.select_next_match());

    assert_eq!(
        doc.search().cursor(),
        Some((2, 1)),
        "next from the first hit on a page with two is the second"
    );

    assert!(doc.select_next_match());
    assert_eq!(
        doc.search().cursor(),
        Some((4, 0)),
        "and then the first hit on the next page that has one"
    );
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

/// Include Comments: text that exists only in a comment's `/Contents` is
/// found, on the comment's page and over its rectangle; a reply is found at
/// the comment it answers. Without the option, nothing.
#[test]
fn edited_search_include_comments_tracks_changed_contents_and_undo() {
    use onionskin_core::review::add_reply;
    use onionskin_core::{add_annotation, Annotation, Rect, Subtype};

    let mut doc = Document::open_path(&onionskin_corpus_testing::seed("hello.pdf")).expect("opens");
    let page = doc.structure().expect("doc").page(0).expect("page").objref;
    let placed = doc
        .edit_annotations("Sticky Note", |tx, structure| {
            let mut note = Annotation::new(Subtype::Text, Rect::new(40.0, 40.0, 60.0, 60.0));
            note.contents = Some("check the zebracorn".into());
            let placed = add_annotation(tx, structure, page, &note, 0)?;
            add_reply(tx, structure, page, placed, "the okapi agrees", None, 0)?;
            Ok(placed)
        })
        .expect("places");

    let plain = SearchOptions::default();
    drain(&mut doc, "zebracorn", plain, 0);
    assert_eq!(doc.search().len(), 0, "page text only");

    let with_comments = SearchOptions {
        include_comments: true,
        ..SearchOptions::default()
    };
    drain(&mut doc, "Zebracorn", with_comments, 0);
    assert_eq!(doc.search().len(), 1);
    let hit = doc.search().current().expect("the cursor lands on it");
    assert_eq!(hit.page, 0);
    assert_eq!(
        hit.quads[0].corners[0],
        (40.0, 60.0),
        "upper-left of the note"
    );

    drain(&mut doc, "okapi", with_comments, 0);
    let hit = doc.search().current().expect("the reply is found");
    assert_eq!(
        hit.quads[0].corners[3],
        (60.0, 40.0),
        "at the note it answers"
    );
    drain(&mut doc, "Zebracorn", with_comments, 0);

    doc.edit_document("Edit Comment Text", |tx| {
        onionskin_core::review::set_contents(tx, placed, "the tapir agrees", 0)
    })
    .expect("edits comment");
    drain(&mut doc, "Zebracorn", with_comments, 0);
    assert_eq!(doc.search().len(), 0, "the old comment text is gone");
    doc.undo().expect("undo succeeds");
    drain(&mut doc, "Zebracorn", with_comments, 0);
    assert_eq!(doc.search().len(), 1, "undo restores the comment hit");
    assert_eq!(
        doc.search().current().unwrap().quads,
        vec![onionskin_core::PageQuad {
            page: 0,
            corners: [(40.0, 60.0), (60.0, 60.0), (40.0, 40.0), (60.0, 40.0)],
        }]
    );
}
