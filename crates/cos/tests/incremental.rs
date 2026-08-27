//! Guarantee test 2: open, edit, save produces `original bytes ++ one
//! incremental section`, and truncating the section yields the byte-exact
//! original.

mod common;

use common::{corpus_dir, pdfs_in};
use onionskin_cos::{BytesSource, Document, FileSource, Object, Provenance};

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
