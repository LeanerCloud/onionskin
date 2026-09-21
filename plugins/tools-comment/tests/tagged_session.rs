//! Comments on a tagged document, several in one session.
//!
//! Each comment attaches a structure element and a `/ParentTree` entry, and
//! the attachment is written from the structure tree it is handed. Handed the
//! file's tree rather than the session's, the second comment is given the
//! first one's key and rewrites the `/ParentTree` without it: the first
//! comment silently leaves the reading order.

use std::collections::BTreeSet;

use onionskin_core::{read_structure, Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_cos::Object;
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_comment::NoteTool;

#[test]
fn every_comment_in_a_session_keeps_its_own_structure_element() {
    let mut doc = Document::open_bytes(tagged_page()).expect("opens");
    let mut viewport = Viewport::new(
        1,
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
        12.0,
    )
    .expect("viewport");
    viewport
        .measure_page(doc.page_geometry(0).expect("measures").clone())
        .expect("measurable");
    viewport.fit(FitMode::Page).expect("fits");

    let mut tool = NoteTool::new();
    for (x, y) in [(100.0, 700.0), (300.0, 500.0), (400.0, 200.0)] {
        let mut ctx = ToolCtx {
            doc: &mut doc,
            viewport: &mut viewport,
        };
        let input = PointerInput {
            at: PagePoint { page: 0, x, y },
            pressure: 1.0,
            modifiers: Modifiers::default(),
            clicks: 1,
        };
        tool.on_pointer_down(&mut ctx, input);
        tool.on_pointer_up(&mut ctx, input);
    }

    let current = doc.structure().expect("the session's document");
    let structure = read_structure(current).expect("reads");
    let tree = structure.tree().expect("still tagged");
    assert_eq!(
        tree.parent_tree.len(),
        1 + 3,
        "the paragraph's entry and one per comment: {:?}",
        tree.parent_tree.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        tree.roots.len(),
        1 + 3,
        "and each comment's element in the root"
    );

    let page = current.page(0).expect("page");
    let Some(Object::Array(annots)) = current
        .resolve(page.dict.get(b"Annots").expect("annots"))
        .ok()
    else {
        panic!("the page has an /Annots array");
    };
    let keys: BTreeSet<i64> = annots
        .iter()
        .map(|annotation| {
            let Ok(Object::Dict(dict)) = current.resolve(annotation) else {
                panic!("an annotation dictionary");
            };
            dict.get(b"StructParent")
                .and_then(Object::as_integer)
                .expect("each comment has a /StructParent")
        })
        .collect();
    assert_eq!(keys.len(), 3, "no two comments share a key: {keys:?}");
}

/// One tagged page: a paragraph whose text is MCID 0.
fn tagged_page() -> Vec<u8> {
    let content = "/P << /MCID 0 >> BDC BT /F1 24 Tf 72 700 Td (Tagged) Tj ET EMC";
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /StructParents 0 /Contents 4 0 R /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /StructTreeRoot /K [6 0 R] /ParentTree << /Nums [0 [6 0 R]] >> /ParentTreeNextKey 1 >>".into(),
        "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 0 >>".into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}
