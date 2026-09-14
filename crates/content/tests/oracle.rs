//! Extraction scored against poppler's `pdftotext`.
//!
//! `pdftotext` is an oracle, not a specification. Perfect parity is not the
//! goal and would not be evidence of correctness if it were: poppler carries
//! predefined CJK CMaps this build does not, and it does layout analysis this
//! milestone deliberately skips. What the comparison buys is the thing prose
//! cannot: a number per corpus that moves when extraction breaks, and a
//! bucketed breakdown of what the differences are.
//!
//! Both sides go through `common::normalize`, which erases line layout,
//! ligature spelling and Unicode punctuation choices, and erases nothing else.
//! The floors asserted at the bottom of each test are set below the measured
//! rate so they catch a regression rather than pin today's exact number.
//!
//! The floors are lower than a first reading suggests they should be, and the
//! reason is worth stating: poppler answers where this crate declines. Given
//! `/Differences [97 /square /triangle]`, a Type 3 glyph name with no Unicode
//! meaning, poppler emits `ab` from the character codes and this crate emits
//! `Mapping::Unmapped`. Every such file scores 0.0 here while being the more
//! honest extraction, so the similarity mean is a regression detector and not
//! a quality score. `error_rate` and the mismatch categories carry the part of
//! the signal that a guessing oracle cannot.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{normalize, similarity, Oracle};
use onionskin_content::{extract_page, page_count, Warning};

/// Pages compared per file. The first pages carry the title and body text that
/// exercise fonts; going deeper multiplies runtime without adding font
/// coverage.
const PAGES: usize = 3;

/// Scores one file and returns its similarity, or records why it could not be
/// scored.
fn score_file(oracle: &mut Oracle, path: &Path) {
    score_file_with(oracle, path, common::pdftotext);
}

fn score_file_with(
    oracle: &mut Oracle,
    path: &Path,
    run_pdftotext: impl FnOnce(&Path, usize, usize) -> Option<Result<String, String>>,
) {
    let doc = match common::open(path) {
        Ok(doc) => doc,
        Err(category) => {
            oracle.skip(path, &category);
            return;
        }
    };
    let count = match page_count(&doc) {
        Ok(0) => {
            oracle.skip(path, "no-pages");
            return;
        }
        Err(error) => {
            oracle.error(path, error.category(), &error.to_string());
            return;
        }
        Ok(n) => n.min(PAGES),
    };

    let mut ours = String::new();
    let mut warnings = Vec::new();
    for index in 0..count {
        match extract_page(&doc, index) {
            Ok(page) => {
                ours.push_str(&page.flatten().text);
                ours.push('\n');
                warnings.extend(page.warnings);
            }
            Err(e) => {
                oracle.error(path, e.category(), &e.to_string());
                return;
            }
        }
    }

    let Some(theirs) = run_pdftotext(path, 1, count) else {
        oracle.skip(path, "no-pdftotext");
        return;
    };
    let theirs = match theirs {
        Ok(text) => text,
        Err(detail) => {
            oracle.error(path, "pdftotext", &detail);
            return;
        }
    };

    let ours = normalize(&ours);
    let theirs = normalize(&theirs);
    // Neither side found text: an image-only or empty page. Counted, not
    // scored, because a similarity of 1.0 there would inflate the mean with
    // work nobody did.
    if ours.is_empty() && theirs.is_empty() {
        oracle.skip(path, "no-text-either-side");
        return;
    }
    let overlap = common::char_overlap(&ours, &theirs);
    let Some(score) = similarity(&ours, &theirs) else {
        oracle.skip(path, "too-long-to-diff");
        return;
    };
    oracle.score(path, score, overlap);
    if score < 0.95 {
        oracle.fail(
            path,
            category(&ours, &theirs, &warnings),
            &format!("similarity {score:.3}, character overlap {overlap:.3}"),
        );
    }
}

/// Why a file scored badly, from the evidence rather than from a guess.
fn category(ours: &str, theirs: &str, warnings: &[Warning]) -> &'static str {
    if warnings
        .iter()
        .any(|w| matches!(w, Warning::UnsupportedCMap { .. }))
    {
        return "unsupported-cmap";
    }
    if warnings
        .iter()
        .any(|w| matches!(w, Warning::UnmappedGlyphs { .. }))
    {
        return "unmapped-glyphs";
    }
    if warnings
        .iter()
        .any(|w| matches!(w, Warning::ContentPartFailed { .. }))
    {
        return "content-decode";
    }
    if warnings
        .iter()
        .any(|w| matches!(w, Warning::FontLoadFailed { .. }))
    {
        return "font-load";
    }
    let (ours_len, theirs_len) = (ours.chars().count(), theirs.chars().count());
    if ours_len * 2 < theirs_len {
        return "missing-text";
    }
    if theirs_len * 2 < ours_len {
        return "extra-text";
    }
    if warnings
        .iter()
        .any(|w| matches!(w, Warning::MissingWidths { .. }))
    {
        return "missing-widths";
    }
    // Same characters, different order or spacing: reading order, which this
    // milestone takes as document order on purpose.
    let sorted = |t: &str| {
        let mut c: Vec<char> = t.chars().filter(|c| !c.is_whitespace()).collect();
        c.sort_unstable();
        c
    };
    if sorted(ours) == sorted(theirs) {
        "reading-order"
    } else {
        "character-differences"
    }
}

