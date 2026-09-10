//! Guarantee test 6: every file in the malformed corpus opens, saving appends
//! an incremental section holding the repaired structures, and the corrupt
//! original bytes survive beneath it byte-intact.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{classic_pdf, classic_pdf_covering, corpus_dir, pdfs_in, skeleton, Tally};
use onionskin_cos::{BytesSource, Document, Error, Object, Provenance, RepairReason};

/// The damage each malformed variant carries, keyed by filename suffix.
fn expected_reason(name: &str) -> fn(&RepairReason) -> bool {
    if name.ends_with("-junk-header.pdf") {
        |r| matches!(r, RepairReason::JunkBeforeHeader { .. })
    } else if name.ends_with("-no-eof.pdf") {
        |r| matches!(r, RepairReason::MissingEof)
    } else if name.ends_with("-truncated.pdf") {
        |r| {
            matches!(
                r,
                RepairReason::MissingStartxref | RepairReason::TruncatedTail { .. }
            )
        }
    } else if name.ends_with("-xref-bad-offsets.pdf") {
        |r| matches!(r, RepairReason::XrefOffsetsWrong { .. })
    } else if name.ends_with("-xref-count-mismatch.pdf") {
        |r| matches!(r, RepairReason::XrefCountMismatch { .. })
    } else {
        |_| true
    }
}

#[test]
fn open_refuses_a_damaged_file_instead_of_opening_it_quietly() {
    let Some(dir) = corpus_dir("malformed") else {
        return;
    };
    for path in pdfs_in(&dir) {
        match Document::open_path(&path) {
            Err(Error::RepairRequired(report)) => {
                assert!(
                    !report.reasons.is_empty(),
                    "{}: a repair report must say what was wrong",
                    path.display()
                );
            }
            Err(other) => panic!("{}: expected RepairRequired, got {other}", path.display()),
            Ok(_) => panic!("{}: a damaged file must not open as clean", path.display()),
        }
    }
}

#[test]
fn every_malformed_file_repairs_and_saves_over_intact_original_bytes() {
    let Some(dir) = corpus_dir("malformed") else {
        return;
    };
    let files = pdfs_in(&dir);
    assert!(!files.is_empty(), "corpus/malformed holds no PDFs");

    let mut tally = Tally::new("malformed");
    for path in &files {
        match repairs_and_saves(path) {
            Ok(()) => tally.pass(path),
            Err(detail) => tally.fail(path, "repair", &detail),
        }
    }
    tally.report();
    assert_eq!(
        tally.passed.len(),
        files.len(),
        "guarantee test 6 requires the whole malformed set"
    );
}

fn repairs_and_saves(path: &Path) -> Result<(), String> {
    let original = std::fs::read(path).map_err(|e| e.to_string())?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();

    let (document, provenance) =
        Document::open_path_repairing(path).map_err(|e| format!("open: {e}"))?;

    let Provenance::Repaired(report) = &provenance else {
        return Err("opened clean; this file is damaged".to_string());
    };
    let matches = expected_reason(&name);
    if !report.reasons.iter().any(matches) {
        return Err(format!(
            "the damage this variant carries was not reported; got {:?}",
            report.reasons
        ));
    }
    if !document.has_pending_changes() {
        return Err("a repaired document must have something to save".to_string());
    }

    let saved = document.save_to_vec().map_err(|e| format!("save: {e}"))?;
    if saved.len() <= original.len() {
        return Err("saving a repaired document must append a section".to_string());
    }
    if saved[..original.len()] != original[..] {
        return Err("the corrupt original bytes were not preserved".to_string());
    }
    let section = &saved[original.len()..];
    if !section.windows(4).any(|w| w == b"xref") {
        return Err("the appended section holds no cross-reference table".to_string());
    }

    // The repaired file must be readable through the section alone.
    let (reopened, reopened_provenance) =
        Document::open_repairing(Box::new(BytesSource::new(saved.clone())))
            .map_err(|e| format!("reopen: {e}"))?;
    reopened
        .catalog()
        .map_err(|e| format!("reopened catalog: {e}"))?;

    // Junk before %PDF- is still there after an append-only save, so that one
    // variant stays "repaired" forever. Every other kind of damage is fixed by
    // the section we just wrote.
    if !name.ends_with("-junk-header.pdf") && !reopened_provenance.is_clean() {
        return Err(format!(
            "the repaired file still needs repair: {:?}",
            reopened_provenance.report().map(|r| &r.reasons)
        ));
    }

    // The seed this variant came from says how many pages the repair should
    // have recovered. Truncation genuinely loses content, so it is exempt.
    if !name.ends_with("-truncated.pdf") {
        let seed = seed_for(path, &name);
        if let Some(seed) = seed {
            let expected = Document::open_path(&seed)
                .and_then(|d| d.page_count())
                .map_err(|e| format!("seed: {e}"))?;
            let got = reopened
                .page_count()
                .map_err(|e| format!("repaired page count: {e}"))?;
            if got != expected {
                return Err(format!("recovered {got} pages, the seed has {expected}"));
            }
        }
    }
    Ok(())
}

