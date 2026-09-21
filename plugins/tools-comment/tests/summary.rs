//! Summarize Comments, asserted on the file it writes, opened and read back.

use std::path::PathBuf;

use onionskin_core::{add_annotation, Annotation, Document, Rect, Subtype};
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_plugin_api::command_ids::SUMMARIZE_COMMENTS;
use onionskin_plugin_api::{CommandCtx, CommandEffect, CommandPlugin};
use onionskin_tools_comment::{
    summarize, summary_path, CommentToolsPlugin, SummaryError, SummaryLayout,
};

const NOW: i64 = 1_789_999_500;

fn comment(document: &mut Document, page: usize, subtype: Subtype, author: &str, contents: &str) {
    let page_ref = document
        .structure()
        .expect("the document")
        .page(page)
        .expect("the page")
        .objref;
    let mut annotation = Annotation::new(subtype, Rect::new(10.0, 10.0, 30.0, 30.0));
    annotation.author = Some(author.to_owned());
    annotation.contents = Some(contents.to_owned());
    document
        .edit_annotations("Comment", |tx, structure| {
            add_annotation(tx, structure, page_ref, &annotation, NOW).map(|_| ())
        })
        .expect("comments");
}

/// two-page.pdf with a note and a stamp on page one and a long text box on
/// page two.
fn commented() -> Document {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    comment(&mut document, 0, Subtype::Text, "Ana", "Check this figure");
    comment(&mut document, 0, Subtype::Stamp, "Bo", "Approved");
    let long = "Revise the second paragraph. ".repeat(250);
    comment(&mut document, 1, Subtype::FreeText, "Chidi", long.trim());
    document
}

fn texts(bytes: Vec<u8>) -> Vec<String> {
    let mut document = Document::open_bytes(bytes).expect("the summary opens");
    (0..document.page_count())
        .map(|page| {
            document
                .page_text(page)
                .expect("extracts")
                .flatten()
                .text
                .replace('\n', " ")
        })
        .collect()
}

#[test]
fn comments_only_lists_every_comment_with_its_author_by_page() {
    let mut document = commented();
    let summary = summarize(&mut document, SummaryLayout::CommentsOnly).expect("summarizes");
    assert_eq!(summary.comment_count, 3);
    let pages = texts(summary.bytes);
    assert_eq!(pages.len(), summary.page_count);
    assert!(
        pages.len() >= 3,
        "page two's long comment runs over: {}",
        pages.len()
    );
    let all = pages.join(" ");
    for expected in [
        "Page 1",
        "Page 2",
        "1. Sticky Note by Ana, 2026-09-21 14:05",
        "Check this figure",
        "2. Stamp by Bo",
        "Approved",
        "3. Text Box by Chidi",
        "Revise the second paragraph.",
    ] {
        assert!(all.contains(expected), "{expected:?} missing from {all}");
    }
    assert!(
        pages[0].contains("Page 1") && !pages[0].contains("Page 2"),
        "each source page's comments start a page of their own"
    );
}

#[test]
fn document_and_comments_puts_each_page_before_its_comments() {
    let mut document = commented();
    let source: Vec<String> = (0..2)
        .map(|page| document.page_text(page).expect("extracts").flatten().text)
        .collect();
    let summary = summarize(&mut document, SummaryLayout::DocumentAndComments).expect("summarizes");
    let pages = texts(summary.bytes);
    assert_eq!(pages[0], source[0].replace('\n', " "), "page one as it is");
    assert!(pages[1].contains("Check this figure"), "then its comments");
    assert_eq!(pages[2], source[1].replace('\n', " "), "then page two");
    assert!(pages[3].contains("Chidi"), "then its comments");
}

#[test]
fn a_document_with_no_comments_says_so_and_writes_nothing() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    assert!(matches!(
        summarize(&mut document, SummaryLayout::CommentsOnly),
        Err(SummaryError::NoComments)
    ));
}

#[test]
fn an_encrypted_document_is_refused() {
    let mut document =
        Document::open_path(&encrypted_fixture("r4-aes-128.pdf")).expect("opens read-only");
    assert!(matches!(
        summarize(&mut document, SummaryLayout::CommentsOnly),
        Err(SummaryError::Document(onionskin_core::Error::Protected(_)))
    ));
}

#[test]
fn the_registered_command_writes_beside_the_document_and_never_over_a_file() {
    let command = CommentToolsPlugin
        .commands()
        .into_iter()
        .find(|command| command.id == SUMMARIZE_COMMENTS)
        .expect("registered");
    assert_eq!(command.effect, CommandEffect::ReadsOut);

    let dir = tempfile::tempdir().expect("dir");
    let path: PathBuf = dir.path().join("report.pdf");
    std::fs::copy(seed("two-page.pdf"), &path).expect("copies");
    let mut document = Document::open_path(&path).expect("opens");
    comment(&mut document, 1, Subtype::Text, "Ana", "Why?");
    (command.run)(&mut CommandCtx {
        doc: &mut document,
        page: 0,
    })
    .expect("summarizes");
    let written = summary_path(&path);
    assert_eq!(written, dir.path().join("report - Comments.pdf"));
    assert!(texts(std::fs::read(&written).expect("written"))
        .join(" ")
        .contains("Why?"));

    let again = (command.run)(&mut CommandCtx {
        doc: &mut document,
        page: 0,
    });
    assert!(again.is_err(), "the existing summary is not replaced");
}