/// Files whose extraction threw rather than scoring badly, as a fraction of
/// the files that opened.
///
/// Scored separately from similarity because they are a different failure: a
/// page that errors produces no text at all, so it never reaches the mean and
/// cannot drag it down. Without this, extraction could start throwing on a
/// tenth of a corpus and every similarity assertion would still pass.
fn error_rate(oracle: &Oracle) -> f64 {
    let attempted = oracle.scores.len() + oracle.errors;
    if attempted == 0 {
        return 0.0;
    }
    oracle.errors as f64 / attempted as f64
}

fn temporary_path(stem: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "onionskin-oracle-{stem}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn a_page_count_error_contributes_to_the_error_rate() {
    let path = temporary_path("negative-count.pdf");
    let bytes = common::build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count -1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".to_vec(),
    ]);
    let mut fixture = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("the temporary fixture is new and writable");
    std::io::Write::write_all(&mut fixture, &bytes).expect("the fixture bytes are writable");
    drop(fixture);

    let mut oracle = Oracle::new("negative page count");
    score_file(&mut oracle, &path);
    std::fs::remove_file(&path).expect("the temporary fixture is removable");

    assert_eq!(error_rate(&oracle), 1.0);
    assert!(!oracle.skipped.contains_key("no-pages"));
}

#[test]
fn a_similarity_mismatch_is_not_an_extraction_error() {
    let mut oracle = Oracle::new("mismatch");
    let path = Path::new("mismatch.pdf");
    oracle.score(path, 0.5, 1.0);
    oracle.fail(path, "future-mismatch-category", "the text differs");

    assert_eq!(oracle.total(), 1);
    assert_eq!(error_rate(&oracle), 0.0);

    oracle.skip(Path::new("skipped.pdf"), "no-text");
    oracle.error(Path::new("broken.pdf"), "extract", "the extractor failed");
    assert_eq!(oracle.total(), 3);
}

#[test]
fn an_explicit_error_needs_no_category_allowlist() {
    let mut oracle = Oracle::new("new category");
    oracle.score(Path::new("good.pdf"), 1.0, 1.0);
    oracle.error(
        Path::new("bad.pdf"),
        "category-added-tomorrow",
        "the extractor stopped",
    );

    assert_eq!(error_rate(&oracle), 0.5);
}

#[cfg(unix)]
#[test]
fn a_nonzero_pdftotext_exit_is_an_error() {
    use std::os::unix::fs::PermissionsExt;

    let directory = temporary_path("pdftotext");
    std::fs::create_dir(&directory).expect("the temporary directory is new and writable");
    let executable = directory.join("pdftotext");
    let input = directory.join("input.pdf");
    std::fs::write(
        &executable,
        b"#!/bin/sh\necho private document text\necho tool exploded >&2\nexit 7\n",
    )
    .expect("the fake tool is writable");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("the fake tool is executable");
    let bytes = common::build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".to_vec(),
    ]);
    std::fs::write(&input, bytes).expect("the fake input is writable");

    let mut oracle = Oracle::new("failed pdftotext");
    score_file_with(&mut oracle, &input, |path, first, last| {
        common::pdftotext_with(&executable, path, first, last)
    });
    let detail = oracle.failed["pdftotext"][0].clone();

    std::fs::write(
        &executable,
        b"#!/bin/sh\necho private document text\nexit 8\n",
    )
    .expect("the stdout-only fake tool is writable");
    let mut stdout_only = Oracle::new("failed pdftotext with stdout");
    score_file_with(&mut stdout_only, &input, |path, first, last| {
        common::pdftotext_with(&executable, path, first, last)
    });
    let stdout_detail = stdout_only.failed["pdftotext"][0].clone();
    std::fs::remove_dir_all(&directory).expect("the test-owned directory is removable");

    assert_eq!(oracle.errors, 1);
    assert_eq!(error_rate(&oracle), 1.0);
    assert!(detail.contains("status: 7"), "missing status: {detail}");
    assert!(detail.contains("tool exploded"), "missing stderr: {detail}");
    assert!(
        !detail.contains("private document text"),
        "document stdout leaked into the diagnostic: {detail}"
    );
    assert_eq!(stdout_only.errors, 1);
    assert_eq!(error_rate(&stdout_only), 1.0);
    assert!(
        stdout_detail.contains("status: 8") && stdout_detail.contains("stdout was 22 bytes"),
        "missing status or byte count: {stdout_detail}"
    );
    assert!(
        !stdout_detail.contains("private document text"),
        "document stdout leaked into the empty-stderr diagnostic: {stdout_detail}"
    );
}

