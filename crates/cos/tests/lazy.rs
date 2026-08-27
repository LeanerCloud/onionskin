//! Decision 11, as a measurement rather than a claim: opening a document and
//! reaching its first page must not read the whole file.

mod common;

use std::sync::atomic::Ordering;
use std::time::Instant;

use common::{corpus_dir, corpus_root, pdfs_in};
use onionskin_cos::{BytesSource, CountingSource, Document, FileSource, Origin, Provenance};

/// Only files this big make the claim interesting; below it, one read window
/// covers the document anyway.
const INTERESTING: u64 = 1024 * 1024;
/// Reading a quarter of a multi-megabyte file to show its first page would
/// already mean the laziness bet had failed.
const BUDGET_PERCENT: u64 = 25;

#[test]
fn opening_a_large_document_reads_far_less_than_the_whole_file() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    let mut candidates: Vec<_> = pdfs_in(&root)
        .into_iter()
        .filter_map(|path| {
            let len = std::fs::metadata(&path).ok()?.len();
            (len >= INTERESTING).then_some((len, path))
        })
        .collect();
    if candidates.is_empty() {
        eprintln!("SKIPPED: the corpus holds no PDF of at least {INTERESTING} bytes");
        return;
    }
    candidates.sort_by_key(|(len, _)| std::cmp::Reverse(*len));

    let mut measured = 0usize;
    for (len, path) in candidates.iter().take(8) {
        let file = FileSource::open(path).expect("corpus file opens");
        let (counting, bytes_read) = CountingSource::new(Box::new(file));

        let started = Instant::now();
        let Ok((document, provenance)) = Document::open_repairing(Box::new(counting)) else {
            continue;
        };
        // A repaired open scans the whole file by design; that is the price of
        // damage, and it is not what this budget is about.
        if !matches!(provenance, Provenance::Clean) {
            continue;
        }
        if document.first_page().is_err() {
            continue;
        }
        let elapsed = started.elapsed();

        let read = bytes_read.load(Ordering::Relaxed);
        let percent = read * 100 / len;
        println!(
            "{}: {} bytes read of {} ({percent}%), {} pages, first page in {:?}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            read,
            len,
            document.page_count().unwrap_or(-1),
            elapsed
        );
        assert!(
            read < *len,
            "opening read {read} bytes of a {len} byte file: that is the whole file"
        );
        assert!(
            percent <= BUDGET_PERCENT,
            "opening read {percent}% of the file, over the {BUDGET_PERCENT}% budget"
        );
        measured += 1;
    }

    assert!(
        measured > 0,
        "no large corpus file opened cleanly, so laziness went unmeasured"
    );
}

/// Decision 7: every parsed object knows the bytes it came from. The check is
/// that the recorded span, cut out of the file on its own, holds exactly that
/// object.
#[test]
fn every_parsed_object_records_the_bytes_it_came_from() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    // The seeds cover plain objects; the external files bring the compressed
    // ones, whose span lives inside a container rather than in the file. Only
    // the seeds are committed, so the compressed half is checked when the
    // external corpus is present and skipped loudly when it is not.
    let external = corpus_dir("external");
    let mut files = pdfs_in(&root.join("seeds"));
    if let Some(dir) = &external {
        files.extend(pdfs_in(dir).into_iter().take(400));
    }

    let mut in_file = 0usize;
    let mut compressed = 0usize;
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(document) = Document::open(Box::new(BytesSource::new(bytes.clone()))) else {
            continue;
        };
        let name = path.display();
        let mut spans = Vec::new();

        for (number, _) in document.xref().iter() {
            let Ok(parsed) = document.get(number) else {
                continue;
            };
            let span = match parsed.origin {
                Origin::File(span) => span,
                // A compressed object's span is inside its container's decoded
                // data, so the bytes to preserve for it are the container's.
                Origin::ObjectStream {
                    container,
                    container_span,
                    within,
                } => {
                    assert!(
                        within.end > within.start,
                        "{name}: object {number} has an empty span in its container"
                    );
                    assert!(
                        bytes[container_span.start as usize..]
                            .starts_with(format!("{container} ").as_bytes()),
                        "{name}: object {number} names container {container}, whose span starts elsewhere"
                    );
                    compressed += 1;
                    continue;
                }
                Origin::Pending => continue,
            };
            let slice = &bytes[span.start as usize..span.end as usize];
            assert!(
                slice.starts_with(format!("{number} {} obj", parsed.objref.generation).as_bytes()),
                "{name}: object {number}'s span does not start at its header"
            );
            let end = slice
                .iter()
                .rposition(|b| !b.is_ascii_whitespace())
                .map_or(slice, |i| &slice[..=i]);
            assert!(
                end.ends_with(b"endobj") || end.ends_with(b"endstream"),
                "{name}: object {number}'s span does not end at the object's end"
            );
            spans.push((number, span));
            in_file += 1;
        }

        // Two objects claiming the same bytes would make redaction and
        // selection-to-source mapping lie later on.
        spans.sort_by_key(|(_, span)| span.start);
        for pair in spans.windows(2) {
            assert!(
                pair[0].1.end <= pair[1].1.start,
                "{name}: objects {} and {} overlap in the file",
                pair[0].0,
                pair[1].0
            );
        }
    }
    assert!(in_file > 0, "no in-file spans were checked");
    if external.is_some() {
        assert!(
            compressed > 0,
            "the external corpus is present but produced no object-stream spans"
        );
    }
    println!("byte spans verified: {in_file} in file, {compressed} in object streams");
}

#[test]
fn an_object_is_parsed_only_when_it_is_asked_for() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    let path = root.join("seeds").join("two-page.pdf");
    if !path.is_file() {
        eprintln!("SKIPPED: {} is absent", path.display());
        return;
    }

    let file = FileSource::open(&path).expect("seed opens");
    let (counting, bytes_read) = CountingSource::new(Box::new(file));
    let document = Document::open(Box::new(counting)).expect("seed opens clean");
    let after_open = bytes_read.load(Ordering::Relaxed);

    let catalog = document.catalog().expect("catalog resolves");
    let after_catalog = bytes_read.load(Ordering::Relaxed);
    assert!(
        after_catalog > after_open,
        "resolving the catalog must be what reads the catalog"
    );
    assert!(catalog.contains(b"Pages"));

    // The same object again comes from the cache, not from the source.
    let before_repeat = bytes_read.load(Ordering::Relaxed);
    document.catalog().expect("catalog resolves again");
    assert_eq!(
        bytes_read.load(Ordering::Relaxed),
        before_repeat,
        "a cached object must not be re-read"
    );
}
