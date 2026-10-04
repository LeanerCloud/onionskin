//! Redaction's content rewrite on synthetic pages: what each kind of
//! content becomes when an area covers it, read back through extraction.

mod common;

use common::{one_page, open_bytes, stream};
use onionskin_content::redact::{covers_glyph, Area, NewResource, PageRedaction};
use onionskin_content::{extract_page, redact_page, redact_page_with_hidden, PageText, Tokenizer};
use onionskin_cos::{Dict, ObjRef, Object};

const FONT: &str = "<< /Font << /F1 5 0 R >> >>";

fn helvetica() -> Vec<u8> {
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()
}

fn courier() -> Vec<u8> {
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec()
}

fn redact(content: &str, resources: &str, extra: &[Vec<u8>], areas: &[Area]) -> PageRedaction {
    let doc = open_bytes(one_page(content, resources, extra));
    redact_page(&doc, 0, areas).expect("redacts")
}

/// The page again, drawn by the rewritten content.
fn reread(redaction: &PageRedaction, resources: &str, extra: &[Vec<u8>]) -> PageText {
    let content = String::from_utf8(redaction.content.bytes.clone()).expect("ascii content");
    let doc = open_bytes(one_page(&content, resources, extra));
    extract_page(&doc, 0).expect("extracts")
}