fn run_corpus(name: &str, relative: &str, limit: Option<usize>) -> Option<Oracle> {
    if !common::have_pdftotext() {
        // Through `missing` rather than a bare print: without the oracle every
        // test in this file returns early and reports a pass, which is the
        // same silent skip an absent corpus produces and deserves the same
        // treatment when CI says the run is mandatory.
        common::missing(&format!("{name} needs pdftotext on PATH"));
        return None;
    }
    let dir = common::corpus_dir(relative)?;
    let mut files = common::pdfs_in(&dir);
    if let Some(limit) = limit {
        // Deterministic thinning: every nth file of the sorted list, so the
        // slice spans the whole corpus rather than its alphabetical head.
        if files.len() > limit {
            let step = files.len().div_ceil(limit);
            files = files.into_iter().step_by(step).collect();
        }
    }
    let mut oracle = Oracle::new(name);
    for file in &files {
        score_file(&mut oracle, file);
    }
    oracle.report();
    Some(oracle)
}

#[test]
fn seeds_match_pdftotext_exactly() {
    let Some(oracle) = run_corpus("seeds", "seeds", None) else {
        return;
    };
    assert!(!oracle.scores.is_empty(), "no seed text was compared");
    assert!(oracle.failed.is_empty(), "{:?}", oracle.failed);
    assert_eq!(oracle.bucket(1.0, f64::INFINITY), oracle.scores.len());
}

#[test]
fn pdf_association_corpus() {
    let Some(oracle) = run_corpus("pdf-association", "external/pdf-association", None) else {
        return;
    };
    assert!(
        oracle.mean() >= 0.72,
        "mean similarity fell to {:.4}",
        oracle.mean()
    );
    assert!(
        error_rate(&oracle) <= 0.06,
        "{:.1}% of files errored outright",
        error_rate(&oracle) * 100.0
    );
}

#[test]
fn verapdf_slice() {
    let Some(oracle) = run_corpus("verapdf", "external/verapdf", Some(400)) else {
        return;
    };
    assert!(
        oracle.mean() >= 0.84,
        "mean similarity fell to {:.4}",
        oracle.mean()
    );
    assert!(
        error_rate(&oracle) <= 0.03,
        "{:.1}% of files errored outright",
        error_rate(&oracle) * 100.0
    );
}

#[test]
fn hayro_custom_text_files() {
    let Some(oracle) = run_corpus("hayro-custom", "external/hayro/pdfs/custom", None) else {
        return;
    };
    assert!(
        oracle.mean() >= 0.72,
        "mean similarity fell to {:.4}",
        oracle.mean()
    );
    assert!(
        error_rate(&oracle) <= 0.05,
        "{:.1}% of files errored outright",
        error_rate(&oracle) * 100.0
    );
}

/// Prints one file's extraction beside `pdftotext`'s, for working out what a
/// low score actually is. `ONIONSKIN_DUMP=<path> cargo test -- --ignored dump`.
#[test]
#[ignore = "developer tool; needs ONIONSKIN_DUMP"]
fn dump() {
    let Some(path) = std::env::var_os("ONIONSKIN_DUMP") else {
        panic!("set ONIONSKIN_DUMP to a PDF path");
    };
    let path = Path::new(&path);
    let doc = common::open(path).expect("opens");
    let count = page_count(&doc).unwrap().min(PAGES);
    let mut ours = String::new();
    for index in 0..count {
        let page = extract_page(&doc, index).expect("extracts");
        println!("-- page {index} warnings: {:?}", page.warnings);
        ours.push_str(&page.flatten().text);
        ours.push('\n');
    }
    let theirs = common::pdftotext(path, 1, count).unwrap().unwrap();
    println!("---- ours ----\n{}", normalize(&ours));
    println!("---- pdftotext ----\n{}", normalize(&theirs));
    println!(
        "---- similarity {:?} ----",
        similarity(&normalize(&ours), &normalize(&theirs))
    );
}

/// Prints the mismatch categories across every corpus at once, which is the
/// number the milestone report quotes.
#[test]
#[ignore = "the full sweep; run it with --ignored when the report needs refreshing"]
fn full_sweep() {
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut all = Oracle::new("all corpora");
    for (name, relative, limit) in [
        ("seeds", "seeds", None),
        ("pdf-association", "external/pdf-association", None),
        ("verapdf", "external/verapdf", None),
        ("hayro-custom", "external/hayro/pdfs/custom", None),
        ("hayro-corpus", "external/hayro-corpus", None),
    ] {
        let Some(oracle) = run_corpus(name, relative, limit) else {
            continue;
        };
        for (category, files) in &oracle.failed {
            *totals.entry(category.clone()).or_default() += files.len();
        }
        all.errors += oracle.errors;
        all.scores.extend(oracle.scores);
        all.overlaps.extend(oracle.overlaps);
        for (k, v) in oracle.skipped {
            all.skipped.entry(k).or_default().extend(v);
        }
    }
    all.report();
    println!("\n== mismatch categories ==");
    for (category, count) in totals {
        println!("   {category}: {count}");
    }
}
