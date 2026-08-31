//! Invariants that must hold on every page of every file, whatever the file
//! does, plus the extraction-time budget.
//!
//! These are the tests that catch the failures an oracle score cannot see: a
//! quad at NaN, a text range pointing outside its own run, provenance naming
//! bytes that are not in the file. A page can score badly against `pdftotext`
//! and still be honest; it cannot break these and be honest.

mod common;

use std::path::Path;
use std::time::Instant;

use onionskin_content::{extract_page, page_count, Mapping, PageText};

/// Pages checked per file.
const PAGES: usize = 2;

/// Checks every structural promise a [`PageText`] makes.
fn assert_invariants(file: &Path, page: &PageText, raw_len: u64) {
    let where_ = common::short(file);
    for run in &page.runs {
        assert_eq!(run.page, page.page, "{where_}: run names the wrong page");

        let provenance = run.provenance;
        assert!(
            provenance.decoded.start <= provenance.decoded.end,
            "{where_}: inverted decoded span"
        );
        if let Some(span) = provenance.file_span() {
            assert!(
                span.start <= span.end && span.end <= raw_len,
                "{where_}: provenance span {span:?} is outside a {raw_len} byte file"
            );
        }

        let mut previous_start = 0usize;
        for glyph in &run.glyphs {
            for (x, y) in glyph.quad.corners {
                assert!(
                    x.is_finite() && y.is_finite(),
                    "{where_}: quad corner ({x}, {y}) is not a number"
                );
            }
            assert_eq!(glyph.quad.page, page.page);
            match &glyph.mapping {
                Mapping::Text(range) => {
                    assert!(
                        range.start <= range.end && range.end <= run.text.len(),
                        "{where_}: glyph text range {range:?} is outside a {} byte run",
                        run.text.len()
                    );
                    assert!(
                        run.text.is_char_boundary(range.start)
                            && run.text.is_char_boundary(range.end),
                        "{where_}: glyph text range splits a character"
                    );
                    // Glyph ranges walk forward through the run's text; a
                    // caller mapping a selection back to glyphs relies on it.
                    // They may repeat rather than strictly advance: every
                    // glyph of an /ActualText span points at the whole
                    // replacement string.
                    assert!(
                        range.start >= previous_start,
                        "{where_}: glyph text ranges are out of order"
                    );
                    previous_start = range.start;
                }
                Mapping::Unmapped => {}
            }
        }
        // A run with text has at least one glyph that produced it.
        if !run.text.trim().is_empty() {
            assert!(
                run.glyphs
                    .iter()
                    .any(|g| matches!(g.mapping, Mapping::Text(_))),
                "{where_}: run has text but no glyph claims it"
            );
        }
    }
}

fn sweep(name: &str, relative: &str, limit: Option<usize>) -> usize {
    let Some(dir) = common::corpus_dir(relative) else {
        return 0;
    };
    let mut files = common::pdfs_in(&dir);
    if let Some(limit) = limit {
        if files.len() > limit {
            let step = files.len().div_ceil(limit);
            files = files.into_iter().step_by(step).collect();
        }
    }

    let mut checked = 0usize;
    let mut opened = 0usize;
    let mut errors = std::collections::BTreeMap::<String, usize>::new();
    for file in &files {
        let Ok(doc) = common::open(file) else {
            continue;
        };
        opened += 1;
        let raw_len = std::fs::metadata(file).map(|m| m.len()).unwrap_or(u64::MAX);
        let count = match page_count(&doc) {
            Ok(count) => count.min(PAGES),
            Err(error) => {
                *errors.entry(error.category().to_string()).or_default() += 1;
                continue;
            }
        };
        for index in 0..count {
            match extract_page(&doc, index) {
                Ok(page) => {
                    assert_invariants(file, &page, raw_len);
                    checked += 1;
                }
                Err(e) => *errors.entry(e.category().to_string()).or_default() += 1,
            }
        }
    }
    println!(
        "\n== {name} == {} files, {opened} opened, {checked} pages checked",
        files.len()
    );
    for (category, count) in &errors {
        println!("   page errors [{category}]: {count}");
    }
    checked
}

#[test]
fn hayro_corpus_pages_keep_their_promises() {
    sweep("hayro-corpus", "external/hayro-corpus", None);
}