/// Real-world damage, as opposed to the synthetic malformed set: whatever the
/// external corpora happen to contain. Every file the round-trip test skipped
/// as "needed-repair" lands here, and has to survive the same contract.
#[test]
fn damaged_files_in_the_external_corpora_repair_the_same_way() {
    // Through `corpus_dir` rather than an `is_dir` check of its own: this
    // walk printed SKIPPED and returned even under ONIONSKIN_CORPUS_REQUIRED,
    // so guarantee 6's own re-run reported a pass over an absent corpus.
    let Some(external) = corpus_dir("external") else {
        return;
    };

    let mut tally = Tally::new("external (damaged only)");
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for path in pdfs_in(&external) {
        let Ok((document, provenance)) = Document::open_path_repairing(&path) else {
            continue;
        };
        let Provenance::Repaired(report) = &provenance else {
            continue;
        };
        for reason in &report.reasons {
            *reasons.entry(reason_name(reason).to_string()).or_default() += 1;
        }
        match repaired_save_survives(&path, &document) {
            Ok(()) => tally.pass(&path),
            Err(detail) => tally.fail(&path, "repair", &detail),
        }
    }
    tally.report();
    println!("  damage seen:");
    for (reason, count) in &reasons {
        println!("    {reason}: {count}");
    }
    assert_eq!(
        tally.failure_count(),
        0,
        "a repaired real-world file must still save over intact original bytes"
    );
}

fn repaired_save_survives(path: &Path, document: &Document) -> Result<(), String> {
    let original = std::fs::read(path).map_err(|e| e.to_string())?;
    let saved = document.save_to_vec().map_err(|e| format!("save: {e}"))?;
    if saved.len() <= original.len() {
        return Err("saving a repaired document must append a section".to_string());
    }
    if saved[..original.len()] != original[..] {
        return Err("the damaged original bytes were not preserved".to_string());
    }
    let (reopened, _) = Document::open_repairing(Box::new(BytesSource::new(saved)))
        .map_err(|e| format!("reopen: {e}"))?;
    reopened
        .catalog()
        .map_err(|e| format!("reopened catalog: {e}"))?;
    Ok(())
}

/// Carry-forward 1 from the spike: repair is decided at open, so an object
/// whose recorded offset is wrong is a `MissingObject` for the rest of the
/// session. The escalation is the deliberate way out, and it is the caller's
/// to call: a scan that fired by itself would leave a document that lies about
/// one object reporting itself clean.
///
/// The fixture is a file whose cross-reference is wrong about object 4 only.
/// Open-time validation checks that `/Root` and `/Pages` are reachable and
/// nothing else, by design, so the file opens clean and the lie surfaces on
/// first access.
#[test]
fn a_clean_document_can_escalate_to_a_scan_when_an_object_turns_out_to_be_missing() {
    let mut bodies = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    // Object 4's entry points at object 3's header, so the offset resolves to
    // some other object rather than to nothing.
    let honest = classic_pdf(&bodies, &[]);
    let object_3_at = find(&honest, b"3 0 obj").expect("object 3 is in the fixture") as u64;
    let bytes = classic_pdf(&bodies, &[(4, object_3_at)]);

    let mut document = Document::open(Box::new(BytesSource::new(bytes.clone())))
        .expect("a file whose /Root and /Pages resolve opens clean");
    assert!(document.provenance().is_clean());
    assert_eq!(document.page_count().ok(), Some(1));
    match document.get(4) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref.number, 4),
        other => panic!("a wrong offset must be MissingObject, not a silent rescan: {other:?}"),
    }

    let provenance = document
        .escalate_to_scan(4)
        .expect("the scan finds the objects the table lost")
        .clone();
    let Provenance::Repaired(report) = &provenance else {
        panic!("escalating must leave the document repaired, not clean");
    };
    assert!(
        report.rebuilt_by_scan,
        "the report must say the table was rebuilt"
    );
    match report.reasons.last() {
        Some(RepairReason::MidSessionScan {
            unreachable,
            entries_corrected,
        }) => {
            assert_eq!(
                *unreachable, 4,
                "the report must name the object that failed"
            );
            assert_eq!(
                *entries_corrected, 1,
                "only the one wrong entry needed correcting"
            );
        }
        other => panic!("expected a MidSessionScan reason, got {other:?}"),
    }
    assert_eq!(
        document.provenance(),
        &provenance,
        "the document must keep the provenance it handed back"
    );

    let parsed = document.get(4).expect("the escalation recovered object 4");
    assert_eq!(parsed.objref.number, 4);
    assert_eq!(document.page_count().ok(), Some(1), "and lost nothing else");

    // The corrected table is written into the appended section, so the next
    // reader does not have to escalate again.
    assert!(document.has_pending_changes());
    let saved = document.save_to_vec().expect("save");
    assert_eq!(
        &saved[..bytes.len()],
        &bytes[..],
        "the damaged original bytes survive the escalation"
    );
    let (reopened, reopened_provenance) =
        Document::open_repairing(Box::new(BytesSource::new(saved)))
            .expect("the saved file reopens");
    assert!(
        reopened_provenance.is_clean(),
        "the repaired file must open clean: {:?}",
        reopened_provenance.report().map(|r| &r.reasons)
    );
    assert!(reopened.get(4).is_ok());
}

