//! `Document::sections`: the generations a file is made of, as byte ranges
//! that partition it. This is the generations panel's data source and the
//! offset a revert truncates at, so a range that is off by a byte is a revert
//! that writes a file the user did not have.

mod common;

use std::path::Path;

use common::{corpus_dir, pdfs_in, Tally};
use onionskin_cos::{BytesSource, Document, Error, FileSource, Object, Section};

fn open(bytes: &[u8]) -> Document {
    Document::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the fixture opens clean")
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

/// Appends one generation carrying `producer`, and returns the new bytes.
fn edited(bytes: &[u8], producer: &[u8]) -> Vec<u8> {
    let mut document = open(bytes);
    document
        .set_info_field("Producer", Object::String(producer.to_vec()))
        .expect("the info field is settable");
    document.save_to_vec().expect("save")
}

fn producer_of(document: &Document) -> Option<String> {
    let info = document.trailer().get(b"Info")?;
    let info = document.resolve(info).ok()?;
    match document.resolve(info.as_dict()?.get(b"Producer")?).ok()? {
        Object::String(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        _ => None,
    }
}

/// The boundaries are checked against lengths this test knows independently:
/// each generation's own file length before the next one was appended. That is
/// the oracle - the walk is not asked to agree with itself.
#[test]
fn every_generation_is_reported_with_the_range_it_occupies() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let first = std::fs::read(dir.join("hello.pdf")).expect("the seed is readable");
    let second = edited(&first, b"generation two");
    let third = edited(&second, b"generation three");

    let document = open(&third);
    assert_eq!(
        document.sections().expect("the chain walks"),
        vec![
            Section {
                start: 0,
                end: first.len() as u64
            },
            Section {
                start: first.len() as u64,
                end: second.len() as u64
            },
            Section {
                start: second.len() as u64,
                end: third.len() as u64
            },
        ],
        "three saves are three generations, each ending where the next begins"
    );

    // The ranges are what a revert truncates at, so each start must be the
    // exact byte at which the generation before it is whole.
    let sections = document.sections().expect("the chain walks");
    assert_eq!(
        &third[..sections[1].start as usize],
        &first[..],
        "truncating at the second generation's start must give the first, byte for byte"
    );
    assert_eq!(
        &third[..sections[2].start as usize],
        &second[..],
        "and truncating at the third's must give the second"
    );
    assert_eq!(
        producer_of(&open(&third[..sections[2].start as usize])).as_deref(),
        Some("generation two"),
        "which is the generation the user would be looking at"
    );
}

#[test]
fn a_file_with_one_generation_is_one_section_covering_all_of_it() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    for path in pdfs_in(&dir) {
        let bytes = std::fs::read(&path).expect("the seed is readable");
        assert_eq!(
            open(&bytes).sections().expect("the chain walks"),
            vec![Section {
                start: 0,
                end: bytes.len() as u64
            }],
            "{}",
            path.display()
        );
    }
}

/// A file whose two sections name each other as `/Prev`. The loader tolerates
/// it - it stops when it revisits a section, so the document opens clean - and
/// a walk that trusted the chain to end would run until the file did.
fn cyclic_prev_chain() -> Vec<u8> {
    let mut bytes = Vec::from(&b"%PDF-1.7\n"[..]);
    let bodies: [&[u8]; 3] = [
        b"<</Type/Catalog/Pages 2 0 R>>",
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Resources<<>>>>",
    ];
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    let first_table = bytes.len();
    bytes.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in &offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    // The offset of the second table is not known yet, so it goes in as a
    // fixed-width placeholder and is patched below. Leading zeros are a legal
    // PDF integer, so the width never changes.
    let placeholder = bytes.len() + b"trailer\n<</Size 4/Root 1 0 R/Prev ".len();
    bytes.extend_from_slice(b"trailer\n<</Size 4/Root 1 0 R/Prev 0000000000>>\n");
    bytes.extend_from_slice(format!("startxref\n{first_table}\n%%EOF\n").as_bytes());

    let spare_at = bytes.len();
    bytes.extend_from_slice(b"4 0 obj\n<</Type/Spare>>\nendobj\n");
    let second_table = bytes.len();
    bytes.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
    bytes.extend_from_slice(format!("4 1\n{spare_at:010} 00000 n \n").as_bytes());
    bytes.extend_from_slice(
        format!("trailer\n<</Size 5/Root 1 0 R/Prev {first_table}>>\nstartxref\n{second_table}\n%%EOF\n")
            .as_bytes(),
    );

    let patch = format!("{second_table:010}");
    bytes[placeholder..placeholder + patch.len()].copy_from_slice(patch.as_bytes());
    bytes
}

