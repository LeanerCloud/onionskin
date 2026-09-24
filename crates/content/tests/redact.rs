//! Redaction's content rewrite on synthetic pages: what each kind of
//! content becomes when an area covers it, read back through extraction.

mod common;

use common::{one_page, open_bytes, stream};
use onionskin_content::redact::{Area, NewResource, PageRedaction};
use onionskin_content::{extract_page, redact_page, redact_page_with_hidden, PageText};
use onionskin_cos::ObjRef;

const FONT: &str = "<< /Font << /F1 5 0 R >> >>";

fn helvetica() -> Vec<u8> {
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()
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
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

fn first_x(page: &PageText, text: &str) -> f64 {
    let run = page
        .runs
        .iter()
        .find(|run| run.text.contains(text))
        .expect("the text is there");
    let at = run.text.find(text).expect("found");
    run.quads_for(at..at + text.len())[0].corners[0].0
}

/// Where "Secret" sits on `BT /F1 10 Tf 10 100 Td (Hello Secret World) Tj`.
fn secret_area(content: &str) -> Area {
    let doc = open_bytes(one_page(content, FONT, &[helvetica()]));
    let page = extract_page(&doc, 0).expect("extracts");
    let run = &page.runs[0];
    let at = run.text.find("Secret").expect("found");
    let quads = run.quads_for(at..at + "Secret".len());
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
