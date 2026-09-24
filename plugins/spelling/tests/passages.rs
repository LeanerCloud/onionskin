//! Check Spelling over a real document: the comments' text and the text
//! fields' values found in page order, the words the dictionary does not
//! know among them, and a correction written as one undo step.

use onionskin_core::Document;
use onionskin_spelling::passages::{correct, misspellings, passages, Source, LABEL};
use onionskin_spelling::Checker;

fn pdf(objects: &[&str]) -> Vec<u8> {
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

/// Two pages. Page 2 has a note and a link; page 1 a text field, a
/// password field and an empty field.
fn document() -> Document {
    Document::open_bytes(pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [8 0 R 9 0 R 10 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>",
        "<< /Type /Page /Parent 2 0 R /Annots [8 0 R 9 0 R 10 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /Annots [5 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /Text /Rect [10 10 30 30] /P 4 0 R /Contents (Teh meeting is at noon) >>",
        "<< /Type /Annot /Subtype /Link /Rect [40 40 60 60] /P 4 0 R /Contents (Lnik) >>",
        "<< >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /V (Jhon Smith recieved) /Rect [10 700 200 720] /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 8192 /T (Secret) /V (pasword) /Rect [10 650 200 670] /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Empty) /Rect [10 600 200 620] /P 3 0 R >>",
    ]))
    .expect("opens")
}

#[test]
fn comments_and_text_fields_are_read_in_page_order() {
    let mut doc = document();
    let found = passages(&mut doc).expect("reads");
    let seen: Vec<_> = found
        .iter()
        .map(|passage| (passage.label.as_str(), passage.text.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("Field Name", "Jhon Smith recieved"),
            ("Comment on page 2", "Teh meeting is at noon"),
        ],
        "no password, no empty field, no link"
    );
    assert!(matches!(found[0].source, Source::Field(_)));
    assert_eq!(found[1].page, Some(1));

    let checker = Checker::english();
    let wrong: Vec<_> = misspellings(&checker, &found)
        .into_iter()
        .map(|found| (found.passage, found.word))
        .collect();
    assert_eq!(
        wrong,
        [
            (0, "Jhon".to_owned()),
            (0, "recieved".to_owned()),
            (1, "Teh".to_owned())
        ]
    );
}

#[test]
fn a_correction_is_one_undo_step_and_a_stale_one_is_refused() {
    let mut doc = document();
    let found = passages(&mut doc).expect("reads");
    let checker = Checker::english();
    let wrong = misspellings(&checker, &found);

    let note = &found[wrong[2].passage];
    let fixed = correct(&mut doc, note, wrong[2].range.clone(), "The", 0).expect("corrects");
    assert_eq!(fixed, "The meeting is at noon");
    assert_eq!(doc.edit().history().undo_label(), Some(LABEL));
    let stale = correct(&mut doc, note, wrong[2].range.clone(), "The", 0).expect_err("changed");
    assert!(stale.to_string().contains("changed"), "{stale}");

    let field = &found[wrong[1].passage];
    let fixed = correct(&mut doc, field, wrong[1].range.clone(), "received", 0).expect("corrects");
    assert_eq!(fixed, "Jhon Smith received");
    let now = passages(&mut doc).expect("reads");
    assert_eq!(now[0].text, "Jhon Smith received");
    assert_eq!(now[1].text, "The meeting is at noon");
    let out_of_range = correct(&mut doc, &now[0], 40..45, "x", 0).expect_err("no such word");
    assert!(
        out_of_range.to_string().contains("not in the text"),
        "{out_of_range}"
    );

    assert!(doc.undo().expect("undoes"));
    assert_eq!(
        passages(&mut doc).expect("reads")[0].text,
        "Jhon Smith recieved"
    );
}
