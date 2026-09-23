//! `Document::sections`: the generations a file is made of, as byte ranges
//! that partition it. This is the generations panel's data source and the
//! offset a revert truncates at, so a range that is off by a byte is a revert
//! that writes a file the user did not have.

mod common;

use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use common::{corpus_dir, corpus_root, missing, pdfs_in, Tally};
use onionskin_cos::{
    BytesSource, Document, Error, FileSource, Object, RepairReason, Section, Source,
};

fn open(bytes: &[u8]) -> Document {
    Document::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the fixture opens clean")
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

const LINEARIZED_HAYRO: &str = "external/hayro/pdfs/custom/font_standard_2.pdf";
const LINEARIZED_VERAPDF: &str =
    "external/verapdf/PDF_UA-1/7.4 Headings/7.4.4 Unnumbered headings/7.4.4-t01-pass-a.pdf";

fn linearized_fixtures() -> Option<(Vec<u8>, Vec<u8>)> {
    let Some(root) = corpus_root() else {
        missing("no corpus found; set ONIONSKIN_CORPUS to the corpus directory");
        return None;
    };
    let paths = [root.join(LINEARIZED_HAYRO), root.join(LINEARIZED_VERAPDF)];
    for path in &paths {
        match std::fs::metadata(path) {
            Ok(metadata) if !metadata.is_file() => {
                missing(&format!("{} is not a regular file", path.display()));
                return None;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing(&format!("{} is absent", path.display()));
                return None;
            }
            Err(error) => panic!("metadata for {} failed: {error}", path.display()),
        }
    }
    let once = std::fs::read(&paths[0])
        .unwrap_or_else(|error| panic!("reading {} failed: {error}", paths[0].display()));
    let updated = std::fs::read(&paths[1])
        .unwrap_or_else(|error| panic!("reading {} failed: {error}", paths[1].display()));
    Some((once, updated))
}

fn retained_temp_root() -> std::path::PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock is after the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "onionskin-cos-sections-{}-{timestamp}",
        std::process::id()
    ));
    std::fs::create_dir(&root).expect("fresh temporary corpus parent must not already exist");
    println!("retaining temporary corpus parent: {}", root.display());
    root
}

fn write_placeholder(root: &Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("fixture path has a parent"))
        .expect("temporary corpus directories can be created");
    std::fs::write(path, []).expect("temporary corpus placeholder can be written");
}

fn run_linearized_child(test_name: &str, root: &Path, required: bool) -> std::process::Output {
    let executable = std::env::current_exe().expect("sections test executable is available");
    let mut command = Command::new(executable);
    command
        .arg(test_name)
        .arg("--exact")
        .arg("--nocapture")
        .env("ONIONSKIN_CORPUS", root)
        .env_remove("ONIONSKIN_CORPUS_REQUIRED");
    if required {
        command.env("ONIONSKIN_CORPUS_REQUIRED", "1");
    }
    command
        .output()
        .unwrap_or_else(|error| panic!("running child {test_name} failed: {error}"))
}

