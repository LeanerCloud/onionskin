//! Decision 11, as a measurement rather than a claim: opening a document and
//! reaching its first page must not read the whole file.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use common::{corpus_dir, corpus_root, pdfs_in};
use onionskin_cos::{
    BytesSource, CountingSource, Document, FileSource, Origin, Provenance, Result, Source,
};

/// Only files this big make the claim interesting; below it, one read window
/// covers the document anyway.
const INTERESTING: u64 = 1024 * 1024;
/// Reading a quarter of a multi-megabyte file to show its first page would
/// already mean the laziness bet had failed. Applied to the bytes the open
/// looked at, counting each one once.
const BUDGET_PERCENT: u64 = 25;
/// How many times over the open may hand out the bytes it looked at.
///
/// `Reader` parses an object by reading a window and quadrupling it until the
/// object fits, re-reading from the object's start each time, so the bytes
/// handed out are a multiple of the bytes looked at rather than a fraction of
/// the file. Stating it that way binds on every file measured here; a ceiling
/// stated against the file size binds only on the one large reader and leaves
/// the rest free to regress by any factor at all.
///
/// Between two anchors, both of them named because neither alone justifies a
/// number. Measured over the sets CI fetches, the whole-open factor runs from
/// 1.31 to 2.31 across the 32 files. The loop's own worst case for a single
/// object is 16/3, when the object is one byte past a window boundary. So this
/// sits above everything observed with room for a file that lands worse, and
/// below what a loop that stopped quadrupling would produce.
const REREAD_CEILING: u64 = 4;

/// Where the ranges are recorded, shared between the wrapper handing them out
/// and the test reading them back.
type Ranges = Arc<Mutex<Vec<(u64, u64)>>>;

/// The byte ranges the parser asked for, so a range asked for twice counts
/// once.
///
/// The budget is a claim about how much of the file an open has to *look at*.
/// `CountingSource` totals every byte handed out, and `Reader` parses an object
/// by reading a window and quadrupling it until the object fits, re-reading
/// from the object's start each time. Those re-reads are real I/O and are
/// printed below, but they are a caching question rather than a laziness one,
/// and totalling them answers both at once and neither correctly.
struct SeenRanges {
    inner: Box<dyn Source>,
    seen: Ranges,
}

impl SeenRanges {
    fn wrap(inner: Box<dyn Source>) -> (Self, Ranges) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let source = SeenRanges {
            inner,
            seen: Arc::clone(&seen),
        };
        (source, seen)
    }
}

impl Source for SeenRanges {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let bytes = self.inner.read_at(offset, len)?;
        self.seen
            .lock()
            .expect("the range log outlives every reader")
            .push((offset, offset + bytes.len() as u64));
        Ok(bytes)
    }
}

/// How much of the file the ranges cover, counting each byte once.
fn covered(mut ranges: Vec<(u64, u64)>) -> u64 {
    ranges.sort_unstable();
    let mut total = 0;
    let mut reached = 0u64;
    for (start, end) in ranges {
        let start = start.max(reached);
        if end > start {
            total += end - start;
            reached = end;
        }
    }
    total
}

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
        common::missing(&format!(
            "the corpus holds no PDF of at least {INTERESTING} bytes, so laziness cannot be measured"
        ));
        return;
    }
    candidates.sort_by_key(|(len, _)| std::cmp::Reverse(*len));

    // Every candidate rather than the largest few: which files the largest few
    // are depends on which corpus sets happen to be fetched, and the one file
    // in the corpus that comes anywhere near this budget is 4 MB sitting among
    // 10 MB neighbours.
    let mut measured = 0usize;
    for (len, path) in candidates.iter() {
        let file = FileSource::open(path).expect("corpus file opens");
        let (counting, stats) = CountingSource::new(Box::new(file));
        let (source, ranges) = SeenRanges::wrap(Box::new(counting));

        let started = Instant::now();
        let Ok((document, provenance)) = Document::open_repairing(Box::new(source)) else {
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

        let handed_out = stats.total();
        let looked_at = covered(
            ranges
                .lock()
                .expect("the range log outlives every reader")
                .clone(),
        );
        println!(
            "{}: looked at {looked_at} of {len} bytes ({}%), {handed_out} handed out ({}%), {} pages, first page in {:?}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            looked_at * 100 / len,
            handed_out * 100 / len,
            document.page_count().unwrap_or(-1),
            elapsed
        );
        // Compared as a product rather than through an integer percentage,
        // which truncates and would pass a file at 25.99% of a 25% budget.
        assert!(
            looked_at * 100 <= BUDGET_PERCENT * len,
            "opening looked at {looked_at} of {len} bytes, over the {BUDGET_PERCENT}% budget"
        );
        assert!(
            handed_out <= REREAD_CEILING * looked_at,
            "opening handed out {handed_out} bytes for the {looked_at} it looked at, over the {REREAD_CEILING}x re-read ceiling"
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
        common::missing(&format!("{} is absent", path.display()));
        return;
    }

    let file = FileSource::open(&path).expect("seed opens");
    let (counting, stats) = CountingSource::new(Box::new(file));
    let document = Document::open(Box::new(counting)).expect("seed opens clean");
    let after_open = stats.total();

    let catalog = document.catalog().expect("catalog resolves");
    let after_catalog = stats.total();
    assert!(
        after_catalog > after_open,
        "resolving the catalog must be what reads the catalog"
    );
    assert!(catalog.contains(b"Pages"));

    // The same object again comes from the cache, not from the source.
    let before_repeat = stats.total();
    document.catalog().expect("catalog resolves again");
    assert_eq!(
        stats.total(),
        before_repeat,
        "a cached object must not be re-read"
    );
}
