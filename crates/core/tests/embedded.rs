//! `core::embedded`: files embedded by this program, as a comment and as a
//! document attachment, read back after a save and a reopen.

mod common;

use std::path::{Path, PathBuf};

use onionskin_core::embedded::{add_to_attachments, embed_file, remove_attachment, NewAttachment};
use onionskin_core::{add_annotation, Annotation, Document, DocumentFile, Error, Rect, Subtype};

const NOW: i64 = 1_758_000_000;

fn seed(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/seeds")
        .join(name)
}

/// Every byte value, and runs that compress, so a stream written without its
/// filter or truncated at a zero byte cannot pass.
fn payload() -> Vec<u8> {
    let mut data: Vec<u8> = (0..=255u8).collect();
    data.extend(std::iter::repeat_n(0u8, 4096));
    data.extend((0..3000u32).map(|value| (value * 7919 % 251) as u8));
    data
}

fn copy_of(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(name);
    std::fs::copy(seed(name), &path).expect("copies");
    (dir, path)
}

fn attach_as_comment(document: &mut Document, name: &str, data: &[u8]) -> Result<(), Error> {
    let page = document
        .structure()
        .expect("the document")
        .page(0)
        .expect("page one")
        .objref;
    document.edit_annotations("Attach File", |tx, structure| {
        let spec = embed_file(
            tx,
            &NewAttachment {
                name,
                data,
                mime: Some("application/octet-stream"),
                description: Some("the numbers"),
            },
            NOW,
        )?;
        let mut annotation =
            Annotation::new(Subtype::FileAttachment, Rect::new(20.0, 40.0, 40.0, 64.0));
        annotation.file = Some(spec);
        annotation.icon = Some("Paperclip".into());
        add_annotation(tx, structure, page, &annotation, NOW).map(|_| ())
    })
}

#[test]
fn an_attached_comment_round_trips_byte_for_byte_and_is_listed_after_a_reopen() {
    let (_dir, path) = copy_of("hello.pdf");
    let data = payload();
    let mut file = DocumentFile::open(&path).expect("opens");
    attach_as_comment(file.document_mut(), "figures.bin", &data).expect("attaches");

    let listed = file.document_mut().attachments().expect("lists").to_vec();
    assert_eq!(listed.len(), 1, "listed before the save, from the session");
    file.save().expect("saves");
    drop(file);

    let mut reopened = Document::open_path(&path).expect("reopens");
    let attachments = reopened.attachments().expect("lists").to_vec();
    assert_eq!(attachments.len(), 1);
    let attachment = &attachments[0];
    assert_eq!(attachment.name, "figures.bin");
    assert_eq!(attachment.page, Some(0), "a comment's file says its page");
    assert_eq!(attachment.size, Some(data.len() as u64));
    assert_eq!(attachment.mime.as_deref(), Some("application/octet-stream"));
    assert_eq!(attachment.description.as_deref(), Some("the numbers"));
    assert_eq!(reopened.attachment_bytes(0).expect("reads"), data);
}

#[test]
fn a_name_that_is_a_path_is_refused_and_nothing_is_written() {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    for name in ["../../.ssh/config", "a\\b", "..", ""] {
        let refused = attach_as_comment(&mut document, name, b"x");
        assert!(
            matches!(refused, Err(Error::InvalidAttachmentName(_))),
            "{name:?}: {refused:?}"
        );
    }
    assert_eq!(document.edit().history().reach(), 0, "no edit was recorded");
    assert!(document.attachments().expect("lists").is_empty());
}

#[test]
fn document_attachments_join_the_name_tree_under_a_free_name_and_keep_the_old_ones() {
    let (_dir, path) = copy_of("hello.pdf");
    let mut file = DocumentFile::open(&path).expect("opens");
    let add = |document: &mut Document, name: &'static str, data: &'static [u8]| {
        document
            .edit_annotations("Add Attachment", |tx, _| {
                let spec = embed_file(
                    tx,
                    &NewAttachment {
                        name,
                        data,
                        mime: None,
                        description: None,
                    },
                    NOW,
                )?;
                add_to_attachments(tx, name, spec)
            })
            .expect("adds")
    };
    assert_eq!(add(file.document_mut(), "b.txt", b"second"), "b.txt");
    assert_eq!(add(file.document_mut(), "a.txt", b"first"), "a.txt");
    assert_eq!(
        add(file.document_mut(), "a.txt", b"again"),
        "a.txt (2)",
        "a key two files would share is renamed, not overwritten"
    );
    file.save().expect("saves");
    drop(file);

    let mut reopened = Document::open_path(&path).expect("reopens");
    let names: Vec<String> = reopened
        .attachments()
        .expect("lists")
        .iter()
        .map(|attachment| attachment.name.clone())
        .collect();
    assert_eq!(names, ["a.txt", "a.txt", "b.txt"], "sorted by key");
    let bytes: Vec<Vec<u8>> = (0..3)
        .map(|index| reopened.attachment_bytes(index).expect("reads"))
        .collect();
    assert_eq!(
        bytes,
        [b"first".to_vec(), b"again".to_vec(), b"second".to_vec()]
    );
}

