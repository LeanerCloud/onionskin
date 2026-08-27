//! Carry-forward 2 from the spike: a stream whose `/Length` is wrong is
//! recovered by finding `endstream`, and that recovery used to be silent. It
//! has to reach the caller, because M5's redaction verifier cannot certify a
//! byte range whose end the parser guessed.
//!
//! The file still opens, and still opens *clean*. A wrong `/Length` is common
//! enough that refusing would reject documents every reader opens, so the
//! doubt is recorded per object rather than raised to a `Provenance`.

mod common;

use common::{classic_pdf, corpus_dir, corpus_root, pdfs_in, skeleton, xref_stream_pdf};
use onionskin_cos::{BytesSource, Document, Object, Origin, Provenance, RecoveredBoundary};

/// The stream's real content. The fixture's `/Length` claims otherwise.
const CONTENT: &[u8] = b"BT /F1 12 Tf ET  \n%%%";

/// A four-object file whose object 4 is a content stream declaring `declared`
/// bytes of data.
fn fixture(declared: i64) -> Vec<u8> {
    let stream = format!(
        "<</Length {declared}>>\nstream\n{}\nendstream",
        String::from_utf8_lossy(CONTENT)
    );
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(stream.as_bytes());
    classic_pdf(&bodies, &[])
}

#[test]
fn a_stream_with_a_wrong_length_parses_and_says_so() {
    let bytes = fixture(999);
    let (document, provenance) =
        Document::open_repairing(Box::new(BytesSource::new(bytes.clone())))
            .expect("the file opens");
    assert_eq!(
        provenance,
        Provenance::Clean,
        "a wrong /Length must not make the whole document a repair case"
    );

    let parsed = document.get(4).expect("the stream object parses");
    assert_eq!(
        parsed.object.as_stream().expect("object 4 is a stream").raw,
        CONTENT,
        "the recovered stream must hold the bytes between stream and endstream"
    );
    assert_eq!(
        parsed.recovered_boundary,
        Some(RecoveredBoundary::LengthWrong {
            declared: 999,
            actual: CONTENT.len() as u64,
        }),
        "the object itself must carry the recovery"
    );
    assert_eq!(
        document.recovered_boundaries().get(&4),
        parsed.recovered_boundary.as_ref(),
        "and the document must aggregate it"
    );
}

/// The recovery is a note, not a change: the file still round-trips, and the
/// note survives the edit that clears the object cache.
#[test]
fn a_recovered_boundary_does_not_disturb_the_round_trip() {
    let bytes = fixture(999);
    let mut document =
        Document::open(Box::new(BytesSource::new(bytes.clone()))).expect("opens clean");
    document.get(4).expect("the stream parses");
    assert!(
        !document.has_pending_changes(),
        "recovering a boundary must not turn a save into an append"
    );
    assert_eq!(
        document.save_to_vec().expect("save"),
        bytes,
        "save-unchanged must still be byte-identical"
    );

    document
        .set_info_field("Producer", Object::String(b"x".to_vec()))
        .expect("settable");
    assert!(
        document.recovered_boundaries().contains_key(&4),
        "an edit clears the object cache; it must not clear what the parser learned"
    );
}

#[test]
fn a_stream_with_the_right_length_reports_nothing() {
    let bytes = fixture(CONTENT.len() as i64);
    let document = Document::open(Box::new(BytesSource::new(bytes))).expect("opens clean");
    let parsed = document.get(4).expect("the stream object parses");
    assert_eq!(parsed.object.as_stream().expect("a stream").raw, CONTENT);
    assert_eq!(parsed.recovered_boundary, None);
    assert!(document.recovered_boundaries().is_empty());
}

