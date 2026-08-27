//! Hostile input: hayro's fuzzer crash corpus, plus a byte-truncation sweep
//! over the seeds. Nothing here has to open. Everything here has to either
//! open or return a typed error, and nothing may panic, hang or read past the
//! end of the file.

mod common;

use common::{corpus_dir, pdfs_in, Tally};
use onionskin_cos::{BytesSource, Document, Error};

/// Opens whatever comes back and exercises it a little, so a bad parse shows
/// up as a panic here rather than in a later milestone.
fn survives(bytes: Vec<u8>) -> Result<bool, Error> {
    let (mut document, _provenance) = Document::open_repairing(Box::new(BytesSource::new(bytes)))?;
    let _ = document.catalog();
    let _ = document.page_count();
    let _ = document.first_page();
    for (number, _) in document.xref().iter().take(64) {
        let _ = document.get(number);
    }
    document.incremental_section()?;

    // The two verbs that rewrite structures the file itself supplied: the free
    // list, and the cross-reference entries a mid-session scan replaces. The
    // fuzz target drives both as well, but that one needs a nightly toolchain
    // and this runs everywhere. Neither may panic; either may refuse.
    let Some(number) = document.xref().iter().map(|(n, _)| n).find(|n| *n != 0) else {
        return Ok(true);
    };
    if document.delete_object(number).is_ok() {
        assert!(
            document.get(number).is_err(),
            "object {number} resolved after being deleted"
        );
        let _ = document.incremental_section();
    }
    let was_clean = document.provenance().is_clean();
    if document.escalate_to_scan(number).is_ok() {
        if was_clean && !document.provenance().is_clean() {
            assert!(
                document.get(number).is_ok(),
                "escalating reported a repair without recovering object {number}"
            );
        }
        let _ = document.incremental_section();
    } else {
        assert_eq!(
            document.provenance().is_clean(),
            was_clean,
            "a failed escalation changed the provenance"
        );
    }
    Ok(true)
}

#[test]
fn the_fuzzer_crash_corpus_never_panics() {
    let Some(dir) = corpus_dir("external/hayro/pdfs/load") else {
        return;
    };
    let files = pdfs_in(&dir);
    assert!(!files.is_empty(), "the load corpus holds no PDFs");

    let mut tally = Tally::new("external/hayro/pdfs/load (fuzzed)");
    for path in &files {
        let bytes = std::fs::read(path).expect("corpus file is readable");
        match survives(bytes) {
            Ok(_) => tally.pass(path),
            // A typed refusal is a pass for this test: the contract is "no
            // panic and no silent nonsense", not "opens everything".
            Err(e) => tally.skip(path, e.category()),
        }
    }
    tally.report();
    assert_eq!(
        tally.failure_count(),
        0,
        "hostile input must never produce an untyped failure"
    );
}

/// An xref stream can claim that an object lives inside an object stream that
/// is that same object. Reaching it must be a typed error, not an unbounded
/// recursion, so the file below keeps a valid catalog to get past the open-time
/// structure check and hides the cycle behind object 2.
#[test]
fn an_object_that_contains_itself_is_refused_rather_than_recursed() {
    let header = b"%PDF-1.5\n";
    let catalog = b"3 0 obj\n<</Type/Catalog>>\nendobj\n";
    let catalog_at = header.len();
    let xref_at = catalog_at + catalog.len();

    // /W [1 2 1]: type, then a two-byte field, then one byte.
    let be = |v: usize| [(v >> 8) as u8, v as u8];
    let mut rows = Vec::new();
    rows.extend_from_slice(&[0, 0, 0, 255]);
    rows.extend_from_slice(&[1, be(xref_at)[0], be(xref_at)[1], 0]);
    // Object 2 claims to be compressed inside object stream 2, which is itself.
    rows.extend_from_slice(&[2, 0, 2, 0]);
    rows.extend_from_slice(&[1, be(catalog_at)[0], be(catalog_at)[1], 0]);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(catalog);
    bytes.extend_from_slice(
        b"1 0 obj\n<</Type/XRef/Size 4/W[1 2 1]/Index[0 4]/Root 3 0 R/Length 16>>\nstream\n",
    );
    bytes.extend_from_slice(&rows);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());

    let (document, _) =
        Document::open_repairing(Box::new(BytesSource::new(bytes))).expect("the file opens");
    assert!(
        document.catalog().is_ok(),
        "the crafted file must reach its catalog, or it is not testing the cycle"
    );
    match document.get(2) {
        Err(Error::DepthExceeded { .. }) => {}
        Err(other) => panic!("expected DepthExceeded, got {other}"),
        Ok(_) => panic!("a self-referential object must not resolve"),
    }
}

/// `/Index` is a pair of arbitrary integers from the file. A start near
/// `i64::MAX` makes the object number of the second row overflow, which is a
/// debug panic and a silent wrap in release.
#[test]
fn an_xref_stream_index_near_the_integer_limit_does_not_overflow() {
    let header = b"%PDF-1.5\n";
    let catalog = b"2 0 obj\n<</Type/Catalog>>\nendobj\n";
    let xref_at = header.len() + catalog.len();

    // Eight /W [1 2 1] rows, so the loop runs past the point where
    // `start + i` leaves i64.
    let rows = [1u8, 0, 0, 0].repeat(8);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(catalog);
    bytes.extend_from_slice(
        format!(
            "1 0 obj\n<</Type/XRef/Size 4/W[1 2 1]/Index[{} 8]/Root 2 0 R/Length {}>>\nstream\n",
            i64::MAX - 2,
            rows.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&rows);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());

    // The contract is a typed outcome, not a particular one: this file names
    // no reachable objects, so repair is the expected route.
    let _ = survives(bytes);
}

#[test]
fn truncating_a_seed_at_every_length_never_panics() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let mut opened = 0usize;
    for path in pdfs_in(&dir) {
        let original = std::fs::read(&path).expect("seed is readable");
        for len in 0..original.len() {
            if survives(original[..len].to_vec()).is_ok() {
                opened += 1;
            }
        }
    }
    // Some prefixes are genuinely recoverable; the point is that the rest fail
    // with an error rather than a panic.
    println!("truncation sweep: {opened} prefixes recovered a document");
}
