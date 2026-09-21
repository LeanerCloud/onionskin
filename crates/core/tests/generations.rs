//! The skins panel's data: every generation of a file, who wrote it and
//! when, and rolling back to one of them by truncating the file.

use std::path::{Path, PathBuf};

use onionskin_core::{DocumentEdit, DocumentFile, Error, RevertRefusal};
use onionskin_corpus_testing::seed;
use onionskin_cos::{Name, Object};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "onionskin-generations-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn copy_seed(dir: &Path, name: &str) -> PathBuf {
    let destination = dir.join(name);
    std::fs::copy(seed(name), &destination).expect("seed copied");
    destination
}

fn describe(document: &mut DocumentFile, text: &str) {
    let (edit, base) = document.edit_mut();
    edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Description"),
            value: Some(Object::String(text.as_bytes().to_vec())),
        },
    )
    .expect("the description is set");
}

/// A seed saved `saves` times: the original and one generation per save.
fn saved(name: &str, saves: usize) -> (PathBuf, DocumentFile) {
    let dir = temp_dir(name);
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");
    for save in 0..saves {
        describe(&mut document, &format!("save {save}"));
        document.save().expect("saves");
    }
    (path, document)
}

#[test]
fn three_saves_are_three_generations_over_the_original_and_all_are_ours() {
    let (path, document) = saved("three", 3);
    let details = document.generation_details().expect("details");
    assert_eq!(details.len(), 4);
    let file_len = std::fs::metadata(&path).expect("stat").len();
    // The ranges partition the file, oldest first.
    assert_eq!(details[0].generation.start, 0);
    for pair in details.windows(2) {
        assert_eq!(pair[0].generation.end, pair[1].generation.start);
    }
    assert_eq!(details[3].generation.end, file_len);
    assert!(!details[0].ours, "the original is nobody's save");
    for detail in &details[1..] {
        assert!(
            detail.ours,
            "generation {} is ours",
            detail.generation.index
        );
        assert!(detail
            .producer
            .as_deref()
            .is_some_and(|producer| producer.starts_with("Onionskin")));
        assert!(detail
            .date
            .as_deref()
            .is_some_and(|date| date.starts_with("D:")));
    }
}

/// Another writer that appends after us copies the trailer forward, stamp
/// and all. The stamp names our section's start, not theirs, so their
/// section is not ours.
#[test]
fn a_section_another_writer_appended_is_not_ours_even_with_our_stamp_carried_forward() {
    let (path, document) = saved("foreign", 1);
    drop(document);
    let mut bytes = std::fs::read(&path).expect("reads");
    let cos = onionskin_cos::Document::open_path(&path).expect("parses");
    let start = bytes.len() as u64;
    // A hand-written section in the classic form, as another producer would
    // append it: an empty subsection and the carried-forward trailer.
    let section = cos
        .section_for(&Default::default(), &{
            let mut edits = std::collections::BTreeMap::new();
            edits.insert(
                Name::new("ForeignWriter"),
                Some(Object::String(b"someone else".to_vec())),
            );
            edits
        })
        .expect("builds")
        .expect("a section");
    bytes.extend_from_slice(&section);
    std::fs::write(&path, &bytes).expect("writes");

    let foreign = onionskin_cos::Document::open_path(&path).expect("parses");
    assert!(
        foreign.trailer().get(b"OnionskinSection").is_some(),
        "the stamp was carried forward into the foreign section"
    );
    let document = DocumentFile::open(&path).expect("reopens");
    let details = document.generation_details().expect("details");
    assert_eq!(details.len(), 3);
    assert!(details[1].ours, "our save");
    assert!(
        details[2].generation.start >= start,
        "the foreign section is last"
    );
    assert!(!details[2].ours, "carried forward, not written by us");
}

#[test]
fn rolling_back_to_the_middle_leaves_the_file_as_it_was_then() {
    let (path, mut document) = saved("roll-back", 3);
    let before = std::fs::read(&path).expect("reads");
    let generations = document.generations().expect("generations");
    let keep = 1;
    let expected = &before[..generations[keep + 1].start as usize];

    document.roll_back_to(keep).expect("rolls back");
    let after = std::fs::read(&path).expect("reads");
    assert_eq!(
        after, expected,
        "byte for byte a truncation at the next one's start"
    );
    assert_eq!(document.generations().expect("generations").len(), keep + 1);
    // The session is the file as it now is.
    let reopened = DocumentFile::open(&path).expect("reopens");
    assert_eq!(reopened.bytes().as_slice(), document.bytes().as_slice());
}

#[test]
fn rolling_back_is_refused_with_unsaved_edits_and_to_the_current_version() {
    let (path, mut document) = saved("roll-back-refused", 2);
    let before = std::fs::read(&path).expect("reads");
    let refusal = |result: Result<(), Error>| match result {
        Err(Error::RevertRefused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert_eq!(
        refusal(document.roll_back_to(2)),
        RevertRefusal::AlreadyCurrent
    );
    assert_eq!(
        refusal(document.roll_back_to(7)),
        RevertRefusal::NoSuchGeneration
    );
    describe(&mut document, "unsaved");
    assert_eq!(
        refusal(document.roll_back_to(0)),
        RevertRefusal::UnsavedEdits
    );
    assert_eq!(
        std::fs::read(&path).expect("reads"),
        before,
        "a refusal writes nothing"
    );
    assert_eq!(
        RevertRefusal::AlreadyCurrent.to_string(),
        "this is already the current version"
    );
}
