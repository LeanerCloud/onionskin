//! Guarantee test 1: open then save-unchanged is byte-identical, and a no-op
//! save appends nothing at all.
//!
//! The external corpora also run here, with a floor on the pass count so the
//! numbers cannot silently regress. The floors are what this spike measured,
//! not aspirations.

mod common;

use std::path::Path;

use common::{corpus_dir, pdfs_in, Tally};
use onionskin_cos::{Document, FileSource};

/// Opens `path` and proves a save-unchanged returns exactly the input bytes.
fn round_trips(path: &Path, tally: &mut Tally) {
    let original = std::fs::read(path).expect("corpus file is readable");
    let source = match FileSource::open(path) {
        Ok(s) => s,
        Err(e) => {
            tally.record(path, &e);
            return;
        }
    };
    let (document, provenance) = match Document::open_repairing(Box::new(source)) {
        Ok(pair) => pair,
        Err(e) => {
            tally.record(path, &e);
            return;
        }
    };

    if !provenance.is_clean() {
        // Guarantee test 1 covers well-formed files only; damaged ones are
        // test 6's business, and are counted separately rather than hidden.
        tally.skip(path, "needed-repair");
        return;
    }

    match document.incremental_section() {
        Ok(None) => {}
        Ok(Some(_)) => {
            tally.fail(path, "non-empty-noop-save", "a no-op save appended bytes");
            return;
        }
        Err(e) => {
            tally.record(path, &e);
            return;
        }
    }

    match document.save_to_vec() {
        Ok(saved) if saved == original => tally.pass(path),
        Ok(saved) => tally.fail(
            path,
            "not-byte-identical",
            &format!("{} bytes in, {} bytes out", original.len(), saved.len()),
        ),
        Err(e) => tally.record(path, &e),
    }
}

/// Runs one corpus. `floor_percent` is the share of the files that were not
/// skipped which must round-trip, and `max_repaired` caps how many the opener
/// may reclassify as damaged: without that cap, a regression that made every
/// file look broken would empty the numerator and denominator together and
/// still report green.
fn run(name: &str, relative: &str, floor_percent: usize, max_repaired: usize) {
    let Some(dir) = corpus_dir(relative) else {
        return;
    };
    let files = pdfs_in(&dir);
    assert!(!files.is_empty(), "{relative} holds no PDFs");

    let mut tally = Tally::new(name);
    for file in &files {
        round_trips(file, &mut tally);
    }
    tally.report();

    let considered = tally.passed.len() + tally.failure_count();
    let floor = considered * floor_percent / 100;
    assert!(
        tally.passed.len() >= floor,
        "{name}: {} of {considered} round-tripped, below the {floor_percent}% floor ({floor})",
        tally.passed.len()
    );
    let repaired = tally.skipped.get("needed-repair").map_or(0, Vec::len);
    assert!(
        repaired <= max_repaired,
        "{name}: {repaired} files were reclassified as needing repair, over the cap of {max_repaired}"
    );
}

#[test]
fn seeds_round_trip_exactly() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let files = pdfs_in(&dir);
    assert!(!files.is_empty(), "corpus/seeds holds no PDFs");

    let mut tally = Tally::new("seeds");
    for file in &files {
        round_trips(file, &mut tally);
    }
    tally.report();
    assert_eq!(
        tally.passed.len(),
        files.len(),
        "every seed must open clean and round-trip byte for byte"
    );
}

#[test]
fn pdf20examples_corpus_round_trips() {
    run(
        "external/pdf-association/pdf20examples",
        "external/pdf-association/pdf20examples",
        100,
        1,
    );
}

#[test]
fn pdf_association_corpus_round_trips() {
    run(
        "external/pdf-association (all)",
        "external/pdf-association",
        100,
        4,
    );
}

#[test]
fn verapdf_corpus_round_trips() {
    run("external/verapdf", "external/verapdf", 100, 4);
}

#[test]
fn hayro_custom_corpus_round_trips() {
    run(
        "external/hayro/pdfs/custom",
        "external/hayro/pdfs/custom",
        100,
        5,
    );
}

// external/hayro/pdfs/load is hayro's fuzzer crash corpus, which guarantee
// test 1 excludes by name. It is exercised by tests/robustness.rs instead.

#[test]
fn hayro_other_corpus_round_trips() {
    run(
        "external/hayro/pdfs/other",
        "external/hayro/pdfs/other",
        100,
        0,
    );
}

#[test]
fn hayro_regression_corpus_round_trips() {
    run("external/hayro-corpus", "external/hayro-corpus", 100, 6);
}
