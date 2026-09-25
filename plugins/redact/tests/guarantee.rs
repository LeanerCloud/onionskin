//! Guarantee test 3: redaction. After redacting text T, the verifier
//! extracts all text and images from the output and finds no trace of T,
//! and a raw byte scan finds no trace of the original object bytes.
//!
//! Driven through the plugin as the shell drives it: every occurrence of T
//! found by Find Text & Redact, marked, and applied. The output is then read
//! back independently of the verifier: every page's text extracted, every
//! decoded stream searched, and the old content streams looked for in the
//! raw bytes.

use onionskin_content::{content as page_content_streams, extract_page, page, Tokenizer};
use onionskin_core::redactions::RedactionLook;
use onionskin_core::{Document, SearchOptions};
use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Document as CosDocument, Object};
use onionskin_redact::apply_redactions;
use onionskin_redact::find::{find, Query};
use onionskin_redact::mark::{mark_found, mark_text};

/// Redacts every occurrence of `word` in `bytes`, and proves it is gone.
fn redact_and_prove(bytes: Vec<u8>, word: &str) {
    let original = CosDocument::open(Box::new(BytesSource::new(bytes.clone()))).expect("opens");
    let mut doc = Document::open_bytes(bytes).expect("opens");
    let options = SearchOptions {
        case_sensitive: true,
        ..SearchOptions::default()
    };
    let found = find(&mut doc, &Query::Text(word.to_owned(), options)).expect("finds");
    assert!(
        !found.is_empty(),
        "{word:?} is in the document to begin with"
    );
    mark_found(&mut doc, &found, &RedactionLook::default()).expect("marks");
    let applied = apply_redactions(&mut doc).expect("applies");

    assert!(
        applied.verification.passed(),
        "the verifier must pass the redacted file: {:?}",
        applied.verification.problems
    );
    let output = CosDocument::open(Box::new(BytesSource::new(applied.bytes.clone())))
        .expect("the redacted file opens");
    let pages = output.page_count().expect("pages") as usize;
    for page in 0..pages {
        let text = extract_page(&output, page).expect("extracts");
        for run in &text.runs {
            assert!(
                !run.decoded_text.contains(word),
                "the redacted word must not be extracted from the output: page {} says {:?}",
                page + 1,
                run.decoded_text
            );
        }
    }
    for (number, raw) in &content_streams(&original, &applied.report.pages) {
        assert!(
            !contains(&applied.bytes, raw),
            "the original content stream bytes must not be in the output: object {number}"
        );
    }
    let kept = output.reachable_from_trailer();
    for number in kept {
        let Ok(parsed) = output.get(number) else {
            continue;
        };
        if let Object::Stream(stream) = parsed.object {
            let decoded = output.decode_stream(&stream).unwrap_or_default();
            assert!(
                !contains(&decoded, word.as_bytes()),
                "the redacted word's bytes must not be in the output: stream {number}"
            );
        }
    }
    assert!(
        !contains(&applied.bytes, word.as_bytes()),
        "the redacted word's bytes must not be in the output"
    );
}

/// The content streams of `pages`, by object number, with their raw bytes.
fn content_streams(doc: &CosDocument, pages: &[usize]) -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    for &page in pages {
        let node = doc.page(page).expect("page");
        let refs: Vec<_> = match node.dict.get(b"Contents") {
            Some(Object::Ref(objref)) => vec![*objref],
            Some(Object::Array(items)) => items.iter().filter_map(Object::as_reference).collect(),
            _ => Vec::new(),
        };
        for objref in refs {
            if let Some(stream) = doc.get(objref.number).expect("stream").object.as_stream() {
                out.push((objref.number, stream.raw.clone()));
            }
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn page_content(doc: &CosDocument) -> Vec<u8> {
    let page = page(doc, 0).expect("page");
    let mut warnings = Vec::new();
    let content = page_content_streams(doc, &page, &mut warnings).expect("reads contents");
    assert!(warnings.is_empty(), "content warnings: {warnings:?}");
    content.bytes
}

fn marked_properties(data: &[u8]) -> Vec<onionskin_cos::Dict> {
    let mut tokenizer = Tokenizer::new(data);
    let mut dictionaries = Vec::new();
    while let Some(operation) = tokenizer.next_operation() {
        if operation.operator.is(b"BDC") {
            if let Some(properties) = operation.tail(1).and_then(|tail| tail[0].as_dict()) {
                dictionaries.push(properties.clone());
            }
        }
    }
    dictionaries
}

fn property_string<'a>(dict: &'a onionskin_cos::Dict, key: &[u8]) -> Option<&'a [u8]> {
    match dict.get(key) {
        Some(onionskin_cos::Object::String(value)) => Some(value.as_slice()),
        _ => None,
    }
}

