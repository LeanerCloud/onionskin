//! `core::redactions`: marks written, read back from a fresh parse, restyled
//! and removed, and marks as Acrobat writes them.

use onionskin_core::redactions::{
    add_redaction, read_redactions, redaction_areas, remove_redaction, set_redaction, Align,
    Overlay, RedactionLook, RedactionMark, BLACK, RED,
};
use onionskin_core::{EditSession, Error, PageQuad, Structure, Transaction};
use onionskin_cos::{BytesSource, Document as CosDocument, ObjRef, Object};

mod common;
use common::{flat, pdf, stream};

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn apply<T>(
    original: &[u8],
    body: impl FnOnce(&mut Transaction<'_>, &Structure) -> onionskin_core::Result<T>,
) -> onionskin_core::Result<(Vec<u8>, T)> {
    let base = open(original);
    let structure = onionskin_core::read_structure(&base).expect("the structure reads");
    let mut edit = EditSession::for_base(&base);
    let value = edit.transact(&base, "Redact", |tx| body(tx, &structure))?;
    let mut bytes = original.to_vec();
    if let Some(section) = base
        .section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
    {
        bytes.extend_from_slice(&section);
    }
    Ok((bytes, value))
}

fn marks(bytes: &[u8]) -> Vec<RedactionMark> {
    let document = open(bytes);
    let count = document.page_count().expect("pages") as usize;
    read_redactions(&document, count).expect("marks read")
}

fn quad(x0: f64, y0: f64, x1: f64, y1: f64) -> PageQuad {
    PageQuad {
        page: 0,
        corners: [(x0, y1), (x1, y1), (x0, y0), (x1, y0)],
    }
}

fn overlay() -> Overlay {
    Overlay {
        text: "(b)(6)".to_owned(),
        size: 9.0,
        color: [1.0, 1.0, 0.0],
        align: Align::Right,
        repeat: true,
    }
}

#[test]
fn a_region_and_a_text_mark_are_written_and_read_back() {
    let quads = [quad(10.0, 20.0, 50.0, 30.0), quad(10.0, 5.0, 30.0, 15.0)];
    let (saved, (region, text)) = apply(&flat(2), |tx, _| {
        let region = add_redaction(
            tx,
            1,
            [300.0, 400.0, 100.0, 200.0],
            &[],
            &RedactionLook::default(),
        )?;
        let text = add_redaction(tx, 0, [0.0; 4], &quads, &RedactionLook::default())?;
        Ok((region, text))
    })
    .expect("marks");
    let found = marks(&saved);
    assert_eq!(found.len(), 2);
    let (first, second) = (&found[0], &found[1]);
    assert_eq!((first.objref, first.page), (text, 0));
    assert_eq!(first.rect, [10.0, 5.0, 50.0, 30.0], "the quads' bounds");
    assert_eq!(first.quads, quads);
    assert_eq!(first.look, RedactionLook::default());
    assert_eq!(redaction_areas(first).len(), 2);
    assert_eq!((second.objref, second.page), (region, 1));
    assert_eq!(second.rect, [100.0, 200.0, 300.0, 400.0]);
    assert!(second.quads.is_empty());
    assert_eq!(redaction_areas(second).len(), 1);
    assert!(second.contains((150.0, 300.0)) && !second.contains((50.0, 300.0)));
    assert_eq!(open(&saved).audit_references().expect("audits"), Vec::new());
}

#[test]
fn a_mark_is_outlined_until_it_is_applied() {
    let (saved, mark) = apply(&flat(1), |tx, _| {
        add_redaction(
            tx,
            0,
            [100.0, 100.0, 200.0, 150.0],
            &[],
            &RedactionLook::default(),
        )
    })
    .expect("marks");
    let document = open(&saved);
    let dict = document.get(mark.number).expect("the mark").object;
    let ap = dict
        .as_dict()
        .and_then(|dict| dict.get(b"AP"))
        .expect("an appearance");
    let normal = ap
        .as_dict()
        .and_then(|ap| ap.get(b"N"))
        .and_then(Object::as_reference);
    let stream = document
        .get(normal.expect("a normal appearance").number)
        .expect("the stream")
        .object;
    let content =
        String::from_utf8(stream.as_stream().expect("a stream").raw.clone()).expect("ascii");
    assert!(content.starts_with("1 0 0 RG"), "{content}");
    assert!(content.contains("100 100 100 50 re S"), "{content}");
}

#[test]
fn a_marks_look_changes_and_the_mark_can_be_removed() {
    let (saved, mark) = apply(&flat(1), |tx, _| {
        add_redaction(
            tx,
            0,
            [0.0; 4],
            &[quad(1.0, 1.0, 9.0, 9.0)],
            &RedactionLook::default(),
        )
    })
    .expect("marks");
    let look = RedactionLook {
        fill: None,
        outline: [0.0, 0.0, 1.0],
        overlay: Some(overlay()),
    };
    let (restyled, ()) = apply(&saved, |tx, _| set_redaction(tx, mark, &look)).expect("restyles");
    assert_eq!(marks(&restyled)[0].look, look);

    let (removed, was) = apply(&restyled, |tx, _| remove_redaction(tx, 0, mark)).expect("removes");
    assert!(was);
    assert!(marks(&removed).is_empty());
    let (_, again) = apply(&removed, |tx, _| remove_redaction(tx, 0, mark)).expect("runs");
    assert!(!again, "nothing to remove the second time");
}

#[test]
fn only_a_mark_takes_a_marks_look() {
    let error = apply(&flat(1), |tx, _| {
        set_redaction(tx, ObjRef::new(3, 0), &RedactionLook::default())
    })
    .unwrap_err();
    assert!(
        matches!(error, Error::NotADictionary { number: 3 }),
        "{error:?}"
    );
    let missing = apply(&flat(1), |tx, _| {
        set_redaction(tx, ObjRef::new(99, 0), &RedactionLook::default())
    })
    .unwrap_err();
    assert!(
        matches!(missing, Error::NotADictionary { number: 99 }),
        "{missing:?}"
    );
    assert!(apply(&flat(1), |tx, _| {
        add_redaction(tx, 5, [0.0; 4], &[], &RedactionLook::default())
    })
    .is_err());
}

#[test]
fn marks_as_acrobat_writes_them() {
    // No /IC is black; /C stands for the outline; /DA's grey and size and
    // /Q centre the overlay; a gray /IC and a mark with no rectangle.
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
           /Annots [5 0 R 6 0 R 7 0 R 8 0 R] >>"
            .to_vec(),
        stream(""),
        b"<< /Type /Annot /Subtype /Redact /Rect [10 10 50 50] /C [0 1 0] \
           /OverlayText (FOIA) /DA (/Helv 0 Tf 0.5 g) /Q 1 >>"
            .to_vec(),
        b"<< /Type /Annot /Subtype /Redact /IC [0.5] /OverlayText () >>".to_vec(),
        b"<< /Type /Annot /Subtype /Square /Rect [0 0 1 1] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Redact /Rect [0 0 1 1] /IC 9 0 R /QuadPoints [1 2 3] >>"
            .to_vec(),
        b"[0 0 1]".to_vec(),
    ]);
    let found = marks(&bytes);
    assert_eq!(found.len(), 3, "the square is not a mark");
    assert_eq!(found[0].look.fill, Some(BLACK));
    assert_eq!(found[0].look.outline, [0.0, 1.0, 0.0]);
    let overlay = found[0].look.overlay.clone().expect("an overlay");
    assert_eq!(
        (
            overlay.text.as_str(),
            overlay.size,
            overlay.color,
            overlay.align,
            overlay.repeat
        ),
        ("FOIA", 0.0, [0.5; 3], Align::Centre, false)
    );
    assert_eq!(found[1].look.fill, Some([0.5; 3]));
    assert_eq!(found[1].look.outline, RED);
    assert_eq!(found[1].look.overlay, None, "empty overlay text is none");
    assert_eq!(found[1].rect, [0.0; 4]);
    assert_eq!(found[2].look.fill, Some([0.0, 0.0, 1.0]), "an indirect /IC");
    assert!(found[2].quads.is_empty(), "a short /QuadPoints has no quad");
    assert_eq!(Align::ALL.map(Align::label), ["Left", "Centre", "Right"]);
}
