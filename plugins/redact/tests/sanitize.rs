//! Remove Hidden Information on a document that carries some of every kind,
//! read back by a fresh parse.

use onionskin_content::extract_page;
use onionskin_core::Document;
use onionskin_cos::{BytesSource, Document as CosDocument, Object};
use onionskin_redact::{apply_with, sanitize, ApplyOptions, RedactError};

fn pdf(objects: &[Vec<u8>], trailer: &str) -> Vec<u8> {
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
        format!("trailer\n<< /Size {size} /Root 1 0 R {trailer} >>\n").as_bytes(),
    );
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

fn stream(dict: &str, data: &str) -> Vec<u8> {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
    .into_bytes()
}

/// Everything Remove Hidden Information takes out, and a link to a page and
/// a form field it keeps.
fn cluttered() -> Vec<u8> {
    let content = "BT /F1 12 Tf 50 150 Td (Visible) Tj ET \
                   /OC /Off BDC BT /F1 12 Tf 50 120 Td (Draft note) Tj ET EMC \
                   /OC /On BDC BT /F1 12 Tf 50 90 Td (Layer shown) Tj ET EMC \
                   BT /F1 12 Tf 250 150 Td (Cropped away) Tj ET";
    pdf(
        &[
            b"<< /Type /Catalog /Pages 2 0 R /Metadata 7 0 R /OpenAction 8 0 R \
               /Names << /JavaScript 9 0 R /EmbeddedFiles 10 0 R >> \
               /OCProperties << /OCGs [11 0 R 12 0 R] /D << /OFF [11 0 R] >> >> \
               /AcroForm << /Fields [16 0 R] /XFA 7 0 R >> /PieceInfo << /App << >> >> >>"
                .to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /CropBox [0 0 200 200] \
               /Contents 4 0 R /Thumb 7 0 R /AA << /O 8 0 R >> \
               /Resources << /Font << /F1 5 0 R >> /Properties << /Off 11 0 R /On 12 0 R >> >> \
               /Annots [13 0 R 14 0 R 15 0 R 16 0 R 17 0 R] >>"
                .to_vec(),
            stream("", content),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
            b"<< /Title (Secret project) /Author (A. Person) >>".to_vec(),
            stream("/Type /Metadata /Subtype /XML", "<x:xmpmeta>Secret project</x:xmpmeta>"),
            b"<< /Type /Action /S /JavaScript /JS (app.alert(1)) >>".to_vec(),
            b"<< /Names [(init) 8 0 R] >>".to_vec(),
            b"<< /Names [(notes.txt) << /Type /Filespec /F (notes.txt) >>] >>".to_vec(),
            b"<< /Type /OCG /Name (Draft) >>".to_vec(),
            b"<< /Type /OCG /Name (Shown) >>".to_vec(),
            b"<< /Type /Annot /Subtype /Text /Rect [10 10 20 20] /Contents (a comment) >>".to_vec(),
            b"<< /Type /Annot /Subtype /Link /Rect [30 10 60 20] /A << /S /GoTo /D [3 0 R /Fit] >> >>"
                .to_vec(),
            b"<< /Type /Annot /Subtype /Link /Rect [70 10 90 20] /A << /S /Launch /F (run.exe) >> >>"
                .to_vec(),
            b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /Rect [100 10 150 20] /V (Ada) >>"
                .to_vec(),
            b"<< /Type /Annot /Subtype /FileAttachment /Rect [160 10 170 20] /FS << /F (a.txt) >> >>"
                .to_vec(),
        ],
        "/Info 6 0 R",
    )
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn hidden_information_is_removed_and_the_page_is_kept() {
    let mut doc = Document::open_bytes(cluttered()).expect("opens");
    let applied = sanitize(&mut doc).expect("sanitizes");
    assert!(
        applied.verification.passed(),
        "{:?}",
        applied.verification.problems
    );
    let report = applied.report.sanitized.expect("a sanitize report");
    assert_eq!(report.metadata, 2, "the information dictionary and the XMP");
    assert!(
        report.scripts >= 3,
        "document JavaScript, XFA, the page's actions: {report:?}"
    );
    assert_eq!(report.actions, 2, "the open action and the launch link's");
    assert_eq!(
        report.attachments, 2,
        "the embedded file and the attachment annotation"
    );
    assert_eq!(report.comments, 1);
    assert!(report.hidden_layers > 0);
    assert_eq!(report.cropped_pages, 1);
    assert!(report.private_data >= 1, "{report:?}");

    let bytes = &applied.bytes;
    for gone in [
        b"Secret project".as_slice(),
        b"app.alert",
        b"notes.txt",
        b"run.exe",
        b"a comment",
        b"Draft",
        b"OCProperties",
    ] {
        assert!(!contains(bytes, gone), "{}", String::from_utf8_lossy(gone));
    }
    let out = CosDocument::open(Box::new(BytesSource::new(bytes.clone()))).expect("opens");
    let text: String = extract_page(&out, 0)
        .expect("extracts")
        .runs
        .iter()
        .map(|run| run.text.clone())
        .collect();
    assert!(
        text.contains("Visible") && text.contains("Layer shown"),
        "{text}"
    );
    assert!(
        !text.contains("Draft note") && !text.contains("Cropped"),
        "{text}"
    );
    let page = out.page(0).expect("page");
    let annots = out
        .resolve(page.dict.get(b"Annots").expect("annots"))
        .expect("resolves");
    let kinds: Vec<String> = annots
        .as_array()
        .expect("an array")
        .iter()
        .filter_map(|item| out.resolve(item).ok())
        .filter_map(|item| {
            item.as_dict()
                .and_then(|dict| dict.get(b"Subtype"))
                .and_then(Object::as_name)
                .cloned()
        })
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
        .collect();
    assert_eq!(kinds, ["Link", "Link", "Widget"]);
    assert!(contains(bytes, b"(Ada)"), "the form field's value stays");
}

#[test]
fn applying_without_marks_or_sanitizing_is_refused() {
    let mut doc = Document::open_bytes(cluttered()).expect("opens");
    let options = ApplyOptions::default();
    assert!(matches!(
        apply_with(&mut doc, &options),
        Err(RedactError::NothingMarked)
    ));
}
