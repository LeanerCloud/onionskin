//! Guarantee test 2: open, edit, save produces `original bytes ++ one
//! incremental section`, and truncating the section yields the byte-exact
//! original.

mod common;

use std::io::{self, Write};

use common::{corpus_dir, corpus_root, pdfs_in, xref_stream_pdf};
use onionskin_cos::{BytesSource, CountingSource, Document, FileSource, Object, Provenance};

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

fn producer_of(document: &Document) -> Option<String> {
    let info = document.trailer().get(b"Info")?;
    let info = document.resolve(info).ok()?;
    let value = document.resolve(info.as_dict()?.get(b"Producer")?).ok()?;
    match value {
        Object::String(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        _ => None,
    }
}

#[test]
fn editing_a_seed_appends_exactly_one_section() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    for path in pdfs_in(&dir) {
        let original = std::fs::read(&path).expect("seed is readable");
        let mut document =
            Document::open(Box::new(FileSource::open(&path).expect("seed opens"))).expect("clean");
        let eofs_before = count(&original, b"%%EOF");

        document
            .set_info_field("Producer", Object::String(b"Onionskin M1 spike".to_vec()))
            .expect("info field is settable");
        assert!(document.has_pending_changes());

        let saved = document.save_to_vec().expect("save");
        let name = path.display();

        assert_eq!(
            &saved[..original.len()],
            &original[..],
            "{name}: the original bytes must survive an edit untouched"
        );
        assert!(
            saved.len() > original.len(),
            "{name}: an edit must append something"
        );
        assert_eq!(
            count(&saved, b"%%EOF"),
            eofs_before + 1,
            "{name}: an edit must append exactly one incremental section"
        );

        let section = &saved[original.len()..];
        assert!(
            count(section, b"/Prev") >= 1,
            "{name}: the appended trailer must chain to the previous xref"
        );

        let (reopened, provenance) =
            Document::open_repairing(Box::new(BytesSource::new(saved.clone())))
                .expect("the saved file reopens");
        assert_eq!(
            provenance,
            Provenance::Clean,
            "{name}: the file we just wrote must open clean"
        );
        assert_eq!(
            producer_of(&reopened).as_deref(),
            Some("Onionskin M1 spike"),
            "{name}: the edit must be visible through the new xref"
        );

        let before = Document::open(Box::new(BytesSource::new(original.clone())))
            .expect("original opens clean");
        assert_eq!(
            reopened.page_count().ok(),
            before.page_count().ok(),
            "{name}: an edit must not disturb the page tree"
        );

        // Rolling the edit back is a truncation and nothing else, at the
        // offset the document itself reports.
        let cut = document.original_len() as usize;
        assert_eq!(cut, original.len());
        let rolled_back = Document::open(Box::new(BytesSource::new(saved[..cut].to_vec())))
            .expect("the truncated file opens clean");
        assert_ne!(
            producer_of(&rolled_back).as_deref(),
            Some("Onionskin M1 spike"),
            "{name}: truncating the section must undo the edit"
        );
    }
}

#[test]
fn a_second_edit_appends_a_second_section() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let path = pdfs_in(&dir).into_iter().next().expect("a seed exists");
    let original = std::fs::read(&path).expect("seed is readable");

    let mut first =
        Document::open(Box::new(FileSource::open(&path).expect("opens"))).expect("clean");
    first
        .set_info_field("Producer", Object::String(b"generation one".to_vec()))
        .expect("settable");
    let once = first.save_to_vec().expect("save");

    let mut second =
        Document::open(Box::new(BytesSource::new(once.clone()))).expect("reopens clean");
    second
        .set_info_field("Producer", Object::String(b"generation two".to_vec()))
        .expect("settable");
    let twice = second.save_to_vec().expect("save");

    assert_eq!(
        &twice[..once.len()],
        &once[..],
        "generation two is appended"
    );
    assert_eq!(
        &twice[..original.len()],
        &original[..],
        "so is generation one"
    );
    assert_eq!(count(&twice, b"%%EOF"), count(&original, b"%%EOF") + 2);

    let latest = Document::open(Box::new(BytesSource::new(twice.clone()))).expect("reopens");
    assert_eq!(producer_of(&latest).as_deref(), Some("generation two"));

    // Rolling back one generation is a truncation, nothing more.
    let rolled_back = Document::open(Box::new(BytesSource::new(twice[..once.len()].to_vec())))
        .expect("the previous generation reopens");
    assert_eq!(producer_of(&rolled_back).as_deref(), Some("generation one"));
}

