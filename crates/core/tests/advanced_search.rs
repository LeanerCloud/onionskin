//! Advanced Search over one document: text inside attached PDFs, two levels
//! deep and no deeper, and document-property criteria over `/Info` and XMP.

use onionskin_core::metadata::{matches_all, PropertyCriterion, PropertyField, PropertyTest};
use onionskin_core::{Document, SearchOptions, ATTACHMENT_SEARCH_DEPTH};

/// A one-page PDF saying `text`, with `attached` as its one embedded file.
fn pdf_saying(text: &str, attached: Option<(&str, &[u8])>, info: &str) -> Vec<u8> {
    let content = format!("BT /F1 18 Tf 72 700 Td ({text}) Tj ET");
    let names = attached.map_or(String::new(), |(name, _)| {
        format!("/Names << /EmbeddedFiles << /Names [({name}) 6 0 R] >> >>")
    });
    let mut objects: Vec<Vec<u8>> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {names} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream(content.as_bytes(), ""),
    ];
    if let Some((name, bytes)) = attached {
        objects.push(
            format!("<< /Type /Filespec /F ({name}) /UF ({name}) /EF << /F 7 0 R >> >>")
                .into_bytes(),
        );
        objects.push(stream(bytes, "/Type /EmbeddedFile"));
    }
    objects.push(info.as_bytes().to_vec());
    let info_number = objects.len();
    write_pdf(&objects, info_number)
}

fn stream(data: &[u8], entries: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} {entries} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn write_pdf(objects: &[Vec<u8>], info: usize) -> Vec<u8> {
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
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    out
}

/// Three levels: the document attaches `annex.pdf`, which attaches
/// `appendix.pdf`, which attaches `deepest.pdf`.
fn nested() -> Document {
    let deepest = pdf_saying("walrus", None, "<< >>");
    let appendix = pdf_saying("pelican", Some(("deepest.pdf", &deepest)), "<< >>");
    let annex = pdf_saying("heron", Some(("appendix.pdf", &appendix)), "<< >>");
    let top = pdf_saying(
        "cover",
        Some(("annex.pdf", &annex)),
        "<< /Author (Ana Pop) /CreationDate (D:20250301) >>",
    );
    Document::open_bytes(top).expect("opens")
}

#[test]
fn a_word_only_in_an_attachment_is_found_there_with_its_path() {
    let mut doc = nested();
    let found = doc
        .search_attachments("heron", SearchOptions::default())
        .expect("searches");
    assert_eq!(found.hits.len(), 1, "{found:?}");
    assert_eq!(found.hits[0].path, ["annex.pdf"]);
    assert_eq!(found.hits[0].page, 0);
    assert_eq!(found.hits[0].text, "heron");
    assert!(found.skipped.is_empty(), "{:?}", found.skipped);

    // The page walk of the document itself does not see it: that is what
    // the option adds.
    let text = doc.page_text(0).expect("reads").clone();
    assert!(onionskin_content::search(&text, "heron", SearchOptions::default()).is_empty());
}

#[test]
fn the_second_level_is_searched_and_the_third_is_not() {
    let mut doc = nested();
    assert_eq!(ATTACHMENT_SEARCH_DEPTH, 2);
    let second = doc
        .search_attachments("pelican", SearchOptions::default())
        .expect("searches");
    assert_eq!(second.hits.len(), 1);
    assert_eq!(second.hits[0].path, ["annex.pdf", "appendix.pdf"]);

    let third = doc
        .search_attachments("walrus", SearchOptions::default())
        .expect("searches");
    assert!(third.hits.is_empty(), "three levels down: {third:?}");
}

#[test]
fn a_broken_attachment_is_named_and_its_siblings_still_searched() {
    let broken = b"%PDF-1.7\nnot really a pdf".to_vec();
    let top = pdf_saying("cover", Some(("broken.pdf", &broken)), "<< >>");
    let mut doc = Document::open_bytes(top).expect("opens");
    let found = doc
        .search_attachments("anything", SearchOptions::default())
        .expect("searches");
    assert!(found.hits.is_empty());
    assert_eq!(found.skipped.len(), 1, "{found:?}");
    assert!(
        found.skipped[0].starts_with("broken.pdf was not searched"),
        "{:?}",
        found.skipped
    );
    // A blank query searches nothing at all.
    assert_eq!(
        doc.search_attachments("  ", SearchOptions::default())
            .expect("searches"),
        Default::default()
    );
}

#[test]
fn property_criteria_read_the_documents_own_info() {
    let mut doc = nested();
    let info = doc.info().expect("info");
    let xmp = doc.xmp().expect("xmp");
    let criterion = |field, test, value: &str| PropertyCriterion {
        field,
        test,
        value: value.into(),
    };
    assert_eq!(
        matches_all(
            &[
                criterion(PropertyField::Author, PropertyTest::Contains, "ana"),
                criterion(PropertyField::Created, PropertyTest::On, "2025-03-01"),
            ],
            &info,
            xmp.as_ref()
        ),
        Ok(true)
    );
    assert_eq!(
        matches_all(
            &[criterion(
                PropertyField::Author,
                PropertyTest::Contains,
                "radu"
            )],
            &info,
            xmp.as_ref()
        ),
        Ok(false)
    );
}