fn text_of(page: &PageText) -> String {
    page.runs
        .iter()
        .map(|run| run.decoded_text.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

fn marked_properties(bytes: &[u8]) -> Vec<Dict> {
    let mut tokenizer = Tokenizer::new(bytes);
    let mut properties = Vec::new();
    while let Some(operation) = tokenizer.next_operation() {
        if operation.operator.is(b"BDC") {
            if let Some(Object::Dict(dict)) = operation.tail(1).and_then(|tail| tail.first()) {
                properties.push(dict.clone());
            }
        }
    }
    properties
}

fn property_string(dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    match dict.get(key) {
        Some(Object::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn do_names(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut tokenizer = Tokenizer::new(bytes);
    let mut names = Vec::new();
    while let Some(operation) = tokenizer.next_operation() {
        if operation.operator.is(b"Do") {
            if let Some(Object::Name(name)) = operation.tail(1).and_then(|tail| tail.first()) {
                names.push(name.as_bytes().to_vec());
            }
        }
    }
    names
}

#[test]
fn actual_text_partial_redaction_removes_one_member_without_replacement_leakage() {
    let content = "BT /F1 10 Tf 1 0 0 1 10 100 Tm /Span << /ActualText (XY) /Alt (Alt text) /E (expanded) >> BDC (AB) Tj (CD) Tj EMC ET";
    let doc = open_bytes(one_page(content, FONT, &[courier()]));
    let before = extract_page(&doc, 0).expect("extracts");
    let source_quads = before
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect::<Vec<_>>();
    let first = source_quads[0].corners;
    let area = Area::rect(first[2].0, first[2].1, first[1].0, first[1].1);
    assert!(covers_glyph(
        std::slice::from_ref(&area),
        &before.runs[0].glyphs[0].quad
    ));
    for quad in &source_quads[1..] {
        assert!(!covers_glyph(std::slice::from_ref(&area), quad));
    }
    let redaction = redact(content, FONT, &[courier()], &[area]);
    assert_eq!(
        redaction
            .removed
            .iter()
            .map(|glyph| glyph.text.as_str())
            .collect::<String>(),
        "A"
    );
    let bytes = String::from_utf8(redaction.content.bytes.clone()).expect("ascii");
    let properties = marked_properties(bytes.as_bytes());
    assert_eq!(properties.len(), 1);
    assert!(!properties[0].contains(b"ActualText"));
    assert!(!properties[0].contains(b"Alt"));
    assert!(!properties[0].contains(b"E"));
    let after = reread(&redaction, FONT, &[courier()]);
    assert_eq!(after.flatten().text, "BCD");
    assert_eq!(
        after
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        [66, 67, 68]
    );
    assert_eq!(
        after
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>(),
        source_quads[1..]
    );
}

#[test]
fn nested_actual_text_redaction_keeps_inner_hole_and_strips_owned_metadata() {
    let content = "BT /F1 10 Tf 1 0 0 1 10 100 Tm /Span << /ActualText (OUT) /Alt (outer) /E (outer expanded) >> BDC (A) Tj /Span << /ActualText (INNER) /Alt (inner) /E (inner expanded) >> BDC (H) Tj EMC (B) Tj EMC ET";
    let doc = open_bytes(one_page(content, FONT, &[courier()]));
    let before = extract_page(&doc, 0).expect("extracts");
    let source_quads = before
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect::<Vec<_>>();
    assert_eq!(source_quads.len(), 3);

    let redact_outer = redact(
        content,
        FONT,
        &[courier()],
        &[Area::rect(
            source_quads[0].corners[2].0,
            source_quads[0].corners[2].1,
            source_quads[0].corners[1].0,
            source_quads[0].corners[1].1,
        )],
    );
    let outer_bytes = String::from_utf8(redact_outer.content.bytes.clone()).expect("ascii");
    assert!(!outer_bytes.contains("ActualText (OUT)"));
    assert!(!outer_bytes.contains("/Alt (outer)") && !outer_bytes.contains("/E (outer expanded)"));
    assert!(outer_bytes.contains("ActualText (INNER)"));
    assert!(outer_bytes.contains("/Alt (inner)") && outer_bytes.contains("/E (inner expanded)"));
    let outer_properties = marked_properties(outer_bytes.as_bytes());
    assert_eq!(outer_properties.len(), 2);
    assert!(!outer_properties[0].contains(b"ActualText"));
    assert!(!outer_properties[0].contains(b"Alt"));
    assert!(!outer_properties[0].contains(b"E"));
    assert_eq!(
        property_string(&outer_properties[1], b"ActualText"),
        Some(b"INNER".to_vec())
    );
    assert_eq!(
        property_string(&outer_properties[1], b"Alt"),
        Some(b"inner".to_vec())
    );
    assert_eq!(
        property_string(&outer_properties[1], b"E"),
        Some(b"inner expanded".to_vec())
    );
    let outer_after = reread(&redact_outer, FONT, &[courier()]);
    assert_eq!(outer_after.flatten().text, "INNERB");
    assert_eq!(
        outer_after
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        [72, 66]
    );
    assert_eq!(
        outer_after
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>(),
        source_quads[1..]
    );

    let redact_inner = redact(
        content,
        FONT,
        &[courier()],
        &[Area::rect(
            source_quads[1].corners[2].0,
            source_quads[1].corners[2].1,
            source_quads[1].corners[1].0,
            source_quads[1].corners[1].1,
        )],
    );
    let inner_bytes = String::from_utf8(redact_inner.content.bytes.clone()).expect("ascii");
    let inner_properties = marked_properties(inner_bytes.as_bytes());
    assert_eq!(inner_properties.len(), 2);
    for properties in &inner_properties {
        assert!(!properties.contains(b"ActualText"));
        assert!(!properties.contains(b"Alt"));
        assert!(!properties.contains(b"E"));
    }
    let inner_after = reread(&redact_inner, FONT, &[courier()]);
    assert_eq!(inner_after.flatten().text, "A B");
    assert_eq!(
        inner_after
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        [65, 66]
    );
    assert_eq!(
        inner_after
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>(),
        [source_quads[0], source_quads[2]]
    );
}

#[test]
fn repeated_actual_text_form_redaction_changes_only_one_placement() {
    let resources = "<< /XObject << /Fm0 5 0 R >> >>";
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << /F1 6 0 R >> >>",
        b"BT /F1 10 Tf 1 0 0 1 10 100 Tm /Span << /ActualText (XY) /Alt (alt) /E (expanded) >> BDC (A) Tj (B) Tj EMC ET",
    );
    let extra = vec![form.clone(), courier()];
    let content = "/Fm0 Do q 1 0 0 1 0 -30 cm /Fm0 Do Q";
    let doc = open_bytes(one_page(content, resources, &extra));
    let before = extract_page(&doc, 0).expect("extracts");
    let original_form = doc.get(5).expect("form object");
    let original_form_stream = original_form.object.as_stream().expect("form stream");
    let original_form_bytes = doc
        .decode_stream(original_form_stream)
        .expect("form content");
    let original_properties = marked_properties(&original_form_bytes);
    assert_eq!(original_properties.len(), 1);
    assert_eq!(
        property_string(&original_properties[0], b"ActualText"),
        Some(b"XY".to_vec())
    );
    assert_eq!(
        property_string(&original_properties[0], b"Alt"),
        Some(b"alt".to_vec())
    );
    assert_eq!(
        property_string(&original_properties[0], b"E"),
        Some(b"expanded".to_vec())
    );
    assert_eq!(before.runs.len(), 4);
    let first = before.runs[0].glyphs[0].quad;
    let second = before.runs[2].glyphs[0].quad;
    let area = Area::rect(
        first.corners[2].0,
        first.corners[2].1,
        first.corners[1].0,
        first.corners[1].1,
    );
    let redaction = redact(content, resources, &extra, &[area]);
    assert!(redaction.content.resources.iter().any(|resource| matches!(
        resource,
        NewResource::Form { original, .. } if original.number == 5
    )));
    let NewResource::Form {
        name,
        content: rewritten_form,
        ..
    } = &redaction.content.resources[0]
    else {
        panic!("{:?}", redaction.content.resources);
    };
    let generated_name = String::from_utf8_lossy(name.as_bytes());
    assert_ne!(generated_name, "Fm0");
    let rewritten_properties = marked_properties(&rewritten_form.bytes);
    assert_eq!(rewritten_properties.len(), 1);
    assert!(!rewritten_properties[0].contains(b"ActualText"));
    assert!(!rewritten_properties[0].contains(b"Alt"));
    assert!(!rewritten_properties[0].contains(b"E"));
    let generated_resources = format!("<< /XObject << /Fm0 5 0 R /{generated_name} 7 0 R >> >>");
    let generated_form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << /F1 6 0 R >> >>",
        &rewritten_form.bytes,
    );
    let materialized_extra = vec![form, courier(), generated_form];
    let after_doc = open_bytes(one_page(
        &String::from_utf8(redaction.content.bytes.clone()).expect("ascii"),
        &generated_resources,
        &materialized_extra,
    ));
    assert_eq!(
        do_names(&redaction.content.bytes),
        vec![name.as_bytes().to_vec(), b"Fm0".to_vec()]
    );
    let after = extract_page(&after_doc, 0).expect("extracts");
    assert_eq!(after.flatten().text, "B\nXY");
    let after_quads = after
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect::<Vec<_>>();
    assert_eq!(
        after
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        [66, 65, 66]
    );
    assert_eq!(
        after_quads,
        vec![
            before.runs[1].glyphs[0].quad,
            second,
            before.runs[3].glyphs[0].quad
        ]
    );
}

fn first_x(page: &PageText, text: &str) -> f64 {
    let run = page
        .runs
        .iter()
        .find(|run| run.decoded_text.contains(text))
        .expect("the text is there");
    let at = run.decoded_text.find(text).expect("found");
    run.quads_for_decoded(at..at + text.len())[0].corners[0].0
}

/// Where "Secret" sits on `BT /F1 10 Tf 10 100 Td (Hello Secret World) Tj`.
fn secret_area(content: &str) -> Area {
    let doc = open_bytes(one_page(content, FONT, &[helvetica()]));
    let page = extract_page(&doc, 0).expect("extracts");
    let run = &page.runs[0];
    let at = run.decoded_text.find("Secret").expect("found");
    let quads = run.quads_for_decoded(at..at + "Secret".len());
    let (first, last) = (quads[0], quads[quads.len() - 1]);
    Area::rect(
        first.corners[2].0,
        first.corners[2].1,
        last.corners[1].0,
        last.corners[1].1,
    )
}

#[test]
fn covered_glyphs_go_and_the_rest_of_the_line_stays_put() {
    for content in [
        "BT /F1 10 Tf 10 100 Td (Hello Secret World) Tj ET",
        "BT /F1 10 Tf 1 Tc 2 Tw 10 100 Td [(Hello Sec) -20 (ret World)] TJ ET",
        "BT /F1 10 Tf 12 TL 10 112 Td (Hello Secret World) ' ET",
        "BT /F1 10 Tf 12 TL 10 112 Td 2 1 (Hello Secret World) \" ET",
    ] {
        let area = secret_area(content);
        let before = extract_page(&open_bytes(one_page(content, FONT, &[helvetica()])), 0)
            .expect("extracts");
        let redaction = redact(content, FONT, &[helvetica()], &[area]);
        assert!(redaction.content.changed, "{content}");
        assert_eq!(redaction.counts.glyphs, 6, "{content}");
        let removed: String = redaction.removed.iter().map(|g| g.text.as_str()).collect();
        assert_eq!(removed, "Secret", "{content}");

        let after = reread(&redaction, FONT, &[helvetica()]);
        let text = text_of(&after);
        assert!(
            !text.contains("Secret") && !text.contains("Sec"),
            "{content}: {text}"
        );
        assert!(
            text.contains("Hello") && text.contains("World"),
            "{content}: {text}"
        );
        let (moved, was) = (first_x(&after, "World"), first_x(&before, "World"));
        assert!(
            (moved - was).abs() < 1e-6,
            "{content}: World at {moved}, was {was}"
        );
    }
}

#[test]
fn text_outside_every_area_is_left_as_written() {
    let content = "BT /F1 10 Tf 10 100 Td (Hello) Tj ET";
    let redaction = redact(
        content,
        FONT,
        &[helvetica()],
        &[Area::rect(0.0, 0.0, 5.0, 5.0)],
    );
    assert!(!redaction.content.changed);
    assert_eq!(
        redaction.content.bytes, b"BT\n/F1 10 Tf\n10 100 Td\n(Hello) Tj\nET",
        "each operation as written, one to a line"
    );
    let untouched = redact(content, FONT, &[helvetica()], &[]);
    assert!(!untouched.content.changed);
}

#[test]
fn text_with_no_font_starting_inside_an_area_goes() {
    let content = "BT 10 100 Td (lost) Tj 12 TL (moved) ' 20 150 Td (kept) Tj ET";
    let redaction = redact(content, "<< >>", &[], &[Area::rect(0.0, 80.0, 50.0, 110.0)]);
    assert_eq!(redaction.counts.fontless, 2);
    let bytes = String::from_utf8(redaction.content.bytes).expect("ascii");
    assert!(
        !bytes.contains("lost") && !bytes.contains("moved"),
        "{bytes}"
    );
    assert!(
        bytes.contains("T*") && bytes.contains("(kept) Tj"),
        "{bytes}"
    );
}

#[test]
fn paths_wholly_inside_go_and_clips_stay() {
    let content = "q 20 20 10 10 re f 0 0 200 200 re S 25 25 m 28 28 l W n Q 22 22 5 5 re W f";
    let redaction = redact(content, "<< >>", &[], &[Area::rect(15.0, 15.0, 40.0, 40.0)]);
    assert_eq!(redaction.counts.paths, 2);
    let bytes = String::from_utf8(redaction.content.bytes).expect("ascii");
    assert_eq!(
        bytes,
        "q\n0 0 200 200 re\nS\n25 25 m\n28 28 l\nW\nn\nQ\n22 22 5 5 re\nW\nn"
    );
}

#[test]
fn an_unpainted_path_is_written_back() {
    let content = "10 10 m 20 20 l BT ET 30 30 m";
    let redaction = redact(content, "<< >>", &[], &[Area::rect(0.0, 0.0, 200.0, 200.0)]);
    let bytes = String::from_utf8(redaction.content.bytes).expect("ascii");
    assert_eq!(bytes, "10 10 m\n20 20 l\nBT\nET\n30 30 m");
}

#[test]
fn inline_images_touching_an_area_go() {
    let content = "q 10 0 0 10 20 20 cm BI /W 1 /H 1 /CS /G /BPC 8 ID A EI Q \
                   q 10 0 0 10 150 150 cm BI /W 1 /H 1 /CS /G /BPC 8 ID A EI Q";
    let redaction = redact(content, "<< >>", &[], &[Area::rect(0.0, 0.0, 50.0, 50.0)]);
    assert_eq!(redaction.counts.inline_images, 1);
    assert_eq!(
        redaction
            .content
            .bytes
            .windows(2)
            .filter(|w| w == b"BI")
            .count(),
        1
    );
}

fn image() -> Vec<u8> {
    stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
        &[0, 0, 0, 0],
    )
}

#[test]
fn an_image_touching_an_area_is_renamed_for_its_scrubbed_copy() {
    let resources = "<< /XObject << /Im0 5 0 R >> >>";
    let content = "q 100 0 0 50 10 10 cm /Im0 Do Q q 20 0 0 20 170 170 cm /Im0 Do Q";
    let redaction = redact(
        content,
        resources,
        &[image()],
        &[Area::rect(0.0, 0.0, 30.0, 30.0)],
    );
    assert_eq!(redaction.counts.images, 1);
    let [NewResource::Image {
        name,
        original,
        placement,
    }] = redaction.content.resources.as_slice()
    else {
        panic!("{:?}", redaction.content.resources);
    };
    assert_eq!(original.number, 5);
    assert_eq!((placement.a, placement.d, placement.e), (100.0, 50.0, 10.0));
    let bytes = String::from_utf8(redaction.content.bytes.clone()).expect("ascii");
    assert!(bytes.contains(&format!("/{} Do", String::from_utf8_lossy(name.as_bytes()))));
    assert!(bytes.contains("/Im0 Do"), "the far one is kept");
}

#[test]
fn a_form_is_rewritten_only_when_its_content_changes() {
    let resources = "<< /XObject << /Fm0 5 0 R /Fm1 6 0 R >> >>";
    let form = |text: &str| {
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << /F1 7 0 R >> >>",
            format!("BT /F1 10 Tf 10 100 Td ({text}) Tj ET").as_bytes(),
        )
    };
    let extra = [form("Secret"), form("Public"), helvetica()];
    let content = "/Fm0 Do q 1 0 0 1 0 50 cm /Fm1 Do Q";
    let redaction = redact(
        content,
        resources,
        &extra,
        &[Area::rect(0.0, 90.0, 200.0, 120.0)],
    );
    assert_eq!(redaction.counts.forms, 1);
    assert_eq!(redaction.counts.glyphs, 6);
    let [NewResource::Form {
        original, content, ..
    }] = redaction.content.resources.as_slice()
    else {
        panic!("{:?}", redaction.content.resources);
    };
    assert_eq!(original.number, 5);
    let inner = String::from_utf8(content.bytes.clone()).expect("ascii");
    assert!(inner.contains("TJ") && !inner.contains("Secret"), "{inner}");
    let bytes = String::from_utf8(redaction.content.bytes.clone()).expect("ascii");
    assert!(
        bytes.contains("/Fm1 Do") && !bytes.contains("/Fm0 Do"),
        "{bytes}"
    );
}

#[test]
fn an_unreadable_form_over_an_area_goes() {
    let resources = "<< /XObject << /Fm0 5 0 R /Fm1 6 0 R >> >>";
    let broken = |bbox: &str| {
        stream(
            &format!("/Type /XObject /Subtype /Form /BBox {bbox} /Filter /FlateDecode"),
            b"not flate",
        )
    };
    let extra = [broken("[0 0 50 50]"), broken("[150 150 200 200]")];
    let redaction = redact(
        "/Fm0 Do /Fm1 Do",
        resources,
        &extra,
        &[Area::rect(0.0, 0.0, 20.0, 20.0)],
    );
    assert_eq!(redaction.counts.forms, 1);
    assert_eq!(redaction.content.bytes, b"/Fm1 Do");
}

#[test]
fn a_sequence_that_lost_a_glyph_no_longer_spells_it() {
    let content =
        "/Span << /ActualText (Secret) /MCID 0 >> BDC BT /F1 10 Tf 10 100 Td (Secret) Tj ET EMC \
                   /Span << /ActualText (Kept) >> BDC BT /F1 10 Tf 10 150 Td (Kept) Tj ET EMC";
    let redaction = redact(
        content,
        FONT,
        &[helvetica()],
        &[Area::rect(0.0, 90.0, 200.0, 120.0)],
    );
    let bytes = String::from_utf8(redaction.content.bytes).expect("ascii");
    assert!(!bytes.contains("Secret"), "{bytes}");
    assert!(bytes.contains("/Span <</MCID 0>> BDC"), "{bytes}");
    assert!(bytes.contains("/ActualText (Kept)"), "{bytes}");
}

#[test]
fn a_soft_mask_group_that_drew_text_is_rewritten() {
    let resources = "<< /ExtGState << /GS0 5 0 R >> /Font << /F1 7 0 R >> >>";
    let extra = [
        b"<< /Type /ExtGState /SMask << /S /Luminosity /G 6 0 R >> >>".to_vec(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Group << /S /Transparency >>",
            b"BT /F1 10 Tf 10 100 Td (Masked) Tj ET",
        ),
        helvetica(),
    ];
    let redaction = redact(
        "/GS0 gs 0 0 200 200 re f",
        resources,
        &extra,
        &[Area::rect(0.0, 90.0, 200.0, 120.0)],
    );
    let [NewResource::GState { group, content, .. }] = redaction.content.resources.as_slice()
    else {
        panic!("{:?}", redaction.content.resources);
    };
    assert_eq!(group.number, 6);
    assert!(!String::from_utf8_lossy(&content.bytes).contains("Masked"));
    assert!(!String::from_utf8_lossy(&redaction.content.bytes).contains("/GS0 gs"));
}

#[test]
fn a_hidden_layer_draws_nothing_and_the_text_after_it_stays_put() {
    let resources = "<< /Font << /F1 5 0 R >> /Properties << /OC1 6 0 R /MC0 7 0 R >> \
                     /XObject << /Im0 8 0 R /Im1 9 0 R >> >>";
    let extra = [
        helvetica(),
        b"<< /Type /OCG /Name (Draft) >>".to_vec(),
        b"<< /Type /OCMD /OCGs [6 0 R] >>".to_vec(),
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0],
        ),
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /OC 6 0 R",
            &[0],
        ),
    ];
    let content = "BT /F1 10 Tf 10 100 Td /OC /OC1 BDC (Hidden ) Tj /Span BMC (more) Tj EMC EMC (Shown) Tj ET \
                   /OC /MC0 BDC 0 0 10 10 re f /Im0 Do 0 0 1 1 re W n EMC /Im1 Do /Im0 Do";
    let doc = open_bytes(one_page(content, resources, &extra));
    let before = extract_page(&doc, 0).expect("extracts");
    let redaction = redact_page_with_hidden(&doc, 0, &[], &[ObjRef::new(6, 0)]).expect("redacts");
    assert_eq!(
        redaction.counts.hidden,
        11 + 4,
        "the glyphs and each painting operator"
    );
    let after = reread(&redaction, resources, &extra);
    let text = text_of(&after);
    assert!(!text.contains("Hidden") && !text.contains("more"), "{text}");
    assert!((first_x(&after, "Shown") - first_x(&before, "Shown")).abs() < 1e-6);
    let bytes = String::from_utf8(redaction.content.bytes).expect("ascii");
    assert_eq!(
        bytes.matches(" Do").count(),
        1,
        "only the last image, which is shown: {bytes}"
    );
    assert!(
        !bytes.contains("re\nf") && !bytes.contains("W\nn"),
        "{bytes}"
    );

    let shown = redact_page(&doc, 0, &[]).expect("redacts");
    assert!(
        !shown.content.changed,
        "with no layer named, nothing is hidden"
    );
}
