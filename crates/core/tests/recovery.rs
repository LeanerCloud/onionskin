//! Autosave's recovery file, from the outside.
//!
//! Every permission here is read back from disk after the write, never assumed
//! from the call that was meant to set it. The case that matters most is the
//! refusal: a directory that already exists at `0o755` has to stop recovery
//! from writing at all, because the alternative is document content in a
//! world-readable place, which is a privacy defect rather than a permissions
//! nit.

use std::path::{Path, PathBuf};

use onionskin_core::{
    Document, DocumentEdit, DocumentFile, Recovered, RecoveryError, RecoveryStore,
};
use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Document as CosDocument, Name, Object};

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .expect("the path exists")
        .permissions()
        .mode()
        & 0o777
}

#[cfg(unix)]
#[test]
fn the_directory_is_owner_only_and_each_file_is_too() {
    let root = temp_dir("modes");
    let store = RecoveryStore::open(&root.join("recovery")).expect("the store opens");
    assert_eq!(mode(store.dir()), 0o700, "directory mode, read back");

    let document = copy_seed(&root, "minimal.pdf");
    let original = std::fs::read(&document).expect("readable");
    let written = store
        .write(&document, &original, b"section bytes")
        .expect("written");
    assert_eq!(mode(&written), 0o600, "file mode, read back");
}

/// The case `known-issues.md` records for the config directory, which must not
/// be inherited here: a directory that already exists wide stays wide, and a
/// document-content store refuses to live in it rather than proceeding.
#[cfg(unix)]
#[test]
fn a_pre_existing_world_readable_directory_is_refused_visibly() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = temp_dir("wide");
    let dir = root.join("recovery");
    std::fs::create_dir_all(&dir).expect("created");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("widened");

    match RecoveryStore::open(&dir) {
        Err(RecoveryError::DirectoryNotPrivate { mode, .. }) => {
            assert_eq!(mode, 0o755, "the refusal names the mode it found");
        }
        other => panic!("a 0755 directory has to be refused, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_dir(&dir).expect("listable").count(),
        0,
        "and nothing was written into it"
    );
    assert_eq!(mode(&dir), 0o755, "nor was it quietly narrowed");
}

/// File names are opaque. The names of the files in a directory are themselves
/// a record of which documents someone had open.
#[test]
fn recovery_file_names_do_not_carry_the_document_name() {
    let root = temp_dir("names");
    let store = RecoveryStore::open(&root.join("recovery")).expect("opens");
    let path = store.path_for(Path::new("/home/someone/Contracts/Acme NDA draft.pdf"));
    let name = path
        .file_name()
        .expect("named")
        .to_string_lossy()
        .into_owned();
    assert!(
        !name.contains("Acme") && !name.contains("NDA") && !name.contains("Contracts"),
        "the recovery file is named {name}, which leaks the document"
    );
}

/// Autosave writes the edits, and replaying them reproduces what the session
/// had.
#[test]
fn a_recovery_replays_to_the_edited_document() {
    let root = temp_dir("replay");
    let document_path = copy_seed(&root, "minimal.pdf");
    let original = std::fs::read(&document_path).expect("readable");
    let store = RecoveryStore::open(&root.join("recovery")).expect("opens");

    let mut document = DocumentFile::open(&document_path).expect("opens");
    document.set_recovery(store.clone());
    set_description(&mut document, "recovered text");
    let written = document.autosave().expect("autosave runs");
    assert!(written.is_some(), "a dirty document writes a recovery");

    let Recovered::Bytes(recovered) = store
        .recover(&document_path, &original)
        .expect("recover runs")
    else {
        panic!("the recovery applies to the unchanged original");
    };
    let reopened = CosDocument::open(Box::new(BytesSource::new(recovered))).expect("reopens");
    let info = reopened
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("the recovered document has /Info");
    let description = reopened
        .get(info.number)
        .expect("parses")
        .object
        .as_dict()
        .and_then(|dict| dict.get(b"Description"))
        .cloned();
    assert_eq!(
        description,
        Some(Object::String(b"recovered text".to_vec()))
    );
}

/// The review risk this package names: can a replay double-apply an edit that
/// was also saved. It cannot, because a saved file is the original plus the
/// section, and that no longer matches what the recovery was built against.
#[test]
fn a_recovery_older_than_a_save_is_stale_rather_than_replayed() {
    let root = temp_dir("stale");
    let document_path = copy_seed(&root, "minimal.pdf");
    let original = std::fs::read(&document_path).expect("readable");
    let store = RecoveryStore::open(&root.join("recovery")).expect("opens");

    let mut document = DocumentFile::open(&document_path).expect("opens");
    set_description(&mut document, "saved text");
    let section = {
        let (edit, base) = document.edit_mut();
        base.section_for(&edit.pending_edits(), &edit.trailer_edits())
            .expect("section")
            .expect("something to write")
    };
    store
        .write(&document_path, &original, &section)
        .expect("written");

    document.save().expect("saves");
    let on_disk = std::fs::read(&document_path).expect("readable");

    assert_eq!(
        store
            .recover(&document_path, &on_disk)
            .expect("recover runs"),
        Recovered::Stale,
        "replaying it would apply the saved edit a second time"
    );
}

/// Deleted after a save, so document content does not outlive its reason to
/// exist outside the document.
#[test]
fn the_recovery_file_is_gone_after_a_save() {
    let root = temp_dir("gone-after-save");
    let document_path = copy_seed(&root, "minimal.pdf");
    let store = RecoveryStore::open(&root.join("recovery")).expect("opens");

    let mut document = DocumentFile::open(&document_path).expect("opens");
    document.set_recovery(store.clone());
    set_description(&mut document, "text");
    let written = document.autosave().expect("autosave").expect("written");
    assert!(written.exists(), "the recovery exists before the save");

    document.save().expect("saves");
    assert!(!written.exists(), "and is gone after it");
}

