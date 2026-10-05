//! The marked-content ids a page's own content opens: written in the
//! operator, named in `/Properties`, and not those inside a form it draws.

mod common;

use common::{one_page, open_bytes, stream};
use onionskin_content::page_mcids;

#[test]
fn a_pages_own_marked_content_ids_are_found() {
    let content = "/P << /MCID 0 >> BDC BT ET EMC /Span /P1 BDC EMC /Artifact BMC EMC \
                   /Figure << /Alt (x) >> BDC EMC /Fm0 Do";
    let resources = "<< /Properties << /P1 << /MCID 3 >> >> /XObject << /Fm0 5 0 R >> >>";
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
        b"/P << /MCID 9 >> BDC EMC",
    );
    let doc = open_bytes(one_page(content, resources, &[form]));
    let found: Vec<i64> = page_mcids(&doc, 0).expect("reads").into_iter().collect();
    assert_eq!(
        found,
        [0, 3],
        "not the form's own, not a sequence without one"
    );
    assert!(page_mcids(&doc, 4).is_err(), "no such page");
}

// ---- the sequences around each item ---------------------------------------

use onionskin_content::{extract_page, page_images, page_marked, page_shapes, MarkedRef};

const FONT: &str = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >>";

fn image() -> Vec<u8> {
    stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
        &[0x80],
    )
}

fn marks_of(texts: &[onionskin_content::TextRun]) -> Vec<(String, Option<MarkedRef>)> {
    texts
        .iter()
        .map(|run| (run.decoded_text.clone(), run.marked.clone()))
        .collect()
}

fn reference(mcid: Option<i64>, tag: &str, artifact: bool, depth: usize) -> Option<MarkedRef> {
    Some(MarkedRef {
        mcid,
        tag: Some(onionskin_cos::Name::new(tag)),
        artifact,
        depth,
    })
}

#[test]
fn each_run_image_and_path_knows_the_sequence_it_was_drawn_in() {
    let content = "BT /F1 12 Tf 10 10 Td (out) Tj ET \
        /P << /MCID 0 >> BDC BT /F1 12 Tf 10 50 Td (in) Tj ET \
        /Span /Sp BDC BT /F1 12 Tf 10 90 Td (nest) Tj ET EMC \
        q 10 0 0 10 0 0 cm /Im0 Do Q 0 0 5 5 re f EMC \
        0 0 1 1 re f";
    let resources =
        format!("<< {FONT} /XObject << /Im0 5 0 R >> /Properties << /Sp << /MCID 7 >> >> >>");
    let doc = open_bytes(one_page(content, &resources, &[image()]));
    let page = page_marked(&doc, 0).expect("reads");

    assert_eq!(
        marks_of(&page.text.runs),
        [
            ("out".to_string(), None),
            ("in".to_string(), reference(Some(0), "P", false, 1)),
            ("nest".to_string(), reference(Some(7), "Span", false, 2)),
        ],
        "the innermost id wins, named or inline, and nothing outside any sequence has one"
    );
    assert_eq!(page.images.len(), 1);
    assert_eq!(page.images[0].marked, reference(Some(0), "P", false, 1));
    assert_eq!(page.shapes.len(), 2);
    assert_eq!(page.shapes[0].marked, reference(Some(0), "P", false, 1));
    assert_eq!(page.shapes[1].marked, None, "painted after the EMC");
}

#[test]
fn an_artifact_is_flagged_even_around_marked_content_and_carries_no_id() {
    let content = "/Artifact BMC BT /F1 12 Tf (a) Tj ET EMC \
        /Artifact << /Type /Pagination >> BDC /P << /MCID 3 >> BDC BT /F1 12 Tf (b) Tj ET EMC EMC";
    let doc = open_bytes(one_page(content, &format!("<< {FONT} >>"), &[]));
    let runs = page_marked(&doc, 0).expect("reads").text.runs;
    assert_eq!(
        marks_of(&runs),
        [
            ("a".to_string(), reference(None, "Artifact", true, 1)),
            ("b".to_string(), reference(Some(3), "P", true, 2)),
        ]
    );
}