fn placed_form_property_sets(
    doc: &CosDocument,
) -> Vec<(onionskin_cos::ObjRef, Vec<u8>, Vec<onionskin_cos::Dict>)> {
    let page = doc.page(0).expect("page");
    let resources_object = doc
        .resolve(page.dict.get(b"Resources").expect("resources"))
        .expect("resolves resources");
    let resources = resources_object.as_dict().expect("resource dictionary");
    let xobjects_object = doc
        .resolve(resources.get(b"XObject").expect("xobjects"))
        .expect("resolves xobjects");
    let xobjects = xobjects_object.as_dict().expect("xobject dictionary");
    let mut placed = Vec::new();
    let content = page_content(doc);
    let mut tokenizer = Tokenizer::new(&content);
    while let Some(operation) = tokenizer.next_operation() {
        if !operation.operator.is(b"Do") {
            continue;
        }
        let Some(onionskin_cos::Object::Name(name)) = operation.operands.last() else {
            continue;
        };
        let Some(reference) = xobjects
            .get(name.as_bytes())
            .and_then(|object| object.as_reference())
        else {
            continue;
        };
        let form_object = doc.get(reference.number).expect("form object");
        let stream = form_object.object.as_stream().expect("form stream");
        let decoded = doc.decode_stream(stream).expect("decodes form stream");
        placed.push((reference, decoded.clone(), marked_properties(&decoded)));
    }
    placed
}

#[test]
fn a_word_on_the_seeds_is_gone_after_redaction() {
    redact_and_prove(
        std::fs::read(seed("hello.pdf")).expect("readable"),
        "Onionskin",
    );
    // On the second page, which is turned a quarter and cropped.
    redact_and_prove(
        std::fs::read(seed("two-page.pdf")).expect("readable"),
        "two",
    );
}

/// A word written in every way a content stream can hide it: split across a
/// `TJ` array, in a hex string, and inside a form XObject drawn twice.
#[test]
fn a_word_however_it_is_drawn_is_gone_after_redaction() {
    let content = "BT /F1 12 Tf 20 250 Td [(Conf) -5 (idential memo)] TJ ET \
                   BT /F1 12 Tf 20 220 Td <436F6E666964656E7469616C> Tj ET \
                   q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 -60 cm /Fm0 Do Q \
                   BT /F1 12 Tf 20 100 Td (Public) Tj ET";
    let form = "BT /F1 12 Tf 20 180 Td (Confidential footer) Tj ET";
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> /XObject << /Fm0 6 0 R >> >> >>"
            .to_vec(),
        stream("", content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >>",
            form,
        ),
    ];
    redact_and_prove(pdf(&objects), "Confidential");
}

fn nested_actual_text_pdf() -> Vec<u8> {
    let content = "BT /F1 10 Tf 20 200 Td \
        /Span << /ActualText (OUT) /Alt (outer-alt) /E (outer-event) >> BDC \
        (A) Tj /Span << /ActualText (INNER) /Alt (inner-alt) /E (inner-event) >> BDC \
        (H) Tj EMC (B) Tj EMC ET";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        stream("", content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
    ])
}

