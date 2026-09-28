//! The facts the Description tab reports about the file: version, page size,
//! tagged, and fast web view. Each is read from the bytes, so each gets a
//! fixture that states it and one that does not.

mod common;

use onionskin_core::metadata::document_facts;
use onionskin_cos::{BytesSource, Document as CosDocument};

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

/// A plain document: 1.7 header, one 612x792 page, no tag tree, not
/// linearized. Every fact reads, and three of them say no.
#[test]
fn an_ordinary_document_reports_its_version_and_size_and_says_no_to_the_rest() {
    let bytes = common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>".to_vec(),
    ]);
    let facts = document_facts(&open(&bytes));
    assert_eq!(
        facts.version.as_deref(),
        Some("1.7"),
        "the header's version"
    );
    assert_eq!(
        facts.page_size.as_deref(),
        Some("612 x 792"),
        "the page size"
    );
    assert!(!facts.tagged, "nothing says the document is tagged");
    assert!(!facts.linearized, "nothing says it is linearized");
}

/// A catalog's `/Version` outranks the header, which is ISO 32000-1 7.5.2: the
/// catalog version applies to the whole file when the two disagree.
#[test]
fn the_catalog_version_wins_over_the_header() {
    let bytes = common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Version /2.0 >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
    ]);
    let facts = document_facts(&open(&bytes));
    assert_eq!(
        facts.version.as_deref(),
        Some("2.0"),
        "the catalog raises the header's 1.7"
    );
}

/// `/MarkInfo /Marked true` is the only thing that means tagged here. A file
/// carrying `/MarkInfo` without the flag, or with it false, is not tagged, and
/// reading the dictionary's presence alone would say it was.
#[test]
fn a_tag_tree_is_reported_only_when_the_file_marks_itself_tagged() {
    let page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec();
    for (catalog, expected) in [
        (
            &b"<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> >>"[..],
            true,
        ),
        (
            &b"<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked false >> >>"[..],
            false,
        ),
        (
            &b"<< /Type /Catalog /Pages 2 0 R /MarkInfo << >> >>"[..],
            false,
        ),
        (&b"<< /Type /Catalog /Pages 2 0 R >>"[..], false),
    ] {
        let bytes = common::pdf(&[
            catalog.to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            page.clone(),
        ]);
        assert_eq!(
            document_facts(&open(&bytes)).tagged,
            expected,
            "{}",
            String::from_utf8_lossy(catalog)
        );
    }
}

/// Linearization lives in the file's FIRST object, which is why this reads
/// object 1 and not the catalog: a linearized file's first object is the
/// linearization dictionary.
#[test]
fn a_linearized_file_is_told_by_its_first_object() {
    let mut bytes = common::pdf(&[
        b"<< /Linearized 1 /L 1234 /H [100 200] >>".to_vec(),
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
    ]);
    assert!(
        document_facts(&open(&bytes)).linearized,
        "the first object carries /Linearized"
    );
    // The same file without it is not linearized, which proves the test reads
    // that key rather than noticing there are four objects.
    let plain = common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
    ]);
    assert!(!document_facts(&open(&plain)).linearized);
    bytes.clear();
}
