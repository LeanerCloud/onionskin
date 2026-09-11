//! Decision 11, as a measurement rather than a claim: opening a document and
//! reaching its first page must not read the whole file.

mod common;

use std::time::Instant;

use common::{classic_pdf, corpus_dir, corpus_root, pdfs_in, skeleton};
use onionskin_cos::{
    BytesSource, CountingSource, Document, Error, FileSource, ObjRef, Object, Origin, Provenance,
    Span,
};

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
        let (counting, stats) = CountingSource::new(Box::new(file));

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

        let read = stats.total();
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
    let (counting, stats) = CountingSource::new(Box::new(file));
    let document = Document::open(Box::new(counting)).expect("seed opens clean");
    let after_open = stats.total();

    let catalog = document.catalog().expect("catalog resolves");
    let after_catalog = stats.total();
    assert_eq!(
        after_catalog, after_open,
        "validation already parsed the catalog before the public access"
    );
    assert!(catalog.contains(b"Pages"));

    let before_page = stats.total();
    document.first_page().expect("first page resolves");
    assert!(
        stats.total() > before_page,
        "an unvalidated leaf must still be read on first access"
    );
    let after_page = stats.total();

    // The same objects again come from the cache, not from the source.
    document.first_page().expect("first page resolves again");
    document.catalog().expect("catalog resolves again");
    assert_eq!(
        stats.total(),
        after_page,
        "a cached object must not be re-read"
    );
}

#[test]
fn validated_large_page_tree_is_reused_before_lazy_leaf_access() {
    const PAGE_COUNT: usize = 10_000;

    let mut bodies = skeleton();
    let pages_body = format!(
        "<</Type/Pages/Kids[{}]/Count {PAGE_COUNT}>>",
        (3..=PAGE_COUNT + 2)
            .map(|number| format!("{number} 0 R "))
            .collect::<String>()
    );
    bodies[1] = pages_body.as_bytes();
    let leaf = bodies[2];
    bodies.extend((0..PAGE_COUNT - 1).map(|_| leaf));
    let mut bytes = classic_pdf(&bodies, &[]);

    let pages_at = bytes
        .windows(b"2 0 obj\n".len())
        .position(|window| window == b"2 0 obj\n")
        .expect("the generated Pages object has its exact header");
    bytes[pages_at + 2] = b'7';
    let pages_end = pages_at
        + bytes[pages_at..]
            .windows(b"\nendobj".len())
            .position(|window| window == b"\nendobj")
            .expect("the generated Pages object has an endobj")
        + b"\nendobj".len();
    let expected_pages = format!("2 7 obj\n{pages_body}\nendobj").into_bytes();
    let source_bytes = bytes.clone();

    let (counting, stats) = CountingSource::new(Box::new(BytesSource::new(bytes)));
    let (document, provenance) =
        Document::open_repairing(Box::new(counting)).expect("large flat page tree opens");
    assert_eq!(provenance, Provenance::Clean);
    let after_open = stats.total();

    document.catalog().expect("catalog resolves");
    document.get(2).expect("Pages resolves");
    assert_eq!(
        document.page_count().expect("page count resolves"),
        PAGE_COUNT as i64
    );
    assert_eq!(
        stats.total(),
        after_open,
        "validated catalog and Pages dictionaries must be reused"
    );
    let pages = document.get(2).expect("Pages remains available");
    assert_eq!(pages.objref, ObjRef::new(2, 7));
    assert_eq!(
        pages.origin,
        Origin::File(Span::new(pages_at as u64, pages_end as u64))
    );
    assert_eq!(pages.recovered_boundary, None);
    let span = pages.origin.file_span().expect("Pages has a file span");
    assert_eq!(
        &source_bytes[span.start as usize..span.end as usize],
        &expected_pages
    );

    let pages_dict = pages.object.as_dict().expect("Pages is a dictionary");
    assert_eq!(
        pages_dict.get(b"Count").and_then(Object::as_integer),
        Some(PAGE_COUNT as i64)
    );
    let kids = pages_dict
        .get(b"Kids")
        .and_then(Object::as_array)
        .expect("Pages has Kids");
    let expected_kids: Vec<_> = (3..=PAGE_COUNT as u32 + 2)
        .map(|number| Object::Ref(ObjRef::new(number, 0)))
        .collect();
    assert_eq!(kids, expected_kids.as_slice());

    let before_leaf = stats.total();
    let first = document.first_page().expect("first page resolves");
    assert_eq!(first.objref.number, 3);
    let first_dict = first.object.as_dict().expect("first page is a dictionary");
    assert_eq!(first_dict.get(b"Type"), Some(&Object::name("Page")));
    let expected_media_box = [
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(200),
        Object::Integer(100),
    ];
    assert_eq!(
        first_dict.get(b"MediaBox").and_then(Object::as_array),
        Some(expected_media_box.as_slice())
    );
    assert!(
        stats.total() > before_leaf,
        "the unvalidated leaf must be read"
    );
    let after_leaf = stats.total();
    let cached = document.get(3).expect("first leaf is cached");
    assert_eq!(cached, first);
    assert_eq!(stats.total(), after_leaf);
    let repeated_first = document.first_page().expect("first page resolves again");
    assert_eq!(repeated_first, first);
    let repeated = document.get(3).expect("first leaf remains cached");
    assert_eq!(repeated, first);
    assert_eq!(stats.total(), after_leaf);
}

#[test]
fn edits_and_deletions_supersede_validated_dictionaries() {
    let bytes = classic_pdf(&skeleton(), &[]);
    let mut edited =
        Document::open(Box::new(BytesSource::new(bytes.clone()))).expect("fixture opens clean");
    let mut catalog = edited.catalog().expect("catalog resolves");
    catalog.set("Marker", Object::name("Edited"));
    edited
        .set_object(1, 0, Object::Dict(catalog))
        .expect("catalog can be replaced");
    let parsed = edited.get(1).expect("edited catalog resolves");
    assert_eq!(parsed.origin, Origin::Pending);
    assert_eq!(parsed.recovered_boundary, None);
    assert_eq!(
        parsed
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Marker"))
            .and_then(Object::as_name)
            .map(|name| name.as_bytes()),
        Some(&b"Edited"[..])
    );

    let mut deleted =
        Document::open(Box::new(BytesSource::new(bytes))).expect("fixture opens clean");
    deleted.delete_object(2).expect("Pages can be deleted");
    match deleted.get(2) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref, ObjRef::new(2, 1)),
        other => panic!("deleted Pages must stay missing, got {other:?}"),
    }
}

#[test]
fn compressed_structural_objects_remain_lazy() {
    for object_number in [1u32, 2] {
        let mut bytes = common::xref_stream_pdf(None);
        let marker = bytes
            .windows(b"\nstream\n".len())
            .position(|window| window == b"\nstream\n")
            .expect("xref stream marker");
        let rows_at = marker + b"\nstream\n".len();
        let row_at = rows_at + object_number as usize * 4;
        bytes[row_at..row_at + 4].copy_from_slice(&[2, 0, 3, 0]);

        let document = Document::open(Box::new(BytesSource::new(bytes)))
            .expect("structural validation only reaches compressed container");
        match document.get(object_number) {
            Err(Error::Unrecoverable { detail }) => assert!(
                detail.contains("object 3 is referenced as an object stream but is not a stream"),
                "unexpected compressed access error: {detail}"
            ),
            other => panic!("compressed structural object must stay undecoded, got {other:?}"),
        }
    }
}