#[test]
fn hayro_custom_pages_keep_their_promises() {
    sweep("hayro-custom", "external/hayro/pdfs/custom", None);
}

#[test]
fn verapdf_pages_keep_their_promises() {
    sweep("verapdf", "external/verapdf", Some(600));
}

#[test]
fn pdf_association_pages_keep_their_promises() {
    sweep("pdf-association", "external/pdf-association", None);
}

/// Every extracted character traces to a stream object whose recorded file
/// span really does hold that object, checked against the raw bytes rather
/// than against the model that produced them.
#[test]
fn provenance_lands_on_a_real_object_header() {
    let Some(dir) = common::corpus_dir("external/hayro/pdfs/custom") else {
        return;
    };
    let mut checked = 0usize;
    for file in common::pdfs_in(&dir).iter().take(120) {
        let Ok(doc) = common::open(file) else {
            continue;
        };
        let Ok(raw) = std::fs::read(file) else {
            continue;
        };
        let Ok(page) = extract_page(&doc, 0) else {
            continue;
        };
        for run in page.runs.iter().take(50) {
            let Some(span) = run.provenance.file_span() else {
                continue;
            };
            let bytes = &raw[span.start as usize..span.end.min(raw.len() as u64) as usize];
            // A content stream is an indirect object by construction, so its
            // span opens with that object's own header.
            assert_eq!(
                object_header(bytes),
                Some(run.provenance.stream.number),
                "{}: span for object {} opens with {:?}",
                common::short(file),
                run.provenance.stream.number,
                String::from_utf8_lossy(&bytes[..40.min(bytes.len())])
            );
            checked += 1;
        }
    }
    println!("provenance checked on {checked} runs");
    assert!(checked > 0 || common::corpus_root().is_none());
}

/// Object number from a `<n> <g> obj` header. `obj` may be followed directly
/// by a delimiter rather than whitespace, which compact producers do.
fn object_header(bytes: &[u8]) -> Option<u32> {
    let mut i = 0usize;
    let number = |bytes: &[u8], i: &mut usize| -> Option<u32> {
        let start = *i;
        while bytes.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        let text = std::str::from_utf8(&bytes[start..*i]).ok()?;
        let value = text.parse().ok()?;
        while bytes.get(*i).is_some_and(|b| b.is_ascii_whitespace()) {
            *i += 1;
        }
        Some(value)
    };
    let object = number(bytes, &mut i)?;
    number(bytes, &mut i)?;
    bytes.get(i..i + 3).filter(|w| *w == b"obj")?;
    Some(object)
}

/// Extraction time per page on the largest text-heavy corpus file. Printed
/// rather than asserted: this is the number the milestone report quotes, and a
/// hard budget belongs with the decision-11 benches, not here.
#[test]
#[ignore = "timing; run with --ignored"]
fn extraction_time_per_page() {
    let Some(root) = common::corpus_root() else {
        return;
    };
    let mut files: Vec<(u64, std::path::PathBuf)> = common::pdfs_in(&root)
        .into_iter()
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.len(), p)))
        .collect();
    files.sort_by_key(|(size, _)| std::cmp::Reverse(*size));

    let mut reported = 0usize;
    for (size, file) in files.iter() {
        if reported >= 8 {
            break;
        }
        let Ok(doc) = common::open(file) else {
            continue;
        };
        let count = match page_count(&doc) {
            Ok(count) => count.min(50),
            Err(error) => {
                eprintln!("SKIPPED: {} page count: {error}", file.display());
                continue;
            }
        };
        if count == 0 {
            continue;
        }
        let start = Instant::now();
        let mut glyphs = 0usize;
        let mut pages = 0usize;
        for index in 0..count {
            if let Ok(page) = extract_page(&doc, index) {
                glyphs += page.runs.iter().map(|r| r.glyphs.len()).sum::<usize>();
                pages += 1;
            }
        }
        let elapsed = start.elapsed();
        // Big files with no text measure the page tree, not extraction.
        if pages == 0 || glyphs < 1000 {
            continue;
        }
        reported += 1;
        println!(
            "{:>10} bytes  {:>4} pages  {:>9.3} ms/page  {:>8} glyphs  {}",
            size,
            pages,
            elapsed.as_secs_f64() * 1000.0 / pages as f64,
            glyphs,
            common::short(file)
        );
    }
}