/// And after a clean close.
#[test]
fn the_recovery_file_is_gone_after_a_clean_close() {
    let root = temp_dir("gone-after-close");
    let document_path = copy_seed(&root, "minimal.pdf");
    let store = RecoveryStore::open(&root.join("recovery")).expect("opens");

    let mut document = DocumentFile::open(&document_path).expect("opens");
    document.set_recovery(store.clone());
    set_description(&mut document, "text");
    let written = document.autosave().expect("autosave").expect("written");
    assert!(written.exists());

    document.close().expect("closes");
    assert!(!written.exists(), "a clean close removes it");
}

/// A document with nothing unsaved has no business leaving content on disk, so
/// autosave on a clean document removes any recovery rather than writing one.
#[test]
fn autosave_on_a_clean_document_writes_nothing_and_removes_a_stale_file() {
    let root = temp_dir("clean-autosave");
    let document_path = copy_seed(&root, "minimal.pdf");
    let store = RecoveryStore::open(&root.join("recovery")).expect("opens");

    let mut document = DocumentFile::open(&document_path).expect("opens");
    document.set_recovery(store.clone());
    set_description(&mut document, "text");
    let written = document.autosave().expect("autosave").expect("written");

    let (edit, base) = document.edit_mut();
    edit.undo(base).expect("undo runs");
    assert_eq!(
        document.autosave().expect("autosave"),
        None,
        "nothing to write"
    );
    assert!(!written.exists(), "and the old recovery is gone");
}

/// Autosave is off unless the app turns it on.
#[test]
fn autosave_is_off_by_default() {
    let root = temp_dir("off");
    let document_path = copy_seed(&root, "minimal.pdf");
    let mut document = DocumentFile::open(&document_path).expect("opens");
    set_description(&mut document, "text");
    assert_eq!(document.autosave().expect("runs"), None);
}

fn set_description(document: &mut Document, text: &str) {
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

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("onionskin-recovery-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn copy_seed(dir: &Path, name: &str) -> PathBuf {
    let destination = dir.join(name);
    std::fs::copy(seed(name), &destination).expect("seed copied");
    destination
}

/// P18: a recovery replayed onto the reopened document is an ordinary
/// undoable edit, the document is dirty, and the next edit's objects land
/// above every number the recovery wrote (T3). Seeding the next number from
/// the reopened file alone collides on the first edit after recovery.
#[test]
fn a_replayed_recovery_is_one_undoable_edit_and_the_next_edit_does_not_collide() {
    let root = temp_dir("replay");
    let store = RecoveryStore::open(&root.join("recovery")).expect("the store opens");
    let path = copy_seed(&root, "minimal.pdf");

    let mut file = DocumentFile::open(&path).expect("opens");
    file.set_recovery(store.clone());
    let highest_written = file
        .document_mut()
        .edit_document("Make Objects", |tx| {
            let mut highest = 0;
            for n in 0..5 {
                let number = tx.reserve();
                let mut dict = onionskin_cos::Dict::new();
                dict.set(Name::new("N"), Object::Integer(n));
                tx.put_object(number, 0, Object::Dict(dict))?;
                highest = number;
            }
            // Named from the trailer so the objects are reachable and
            // survive collapse.
            let root = tx.trailer_value(b"Root").expect("a root");
            let catalog = root.as_reference().expect("a reference");
            let mut dict = tx
                .object(catalog.number)?
                .expect("the catalog")
                .object
                .as_dict()
                .cloned()
                .expect("a dictionary");
            let refs = (highest - 4..=highest)
                .map(|n| Object::Ref(onionskin_cos::ObjRef::new(n, 0)))
                .collect();
            dict.set(Name::new("PieceInfo"), Object::Array(refs));
            tx.put_object(catalog.number, 0, Object::Dict(dict))?;
            Ok(highest)
        })
        .expect("edits");
    file.autosave()
        .expect("autosaves")
        .expect("a recovery file");
    drop(file);

    let original = std::fs::read(&path).expect("the file is unchanged");
    let Recovered::Bytes(recovered) = store.recover(&path, &original).expect("reads") else {
        panic!("the recovery applies to the unchanged file");
    };
    let mut document = Document::open_path(&path).expect("reopens");
    assert!(!document.is_dirty());
    let replayed = document.replay_recovery(&recovered).expect("replays");
    assert!(
        replayed >= 6,
        "five new objects and the catalog: {replayed}"
    );
    assert!(document.is_dirty(), "recovered edits are unsaved edits");
    let catalog = document
        .structure()
        .expect("doc")
        .catalog()
        .expect("catalog");
    assert!(
        catalog.get(b"PieceInfo").is_some(),
        "the recovered edit is there"
    );

    let next = document
        .edit_document("One More", |tx| Ok(tx.reserve()))
        .expect("edits");
    assert!(
        next > highest_written,
        "the next object ({next}) lands above the recovered ones ({highest_written})"
    );

    // "One More" wrote nothing, so it is not a step; the one step is the
    // recovery.
    assert!(document.undo().expect("undoes the recovery"));
    assert!(!document.is_dirty(), "undo takes the recovery back");
}

#[test]
fn bytes_that_do_not_extend_the_document_are_refused() {
    let mut document = Document::open_path(&seed("minimal.pdf")).expect("opens");
    let other = std::fs::read(seed("hello.pdf")).expect("reads");
    assert!(document.replay_recovery(&other).is_err());
    assert!(!document.is_dirty());
}
