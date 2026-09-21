//! Combine and Split, through the functions the dialogs and the command call,
//! asserted on the files they write, parsed back.
//!
//! The page-level guarantees - every page drawn as in its source, independent
//! copies, the structure tree - are `core::pages::Assembly`'s and asserted in
//! `crates/core/tests/assemble.rs`. This suite asserts what the commands add:
//! which inputs, in what order, refused how, written where, and all or nothing.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use onionskin_commands_core::combine::{self, CombineError, Input};
use onionskin_commands_core::publish::PublishError;
use onionskin_commands_core::split::{self, SplitBy, SplitError};
use onionskin_commands_core::CoreCommandsPlugin;
use onionskin_core::protection::Refusal;
use onionskin_core::Document;
use onionskin_corpus_testing::{encrypted_fixture, organize_fixture, seed};
use onionskin_plugin_api::command_ids::SPLIT_DOCUMENT;
use onionskin_plugin_api::{
    Availability, CommandCtx, CommandError, CommandPlugin, PluginManifest, PluginRegistry,
    Requirement, Session,
};

// ---------------------------------------------------------------------------
// Combine
// ---------------------------------------------------------------------------

#[test]
fn combine_writes_every_input_in_order() {
    let dir = tempfile::tempdir().expect("dir");
    let third = write(dir.path(), "third.pdf", &numbered(3, &[]));
    let output = dir.path().join("combined.pdf");
    let combined = combine::combine(
        &[
            Input::whole(organize_fixture("embedded-font.pdf")),
            Input::whole(seed("two-page.pdf")),
            Input::whole(&third),
        ],
        &output,
    )
    .expect("combines");

    assert_eq!(combined.page_count, 2 + 2 + 3, "the sum of the inputs");
    let mut expected = texts_of(&organize_fixture("embedded-font.pdf"));
    expected.extend(texts_of(&seed("two-page.pdf")));
    expected.extend(texts_of(&third));
    assert_eq!(texts_of(&output), expected);
}

/// Position 2 of 3: a check written once, for the first input, lets this one
/// through. The refusal names the file, and no output is written.
#[test]
fn an_encrypted_input_in_position_two_refuses_the_combine_and_names_it() {
    let dir = tempfile::tempdir().expect("dir");
    let first = write(dir.path(), "first.pdf", &numbered(1, &[]));
    let third = write(dir.path(), "third.pdf", &numbered(1, &[]));
    let encrypted = encrypted_fixture("r4-aes-128.pdf");
    let output = dir.path().join("combined.pdf");

    let refused = combine::combine(
        &[
            Input::whole(&first),
            Input::whole(&encrypted),
            Input::whole(&third),
        ],
        &output,
    );
    match refused {
        Err(CombineError::Input {
            path,
            source: onionskin_core::Error::Protected(Refusal::EncryptedSource),
        }) => assert_eq!(path, encrypted, "the refusal names the encrypted file"),
        other => panic!("not refused as encrypted: {other:?}"),
    }
    assert!(!output.exists(), "no output was written");
}

#[test]
fn a_file_that_is_not_a_pdf_is_named() {
    let dir = tempfile::tempdir().expect("dir");
    let bogus = write(dir.path(), "notes.pdf", b"not a pdf at all");
    let refused = combine::combine(&[Input::whole(&bogus)], &dir.path().join("out.pdf"));
    assert!(
        matches!(&refused, Err(CombineError::Open { path, .. }) if *path == bogus),
        "{refused:?}"
    );
}

#[test]
fn a_file_combined_with_itself_is_there_twice() {
    let dir = tempfile::tempdir().expect("dir");
    let source = write(dir.path(), "source.pdf", &numbered(2, &[]));
    let output = dir.path().join("twice.pdf");
    combine::combine(&[Input::whole(&source), Input::whole(&source)], &output).expect("combines");
    assert_eq!(texts_of(&output), ["Page 1", "Page 2", "Page 1", "Page 2"]);
}

/// The dialog's per-file expansion: pages picked out of one file, in the
/// order picked.
#[test]
fn an_expanded_input_contributes_the_pages_picked_in_the_order_picked() {
    let dir = tempfile::tempdir().expect("dir");
    let source = write(dir.path(), "source.pdf", &numbered(4, &[]));
    let output = dir.path().join("picked.pdf");
    combine::combine(
        &[Input {
            path: source,
            pages: Some(vec![3, 0]),
        }],
        &output,
    )
    .expect("combines");
    assert_eq!(texts_of(&output), ["Page 4", "Page 1"]);
}

#[test]
fn an_existing_output_is_not_overwritten() {
    let dir = tempfile::tempdir().expect("dir");
    let source = write(dir.path(), "source.pdf", &numbered(1, &[]));
    let output = write(dir.path(), "taken.pdf", b"mine");
    let refused = combine::combine(&[Input::whole(&source)], &output);
    assert!(matches!(
        refused,
        Err(CombineError::Publish(PublishError::Exists(_)))
    ));
    assert_eq!(std::fs::read(&output).expect("read"), b"mine");
}

