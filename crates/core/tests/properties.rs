//! A comment's properties: colour, opacity, author and subject are written
//! and drawn in one edit, which Undo takes back whole.

use onionskin_core::properties::{set_properties, CommentProperties};
use onionskin_core::{add_annotation, Annotation, Color, Document, Rect, Subtype};
use onionskin_corpus_testing::seed;

const NOW: i64 = 1_758_000_000;

fn with_rectangle() -> (Document, onionskin_core::ObjRef) {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    let page = document
        .structure()
        .expect("doc")
        .page(0)
        .expect("page")
        .objref;
    let placed = document
        .edit_annotations("Rectangle", |tx, structure| {
            let mut annotation =
                Annotation::new(Subtype::Square, Rect::new(20.0, 20.0, 120.0, 80.0));
            annotation.color = Some(Color::new(0.0, 0.0, 1.0));
            add_annotation(tx, structure, page, &annotation, NOW)
        })
        .expect("places");
    (document, placed)
}

/// The appearance stream the annotation now names, as text.
fn appearance(document: &mut Document, annotation: onionskin_core::ObjRef) -> String {
    let structure = document.structure().expect("doc");
    let dict = structure.get(annotation.number).expect("reads").object;
    let ap = dict
        .as_dict()
        .and_then(|dict| dict.get(b"AP"))
        .and_then(|ap| ap.as_dict())
        .and_then(|ap| ap.get(b"N"))
        .and_then(|n| n.as_reference())
        .expect("an appearance");
    let stream = structure.get(ap.number).expect("reads").object;
    let stream = stream.as_stream().expect("a stream");
    format!(
        "{:?}\n{}",
        stream.dict,
        String::from_utf8_lossy(&stream.raw)
    )
}

#[test]
fn new_properties_are_written_drawn_and_undone_as_one_edit() {
    let (mut document, placed) = with_rectangle();
    let before = appearance(&mut document, placed);
    assert!(before.contains("0 0 1 RG"), "{before}");

    document
        .edit_document("Comment Properties", |tx| {
            set_properties(
                tx,
                placed,
                &CommentProperties {
                    color: Some(Color::new(1.0, 0.0, 0.0)),
                    opacity: 0.5,
                    author: Some("  Ana Pop ".into()),
                    subject: Some("Layout".into()),
                },
                NOW,
            )
        })
        .expect("sets");

    let read = &document.annotations().expect("reads")[0];
    assert_eq!(read.color, Some(Color::new(1.0, 0.0, 0.0)));
    assert_eq!(read.opacity, Some(0.5));
    assert_eq!(read.author.as_deref(), Some("Ana Pop"), "trimmed");
    assert_eq!(read.subject.as_deref(), Some("Layout"));
    let after = appearance(&mut document, placed);
    assert!(
        after.contains("1 0 0 RG"),
        "the new colour is drawn: {after}"
    );
    assert!(after.contains("ExtGState"), "the opacity is drawn: {after}");

    document.undo().expect("undoes");
    let read = &document.annotations().expect("reads")[0];
    assert_eq!(read.color, Some(Color::new(0.0, 0.0, 1.0)));
    assert_eq!(read.opacity, None);
    assert_eq!(read.author, None);
    assert!(appearance(&mut document, placed).contains("0 0 1 RG"));
}

#[test]
fn full_opacity_and_blank_text_remove_their_keys() {
    let (mut document, placed) = with_rectangle();
    document
        .edit_document("Comment Properties", |tx| {
            set_properties(
                tx,
                placed,
                &CommentProperties {
                    color: None,
                    opacity: 3.0,
                    author: Some("   ".into()),
                    subject: None,
                },
                NOW,
            )
        })
        .expect("sets");
    let read = &document.annotations().expect("reads")[0];
    assert_eq!(read.opacity, None, "1.0 after clamping is the default");
    assert_eq!(read.author, None);
    assert_eq!(read.color, None);
}
