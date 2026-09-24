//! Document metadata and the initial view, written and read back after a
//! save and a reopen, through two independent readers.
//!
//! The mutations this must catch: writing `/Info` and skipping XMP fails the
//! round trip on the XMP reader; an XMP reader that answered from `/Info`
//! would pass that, so the XMP side is also asserted on a packet whose values
//! differ from `/Info`'s.

mod common;

use std::path::PathBuf;

use onionskin_core::metadata::{
    write_initial_view, write_properties, Description, InitialView, OpenFit, PageLayout, PageMode,
    PropertiesEdit,
};
use onionskin_core::{Document, DocumentFile, Error};
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_cos::Object;

const NOW: i64 = 1_789_999_500;

fn copy_of(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(name);
    std::fs::copy(seed(name), &path).expect("copies");
    (dir, path)
}

fn edit() -> PropertiesEdit {
    PropertiesEdit {
        description: Description {
            title: Some("Quarterly Report — Q3".into()),
            author: Some("Ana; Bo".into()),
            subject: Some("Figures & forecasts".into()),
            keywords: Some("budget, q3".into()),
        },
        custom: vec![("Department".into(), "Finance".into())],
    }
}

fn set(document: &mut Document, edit: &PropertiesEdit) -> Result<(), Error> {
    document.edit_document("Document Properties", |tx| write_properties(tx, edit, NOW))
}

#[test]
fn a_description_reads_back_from_info_and_from_xmp_after_a_reopen() {
    let (_dir, path) = copy_of("two-page.pdf");
    let mut file = DocumentFile::open(&path).expect("opens");
    set(file.document_mut(), &edit()).expect("writes");
    file.save().expect("saves");
    drop(file);

    let mut reopened = Document::open_path(&path).expect("reopens");
    let info = reopened.info().expect("reads /Info");
    assert_eq!(info.description, edit().description);
    assert_eq!(
        info.custom,
        [("Department".to_owned(), "Finance".to_owned())]
    );
    assert_eq!(info.modified.as_deref(), Some("D:20260921140500Z00'00'"));

    let xmp = reopened.xmp().expect("reads").expect("a packet");
    assert_eq!(xmp.title.as_deref(), Some("Quarterly Report — Q3"));
    assert_eq!(xmp.authors, ["Ana", "Bo"], "one creator per author");
    assert_eq!(xmp.subject.as_deref(), Some("Figures & forecasts"));
    assert_eq!(xmp.keywords.as_deref(), Some("budget, q3"));
    assert_eq!(xmp.modified.as_deref(), Some("2026-09-21T14:05:00Z"));
}

/// minimal.pdf has no `/Info`. The edit creates it and points the trailer at
/// it; undo takes the trailer key away again.
#[test]
fn a_document_without_info_gets_one_and_undo_puts_the_trailer_back() {
    let mut document = Document::open_path(&seed("minimal.pdf")).expect("opens");
    let trailer_info = |document: &mut Document| {
        document
            .structure()
            .expect("doc")
            .trailer()
            .get(b"Info")
            .cloned()
    };
    assert_eq!(trailer_info(&mut document), None, "the seed has no /Info");

    set(&mut document, &edit()).expect("writes");
    assert!(matches!(trailer_info(&mut document), Some(Object::Ref(_))));
    assert_eq!(
        document.info().expect("reads").description.title.as_deref(),
        Some("Quarterly Report — Q3")
    );

    let (session, base) = document.edit_mut();
    assert!(session.undo(base).expect("undoes"));
    assert_eq!(
        trailer_info(&mut document),
        None,
        "the trailer is back as it was"
    );
    assert_eq!(document.info().expect("reads"), Default::default());
    assert_eq!(
        document.xmp().expect("reads"),
        None,
        "and the packet is gone"
    );
}

#[test]
fn clearing_a_field_removes_it_and_a_dropped_custom_key_goes() {
    let (_dir, path) = copy_of("two-page.pdf");
    let mut file = DocumentFile::open(&path).expect("opens");
    set(file.document_mut(), &edit()).expect("writes");
    let cleared = PropertiesEdit {
        description: Description {
            title: Some("  ".into()),
            ..edit().description
        },
        custom: Vec::new(),
    };
    set(file.document_mut(), &cleared).expect("writes");
    let info = file.document_mut().info().expect("reads");
    assert_eq!(
        info.description.title, None,
        "blank is removed, not written"
    );
    assert!(info.custom.is_empty());
    assert_eq!(
        file.document_mut()
            .xmp()
            .expect("reads")
            .expect("packet")
            .title,
        None
    );
}

#[test]
fn a_custom_key_that_is_a_standard_one_is_refused_and_nothing_is_written() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    let bad = PropertiesEdit {
        custom: vec![("Producer".into(), "Me".into())],
        ..edit()
    };
    assert!(matches!(
        set(&mut document, &bad),
        Err(Error::InvalidMetadataKey(_))
    ));
    assert_eq!(document.edit().history().reach(), 0);
}