#[test]
fn nested_actual_text_redaction_observes_open_ancestor_spelling() {
    let original = nested_actual_text_pdf();
    let mut outer = Document::open_bytes(original.clone()).expect("opens outer case");
    let source = extract_page(
        &CosDocument::open(Box::new(BytesSource::new(original.clone()))).expect("source"),
        0,
    )
    .expect("extracts")
    .clone();
    assert_eq!(source.runs.len(), 3);
    let outer_quad = source.runs[0].glyphs[0].quad;
    let outer_case_a = source.runs[0].glyphs[0].quad;
    let inner_and_b = [source.runs[1].glyphs[0].quad, source.runs[2].glyphs[0].quad];
    let inner_case_b = source.runs[2].glyphs[0].quad;
    mark_text(&mut outer, 0, &[outer_quad], &RedactionLook::default()).expect("marks outer A");
    let applied = apply_redactions(&mut outer).expect("applies outer case");
    assert!(
        applied.verification.passed(),
        "{:?}",
        applied.verification.problems
    );
    let out = CosDocument::open(Box::new(BytesSource::new(applied.bytes.clone())))
        .expect("reopens outer");
    let page = extract_page(&out, 0).expect("extracts outer");
    assert_eq!(page.flatten().text, "INNERB");
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.code)
            .collect::<Vec<_>>(),
        [72, 66]
    );
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.quad)
            .collect::<Vec<_>>(),
        inner_and_b
    );
    let outer_properties = marked_properties(&page_content(&out));
    assert_eq!(outer_properties.len(), 2);
    assert!(!outer_properties[0].contains(b"ActualText"));
    assert!(!outer_properties[0].contains(b"Alt"));
    assert!(!outer_properties[0].contains(b"E"));
    assert_eq!(
        property_string(&outer_properties[1], b"ActualText"),
        Some(b"INNER".as_slice())
    );
    assert_eq!(
        property_string(&outer_properties[1], b"Alt"),
        Some(b"inner-alt".as_slice())
    );
    assert_eq!(
        property_string(&outer_properties[1], b"E"),
        Some(b"inner-event".as_slice())
    );

    let mut inner = Document::open_bytes(original).expect("opens inner case");
    mark_text(
        &mut inner,
        0,
        &[source.runs[1].glyphs[0].quad],
        &RedactionLook::default(),
    )
    .expect("marks inner H");
    let applied = apply_redactions(&mut inner).expect("applies inner case");
    assert!(
        applied.verification.passed(),
        "{:?}",
        applied.verification.problems
    );
    let out = CosDocument::open(Box::new(BytesSource::new(applied.bytes.clone())))
        .expect("reopens inner");
    let page = extract_page(&out, 0).expect("extracts inner");
    assert_eq!(page.flatten().text, "A B");
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.code)
            .collect::<Vec<_>>(),
        [65, 66]
    );
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.quad)
            .collect::<Vec<_>>(),
        [outer_case_a, inner_case_b]
    );
    let inner_properties = marked_properties(&page_content(&out));
    assert_eq!(inner_properties.len(), 2);
    assert!(inner_properties.iter().all(|dict| {
        !dict.contains(b"ActualText") && !dict.contains(b"Alt") && !dict.contains(b"E")
    }));
}

#[test]
fn whole_outer_actual_text_redaction_preserves_inner_semantics() {
    let original = nested_actual_text_pdf();
    let source_doc =
        CosDocument::open(Box::new(BytesSource::new(original.clone()))).expect("source");
    let source = extract_page(&source_doc, 0).expect("extracts source");
    let outer_a = source.runs[0].glyphs[0].quad;
    let inner_h = source.runs[1].glyphs[0].quad;
    let outer_b = source.runs[2].glyphs[0].quad;

    let mut doc = Document::open_bytes(original).expect("opens");
    let options = SearchOptions {
        case_sensitive: true,
        ..SearchOptions::default()
    };
    let found = find(&mut doc, &Query::Text("OUT".to_owned(), options)).expect("finds OUT");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "OUT");
    assert_eq!(found[0].quads, vec![outer_a, outer_b]);
    assert!(!found[0].quads.contains(&inner_h));

    mark_found(&mut doc, &found, &RedactionLook::default()).expect("marks OUT");
    let applied = apply_redactions(&mut doc).expect("applies");
    assert!(
        applied.verification.passed(),
        "{:?}",
        applied.verification.problems
    );
    let out =
        CosDocument::open(Box::new(BytesSource::new(applied.bytes))).expect("reopens output");
    let page = extract_page(&out, 0).expect("extracts output");
    assert_eq!(page.flatten().text, "INNER");
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.code)
            .collect::<Vec<_>>(),
        [72]
    );
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.quad)
            .collect::<Vec<_>>(),
        [inner_h]
    );

    let properties = marked_properties(&page_content(&out));
    assert_eq!(properties.len(), 2);
    assert!(!properties[0].contains(b"ActualText"));
    assert!(!properties[0].contains(b"Alt"));
    assert!(!properties[0].contains(b"E"));
    assert_eq!(
        property_string(&properties[1], b"ActualText"),
        Some(b"INNER".as_slice())
    );
    assert_eq!(
        property_string(&properties[1], b"Alt"),
        Some(b"inner-alt".as_slice())
    );
    assert_eq!(
        property_string(&properties[1], b"E"),
        Some(b"inner-event".as_slice())
    );
}

