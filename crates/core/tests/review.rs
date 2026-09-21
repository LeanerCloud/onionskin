//! The Comments pane's writes: a comment's text, a reply, and a status, read
//! back through `Document::annotations`, which reads the edited document.

use onionskin_core::review::{add_reply, set_contents, set_status, REVIEW_MODEL};
use onionskin_core::{
    add_annotation, Annotation, BaseFont, Color, Document, Intent, Rect, Subtype, TextStyle,
};
use onionskin_corpus_testing::seed;

const NOW: i64 = 1_758_000_000;

fn with_text_box() -> (Document, onionskin_core::ObjRef) {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    let page = document
        .structure()
        .expect("doc")
        .page(0)
        .expect("page")
        .objref;
    let placed = document
        .edit_annotations("Text Box", |tx, structure| {
            let mut annotation =
                Annotation::new(Subtype::FreeText, Rect::new(20.0, 20.0, 180.0, 60.0));
            annotation.text_style = Some(TextStyle::new(BaseFont::Courier, 10.0, Color::BLACK));
            annotation.intent = Some(Intent::FreeTextTypewriter);
            add_annotation(tx, structure, page, &annotation, NOW)
        })
        .expect("places");
    (document, placed)
}

#[test]
fn a_text_box_given_text_draws_it_in_its_own_style() {
    let (mut document, placed) = with_text_box();
    document
        .edit_document("Edit Comment Text", |tx| {
            set_contents(tx, placed, "Hello there", NOW)
        })
        .expect("sets");
    let annotations = document.annotations().expect("reads");
    assert_eq!(annotations[0].contents.as_deref(), Some("Hello there"));

    let structure = document.structure().expect("doc");
    let dict = structure.get(placed.number).expect("reads").object;
    let ap = dict
        .as_dict()
        .and_then(|dict| dict.get(b"AP"))
        .and_then(|ap| ap.as_dict())
        .and_then(|ap| ap.get(b"N"))
        .and_then(|n| n.as_reference())
        .expect("an appearance");
    let stream = structure.get(ap.number).expect("reads").object;
    let raw = String::from_utf8_lossy(&stream.as_stream().expect("a stream").raw).into_owned();
    assert!(raw.contains("(Hello there) Tj"), "{raw}");
    assert!(
        raw.contains("/Cour 10 Tf"),
        "the box's own font and size: {raw}"
    );
}

/// Status is a hidden reply carrying `/State` and `/StateModel`, as Acrobat
/// writes it, never a key on the comment itself.
#[test]
fn a_reply_and_a_status_are_hidden_answers_naming_the_comment() {
    let (mut document, placed) = with_text_box();
    let page = document
        .structure()
        .expect("doc")
        .page(0)
        .expect("page")
        .objref;
    document
        .edit_annotations("Reply", |tx, structure| {
            add_reply(tx, structure, page, placed, "Agreed", Some("Ana"), NOW)
        })
        .expect("replies");
    document
        .edit_annotations("Set Status", |tx, structure| {
            set_status(
                tx,
                structure,
                page,
                placed,
                REVIEW_MODEL,
                "Accepted",
                Some("Ana"),
                NOW,
            )
        })
        .expect("sets status");

    let annotations = document.annotations().expect("reads");
    let answers: Vec<_> = annotations
        .iter()
        .filter(|annotation| annotation.in_reply_to == Some(placed))
        .collect();
    assert_eq!(answers.len(), 2);
    assert!(answers.iter().all(|answer| answer.flags.is_hidden()));
    assert_eq!(answers[0].contents.as_deref(), Some("Agreed"));
    assert_eq!(answers[0].author.as_deref(), Some("Ana"));
    assert_eq!(
        answers[1].state,
        Some(("Accepted".to_owned(), "Review".to_owned()))
    );
    let comment = annotations
        .iter()
        .find(|annotation| annotation.objref == placed)
        .expect("the comment");
    assert_eq!(
        comment.state, None,
        "the status is not written on the comment"
    );
}
