//! `ContentMap` beyond the outcomes `tests/structure.rs` walks: claims, `/OBJR`
//! kids, `/ActualText`, a `/Pg` that only the element states, images, and an
//! `/MCR`'s `/Stm` surviving a rewrite of its `/K`.

use onionskin_core::{
    read_structure, ContentMap, EditSession, ElementContent, ItemKind, Kid, Unplaced,
};
use onionskin_cos::{BytesSource, Document as CosDocument, ObjRef};

/// One page whose content is `content`, an image `Im0` as object 6, and one
/// element per entry of `elements` (objects 7 onward) under the root.
fn document(content: &str, elements: &[&str]) -> CosDocument {
    let kids: String = (0..elements.len())
        .map(|n| format!("{} 0 R ", 7 + n))
        .collect();
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources \
          << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> \
          /XObject << /Im0 6 0 R >> >> >>"
            .to_vec(),
        stream(content.as_bytes(), ""),
        format!("<< /Type /StructTreeRoot /K [{kids}] >>").into_bytes(),
        stream(
            &[0x80],
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
             /BitsPerComponent 8",
        ),
    ];
    objects.extend(elements.iter().map(|e| e.as_bytes().to_vec()));
    open(&pdf(&objects))
}

fn stream(data: &[u8], entries: &str) -> Vec<u8> {
    let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn content_of(doc: &CosDocument, map: &mut ContentMap<'_>, number: u32) -> ElementContent {
    let structure = read_structure(doc).expect("the tree reads");
    map.content_of(&structure.tree().expect("tagged").elements[&number])
        .expect("the content reads")
}

const TWO_RUNS: &str = "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td (a) Tj (b) Tj ET EMC";

#[test]
fn a_map_that_delivers_each_claim_once_reports_the_second_and_a_plain_one_repeats_it() {
    let element = "<< /S /P /Pg 3 0 R /K 0 >>";
    let doc = document(TWO_RUNS, &[element, element, element]);
    let mut plain = ContentMap::new(&doc).expect("indexes");
    for number in 7..=9 {
        assert_eq!(content_of(&doc, &mut plain, number).items.len(), 2);
    }
    let mut once = ContentMap::new(&doc).expect("indexes").each_claim_once();
    assert_eq!(content_of(&doc, &mut once, 7).items.len(), 2);
    for number in [8, 9] {
        let later = content_of(&doc, &mut once, number);
        assert!(later.items.is_empty());
        assert_eq!(later.unplaced, [Unplaced::Claimed { page: 0, mcid: 0 }]);
    }
}

#[test]
fn an_objr_kid_is_reported_as_an_object_not_as_nothing() {
    let doc = document(
        TWO_RUNS,
        &["<< /S /Link /Pg 3 0 R /K << /Type /OBJR /Pg 3 0 R /Obj 99 0 R >> >>"],
    );
    let found = content_of(&doc, &mut ContentMap::new(&doc).expect("indexes"), 7);
    assert!(found.items.is_empty() && found.unplaced.is_empty());
    assert_eq!(found.objects, [ObjRef::new(99, 0)]);
}

#[test]
fn a_marked_content_actual_text_is_the_text_and_is_said_once_for_all_its_runs() {
    let doc = document(
        "/Span << /MCID 0 /ActualText (fi) >> BDC BT /F1 12 Tf 10 10 Td (x) Tj (y) Tj ET EMC",
        &["<< /S /Span /Pg 3 0 R /K 0 >>"],
    );
    let found = content_of(&doc, &mut ContentMap::new(&doc).expect("indexes"), 7);
    let texts: Vec<_> = found.items.iter().map(|i| i.text.as_deref()).collect();
    assert_eq!(texts, [Some("fi"), Some("")]);
}

#[test]
fn an_mcr_without_a_page_takes_the_elements_page() {
    let doc = document(
        TWO_RUNS,
        &["<< /S /P /Pg 3 0 R /K << /Type /MCR /MCID 0 >> >>"],
    );
    let found = content_of(&doc, &mut ContentMap::new(&doc).expect("indexes"), 7);
    assert_eq!(found.items.len(), 2);
    assert!(found.unplaced.is_empty());
}

#[test]
fn an_image_is_content_with_the_bounds_it_is_drawn_in() {
    let doc = document(
        "/Figure << /MCID 0 >> BDC q 30 0 0 20 10 40 cm /Im0 Do Q EMC",
        &["<< /S /Figure /Pg 3 0 R /K 0 >>"],
    );
    let found = content_of(&doc, &mut ContentMap::new(&doc).expect("indexes"), 7);
    let [item] = &found.items[..] else {
        panic!("{:?}", found.items);
    };
    assert_eq!(item.kind, ItemKind::Image);
    assert_eq!(item.bounds, [10.0, 40.0, 40.0, 60.0]);
    assert_eq!(item.text, None);
}

/// Rewriting an element's `/K` rebuilds each `/MCR` from what the reader kept,
/// so an `/MCR` into a form's stream must keep its `/Stm` and `/StmOwn` or the
/// edit turns a form's content into a page's. The edit has to reach that
/// element: here a page removal dooms its other kid, a `Span` on the removed
/// page, so its `/K` is rewritten.
#[test]
fn a_rewritten_mcr_keeps_its_stream_and_its_owner() {
    let original = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /StructParents 0 >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /StructParents 1 >>"
            .to_vec(),
        b"<< /Type /StructTreeRoot /K [6 0 R] /ParentTree 8 0 R /ParentTreeNextKey 2 >>".to_vec(),
        b"<< /S /Figure /Pg 3 0 R /K [<< /Type /MCR /Pg 3 0 R /MCID 0 /Stm 9 0 R /StmOwn 10 0 R >> 7 0 R] >>"
            .to_vec(),
        b"<< /S /Span /Pg 4 0 R /K 0 >>".to_vec(),
        b"<< /Nums [0 [6 0 R] 1 [7 0 R]] >>".to_vec(),
        stream(b"", "/Type /XObject /Subtype /Form /BBox [0 0 1 1]"),
        b"<< /Type /Annot /Subtype /Widget >>".to_vec(),
    ]);
    let base = open(&original);
    let structure = read_structure(&base).expect("reads");
    let mut edit = EditSession::for_base(&base);
    let outcome = edit
        .transact(&base, "Remove Page", |tx| {
            let done = onionskin_core::remove_page(tx, &structure, ObjRef::new(4, 0))?;
            let mut pages = onionskin_cos::Dict::new();
            pages.set(
                onionskin_cos::Name::new("Type"),
                onionskin_cos::Object::name("Pages"),
            );
            pages.set(
                onionskin_cos::Name::new("Kids"),
                onionskin_cos::Object::Array(vec![onionskin_cos::Object::Ref(ObjRef::new(3, 0))]),
            );
            pages.set(
                onionskin_cos::Name::new("Count"),
                onionskin_cos::Object::Integer(1),
            );
            tx.put_object(2, 0, onionskin_cos::Object::Dict(pages))?;
            Ok(done)
        })
        .expect("the removal commits");
    assert_eq!(outcome, onionskin_core::Maintenance::Changed);

    let mut bytes = original.clone();
    bytes.extend_from_slice(
        &base
            .section_for(&edit.pending_edits(), &edit.trailer_edits())
            .expect("the section builds")
            .expect("there is one"),
    );
    let after = open(&bytes);
    let tree = read_structure(&after).expect("reads");
    let kids = &tree.tree().expect("tagged").elements[&6].kids;
    assert!(
        matches!(
            kids[..],
            [Kid::MarkedContent {
                mcid: 0,
                stream: Some(stream),
                stream_owner: Some(owner),
                ..
            }] if stream == ObjRef::new(9, 0) && owner == ObjRef::new(10, 0)
        ),
        "the Span went with its page and the MCR kept both: {kids:?}"
    );
}