/// Escalation is not a way of ignoring a deliberate deletion: an object marked
/// free stays free, however plainly its bytes are still in the file.
#[test]
fn escalating_does_not_resurrect_a_deleted_object() {
    let mut bodies = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    let mut document = Document::open(Box::new(BytesSource::new(classic_pdf(&bodies, &[]))))
        .expect("the fixture opens clean");
    document.delete_object(4).expect("deletable");
    let saved = document.save_to_vec().expect("save");

    let mut reopened = Document::open(Box::new(BytesSource::new(saved))).expect("reopens clean");
    match reopened.escalate_to_scan(4) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref.number, 4),
        other => panic!("a scan must not undo a deletion, got {other:?}"),
    }
    match reopened.get(4) {
        Err(Error::MissingObject(_)) => {}
        other => panic!("the deleted object must stay gone, got {other:?}"),
    }
    assert!(
        reopened.provenance().is_clean(),
        "an escalation that recovered nothing must not report a repair"
    );
    assert!(!reopened.has_pending_changes());
}

/// The scan recovers objects the table never knew about, which raises the
/// document's high-water mark. A caller that adds an object afterwards must
/// not be handed one of those numbers back.
#[test]
fn escalating_does_not_hand_out_the_numbers_the_scan_recovered() {
    let mut bodies = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    // The table covers objects 1 to 3. Object 4 is in the file and out of the
    // cross-reference, so the document believes 4 is the next free number.
    let bytes = classic_pdf_covering(&bodies, &[], 3);

    let mut document =
        Document::open(Box::new(BytesSource::new(bytes.clone()))).expect("opens clean");
    assert!(document.get(4).is_err(), "object 4 is not in the table yet");
    document
        .escalate_to_scan(4)
        .expect("the scan finds object 4");
    assert!(document.get(4).is_ok(), "the escalation recovered it");

    let added = document
        .add_object(Object::Integer(11))
        .expect("the document has numbers left");
    assert!(
        added.number > 4,
        "add_object handed out object {}, which the scan had just recovered",
        added.number
    );

    let saved = document.save_to_vec().expect("save");
    let (reopened, _) =
        Document::open_repairing(Box::new(BytesSource::new(saved))).expect("the save reopens");
    let four = reopened
        .get(4)
        .expect("object 4 is still in the saved file");
    assert_eq!(
        four.object
            .as_dict()
            .and_then(|d| d.get(b"Which"))
            .and_then(Object::as_integer),
        Some(4),
        "the recovered object was overwritten by the one add_object handed out"
    );
}

/// The escalation must answer for the object it was asked about. A scan that
/// corrects nothing leaves a clean document clean, rather than inventing a
/// repair and a section to record it in.
#[test]
fn escalating_for_an_object_that_is_not_there_does_not_fabricate_a_repair() {
    let bytes = classic_pdf(&skeleton(), &[]);
    let mut document =
        Document::open(Box::new(BytesSource::new(bytes.clone()))).expect("opens clean");

    match document.escalate_to_scan(999) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref.number, 999),
        other => panic!("escalating for an object no scan can find must fail, got {other:?}"),
    }
    assert!(
        document.provenance().is_clean(),
        "a failed escalation must leave the provenance alone"
    );
    assert!(
        !document.has_pending_changes(),
        "a failed escalation must not turn a save into an append"
    );
    assert_eq!(
        document.save_to_vec().expect("save"),
        bytes,
        "save-unchanged must still be byte-identical"
    );
}