#[test]
fn add_folder_adds_the_pdfs_in_it_by_name_and_nothing_else() {
    let dir = tempfile::tempdir().expect("dir");
    for name in ["b.pdf", "a.PDF", "notes.txt"] {
        write(dir.path(), name, b"x");
    }
    std::fs::create_dir(dir.path().join("nested.pdf")).expect("a directory named like a pdf");
    let found = combine::pdfs_in_folder(dir.path()).expect("lists");
    let names: Vec<_> = found
        .iter()
        .map(|path| {
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["a.PDF", "b.pdf"]);
}

#[test]
fn the_page_count_the_dialog_previews_is_the_files() {
    assert_eq!(
        combine::page_count(&organize_fixture("embedded-font.pdf")).expect("counts"),
        2
    );
}

// ---------------------------------------------------------------------------
// Split
// ---------------------------------------------------------------------------

/// Ten pages in threes: four files, and the fourth holds page ten.
#[test]
fn ten_pages_split_in_threes_is_four_files_the_last_with_page_ten() {
    let dir = tempfile::tempdir().expect("dir");
    let mut document = Document::open_bytes(numbered(10, &[])).expect("opens");
    let written = split::split(
        &mut document,
        SplitBy::PageCount(NonZeroUsize::new(3).expect("nonzero")),
        dir.path(),
        "Report",
    )
    .expect("splits");

    assert_eq!(written.files.len(), 4);
    let parts: Vec<Vec<String>> = written.files.iter().map(|path| texts_of(path)).collect();
    assert_eq!(parts[0], ["Page 1", "Page 2", "Page 3"]);
    assert_eq!(parts[3], ["Page 10"], "the last part holds the tenth page");
    assert_eq!(
        written.files[3],
        dir.path().join("Report - Part 4.pdf"),
        "named after the document, numbered from one"
    );
}

/// Top-level bookmarks at pages 1, 4 and 9, and one that names no page.
#[test]
fn splitting_at_bookmarks_cuts_there_and_reports_the_one_that_names_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let bookmarks = [
        ("One", Some(0)),
        ("Four", Some(3)),
        ("Nowhere", None),
        ("Nine", Some(8)),
    ];
    let mut document = Document::open_bytes(numbered(10, &bookmarks)).expect("opens");
    let written = split::split(
        &mut document,
        SplitBy::TopLevelBookmarks,
        dir.path(),
        "Book",
    )
    .expect("splits");

    let firsts: Vec<String> = written
        .files
        .iter()
        .map(|path| texts_of(path)[0].clone())
        .collect();
    assert_eq!(firsts, ["Page 1", "Page 4", "Page 9"]);
    assert_eq!(texts_of(&written.files[2]), ["Page 9", "Page 10"]);
    assert_eq!(written.unresolved, ["Nowhere"]);
}

/// Best effort, and exactly what it promises: no part over the target unless
/// it is a single page, every page present once and in order.
#[test]
fn splitting_by_size_keeps_every_part_under_the_target_unless_it_is_one_page() {
    let mut document = Document::open_bytes(numbered(7, &[])).expect("opens");
    let (_, one_page) = split::split_bytes(
        &mut document,
        SplitBy::PageCount(NonZeroUsize::new(1).expect("nonzero")),
    )
    .expect("measures");
    let target = one_page[0].len() as u64 * 2;

    let (plan, parts) =
        split::split_bytes(&mut document, SplitBy::FileSize(target)).expect("splits");
    assert!(plan.parts.len() > 1, "the target is small enough to cut");
    for (range, bytes) in plan.parts.iter().zip(&parts) {
        assert!(
            bytes.len() as u64 <= target || range.len() == 1,
            "{range:?} is {} bytes against {target}",
            bytes.len()
        );
    }
    let covered: Vec<usize> = plan.parts.iter().flat_map(|range| range.clone()).collect();
    assert_eq!(covered, (0..7).collect::<Vec<_>>());
}

/// A target smaller than any page: every page on its own, none refused.
#[test]
fn a_page_larger_than_the_target_gets_a_file_of_its_own() {
    let mut document = Document::open_bytes(numbered(3, &[])).expect("opens");
    let (plan, _) = split::split_bytes(&mut document, SplitBy::FileSize(1)).expect("splits");
    assert_eq!(plan.parts, [0..1, 1..2, 2..3]);
}

/// The session-scoped shape of the encrypted-source rule: the open document
/// is refused before anything is planned, and nothing is written.
#[test]
fn splitting_an_encrypted_document_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let mut document =
        Document::open_path(&encrypted_fixture("r4-aes-128.pdf")).expect("opens read-only");
    let refused = split::split(
        &mut document,
        SplitBy::PageCount(NonZeroUsize::new(1).expect("nonzero")),
        dir.path(),
        "Secret",
    );
    assert!(matches!(
        refused,
        Err(SplitError::Refused(Refusal::EncryptedSource))
    ));
    assert_eq!(std::fs::read_dir(dir.path()).expect("list").count(), 0);
}

