//! Guarantee test 8: tag integrity. Editing a tagged document leaves its
//! structure tree valid and consistent with the edited content, checked by
//! the accessibility plugin's own checker.
//!
//! Each document goes through every kind of edit that rewrites content or
//! moves structure, the way the editing plugins make them: a line of text
//! rewritten, a word replaced everywhere, new text added, a link made, a
//! page turned, pages reordered and a page deleted. After each one the
//! checker runs over the document as edited, and whatever it finds must be
//! something it found before the edit: an edit may not break what was
//! whole, and is not asked to mend what was not.
//!
//! Two sets: tagged documents written here, which are whole to begin with
//! and must stay whole, and the PDF/UA conformance files under
//! `external/verapdf/`.

use onionskin_core::links::{LinkLook, LinkTarget};
use onionskin_core::text_edit::{page_lines, MatchOptions};
use onionskin_core::Document;
use onionskin_corpus_testing::{corpus_dir, pdfs_in};
use onionskin_tools_accessibility::checker::{check, Report};
use onionskin_tools_edit::links::create_link;
use onionskin_tools_edit::text::{add_text, edit_line, find, replace};
use onionskin_tools_organize::{delete_pages, move_pages, rotate_pages, Turn};

/// The checker's report on `doc` as it is now.
fn report(doc: &mut Document) -> Report {
    check(doc.structure().expect("the document reads")).expect("the checker runs")
}

/// An edit, by name, and whether it applied: one the document cannot take,
/// such as a text edit on a page with no text, is not a failure of this
/// guarantee.
type Edit = (&'static str, fn(&mut Document) -> bool);

/// The first line of the first page with a line that can be rewritten,
/// rewritten with a word added.
fn rewrite_a_line(doc: &mut Document) -> bool {
    for page in 0..doc.page_count() {
        let Ok(lines) = page_lines(doc.structure().expect("reads"), page) else {
            continue;
        };
        for (index, line) in lines.iter().enumerate().take(8) {
            if !line.is_mapped() || line.text.trim().is_empty() {
                continue;
            }
            let text = format!("{} again", line.text);
            if edit_line(doc, page, index, &line.text, &text).is_ok() {
                return true;
            }
        }
    }
    false
}

/// The document's first word of four letters or more, replaced
/// everywhere by itself: every line holding it rewritten.
fn replace_a_word(doc: &mut Document) -> bool {
    let Some(word) = (0..doc.page_count().min(3)).find_map(|page| {
        let lines = page_lines(doc.structure().ok()?, page).ok()?;
        lines
            .iter()
            .filter(|line| line.is_mapped())
            .find_map(|line| {
                line.text
                    .split(|ch: char| !ch.is_alphabetic())
                    .find(|word| word.chars().count() >= 4)
                    .map(str::to_owned)
            })
    }) else {
        return false;
    };
    let options = MatchOptions {
        case_sensitive: true,
        whole_word: true,
    };
    let Ok(found) = find(doc, &word, options) else {
        return false;
    };
    replace(doc, &found, &word, "Replace All").is_ok_and(|count| count > 0)
}

fn add_new_text(doc: &mut Document) -> bool {
    add_text(doc, 0, (36.0, 36.0), "Added by Onionskin").is_ok()
}

fn make_a_link(doc: &mut Document) -> bool {
    let target = LinkTarget::Web("https://example.com/".to_owned());
    create_link(
        doc,
        0,
        [36.0, 60.0, 136.0, 80.0],
        &target,
        LinkLook::default(),
    )
    .is_ok()
}

fn turn_a_page(doc: &mut Document) -> bool {
    rotate_pages(doc, &[0], Turn::Clockwise).is_ok()
}

fn reorder_pages(doc: &mut Document) -> bool {
    let last = doc.page_count().saturating_sub(1);
    last > 0 && move_pages(doc, &[last], 0).is_ok()
}

fn delete_a_page(doc: &mut Document) -> bool {
    let count = doc.page_count();
    count > 1 && delete_pages(doc, &[count - 1]).is_ok()
}

const EDITS: [Edit; 7] = [
    ("Edit Text", rewrite_a_line),
    ("Replace All", replace_a_word),
    ("Add Text", add_new_text),
    ("Create Link", make_a_link),
    ("Rotate Page", turn_a_page),
    ("Move Pages", reorder_pages),
    ("Delete Page", delete_a_page),
];

/// Put `doc` through every edit, checking after each. How many applied.
fn edit_and_check(name: &str, mut doc: Document) -> usize {
    let mut applied = 0;
    for (edit, apply) in EDITS {
        let before = report(&mut doc);
        if !apply(&mut doc) {
            continue;
        }
        applied += 1;
        let broken = report(&mut doc).new_since(&before);
        assert!(
            broken.is_empty(),
            "an edit must leave no finding the document did not have: {edit} on {name} left {broken:?}"
        );
    }
    applied
}

fn pdf(objects: &[String]) -> Vec<u8> {
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

fn stream(content: &str) -> String {
    format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Three tagged pages: a heading and two paragraphs each, every one its
/// own marked content, under a document element.
fn tagged_pages() -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 9 0 R /MarkInfo << /Marked true >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_owned(),
    ];
    for page in 0..3 {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /StructParents {page} /Contents {} 0 R >>",
            6 + page
        ));
    }
    for page in 1..=3 {
        objects.push(stream(&format!(
            "/H1 << /MCID 0 >> BDC BT /F1 24 Tf 72 700 Td (Chapter {page}) Tj ET EMC \
             /P << /MCID 1 >> BDC BT /F1 12 Tf 72 660 Td (The chapter begins here.) Tj ET EMC \
             /P << /MCID 2 >> BDC BT /F1 12 Tf 72 640 Td (It ends on the next page.) Tj ET EMC"
        )));
    }
    // 9: the root; 10: the document element; 11..=19: three elements a page.
    let kids: Vec<String> = (11..=19).map(|number| format!("{number} 0 R")).collect();
    let nums: Vec<String> = (0..3)
        .map(|page| {
            let first = 11 + page * 3;
            format!("{page} [{first} 0 R {} 0 R {} 0 R]", first + 1, first + 2)
        })
        .collect();
    objects.push(format!(
        "<< /Type /StructTreeRoot /K [10 0 R] /ParentTree << /Nums [{}] >> /ParentTreeNextKey 3 >>",
        nums.join(" ")
    ));
    objects.push(format!(
        "<< /Type /StructElem /S /Document /P 9 0 R /K [{}] >>",
        kids.join(" ")
    ));
    for page in 0..3 {
        for (mcid, kind) in ["H1", "P", "P"].iter().enumerate() {
            objects.push(format!(
                "<< /Type /StructElem /S /{kind} /P 10 0 R /Pg {} 0 R /K {mcid} >>",
                3 + page
            ));
        }
    }
    pdf(&objects)
}