fn repeated_form_pdf() -> Vec<u8> {
    let page_content = "q /Fm0 Do Q q 1 0 0 1 0 -30 cm /Fm0 Do Q";
    let form_content = "BT /F1 10 Tf 20 200 Td /Span << /ActualText (XY) /Alt (form-alt) /E (form-event) >> BDC (A) Tj (B) Tj EMC ET";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /Fm0 6 0 R >> >> >>".to_vec(),
        stream("", page_content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >>", form_content),
    ])
}

#[test]
fn partial_repeated_form_redaction_keeps_second_occurrence() {
    let original = repeated_form_pdf();
    let mut doc = Document::open_bytes(original.clone()).expect("opens form");
    let source_doc = CosDocument::open(Box::new(BytesSource::new(original))).expect("source");
    let source = extract_page(&source_doc, 0).expect("extracts source");
    let source_forms = placed_form_property_sets(&source_doc);
    assert_eq!(source_forms.len(), 2);
    assert_eq!(source_forms[0].0, source_forms[1].0);
    assert_eq!(source_forms[0].1, source_forms[1].1);
    assert_eq!(source.runs.len(), 4);
    assert!(source.runs.iter().all(|run| run.glyphs.len() == 1));
    assert_eq!(
        source
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.code)
            .collect::<Vec<_>>(),
        [65, 66, 65, 66]
    );
    let second_a = source.runs[2].glyphs[0].quad;
    let second_b = source.runs[3].glyphs[0].quad;
    mark_text(
        &mut doc,
        0,
        &[source.runs[0].glyphs[0].quad],
        &RedactionLook::default(),
    )
    .expect("marks first A");
    let applied = apply_redactions(&mut doc).expect("applies");
    assert!(
        applied.verification.passed(),
        "{:?}",
        applied.verification.problems
    );
    let out =
        CosDocument::open(Box::new(BytesSource::new(applied.bytes.clone()))).expect("reopens form");
    let page = extract_page(&out, 0).expect("extracts output");
    assert_eq!(page.flatten().text, "B\nXY");
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.code)
            .collect::<Vec<_>>(),
        [66, 65, 66]
    );
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.quad)
            .collect::<Vec<_>>(),
        [source.runs[1].glyphs[0].quad, second_a, second_b]
    );
    let form_properties = placed_form_property_sets(&out);
    assert_eq!(
        form_properties.len(),
        2,
        "both placements resolve through explicit forms"
    );
    assert_ne!(
        form_properties[0].0, form_properties[1].0,
        "the redacted first placement uses a distinct form resource"
    );
    assert_eq!(form_properties[0].2.len(), 1);
    assert!(!form_properties[0].2[0].contains(b"ActualText"));
    assert!(!form_properties[0].2[0].contains(b"Alt"));
    assert!(!form_properties[0].2[0].contains(b"E"));
    assert_eq!(form_properties[1].1, source_forms[1].1);
    assert_eq!(form_properties[1].2.len(), 1);
    assert_eq!(
        property_string(&form_properties[1].2[0], b"ActualText"),
        Some(b"XY".as_slice())
    );
    assert_eq!(
        property_string(&form_properties[1].2[0], b"Alt"),
        Some(b"form-alt".as_slice())
    );
    assert_eq!(
        property_string(&form_properties[1].2[0], b"E"),
        Some(b"form-event".as_slice())
    );
}

fn stream(dict: &str, data: &str) -> Vec<u8> {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
    .into_bytes()
}

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}