#[test]
fn editing_an_xref_stream_file_writes_a_clean_classic_trailer() {
    let original = xref_stream_pdf(None);
    let (document, provenance) =
        Document::open_repairing(Box::new(BytesSource::new(original.clone())))
            .expect("the fixture opens");
    assert_eq!(
        provenance,
        Provenance::Clean,
        "the fixture must be a valid xref-stream file, or this tests nothing"
    );
    assert_eq!(document.page_count().ok(), Some(1));

    let mut document = document;
    document
        .set_info_field("Producer", Object::String(b"Onionskin M1 spike".to_vec()))
        .expect("info field is settable");
    let saved = document.save_to_vec().expect("save");
    assert_eq!(&saved[..original.len()], &original[..]);

    let section = &saved[original.len()..];
    let trailer_at = section
        .windows(7)
        .position(|w| w == b"trailer")
        .expect("the appended section has a trailer");
    let trailer = &section[trailer_at..];
    for key in [
        &b"/Type"[..],
        b"/W",
        b"/Index",
        b"/Filter",
        b"/DecodeParms",
        b"/Length",
    ] {
        assert!(
            !trailer.windows(key.len()).any(|w| w == key),
            "the appended trailer carries {}, which belongs to the xref stream",
            String::from_utf8_lossy(key)
        );
    }

    let (reopened, provenance) = Document::open_repairing(Box::new(BytesSource::new(saved)))
        .expect("the saved file reopens");
    assert_eq!(
        provenance,
        Provenance::Clean,
        "a file we wrote must open clean"
    );
    assert_eq!(
        producer_of(&reopened).as_deref(),
        Some("Onionskin M1 spike")
    );
    assert_eq!(reopened.page_count().ok(), Some(1));
}

/// A save must not hold the document. `cos` copies the original through in
/// 64 KiB pieces, so no single read or write may be bigger than that however
/// large the file is.
const COPY_BUDGET: u64 = 64 * 1024;

/// Records the largest single write. The copy loop's writes are what this
/// bounds; the appended section is one write of its own, sized by the edits
/// rather than by the file.
#[derive(Default)]
struct MeasuringSink {
    written: u64,
    largest: u64,
}

impl Write for MeasuringSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written += buf.len() as u64;
        self.largest = self.largest.max(buf.len() as u64);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Carry-forward 3 from the spike: `save_to_vec` read the whole source into
/// memory. Saving the biggest file in the corpus must now cost one chunk,
/// measured on both sides at once - the source, through a counting source, and
/// the sink, through the writer the save is handed.
///
/// One file is enough and the biggest is the one worth having, so the loop
/// walks the candidates only until one of them opens cleanly.
#[test]
fn saving_a_large_file_streams_it_in_bounded_chunks() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    let mut candidates: Vec<_> = pdfs_in(&root)
        .into_iter()
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.len(), path)))
        .filter(|(len, _)| *len > 4 * COPY_BUDGET)
        .collect();
    if candidates.is_empty() {
        eprintln!(
            "SKIPPED: the corpus holds no PDF bigger than {} bytes",
            4 * COPY_BUDGET
        );
        return;
    }
    candidates.sort_by_key(|(len, _)| std::cmp::Reverse(*len));

    let mut measured = 0usize;
    for (len, path) in candidates.into_iter().take(8) {
        let file = FileSource::open(&path).expect("corpus file opens");
        let (counting, stats) = CountingSource::new(Box::new(file));
        // A repaired save writes a full table, which walks every object and
        // reads whatever those objects cost. That is repair's budget, not the
        // copy loop's, so this test measures a clean file.
        let Ok(mut document) = Document::open(Box::new(counting)) else {
            continue;
        };
        if document
            .set_info_field("Producer", Object::String(b"Onionskin".to_vec()))
            .is_err()
        {
            continue;
        }

        // Opening reads the xref, whose own window is legitimately large on a
        // big file. The budget is about the save, so the save is measured on
        // its own.
        stats.reset();
        let mut sink = MeasuringSink::default();
        document.save_to_writer(&mut sink).expect("save streams");

        println!(
            "{}: {len} bytes, largest read {}, largest write {}, {} read for {} written",
            path.display(),
            stats.largest_read(),
            sink.largest,
            stats.total(),
            sink.written
        );
        assert!(
            sink.written > len,
            "the save wrote {} bytes of a {len} byte file plus a section",
            sink.written
        );
        assert!(
            stats.total() >= len,
            "the save read {} bytes of a {len} byte file, so it did not stream all of it",
            stats.total()
        );
        assert!(
            stats.largest_read() <= COPY_BUDGET,
            "the save read {} bytes at once, over the {COPY_BUDGET} byte budget",
            stats.largest_read()
        );
        assert!(
            sink.largest <= COPY_BUDGET,
            "the save wrote {} bytes at once, over the {COPY_BUDGET} byte budget",
            sink.largest
        );
        measured += 1;
        break;
    }

    assert!(
        measured > 0,
        "no large corpus file opened cleanly, so the save budget went unmeasured"
    );
}