#[test]
fn an_existing_tree_of_kids_is_kept_when_one_is_added() {
    use common::{pdf, stream};

    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Names 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
        b"<< /EmbeddedFiles 5 0 R >>".to_vec(),
        b"<< /Kids [6 0 R] >>".to_vec(),
        b"<< /Names [(old.txt) 7 0 R] /Limits [(old.txt) (old.txt)] >>".to_vec(),
        b"<< /Type /Filespec /F (old.txt) /EF << /F 8 0 R >> >>".to_vec(),
        stream("kept"),
    ]);
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("kids.pdf");
    std::fs::write(&path, bytes).expect("writes");
    let mut file = DocumentFile::open(&path).expect("opens");
    file.document_mut()
        .edit_annotations("Add Attachment", |tx, _| {
            let spec = embed_file(
                tx,
                &NewAttachment {
                    name: "new.txt",
                    data: b"added",
                    mime: None,
                    description: None,
                },
                NOW,
            )?;
            add_to_attachments(tx, "new.txt", spec)
        })
        .expect("adds");
    file.save().expect("saves");
    drop(file);

    let mut reopened = Document::open_path(&path).expect("reopens");
    let names: Vec<String> = reopened
        .attachments()
        .expect("lists")
        .iter()
        .map(|attachment| attachment.name.clone())
        .collect();
    assert_eq!(names, ["new.txt", "old.txt"]);
    assert_eq!(reopened.attachment_bytes(1).expect("reads"), b"kept");
}

/// The appearance is drawn here, not left to a reader: the paperclip's ink
/// is on the page.
#[test]
fn an_attachment_comment_draws_its_own_icon() {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    let blank = ink(&mut document);
    attach_as_comment(&mut document, "icon.bin", b"x").expect("attaches");
    assert!(ink(&mut document) > blank + 20, "the icon drew something");
}

fn ink(document: &mut Document) -> usize {
    let render = document.render_page_now(0, 2.0).expect("renders");
    render
        .raster
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| i32::from(pixel[2]) > i32::from(pixel[0]) + 40)
        .count()
}

/// Delete rewrites the name tree without the entry and frees nothing: the
/// file specification and the stream stay behind, unreferenced, and the
/// appended section carries no free entry.
#[test]
fn a_deleted_attachment_leaves_the_tree_and_its_bytes_become_garbage() {
    use common::{pdf, stream};

    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Names 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
        b"<< /EmbeddedFiles 5 0 R >>".to_vec(),
        b"<< /Names [(gone.txt) 6 0 R (kept.txt) 8 0 R] >>".to_vec(),
        b"<< /Type /Filespec /F (gone.txt) /EF << /F 7 0 R >> >>".to_vec(),
        stream("gone"),
        b"<< /Type /Filespec /F (kept.txt) /EF << /F 9 0 R >> >>".to_vec(),
        stream("kept"),
    ]);
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("two.pdf");
    std::fs::write(&path, &bytes).expect("writes");
    let mut file = DocumentFile::open(&path).expect("opens");
    let removed = file
        .document_mut()
        .edit_document("Delete Attachment", |tx| remove_attachment(tx, 7))
        .expect("deletes");
    assert_eq!(removed, 1);
    file.save().expect("saves");
    drop(file);

    let saved = std::fs::read(&path).expect("reads");
    let appended = String::from_utf8_lossy(&saved[bytes.len()..]).into_owned();
    assert!(
        appended.contains("xref"),
        "a classic section, so its entries can be read"
    );
    let frees: Vec<&str> = appended
        .lines()
        .filter(|line| line.trim_end().ends_with(" f") && !line.starts_with("0000000000 65535"))
        .collect();
    assert_eq!(frees, Vec::<&str>::new(), "nothing is freed");

    let mut reopened = Document::open_path(&path).expect("reopens");
    let names: Vec<String> = reopened
        .attachments()
        .expect("lists")
        .iter()
        .map(|attachment| attachment.name.clone())
        .collect();
    assert_eq!(names, ["kept.txt"]);
    let structure = reopened.structure().expect("doc");
    assert_eq!(structure.audit_references().expect("audits"), Vec::new());
    assert!(structure.get(7).is_ok(), "the stream is still in the file");
}

#[test]
fn deleting_a_comments_file_takes_the_comment_off_its_page() {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    attach_as_comment(&mut document, "note.bin", b"x").expect("attaches");
    let stream = document.attachments().expect("lists")[0].stream;
    let removed = document
        .edit_document("Delete Attachment", |tx| remove_attachment(tx, stream))
        .expect("deletes");
    assert_eq!(removed, 1);
    assert!(document.attachments().expect("lists").is_empty());
    assert!(
        document
            .edit_document("Delete Attachment", |tx| remove_attachment(tx, stream))
            .expect("runs")
            == 0,
        "a second delete finds nothing"
    );
}