/// All or nothing: one part's name already taken refuses the split before
/// any part is written.
#[test]
fn a_split_whose_part_name_is_taken_writes_no_part() {
    let dir = tempfile::tempdir().expect("dir");
    write(dir.path(), "Doc - Part 2.pdf", b"mine");
    let mut document = Document::open_bytes(numbered(4, &[])).expect("opens");
    let refused = split::split(
        &mut document,
        SplitBy::PageCount(NonZeroUsize::new(2).expect("nonzero")),
        dir.path(),
        "Doc",
    );
    assert!(matches!(
        refused,
        Err(SplitError::Publish(PublishError::Exists(_)))
    ));
    assert!(!dir.path().join("Doc - Part 1.pdf").exists());
}

// ---------------------------------------------------------------------------
// The command
// ---------------------------------------------------------------------------

#[test]
fn the_split_command_writes_beside_the_document() {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(
        dir.path(),
        "Annual.pdf",
        &numbered(4, &[("A", Some(0)), ("B", Some(2))]),
    );
    let mut document = Document::open_path(&path).expect("opens");
    run(&mut document, SPLIT_DOCUMENT).expect("splits");
    assert_eq!(
        texts_of(&dir.path().join("Annual - Part 2.pdf")),
        ["Page 3", "Page 4"]
    );
}

#[test]
fn the_split_command_needs_a_file_to_write_beside() {
    let mut document = Document::open_bytes(numbered(2, &[("A", Some(0))])).expect("opens");
    let failed = run(&mut document, SPLIT_DOCUMENT);
    assert!(
        matches!(&failed, Err(CommandError::Failed { reason, .. }) if reason.contains("saved")),
        "{failed:?}"
    );
}

/// The menu asks through the requirement query, and a document that may not be
/// read out disables the entry with its own reason.
#[test]
fn the_split_entry_is_disabled_where_reading_out_is_refused() {
    let mut registry = PluginRegistry::new();
    CoreCommandsPlugin.register(&mut registry);
    let requirement = Requirement::Command {
        id: SPLIT_DOCUMENT,
        reason: "not installed",
    };
    let reason = Refusal::EncryptedSource.reason();
    let session = |read_out_refusal| Session {
        registry: &registry,
        has_text_selection: false,
        edit_refusal: None,
        read_out_refusal,
    };
    assert_eq!(
        requirement.availability(&session(None)),
        Availability::Enabled
    );
    assert_eq!(
        requirement.availability(&session(Some(reason))),
        Availability::Disabled(reason)
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run(document: &mut Document, id: &str) -> Result<(), CommandError> {
    let command = CoreCommandsPlugin
        .commands()
        .into_iter()
        .find(|command| command.id == id)
        .unwrap_or_else(|| panic!("{id} is registered"));
    (command.run)(&mut CommandCtx {
        doc: document,
        page: 0,
    })
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write");
    path
}

fn texts_of(path: &Path) -> Vec<String> {
    let mut document = Document::open_path(path).expect("opens");
    (0..document.page_count())
        .map(|index| {
            document
                .page_text(index)
                .expect("extracts")
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect()
}

/// `count` pages each saying "Page n", with top-level bookmarks
/// `(title, page)`; a `None` page gives a bookmark with no destination.
fn numbered(count: usize, bookmarks: &[(&str, Option<usize>)]) -> Vec<u8> {
    let font = 3 + count;
    let first_content = font + 1;
    let outlines = first_content + count;
    let first_item = outlines + 1;
    let kids: Vec<String> = (0..count)
        .map(|index| format!("{} 0 R", 3 + index))
        .collect();
    let outline_entry = if bookmarks.is_empty() {
        String::new()
    } else {
        format!(" /Outlines {outlines} 0 R")
    };
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R{outline_entry} >>"),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} /MediaBox [0 0 300 300] /Resources << /Font << /F1 {font} 0 R >> >> >>",
            kids.join(" ")
        ),
    ];
    for index in 0..count {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>",
            first_content + index
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());
    for index in 0..count {
        let text = format!("BT /F1 18 Tf 20 20 Td (Page {}) Tj ET", index + 1);
        objects.push(format!(
            "<< /Length {} >>\nstream\n{text}\nendstream",
            text.len()
        ));
    }
    if !bookmarks.is_empty() {
        let last = first_item + bookmarks.len() - 1;
        objects.push(format!(
            "<< /Type /Outlines /First {first_item} 0 R /Last {last} 0 R /Count {} >>",
            bookmarks.len()
        ));
        for (index, (title, page)) in bookmarks.iter().enumerate() {
            let number = first_item + index;
            let mut item = format!("<< /Title ({title}) /Parent {outlines} 0 R");
            if index > 0 {
                item.push_str(&format!(" /Prev {} 0 R", number - 1));
            }
            if number < last {
                item.push_str(&format!(" /Next {} 0 R", number + 1));
            }
            if let Some(page) = page {
                item.push_str(&format!(" /Dest [{} 0 R /Fit]", 3 + page));
            }
            item.push_str(" >>");
            objects.push(item);
        }
    }

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