/// A cross-reference that sends a compressed object to the wrong slot inside
/// its own container. The container is where the table says it is, so a check
/// that only asks whether the container can be found calls the entry usable
/// and the escalation does nothing at all.
fn wrong_object_stream_slot() -> Vec<u8> {
    let five: &[u8] = b"<</Type/Spare/Which 5>>";
    let six: &[u8] = b"<</Type/Spare/Which 6>>";
    let header = format!("5 0 6 {} ", five.len() + 1);
    let first = header.len();
    let mut data = header.into_bytes();
    data.extend_from_slice(five);
    data.push(b' ');
    data.extend_from_slice(six);

    let mut bytes = Vec::from(&b"%PDF-1.5\n"[..]);
    let mut offsets = [0u64; 8];
    for (index, body) in skeleton().iter().enumerate() {
        offsets[index + 1] = bytes.len() as u64;
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    offsets[4] = bytes.len() as u64;
    bytes.extend_from_slice(
        format!(
            "4 0 obj\n<</Type/ObjStm/N 2/First {first}/Length {}>>\nstream\n",
            data.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&data);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    offsets[7] = bytes.len() as u64;
    // /W [1 2 1]: type, a two-byte field, then one byte.
    let row = |kind: u8, field: u64, last: u8| [kind, (field >> 8) as u8, field as u8, last];
    let mut rows = Vec::new();
    rows.extend_from_slice(&row(0, 0, 255));
    for offset in &offsets[1..=4] {
        rows.extend_from_slice(&row(1, *offset, 0));
    }
    // Object 5 really sits in slot 0. The table says slot 1, which holds 6.
    rows.extend_from_slice(&row(2, 4, 1));
    rows.extend_from_slice(&row(2, 4, 1));
    rows.extend_from_slice(&row(1, offsets[7], 0));

    bytes.extend_from_slice(
        format!(
            "7 0 obj\n<</Type/XRef/Size 8/W[1 2 1]/Index[0 8]/Root 1 0 R/Length {}>>\nstream\n",
            rows.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&rows);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(format!("startxref\n{}\n%%EOF\n", offsets[7]).as_bytes());
    bytes
}

/// The M2 viewer's case on any PDF 1.5+ file: the object it cannot reach is a
/// compressed one. An escalation that cannot correct a compressed entry cannot
/// help there at all.
#[test]
fn escalating_repairs_an_object_the_table_puts_in_the_wrong_object_stream_slot() {
    let bytes = wrong_object_stream_slot();
    let mut document = Document::open(Box::new(BytesSource::new(bytes))).expect("opens clean");
    assert!(
        document.get(6).is_ok(),
        "object 6 is in the slot the table names, so the fixture must resolve it"
    );
    assert!(
        document.get(5).is_err(),
        "object 5 is not in the slot the table names, or this tests nothing"
    );

    document
        .escalate_to_scan(5)
        .expect("the scan knows what the container really holds");
    let five = document
        .get(5)
        .expect("the escalation must reach the compressed object");
    assert_eq!(
        five.object
            .as_dict()
            .and_then(|d| d.get(b"Which"))
            .and_then(Object::as_integer),
        Some(5)
    );
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn reason_name(reason: &RepairReason) -> &'static str {
    match reason {
        RepairReason::JunkBeforeHeader { .. } => "junk-before-header",
        RepairReason::MissingEof => "missing-%%EOF",
        RepairReason::MissingStartxref => "missing-startxref",
        RepairReason::BrokenXrefSection { .. } => "broken-xref-section",
        RepairReason::XrefCountMismatch { .. } => "xref-count-mismatch",
        RepairReason::XrefOffsetsWrong { .. } => "xref-offsets-wrong",
        RepairReason::TrailerRootRecovered => "root-recovered-by-scan",
        RepairReason::ObjectStreamLost { .. } => "object-stream-lost",
        RepairReason::TruncatedTail { .. } => "truncated-tail",
        RepairReason::MidSessionScan { .. } => "escalated-mid-session",
    }
}

fn seed_for(path: &Path, name: &str) -> Option<std::path::PathBuf> {
    let stem = name.strip_suffix(".pdf")?;
    let seed_stem = [
        "-junk-header",
        "-no-eof",
        "-truncated",
        "-xref-bad-offsets",
        "-xref-count-mismatch",
    ]
    .iter()
    .find_map(|suffix| stem.strip_suffix(suffix))?;
    let seeds = path.parent()?.parent()?.join("seeds");
    let candidate = seeds.join(format!("{seed_stem}.pdf"));
    candidate.is_file().then_some(candidate)
}