#[test]
fn a_form_takes_the_sequence_around_its_do_and_cannot_unbalance_it() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>",
        b"EMC BT /F1 12 Tf (stray) Tj ET /P << /MCID 9 >> BDC BT (own) Tj ET EMC /Span BDC BT (open) Tj ET",
    );
    let content = "/P << /MCID 2 >> BDC /Fm0 Do BT /F1 12 Tf 10 10 Td (after) Tj ET EMC \
                   BT /F1 12 Tf (outside) Tj ET";
    let resources = format!("<< {FONT} /XObject << /Fm0 5 0 R >> >>");
    let doc = open_bytes(one_page(content, &resources, &[form]));
    let runs = page_marked(&doc, 0).expect("reads").text.runs;
    assert_eq!(
        marks_of(&runs),
        [
            ("stray".to_string(), reference(Some(2), "P", false, 1),),
            ("own".to_string(), reference(Some(2), "P", false, 2),),
            ("open".to_string(), reference(Some(2), "P", false, 2),),
            ("after".to_string(), reference(Some(2), "P", false, 1),),
            ("outside".to_string(), None),
        ],
        "a stray EMC in the form leaves the page's sequence open, the form's own id is not \
         the page's, and the form's unclosed BDC is gone once the form ends"
    );
}

#[test]
fn nothing_is_tagged_unless_asked_for() {
    let content = "/P << /MCID 0 >> BDC BT /F1 12 Tf (a) Tj ET /Im0 Do 0 0 5 5 re f EMC";
    let resources = format!("<< {FONT} /XObject << /Im0 5 0 R >> >>");
    let doc = open_bytes(one_page(content, &resources, &[image()]));

    let plain = extract_page(&doc, 0).expect("reads");
    assert!(plain.runs.iter().all(|run| run.marked.is_none()));
    assert!(page_images(&doc, 0)
        .expect("reads")
        .iter()
        .all(|image| image.marked.is_none()));
    assert!(page_shapes(&doc, 0)
        .expect("reads")
        .iter()
        .all(|shape| shape.marked.is_none()));

    let mut tagged = page_marked(&doc, 0).expect("reads").text.runs;
    for run in &mut tagged {
        assert!(run.marked.take().is_some());
    }
    assert_eq!(
        tagged, plain.runs,
        "tagging adds the reference and changes nothing else about a run (a run's \
         /ActualText compares by identity, so this fixture has none)"
    );
}

#[test]
fn a_malformed_open_still_counts_so_its_emc_closes_it_and_nothing_else() {
    let content = "/P << /MCID 0 >> BDC 5 << >> BDC EMC BT /F1 12 Tf (x) Tj ET EMC \
                   BDC EMC BT /F1 12 Tf (y) Tj ET";
    let doc = open_bytes(one_page(content, &format!("<< {FONT} >>"), &[]));
    let runs = page_marked(&doc, 0).expect("reads").text.runs;
    assert_eq!(
        marks_of(&runs),
        [
            ("x".to_string(), reference(Some(0), "P", false, 1)),
            ("y".to_string(), None),
        ],
        "the nameless BDC is closed by its own EMC, leaving the page's sequence open for x"
    );
}

#[test]
fn nesting_deeper_than_the_old_cap_stays_aligned() {
    let nested = "/Span BMC ".repeat(300);
    let closed = "EMC ".repeat(300);
    let content = format!(
        "/P << /MCID 0 >> BDC {nested} BT /F1 12 Tf (deep) Tj ET {closed} BT /F1 12 Tf (back) Tj ET EMC"
    );
    let doc = open_bytes(one_page(&content, &format!("<< {FONT} >>"), &[]));
    let runs = page_marked(&doc, 0).expect("reads").text.runs;
    let [deep, back] = &runs[..] else {
        panic!("{}", runs.len());
    };
    assert_eq!(deep.marked, reference(Some(0), "P", false, 301));
    assert_eq!(back.marked, reference(Some(0), "P", false, 1));
}

#[test]
fn a_bmc_has_no_property_list_so_its_tag_is_not_looked_up_as_one() {
    let content = "/P BMC BT /F1 12 Tf (x) Tj ET EMC";
    let resources = format!("<< {FONT} /Properties << /P << /MCID 4 >> >> >>");
    let doc = open_bytes(one_page(content, &resources, &[]));
    let runs = page_marked(&doc, 0).expect("reads").text.runs;
    assert_eq!(
        marks_of(&runs),
        [("x".to_string(), reference(None, "P", false, 1))]
    );
    assert!(
        page_mcids(&doc, 0).expect("reads").is_empty(),
        "nor does page_mcids"
    );
}

#[test]
fn content_only_a_forms_own_sequence_encloses_has_no_page_id() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>",
        b"/Span << /MCID 9 >> BDC BT /F1 12 Tf (form) Tj ET EMC",
    );
    let resources = format!("<< {FONT} /XObject << /Fm0 5 0 R >> >>");
    let doc = open_bytes(one_page("/Fm0 Do", &resources, &[form]));
    let runs = page_marked(&doc, 0).expect("reads").text.runs;
    assert_eq!(
        marks_of(&runs),
        [("form".to_string(), reference(None, "Span", false, 1))],
        "reachable only through an /MCR with a /Stm"
    );
}