/// Another producer's packet: its title differs from `/Info`'s, it carries a
/// schema this writer does not own, and it survives the edit.
#[test]
fn another_producers_packet_is_read_as_itself_and_its_other_schemas_survive() {
    use common::pdf;

    let packet = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"><dc:title><rdf:Alt><rdf:li xml:lang="x-default">Packet Title</rdf:li></rdf:Alt></dc:title><pdfaid:part>2</pdfaid:part></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    let mut metadata = format!(
        "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n",
        packet.len()
    )
    .into_bytes();
    metadata.extend_from_slice(packet.as_bytes());
    metadata.extend_from_slice(b"\nendstream");
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
        metadata,
    ]);
    let mut document = Document::open_bytes(bytes).expect("opens");
    assert_eq!(
        document
            .xmp()
            .expect("reads")
            .expect("packet")
            .title
            .as_deref(),
        Some("Packet Title"),
        "the packet's own value, not /Info's (there is none)"
    );
    set(&mut document, &edit()).expect("writes");
    let current = document.structure().expect("doc");
    let catalog = current.catalog().expect("catalog");
    let Ok(Object::Stream(stream)) = current.resolve(catalog.get(b"Metadata").expect("/Metadata"))
    else {
        panic!("a metadata stream");
    };
    let text = String::from_utf8(current.decode_stream(&stream).expect("decodes")).expect("utf-8");
    assert!(text.contains("<pdfaid:part>2</pdfaid:part>"), "{text}");
    assert!(!text.contains("Packet Title"));
}

#[test]
fn an_initial_view_is_what_a_reopened_session_reports() {
    let (_dir, path) = copy_of("two-page.pdf");
    let mut file = DocumentFile::open(&path).expect("opens");
    let view = InitialView {
        layout: Some(PageLayout::TwoColumnLeft),
        mode: Some(PageMode::UseOutlines),
        page: Some(1),
        fit: OpenFit::Width,
    };
    file.document_mut()
        .edit_document("Initial View", |tx| write_initial_view(tx, &view))
        .expect("writes");
    file.save().expect("saves");
    drop(file);

    let mut reopened = Document::open_path(&path).expect("reopens");
    assert_eq!(reopened.initial_view().expect("reads"), view);
}

#[test]
fn every_fit_round_trips_and_no_page_removes_the_open_action() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    for fit in [
        OpenFit::Default,
        OpenFit::Page,
        OpenFit::Width,
        OpenFit::Height,
        OpenFit::Visible,
        OpenFit::Zoom(1.5),
    ] {
        let view = InitialView {
            page: Some(0),
            fit,
            ..InitialView::default()
        };
        document
            .edit_document("Initial View", |tx| write_initial_view(tx, &view))
            .expect("writes");
        assert_eq!(document.initial_view().expect("reads"), view);
    }
    document
        .edit_document("Initial View", |tx| {
            write_initial_view(tx, &InitialView::default())
        })
        .expect("writes");
    assert_eq!(
        document.initial_view().expect("reads"),
        InitialView::default()
    );

    let beyond = InitialView {
        page: Some(9),
        ..InitialView::default()
    };
    assert!(matches!(
        document.edit_document("Initial View", |tx| write_initial_view(tx, &beyond)),
        Err(Error::NoSuchPage { page: 9, count: 2 })
    ));
}

#[test]
fn an_encrypted_document_refuses_a_properties_edit() {
    let mut document =
        Document::open_path(&encrypted_fixture("r6-aes-256-print-only.pdf")).expect("opens");
    assert!(matches!(
        set(&mut document, &edit()),
        Err(Error::Protected(_))
    ));
}

/// Fonts on the page, fonts a Form XObject uses, an embedded subset and a
/// composite font whose program hangs off its descendant; a font named twice
/// is listed once.
#[test]
fn the_fonts_list_follows_forms_and_reads_embedding_from_the_descriptor() {
    use common::{pdf, stream, stream_with};
    use onionskin_core::metadata::FontEntry;

    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> /XObject << /X1 9 0 R >> >> >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Garamond /FontDescriptor 7 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /ABCDEF+Garamond /FontFile2 8 0 R >>".to_vec(),
        stream("font program"),
        stream_with("", "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << /Font << /F3 10 0 R >> >>"),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Noto /DescendantFonts [11 0 R] >>".to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Noto /FontDescriptor 12 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /Noto /FontFile2 8 0 R >>".to_vec(),
    ]);
    let mut document = Document::open_bytes(bytes).expect("opens");
    let entry = |name: &str, kind: &str, embedded, subset| FontEntry {
        name: name.into(),
        kind: kind.into(),
        embedded,
        subset,
    };
    assert_eq!(
        document.fonts().expect("reads"),
        [
            entry("Garamond", "TrueType", true, true),
            entry("Helvetica", "Type1", false, false),
            entry("Noto", "Type0", true, false),
        ]
    );
}