#[test]
fn a_cyclic_prev_chain_is_reported_rather_than_walked_forever() {
    let bytes = cyclic_prev_chain();
    let document = open(&bytes);
    assert_eq!(
        document.page_count().ok(),
        Some(1),
        "the fixture has to be a document that opens, or it tests nothing"
    );

    match document.sections() {
        Err(Error::SectionChain { detail, .. }) => assert!(
            detail.contains("already walked"),
            "the report must name the cycle, not something else: {detail}"
        ),
        other => panic!("a cyclic /Prev must be reported, got {other:?}"),
    }
}

/// A document that opens by repair still has to say honestly that it cannot
/// name the file's generations, rather than reporting the one range it can see.
#[test]
fn a_startxref_that_points_at_nothing_is_reported() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let bytes = std::fs::read(dir.join("hello.pdf")).expect("the seed is readable");
    let at = bytes
        .windows(9)
        .rposition(|w| w == b"startxref")
        .expect("the seed has a startxref");
    let mut broken = bytes[..at].to_vec();
    broken.extend_from_slice(b"startxref\n7\n%%EOF\n");

    let (document, provenance) = Document::open_repairing(Box::new(BytesSource::new(broken)))
        .expect("the file still opens, by scanning");
    assert!(!provenance.is_clean());
    match document.sections() {
        Err(Error::SectionChain { .. }) => {}
        other => panic!("a startxref that names no section must be reported, got {other:?}"),
    }
}

/// Every file in the external corpora that carries more than one `%%EOF`: the
/// ranges must partition it exactly. A floor rather than an exact count, so a
/// corpus refresh that adds files does not fail the build for adding them.
fn partitions(path: &Path, tally: &mut Tally) {
    let bytes = std::fs::read(path).expect("the corpus file is readable");
    if count(&bytes, b"%%EOF") < 2 {
        return;
    }
    let source = match FileSource::open(path) {
        Ok(s) => s,
        Err(e) => return tally.skip(path, e.category()),
    };
    // A file that does not open, and one whose own cross-reference chain the
    // opener had to repair, are both counted apart from this walk: neither is
    // evidence about the ranges, and hiding them in the failure bucket would
    // put a floor on the wrong number. The size assertion below is what keeps
    // the counted set from emptying quietly.
    let document = match Document::open_repairing(Box::new(source)) {
        Ok((document, provenance)) if provenance.is_clean() => document,
        Ok(_) => return tally.skip(path, "needed-repair"),
        Err(e) => return tally.skip(path, e.category()),
    };
    let sections = match document.sections() {
        Ok(sections) => sections,
        Err(e) => return tally.record(path, &e),
    };

    let Some(first) = sections.first() else {
        return tally.fail(path, "no-sections", "the walk reported no generations");
    };
    if first.start != 0 {
        return tally.fail(
            path,
            "gap",
            &format!("the first generation starts at {}", first.start),
        );
    }
    for pair in sections.windows(2) {
        if pair[0].end != pair[1].start {
            return tally.fail(
                path,
                "gap",
                &format!(
                    "a generation ends at {} and the next starts at {}",
                    pair[0].end, pair[1].start
                ),
            );
        }
    }
    // Every boundary but the first has to sit at the end of a generation, which
    // in a PDF means the `%%EOF` that closes one. Checked against the file's
    // bytes rather than against the walk that produced the number.
    for section in sections.iter().skip(1) {
        let before = &bytes[..section.start as usize];
        let before = before.strip_suffix(b"\n").unwrap_or(before);
        let before = before.strip_suffix(b"\r").unwrap_or(before);
        if !before.ends_with(b"%%EOF") {
            return tally.fail(
                path,
                "not-at-an-eof",
                &format!(
                    "a generation starts at {}, which is not past a %%EOF",
                    section.start
                ),
            );
        }
    }

    // Every boundary is a point a revert may truncate at, so the bytes before
    // it have to be a document. A walk that reported a cross-reference table
    // that is not the end of a generation - a linearized file's first-page
    // table is one - still partitions the file, so this is the assertion that
    // separates the two.
    for section in sections.iter().skip(1) {
        let prefix = BytesSource::new(bytes[..section.start as usize].to_vec());
        match Document::open_repairing(Box::new(prefix)).and_then(|(d, _)| d.page_count()) {
            Ok(_) => {}
            Err(e) => {
                return tally.fail(
                    path,
                    "prefix-not-a-document",
                    &format!(
                        "truncating at {} leaves something that is not: {e}",
                        section.start
                    ),
                )
            }
        }
    }

    let last = sections.last().expect("checked above");
    if last.end != bytes.len() as u64 {
        return tally.fail(
            path,
            "short",
            &format!(
                "the last generation ends at {} of {}",
                last.end,
                bytes.len()
            ),
        );
    }
    tally.pass(path);
}