#[test]
fn save_to_path_writes_what_save_to_vec_returns() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("onionskin-cos-save-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch directory");

    for path in pdfs_in(&dir) {
        let mut document =
            Document::open(Box::new(FileSource::open(&path).expect("seed opens"))).expect("clean");
        document
            .set_info_field("Producer", Object::String(b"Onionskin".to_vec()))
            .expect("settable");

        let target = scratch.join(path.file_name().expect("seed has a name"));
        document.save_to_path(&target).expect("save to path");
        assert_eq!(
            std::fs::read(&target).expect("the saved file is readable"),
            document.save_to_vec().expect("save to vec"),
            "{}: the two sinks must produce the same bytes",
            path.display()
        );
        assert!(
            !std::fs::read_dir(&scratch)
                .expect("scratch is readable")
                .flatten()
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .contains("onionskin-save")),
            "the temporary file a save writes through must not survive it"
        );
    }
    std::fs::remove_dir_all(&scratch).expect("scratch cleans up");
}

/// Saving over the file the document was open on is the ordinary case for an
/// editor, and the one where a save that truncated first would destroy the
/// bytes it is still reading.
#[test]
fn saving_over_the_open_file_keeps_the_original_bytes_underneath() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let source = dir.join("hello.pdf");
    let original = std::fs::read(&source).expect("seed is readable");

    let scratch =
        std::env::temp_dir().join(format!("onionskin-cos-inplace-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let path = scratch.join("hello.pdf");
    std::fs::write(&path, &original).expect("the copy is writable");

    let mut document = Document::open_path(&path).expect("the copy opens clean");
    document
        .set_info_field("Producer", Object::String(b"saved in place".to_vec()))
        .expect("settable");
    document.save_to_path(&path).expect("save in place");

    let saved = std::fs::read(&path).expect("the saved file is readable");
    assert_eq!(
        &saved[..original.len()],
        &original[..],
        "an in-place save must leave the original bytes byte-exact"
    );
    assert_eq!(
        producer_of(&Document::open_path(&path).expect("reopens clean")).as_deref(),
        Some("saved in place")
    );
    std::fs::remove_dir_all(&scratch).expect("scratch cleans up");
}

/// A save writes the original first and the section after it, so a section
/// that cannot be assembled must be found out about before a byte is written.
/// Otherwise a writer that is not a file - a socket, a pipe, an upload - ends
/// up holding a PDF-shaped prefix with no update on the end of it.
#[test]
fn a_section_that_cannot_be_assembled_leaves_the_writer_untouched() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let path = dir.join("hello.pdf");
    let mut document = Document::open_path(&path).expect("the seed opens clean");
    // PDF has no notation for infinity, so the writer refuses it rather than
    // inventing a number: an edit that cannot be serialized at all.
    document
        .set_object(1, 0, Object::Real(f64::INFINITY))
        .expect("the edit is accepted; it is the save that must refuse it");

    let mut sink = MeasuringSink::default();
    assert!(
        document.save_to_writer(&mut sink).is_err(),
        "an unwritable object must fail the save"
    );
    assert_eq!(
        sink.written, 0,
        "the save wrote {} bytes of a document it could not finish",
        sink.written
    );
}

/// Replacing a file must not hand out the bytes more widely than the file it
/// replaces did. The temporary a save writes through starts with the process
/// default, so the mode has to be carried across before the rename.
#[cfg(unix)]
#[test]
fn saving_over_a_file_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("onionskin-cos-mode-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let path = scratch.join("hello.pdf");
    std::fs::copy(dir.join("hello.pdf"), &path).expect("the copy is writable");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("the mode is settable");

    let mut document = Document::open_path(&path).expect("the copy opens clean");
    document
        .set_info_field("Producer", Object::String(b"private".to_vec()))
        .expect("settable");
    document.save_to_path(&path).expect("save in place");

    let mode = std::fs::metadata(&path)
        .expect("the saved file is there")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o600,
        "saving turned a file only its owner could read into mode {mode:o}"
    );
    std::fs::remove_dir_all(&scratch).expect("scratch cleans up");
}

#[test]
fn a_document_with_no_info_dictionary_gains_one() {
    // corpus/seeds/minimal.pdf has no /Info, so the edit has to create the
    // dictionary and add the trailer entry that points at it.
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    let path = dir.join("minimal.pdf");
    let original = std::fs::read(&path).expect("minimal.pdf is readable");
    let mut document =
        Document::open(Box::new(FileSource::open(&path).expect("opens"))).expect("clean");
    assert!(
        document.trailer().get(b"Info").is_none(),
        "this test needs a seed without an /Info dictionary"
    );

    document
        .set_info_field("Producer", Object::String(b"created by the edit".to_vec()))
        .expect("settable");
    let saved = document.save_to_vec().expect("save");
    assert_eq!(&saved[..original.len()], &original[..]);

    let reopened = Document::open(Box::new(BytesSource::new(saved))).expect("reopens clean");
    assert_eq!(
        producer_of(&reopened).as_deref(),
        Some("created by the edit")
    );
}