#[test]
fn tagged_documents_stay_whole_through_every_edit() {
    let mut doc = Document::open_bytes(tagged_pages()).expect("opens");
    assert!(
        report(&mut doc).is_clean(),
        "the fixture is whole to begin with: {:?}",
        report(&mut doc)
    );
    let applied = edit_and_check("the tagged fixture", doc);
    assert_eq!(applied, EDITS.len(), "every edit applies to it");

    let mut doc = Document::open_bytes(tagged_pages()).expect("opens");
    for (_, apply) in EDITS {
        assert!(apply(&mut doc));
    }
    let after = report(&mut doc);
    assert!(
        after.is_clean(),
        "the tree must still be valid after every edit: {after:?}"
    );
    assert_eq!(doc.page_count(), 2);
}

/// Files the PDF/UA sets hold that are tagged and open, at least.
const MIN_TAGGED_FILES: usize = 400;

#[test]
fn the_pdf_ua_corpus_is_edited_without_breaking_its_tags() {
    let Some(dir) = corpus_dir("external/verapdf") else {
        return;
    };
    let mut tagged = 0;
    let mut edits = 0;
    for path in pdfs_in(&dir) {
        let shown = path
            .strip_prefix(&dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        if !shown.starts_with("PDF_UA") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(mut doc) = Document::open_bytes(bytes) else {
            continue;
        };
        let Ok(structure) = doc.structure() else {
            continue;
        };
        if !onionskin_core::read_structure(structure).is_ok_and(|found| found.is_tagged()) {
            continue;
        }
        tagged += 1;
        edits += edit_and_check(&shown, doc);
    }
    eprintln!("guarantee 8: {tagged} tagged files, {edits} edits checked");
    assert!(
        tagged >= MIN_TAGGED_FILES,
        "the PDF/UA sets must hold {MIN_TAGGED_FILES} tagged files at least, found {tagged}"
    );
    assert!(
        edits >= tagged * 4,
        "most edits must apply: {edits} over {tagged} files"
    );
}