/// Three named files, so the sweep above is not the only thing making this
/// claim. Each carries several generations, and the count is checked against
/// the number of `%%EOF` markers in the file, which is one per generation.
#[test]
fn named_multi_generation_fixtures_report_every_generation() {
    let Some(dir) = corpus_dir("external/verapdf/PDF_UA-1") else {
        return;
    };
    for (relative, generations) in [
        (
            "7.4 Headings/7.4.2 Numbered headings/7.4.2-t01-pass-b.pdf",
            7,
        ),
        ("7.7 Mathematical expressions/7.7-t01-pass-b.pdf", 6),
        ("7.9 Notes and references/7.9-t01-pass-a.pdf", 4),
    ] {
        let path = dir.join(relative);
        let bytes = std::fs::read(&path).expect("the fixture is readable");
        assert_eq!(
            count(&bytes, b"%%EOF"),
            generations,
            "{relative} must carry one end marker per generation, or the count below is not an oracle"
        );

        let sections = open(&bytes).sections().expect("the chain walks");
        assert_eq!(sections.len(), generations, "{relative}");
        assert_eq!(sections[0].start, 0, "{relative}");
        assert_eq!(
            sections.last().expect("at least one").end,
            bytes.len() as u64,
            "{relative}"
        );
    }
}

/// A linearized file carries a `%%EOF` after its first-page cross-reference,
/// at the front of the file, and that marker ends no generation: the bytes
/// before it are a header and a table. Counting cross-reference sections
/// rather than generations reports it as one, and the sweep above cannot see
/// that, because a phantom split still partitions the file.
#[test]
fn a_linearized_files_first_page_table_is_not_a_generation() {
    let Some(root) = corpus_dir("external") else {
        return;
    };

    // Linearized, never updated: two `%%EOF` markers, one generation.
    let once = std::fs::read(root.join("hayro/pdfs/custom/font_standard_2.pdf"))
        .expect("the fixture is readable");
    assert_eq!(count(&once, b"%%EOF"), 2, "the fixture must be linearized");
    assert_eq!(
        open(&once).sections().expect("the chain walks"),
        vec![Section {
            start: 0,
            end: once.len() as u64
        }],
        "a linearized file that was never updated is one generation"
    );

    // Linearized and updated since: three markers, two generations.
    let updated = std::fs::read(
        root.join("verapdf/PDF_UA-1/7.4 Headings/7.4.4 Unnumbered headings/7.4.4-t01-pass-a.pdf"),
    )
    .expect("the fixture is readable");
    assert_eq!(count(&updated, b"%%EOF"), 3);
    let document = open(&updated);
    let sections = document.sections().expect("the chain walks");
    assert_eq!(
        sections.len(),
        2,
        "one generation per update, not per table"
    );

    // The boundary that remains is one a revert may truncate at.
    let rolled_back = open(&updated[..sections[1].start as usize]);
    assert_eq!(
        rolled_back.page_count().ok(),
        document.page_count().ok(),
        "truncating at the reported start must leave the document as it was"
    );
}

/// A document cos had to rebuild the cross-reference for was not read through
/// the chain in the file, so it has no generations to report. Handing back
/// ranges derived from that chain would be reporting a structure this document
/// does not rest on.
#[test]
fn a_document_whose_cross_reference_was_rebuilt_reports_no_chain() {
    let mut bodies: Vec<&[u8]> = common::skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    // The table lies about where the catalog is, which is what sends the open
    // down the scan. The `startxref` still points at a real table, so a walk
    // of the chain would succeed and report ranges.
    let bytes = common::classic_pdf(&bodies, &[(1, 3)]);

    let (document, provenance) = Document::open_repairing(Box::new(BytesSource::new(bytes)))
        .expect("the file opens by scanning");
    assert!(!provenance.is_clean());
    assert_eq!(document.page_count().ok(), Some(1), "the scan recovered it");

    match document.sections() {
        Err(Error::SectionChain { detail, .. }) => assert!(
            detail.contains("rebuilt"),
            "the report must say why there is no chain: {detail}"
        ),
        other => panic!("a rebuilt cross-reference has no chain to report, got {other:?}"),
    }
}

#[test]
fn multi_generation_corpus_files_partition_into_sections() {
    let Some(external) = corpus_dir("external") else {
        return;
    };
    let mut tally = Tally::new("external, files with more than one %%EOF");
    for file in pdfs_in(&external) {
        partitions(&file, &mut tally);
    }
    tally.report();

    let considered = tally.passed.len() + tally.failure_count();
    assert!(
        considered >= 200,
        "only {considered} corpus files carry more than one %%EOF, so this walk proved nothing"
    );
    assert!(
        tally.passed.len() * 100 >= considered * 99,
        "{} of {considered} multi-generation files partitioned, below the 99% floor",
        tally.passed.len()
    );
}