/// The one recovery no amount of fetching objects would reveal: the
/// cross-reference stream's own boundary. Every object in the document is
/// found through the table that stream carries, so if its end was guessed,
/// so was everything.
#[test]
fn a_cross_reference_stream_with_a_wrong_length_reports_its_own_recovery() {
    let bytes = xref_stream_pdf(Some(999));
    let (document, provenance) =
        Document::open_repairing(Box::new(BytesSource::new(bytes))).expect("the file opens");
    assert_eq!(
        provenance,
        Provenance::Clean,
        "a wrong /Length on the xref stream is still not a repair case"
    );
    assert_eq!(document.page_count().ok(), Some(1), "the table still works");

    let boundaries = document.recovered_boundaries();
    let recovered = boundaries
        .get(&4)
        .expect("the cross-reference stream is object 4, and its boundary was guessed");
    assert!(
        matches!(
            recovered,
            RecoveredBoundary::LengthWrong { declared: 999, .. }
        ),
        "expected the declared length to be reported, got {recovered}"
    );

    // Nothing was fetched through the table before the note existed: opening is
    // what parsed the stream, so the note is there from the start.
    let untouched = Document::open(Box::new(BytesSource::new(xref_stream_pdf(Some(999)))))
        .expect("opens clean");
    assert!(untouched.recovered_boundaries().contains_key(&4));
}

#[test]
fn a_cross_reference_stream_with_the_right_length_reports_nothing() {
    let document =
        Document::open(Box::new(BytesSource::new(xref_stream_pdf(None)))).expect("opens clean");
    assert_eq!(document.page_count().ok(), Some(1));
    assert!(document.recovered_boundaries().is_empty());
}

/// What the wild actually contains, and the invariant that has to hold for
/// every one of them: the length the recovery reports is the length of the
/// bytes the object came back with. A note that disagreed with its own stream
/// would be worse than no note.
#[test]
fn recovered_boundaries_across_the_corpus_agree_with_the_bytes() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    let mut files_with_recoveries = 0usize;
    let mut recoveries = 0usize;
    // `pdfs_in` sorts by path, so taking a prefix would only ever walk the
    // first corpus. A stride crosses all of them and still bounds the run.
    let all = pdfs_in(&root);
    let stride = (all.len() / 600).max(1);
    let sampled: Vec<_> = all.into_iter().step_by(stride).collect();
    println!("sampling {} corpus files, every {stride}", sampled.len());
    for path in sampled {
        let Ok((document, _)) = Document::open_path_repairing(&path) else {
            continue;
        };
        let mut seen = 0usize;
        for (number, _) in document.xref().iter() {
            let Ok(parsed) = document.get(number) else {
                continue;
            };
            let Some(boundary) = parsed.recovered_boundary else {
                continue;
            };
            seen += 1;
            // A compressed object inherits its container's note, so only an
            // object with bytes of its own can be checked against it.
            if let (Origin::File(_), Some(stream)) = (parsed.origin, parsed.object.as_stream()) {
                assert_eq!(
                    stream.raw.len() as u64,
                    boundary.actual(),
                    "{}: object {number} reports {boundary} but came back with {} bytes",
                    path.display(),
                    stream.raw.len()
                );
            }
        }
        assert_eq!(
            document.recovered_boundaries().len(),
            seen,
            "{}: the aggregate lost recoveries the objects reported",
            path.display()
        );
        if seen > 0 {
            files_with_recoveries += 1;
            recoveries += seen;
        }
    }
    println!("{recoveries} recovered stream boundaries in {files_with_recoveries} corpus files");
}

/// The seeds are well-formed, so walking every object in them must produce no
/// recoveries at all. A test that only ever saw the broken fixture could not
/// tell a working detector from one that fires on everything.
#[test]
fn a_clean_file_reports_no_recovered_boundaries() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    for path in pdfs_in(&dir) {
        let document = Document::open_path(&path).expect("seeds open clean");
        for (number, _) in document.xref().iter() {
            let Ok(parsed) = document.get(number) else {
                continue;
            };
            assert_eq!(
                parsed.recovered_boundary,
                None,
                "{}: object {number} reported a recovered boundary in a well-formed file",
                path.display()
            );
        }
        assert!(
            document.recovered_boundaries().is_empty(),
            "{}: a well-formed seed reported recoveries",
            path.display()
        );
    }
}