fn assert_linearized_child(test_name: &str, root: &Path, expected_reason: &str, required: bool) {
    let output = run_linearized_child(test_name, root, required);
    let mut transcript = String::from_utf8_lossy(&output.stdout).into_owned();
    transcript.push_str(&String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        transcript.matches("running 1 test").count(),
        1,
        "child must run exactly one selected test; transcript:\n{transcript}"
    );
    assert_eq!(
        output.status.success(),
        !required,
        "{} mode has wrong status; transcript:\n{}",
        if required { "required" } else { "normal" },
        transcript
    );
    let marker = if required {
        "corpus required but"
    } else {
        "SKIPPED:"
    };
    assert!(
        transcript.contains(marker) && transcript.contains(expected_reason),
        "child transcript lacks {marker:?} and {expected_reason:?}:\n{transcript}"
    );
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

#[test]
fn a_null_prev_ends_the_section_chain() {
    let (classic, _) = classic_footer(b"\n", b"\n");
    let trailer_end = classic
        .windows(13)
        .rposition(|window| window == b">>\nstartxref\n")
        .expect("classic fixture has a trailer before startxref");

    let mut with_null = classic.clone();
    with_null.splice(trailer_end..trailer_end, b"/Prev null".iter().copied());
    let document = open(&with_null);
    assert!(document.trailer().get(b"Prev").is_none());
    assert_eq!(
        document.sections().expect("null /Prev ends the chain"),
        vec![Section {
            start: 0,
            end: with_null.len() as u64,
        }]
    );

    let mut malformed = classic;
    malformed.splice(trailer_end..trailer_end, b"/Prev true".iter().copied());
    let malformed_document = open(&malformed);
    assert!(matches!(
        malformed_document.trailer().get(b"Prev"),
        Some(Object::Bool(true))
    ));
    assert!(matches!(
        malformed_document.sections(),
        Err(Error::SectionChain { .. })
    ));
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
    let Some((once, updated)) = linearized_fixtures() else {
        return;
    };

    // Linearized, never updated: two `%%EOF` markers, one generation.
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

/// One corrupt offset must not talk the walk out of the generation it is in.
/// A table naming an object past its own `%%EOF` is how a linearized file's
/// first-page table is told apart from a generation, and an entry pointing
/// past the end of the file looks exactly like one - `issue391.pdf` in the
/// corpus carries `7000000788` and opens clean.
#[test]
fn an_offset_past_the_end_of_the_file_is_a_corrupt_entry_not_a_later_generation() {
    let mut bodies: Vec<&[u8]> = common::skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    let bytes = common::classic_pdf(&bodies, &[(4, 7_000_000_788)]);

    let document = open(&bytes);
    assert_eq!(
        document.sections().expect("the chain walks"),
        vec![Section {
            start: 0,
            end: bytes.len() as u64
        }],
        "the file has one generation, whatever its table says about object 4"
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

/// Builds the classic fixture with a deliberately mixed footer. Keeping the
/// separator before `startxref` as LF makes `footer_start` independently
/// equal to `startxref` minus one, while the other footer line endings vary.
fn classic_footer(separator: &[u8], final_eol: &[u8]) -> (Vec<u8>, usize) {
    let base = common::classic_pdf(&common::skeleton(), &[]);
    let before_startxref = b"\nstartxref\n";
    let start = base
        .windows(before_startxref.len())
        .rposition(|window| window == before_startxref)
        .expect("classic fixture has a final startxref")
        + before_startxref.len();
    let number_end = base[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|relative| start + relative)
        .expect("classic fixture has a startxref number");
    let value = &base[start..number_end];
    let mut bytes = base[..start - before_startxref.len() + 1].to_vec();
    bytes.extend_from_slice(b"startxref");
    bytes.extend_from_slice(separator);
    bytes.extend_from_slice(value);
    bytes.extend_from_slice(separator);
    bytes.extend_from_slice(b"%%EOF");
    bytes.extend_from_slice(final_eol);
    let footer_start = bytes
        .windows(9)
        .rposition(|window| window == b"startxref")
        .expect("rebuilt footer has startxref")
        - 1;
    (bytes, footer_start)
}

fn insert_before_final_startxref(bytes: &[u8], inserted: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(9)
        .rposition(|window| window == b"startxref")
        .expect("fixture has a final startxref");
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(inserted);
    out.extend_from_slice(&bytes[at..]);
    out
}

fn insert_after_startxref_number(bytes: &[u8], inserted: &[u8]) -> Vec<u8> {
    let startxref = bytes
        .windows(9)
        .rposition(|window| window == b"startxref")
        .expect("fixture has a final startxref");
    let after_keyword = startxref + 9;
    let number_start = after_keyword
        + if bytes[after_keyword..].starts_with(b"\r\n") {
            2
        } else {
            1
        };
    let number_end = bytes[number_start..]
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r')
        .map(|relative| number_start + relative)
        .expect("fixture has a startxref number");
    let eol_len = if bytes[number_end..].starts_with(b"\r\n") {
        2
    } else {
        1
    };
    let insertion = number_end + eol_len;
    let mut out = bytes[..insertion].to_vec();
    out.extend_from_slice(inserted);
    out.extend_from_slice(&bytes[insertion..]);
    out
}

fn insert_before_final_eof_marker(bytes: &[u8], inserted: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(5)
        .rposition(|window| window == b"%%EOF")
        .expect("fixture has a final EOF marker");
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(inserted);
    out.extend_from_slice(&bytes[at..]);
    out
}

fn footer_with_marker_end(marker_end: usize, suffix: &[u8]) -> Vec<u8> {
    let (classic, _) = classic_footer(b"\n", b"\n");
    let startxref = classic
        .windows(9)
        .rposition(|window| window == b"startxref")
        .expect("fixture has a final startxref");
    let marker = classic
        .windows(5)
        .rposition(|window| window == b"%%EOF")
        .expect("fixture has a final EOF marker");
    let padding = marker_end
        .checked_sub(marker + 5)
        .expect("marker end is after the original marker");
    let mut bytes = classic[..startxref].to_vec();
    bytes.extend(std::iter::repeat_n(b' ', padding));
    bytes.extend_from_slice(&classic[startxref..marker + 5]);
    bytes.extend_from_slice(suffix);
    bytes
}

fn assert_saved_boundaries(original: &[u8], saved: &[u8], expected_start: usize) {
    let sections = open(saved).sections().expect("the generation chain walks");
    assert_eq!(
        sections,
        vec![
            Section {
                start: 0,
                end: expected_start as u64,
            },
            Section {
                start: expected_start as u64,
                end: saved.len() as u64,
            },
        ],
        "footer parsing must report the exact old-generation boundary"
    );
    assert_eq!(
        &saved[..original.len()],
        original,
        "saving must preserve every original byte"
    );
    let prefix = open(&saved[..expected_start]);
    assert_eq!(prefix.page_count().ok(), Some(1), "the old prefix reopens");
}

#[test]
fn immediate_footer_ignores_decoys_and_eof_marker_lookalikes() {
    let (classic, _) = classic_footer(b"\n", b"\n");
    for (_name, original) in [
        (
            "comment before startxref",
            insert_before_final_startxref(&classic, b"% decoy %%EOF\n"),
        ),
        (
            "comment after startxref number",
            insert_after_startxref_number(&classic, b"% decoy %%EOF\n"),
        ),
        (
            "junk EOF marker",
            insert_before_final_eof_marker(&classic, b"%%EOFjunk\n"),
        ),
        (
            "ordinary EOF-looking comment",
            insert_before_final_eof_marker(&classic, b"%%EOF decoy\n"),
        ),
    ] {
        let saved = edited(&original, b"footer regression");
        assert_saved_boundaries(&original, &saved, original.len());
    }
}

#[test]
fn footer_search_budget_reserves_lookahead_for_eol_only() {
    let (_, footer_start) = classic_footer(b"\n", b"\n");
    let search_end = footer_start + 4096;

    for (name, suffix) in [("LF", b"\n".as_slice()), ("CRLF", b"\r\n".as_slice())] {
        let bytes = footer_with_marker_end(search_end, suffix);
        assert_eq!(
            open(&bytes).sections().expect("boundary EOL is accepted"),
            vec![Section {
                start: 0,
                end: bytes.len() as u64,
            }],
            "{name} at the search boundary"
        );
    }

    let with_horizontal_lookahead = footer_with_marker_end(search_end, b" \n");
    assert!(
        matches!(
            open(&with_horizontal_lookahead).sections(),
            Err(Error::SectionChain { .. })
        ),
        "horizontal whitespace must fit within the search budget"
    );

    let marker_outside = footer_with_marker_end(search_end + 1, b"\n");
    assert!(
        matches!(
            open(&marker_outside).sections(),
            Err(Error::SectionChain { .. })
        ),
        "marker bytes outside the search budget must be rejected"
    );

    let with_horizontal_pdf_whitespace = footer_with_marker_end(search_end - 2, b"\0\x0c\n");
    assert_eq!(
        open(&with_horizontal_pdf_whitespace)
            .sections()
            .expect("NUL and form-feed are accepted horizontal whitespace"),
        vec![Section {
            start: 0,
            end: with_horizontal_pdf_whitespace.len() as u64,
        }]
    );
}

#[test]
fn footer_line_endings_and_missing_final_eol_preserve_boundaries() {
    for (name, separator, final_eol, has_final_eol) in [
        ("LF", b"\n".as_slice(), b"\n".as_slice(), true),
        ("CR", b"\r".as_slice(), b"\r".as_slice(), true),
        ("CRLF", b"\r\n".as_slice(), b"\r\n".as_slice(), true),
        ("EOF", b"\n".as_slice(), b"".as_slice(), false),
    ] {
        let (original, _) = classic_footer(separator, final_eol);
        if !has_final_eol {
            assert_eq!(
                open(&original).sections().expect("standalone footer walks"),
                vec![Section {
                    start: 0,
                    end: original.len() as u64,
                }],
                "EOF: standalone file has one complete section"
            );
        }
        let saved = edited(&original, b"footer regression");
        let expected_start = if has_final_eol {
            original.len()
        } else {
            original.len() + 1
        };
        assert_saved_boundaries(&original, &saved, expected_start);
        assert_eq!(
            &saved[..original.len()],
            original,
            "{name}: original bytes remain unchanged"
        );
        if !has_final_eol {
            assert_eq!(saved[original.len()], b'\n', "{name}: writer separator");
        }
    }
}

#[test]
fn unrelated_token_between_trailer_and_startxref_refuses_section_walk() {
    let (classic, _) = classic_footer(b"\n", b"\n");
    let bytes = insert_before_final_startxref(&classic, b"5 0 obj\n<</Decoy true>>\nendobj\n");
    let (document, provenance) = Document::open_repairing(Box::new(BytesSource::new(bytes)))
        .expect("the token does not prevent opening the xref");
    assert!(provenance.is_clean(), "the unrelated token is not a repair");
    assert!(
        matches!(document.sections(), Err(Error::SectionChain { .. })),
        "a non-comment token before startxref must be refused"
    );
}

struct FaultSource {
    inner: BytesSource,
    footer_start: u64,
    footer_end: u64,
    fault_at: u64,
    fault: Fault,
    armed: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
enum Fault {
    Short1,
    Io,
    Empty,
    Oversized,
}

impl FaultSource {
    fn new(
        bytes: Vec<u8>,
        footer_start: usize,
        footer_end: usize,
        fault_at: usize,
        fault: Fault,
    ) -> (Self, Arc<AtomicBool>) {
        let armed = Arc::new(AtomicBool::new(false));
        (
            FaultSource {
                inner: BytesSource::new(bytes),
                footer_start: footer_start as u64,
                footer_end: footer_end as u64,
                fault_at: fault_at as u64,
                fault,
                armed: Arc::clone(&armed),
            },
            armed,
        )
    }
}

impl Source for FaultSource {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read_at(&self, offset: u64, len: usize) -> onionskin_cos::Result<Vec<u8>> {
        if !self.armed.load(Ordering::Relaxed)
            || offset < self.footer_start
            || offset >= self.footer_end
        {
            return self.inner.read_at(offset, len);
        }

        if offset < self.fault_at {
            let capped = len.min((self.fault_at - offset) as usize);
            return self.inner.read_at(offset, capped);
        }

        match self.fault {
            Fault::Short1 => self.inner.read_at(offset, len.min(1)),
            Fault::Io => Err(Error::Io(io::Error::other("footer fault"))),
            Fault::Empty => Ok(Vec::new()),
            Fault::Oversized => {
                let mut bytes = self.inner.read_at(offset, len)?;
                while bytes.len() <= len {
                    bytes.push(0);
                }
                Ok(bytes)
            }
        }
    }
}

fn sections_with_fault(
    bytes: Vec<u8>,
    footer_start: usize,
    footer_end: usize,
    fault_at: usize,
    fault: Fault,
) -> onionskin_cos::Result<Vec<Section>> {
    let (source, armed) = FaultSource::new(bytes, footer_start, footer_end, fault_at, fault);
    let (document, provenance) = Document::open_repairing(Box::new(source))?;
    assert!(
        provenance.is_clean(),
        "faults must be armed only after opening"
    );
    armed.store(true, Ordering::Relaxed);
    document.sections()
}

#[test]
fn footer_short_reads_preserve_generation_boundaries() {
    let (original, footer_start) = classic_footer(b"\n", b"\r\n");
    let saved = edited(&original, b"short footer regression");
    let marker_end = original.len() - 2;
    for fault_at in [footer_start, marker_end] {
        let sections = sections_with_fault(
            saved.clone(),
            footer_start,
            original.len(),
            fault_at,
            Fault::Short1,
        )
        .expect("short footer reads remain parseable");
        assert_eq!(
            sections,
            vec![
                Section {
                    start: 0,
                    end: original.len() as u64,
                },
                Section {
                    start: original.len() as u64,
                    end: saved.len() as u64,
                },
            ],
            "one-byte short reads at {fault_at} must preserve the original boundary"
        );
        let prefix = open(&saved[..original.len()]);
        assert_eq!(prefix.page_count().ok(), Some(1), "the old prefix reopens");
    }
}

#[test]
fn footer_io_and_empty_faults_propagate_at_footer_or_eol() {
    let (original, footer_start) = classic_footer(b"\n", b"\r\n");
    let marker_end = original.len() - 2;
    for (fault, kind, label) in [
        (Fault::Io, io::ErrorKind::Other, "io"),
        (Fault::Empty, io::ErrorKind::UnexpectedEof, "empty"),
    ] {
        for fault_at in [footer_start, marker_end] {
            let result = sections_with_fault(
                original.clone(),
                footer_start,
                original.len(),
                fault_at,
                fault,
            );
            match result {
                Err(Error::Io(error)) => assert_eq!(error.kind(), kind, "{label} at {fault_at}"),
                other => panic!("{label} at {fault_at} must propagate as I/O, got {other:?}"),
            }
        }
    }
}

#[test]
fn oversized_footer_reads_are_invalid_data() {
    let (original, footer_start) = classic_footer(b"\n", b"\r\n");
    let marker_end = original.len() - 2;
    for fault_at in [footer_start, marker_end] {
        match sections_with_fault(
            original.clone(),
            footer_start,
            original.len(),
            fault_at,
            Fault::Oversized,
        ) {
            Err(Error::Io(error)) => assert_eq!(error.kind(), io::ErrorKind::InvalidData),
            other => panic!("oversized footer read at {fault_at} got {other:?}"),
        }
    }
}

#[test]
fn linearized_header_bias_keeps_one_and_two_generation_classification() {
    let Some((once, updated)) = linearized_fixtures() else {
        return;
    };
    assert!(
        count(&once, b"startxref\r0\r%%EOF") > 0,
        "the front linearized footer uses the real CR form"
    );

    for (name, bytes, generations) in [("one", once, 1usize), ("two", updated, 2usize)] {
        let expected = open(&bytes)
            .sections()
            .expect("unprefixed linearized chain walks");
        let prefix_len = b"junk-prefix".len() as u64;
        let mut prefixed = b"junk-prefix".to_vec();
        prefixed.extend_from_slice(&bytes);
        let (document, provenance) =
            Document::open_repairing(Box::new(BytesSource::new(prefixed.clone())))
                .expect("junk-prefixed linearized file opens by repair");
        assert!(
            matches!(provenance, onionskin_cos::Provenance::Repaired(report)
                if report.reasons.iter().any(|reason| matches!(reason, RepairReason::JunkBeforeHeader { .. }))),
            "{name}-generation fixture must report junk-before-header repair"
        );
        let sections = document.sections().expect("header-biased chain walks");
        assert_eq!(
            sections.len(),
            generations,
            "{name}-generation classification"
        );
        for (index, (actual, original)) in sections.iter().zip(expected.iter()).enumerate() {
            let shifted = Section {
                start: if index == 0 {
                    0
                } else {
                    original.start + prefix_len
                },
                end: original.end + prefix_len,
            };
            assert_eq!(actual, &shifted, "{name}: range {index} keeps header bias");
        }

        let reported_prefix = if generations == 2 {
            sections[1].start as usize
        } else {
            sections[0].end as usize
        };
        let original_prefix = if generations == 2 {
            expected[1].start as usize
        } else {
            expected[0].end as usize
        };
        let reported = &prefixed[..reported_prefix];
        let (reopened, reopened_provenance) =
            Document::open_repairing(Box::new(BytesSource::new(reported.to_vec())))
                .expect("reported old prefix reopens with header repair");
        assert!(
            !reopened_provenance.is_clean(),
            "{name}: prefix repair is reported"
        );
        assert_eq!(
            reopened.page_count().ok(),
            open(&bytes[..original_prefix]).page_count().ok(),
            "{name}: reported prefix preserves page count"
        );
        if generations == 2 {
            assert!(
                sections[1].start > prefix_len,
                "two-generation boundary includes the header prefix"
            );
        }
    }
}

#[test]
fn missing_linearized_fixtures_follow_corpus_policy_in_subprocesses() {
    let retained_parent = retained_temp_root();
    let missing_root = retained_parent.join("no-root");
    let partial_hayro = retained_parent.join("missing-hayro");
    write_placeholder(
        &partial_hayro,
        "external/verapdf/PDF_UA-1/7.4 Headings/7.4.4 Unnumbered headings/7.4.4-t01-pass-a.pdf",
    );
    let partial_verapdf = retained_parent.join("missing-verapdf");
    write_placeholder(
        &partial_verapdf,
        "external/hayro/pdfs/custom/font_standard_2.pdf",
    );

    let cases = [
        (
            "no corpus root",
            missing_root,
            "no corpus found; set ONIONSKIN_CORPUS to the corpus directory",
        ),
        ("missing hayro fixture", partial_hayro, LINEARIZED_HAYRO),
        (
            "missing veraPDF fixture",
            partial_verapdf,
            LINEARIZED_VERAPDF,
        ),
    ];
    for (label, root, expected_reason) in cases {
        println!("running retained missing-fixture case: {label}");
        for test_name in [
            "a_linearized_files_first_page_table_is_not_a_generation",
            "linearized_header_bias_keeps_one_and_two_generation_classification",
        ] {
            assert_linearized_child(test_name, &root, expected_reason, false);
            assert_linearized_child(test_name, &root, expected_reason, true);
        }
    }
}
