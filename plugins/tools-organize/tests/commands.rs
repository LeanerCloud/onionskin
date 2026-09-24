//! The Organize Pages toolset through the plugin surface: its commands, run
//! the way the shell runs them, and the selection-taking functions the page
//! grid and dialogs call.
//!
//! What `core::pages` already proves - page order through a fresh parse, the
//! importer's render comparison, the structure invariant - is not repeated
//! here. This suite asserts what this crate adds: that each command does its
//! operation to the page the viewport is on, as one undo step, declared as an
//! edit so an encrypted document disables it; and that the cross-document
//! functions keep each document's undo its own.

use std::collections::BTreeSet;

use onionskin_core::protection::Refusal;
use onionskin_core::{Document, Error};
use onionskin_corpus_testing::{encrypted_fixture, organize_fixture};
use onionskin_plugin_api::command_ids::{
    DELETE_PAGE, INSERT_BLANK_PAGE, MOVE_PAGE_EARLIER, MOVE_PAGE_LATER, RESET_PAGE_NUMBERING,
    ROTATE_PAGE_CLOCKWISE, ROTATE_PAGE_COUNTERCLOCKWISE,
};
use onionskin_plugin_api::{
    Availability, CommandCtx, CommandEffect, CommandError, PluginManifest, PluginRegistry,
    Requirement, Session,
};
use onionskin_tools_organize::{
    copy_pages_between, extract_pages_to, insert_pages_from, move_pages_between,
    replace_pages_from, OrganizeToolsPlugin,
};

const IDS: [&str; 7] = [
    ROTATE_PAGE_CLOCKWISE,
    ROTATE_PAGE_COUNTERCLOCKWISE,
    DELETE_PAGE,
    INSERT_BLANK_PAGE,
    MOVE_PAGE_EARLIER,
    MOVE_PAGE_LATER,
    RESET_PAGE_NUMBERING,
];

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

#[test]
fn the_plugin_registers_exactly_its_commands_each_declared_an_edit() {
    let registry = registry();
    let registered: BTreeSet<&str> = registry.commands().iter().map(|c| c.id).collect();
    assert_eq!(registered, IDS.into_iter().collect());
    for command in registry.commands() {
        assert_eq!(
            command.effect,
            CommandEffect::Edits,
            "{} changes the document",
            command.id
        );
        assert_eq!(
            command.keybind, None,
            "{}: a keystroke needs its window test first",
            command.id
        );
    }
}

/// The shared requirement query, not a list in the shell: an encrypted
/// document disables every one of them with its own reason.
#[test]
fn an_encrypted_document_disables_every_organize_command() {
    let registry = registry();
    let refusal = Refusal::EncryptedSource.reason();
    for id in IDS {
        let requirement = Requirement::Command {
            id,
            reason: "not installed",
        };
        let session = |edit_refusal| Session {
            registry: &registry,
            has_text_selection: false,
            edit_refusal,
            comment_refusal: edit_refusal,
            read_out_refusal: None,
        };
        assert_eq!(
            requirement.availability(&session(None)),
            Availability::Enabled
        );
        assert_eq!(
            requirement.availability(&session(Some(refusal))),
            Availability::Disabled(refusal),
            "{id}"
        );
    }
}

/// And run anyway - a stale menu, a keystroke - the document still refuses,
/// because `core` does, and nothing is written.
#[test]
fn a_command_run_on_an_encrypted_document_is_refused_by_core() {
    let mut document = Document::open_path(&encrypted_fixture("r6-aes-256-print-only.pdf"))
        .expect("opens read-only");
    assert_eq!(document.page_count(), 1, "the fixture is one page");
    for id in IDS {
        let outcome = run(&mut document, id, 0);
        if matches!(id, MOVE_PAGE_EARLIER | MOVE_PAGE_LATER) {
            // A one-page document gives a move nowhere to go: the command is a
            // no-op before it reaches `core`, and a no-op writes nothing.
            assert!(outcome.is_ok(), "{id}: {outcome:?}");
            continue;
        }
        assert!(
            matches!(
                outcome,
                Err(CommandError::Edit {
                    source: Error::Protected(Refusal::Restricted(_)),
                    ..
                })
            ),
            "{id}: {outcome:?}"
        );
    }
    assert!(!document.is_dirty());
}

// ---------------------------------------------------------------------------
// Each command, on the page the viewport is on
// ---------------------------------------------------------------------------

#[test]
fn rotate_writes_the_current_pages_rotate_and_only_that_page() {
    let mut document = numbered(3);
    run(&mut document, ROTATE_PAGE_CLOCKWISE, 1).expect("rotates");
    assert_eq!(rotations(&mut document), [None, Some(90), None]);
    run(&mut document, ROTATE_PAGE_COUNTERCLOCKWISE, 1).expect("rotates");
    run(&mut document, ROTATE_PAGE_COUNTERCLOCKWISE, 1).expect("rotates");
    assert_eq!(rotations(&mut document), [None, Some(270), None]);
    assert_eq!(undo_steps(&mut document), 3, "one undo step per command");
}

#[test]
fn delete_removes_the_current_page_and_refuses_the_last_one() {
    let mut document = numbered(2);
    run(&mut document, DELETE_PAGE, 0).expect("deletes");
    assert_eq!(texts(&mut document), ["Page 2"]);

    let refused = run(&mut document, DELETE_PAGE, 0);
    assert!(
        matches!(
            refused,
            Err(CommandError::Edit {
                source: Error::WouldLeaveNoPages,
                ..
            })
        ),
        "{refused:?}"
    );
    assert_eq!(texts(&mut document), ["Page 2"]);
}

#[test]
fn insert_blank_page_goes_after_the_current_page_at_its_size() {
    let mut document =
        Document::open_bytes(numbered_pdf(&[[0.0, 0.0, 300.0, 400.0]; 2])).expect("opens");
    run(&mut document, INSERT_BLANK_PAGE, 0).expect("inserts");
    assert_eq!(texts(&mut document), ["Page 1", "", "Page 2"]);
    let blank = document.page_geometry(1).expect("geometry").media_box;
    assert_eq!(blank, [0.0, 0.0, 300.0, 400.0]);
}

#[test]
fn move_earlier_and_later_swap_with_the_neighbour() {
    let mut document = numbered(3);
    run(&mut document, MOVE_PAGE_LATER, 0).expect("moves");
    assert_eq!(texts(&mut document), ["Page 2", "Page 1", "Page 3"]);
    run(&mut document, MOVE_PAGE_EARLIER, 2).expect("moves");
    assert_eq!(texts(&mut document), ["Page 2", "Page 3", "Page 1"]);
}

/// Moving the first page earlier or the last one later is a no-op, and a
/// no-op is not an undo entry.
#[test]
fn a_move_off_either_end_changes_nothing() {
    let mut document = numbered(2);
    run(&mut document, MOVE_PAGE_EARLIER, 0).expect("runs");
    run(&mut document, MOVE_PAGE_LATER, 1).expect("runs");
    assert_eq!(texts(&mut document), ["Page 1", "Page 2"]);
    assert_eq!(undo_steps(&mut document), 0);
}

#[test]
fn reset_numbering_removes_the_page_labels() {
    let mut document = Document::open_bytes(labelled()).expect("opens");
    assert!(has_labels(&mut document), "the fixture is labelled");
    run(&mut document, RESET_PAGE_NUMBERING, 0).expect("resets");
    assert!(!has_labels(&mut document));
}

// ---------------------------------------------------------------------------
// Between documents
// ---------------------------------------------------------------------------

/// The review risk: a move is a copy in one document and a delete in the
/// other, and the source's Undo has to be able to put its pages back.
#[test]
fn a_move_between_documents_leaves_the_sources_undo_able_to_restore_it() {
    let mut source = numbered(3);
    let mut destination = numbered(1);
    move_pages_between(&mut destination, &mut source, &[1], 1).expect("moves");

    assert_eq!(texts(&mut destination), ["Page 1", "Page 2"]);
    assert_eq!(texts(&mut source), ["Page 1", "Page 3"]);
    assert_eq!(
        undo_steps(&mut source),
        1,
        "the delete is one step in the source"
    );

    let (edit, base) = source.edit_mut();
    assert!(edit.undo(base).expect("undo"));
    assert_eq!(texts(&mut source), ["Page 1", "Page 2", "Page 3"]);
    assert_eq!(
        texts(&mut destination),
        ["Page 1", "Page 2"],
        "and the destination keeps its copy: each document's undo is its own"
    );
}

#[test]
fn moving_every_page_out_of_a_document_changes_neither() {
    let mut source = numbered(2);
    let mut destination = numbered(1);
    let refused = move_pages_between(&mut destination, &mut source, &[0, 1], 0);
    assert!(matches!(
        refused,
        Err(CommandError::Edit {
            source: Error::WouldLeaveNoPages,
            ..
        })
    ));
    assert!(!source.is_dirty() && !destination.is_dirty());
}

#[test]
fn copy_between_documents_takes_every_page_when_none_are_named() {
    let mut source = numbered(2);
    let mut destination = numbered(1);
    copy_pages_between(&mut destination, &mut source, None, 0).expect("copies");
    assert_eq!(texts(&mut destination), ["Page 1", "Page 2", "Page 1"]);
    assert!(!source.is_dirty(), "a copy leaves its source alone");
}

#[test]
fn insert_from_a_file_brings_its_pages_in() {
    let mut document = numbered(1);
    insert_pages_from(
        &mut document,
        &organize_fixture("embedded-font.pdf"),
        Some(&[1]),
        1,
    )
    .expect("inserts");
    assert_eq!(texts(&mut document), ["Page 1", "Second source page"]);
}

/// The per-input shape of the encrypted-source rule: the file is picked after
/// the command runs, so no session query saw it. Refused, and nothing written.
#[test]
fn insert_and_replace_from_an_encrypted_file_are_refused_and_write_nothing() {
    let encrypted = encrypted_fixture("r6-aes-256-print-only.pdf");
    let mut document = numbered(2);
    let refused = insert_pages_from(&mut document, &encrypted, None, 0);
    assert!(is_encrypted_refusal(&refused), "{refused:?}");
    assert!(!document.is_dirty());

    let mut source = Document::open_path(&encrypted).expect("opens read-only");
    let refused = replace_pages_from(&mut document, &mut source, &[0], &[1]);
    assert!(is_encrypted_refusal(&refused), "{refused:?}");
    assert!(!document.is_dirty());
}

#[test]
fn extract_to_a_file_writes_the_pages_and_can_delete_them_after() {
    let dir = scratch("extract");
    let mut document = numbered(3);

    let kept = dir.join("kept.pdf");
    extract_pages_to(&mut document, &[2, 0], &kept, false).expect("extracts");
    let mut extracted = Document::open_path(&kept).expect("the extract opens");
    assert_eq!(texts(&mut extracted), ["Page 3", "Page 1"]);
    assert!(
        !document.is_dirty(),
        "extracting alone changes nothing here"
    );

    let moved = dir.join("moved.pdf");
    extract_pages_to(&mut document, &[1], &moved, true).expect("extracts");
    assert_eq!(
        texts(&mut Document::open_path(&moved).expect("opens")),
        ["Page 2"]
    );
    assert_eq!(texts(&mut document), ["Page 1", "Page 3"]);
}

#[test]
fn extracting_from_an_encrypted_document_is_refused_and_writes_no_file() {
    let dir = scratch("extract-encrypted");
    let mut document = Document::open_path(&encrypted_fixture("r6-aes-256-print-only.pdf"))
        .expect("opens read-only");
    let target = dir.join("out.pdf");
    let refused = extract_pages_to(&mut document, &[0], &target, false);
    assert!(is_encrypted_refusal(&refused), "{refused:?}");
    assert!(!target.exists(), "no decrypted copy on disk");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    OrganizeToolsPlugin.register(&mut registry);
    registry
}

/// Run a registered command the way the canvas does.
fn run(document: &mut Document, id: &str, page: usize) -> Result<(), CommandError> {
    let registry = registry();
    let command = registry
        .commands()
        .iter()
        .find(|command| command.id == id)
        .unwrap_or_else(|| panic!("{id} is registered"));
    (command.run)(&mut CommandCtx {
        doc: document,
        page,
    })
}

fn is_encrypted_refusal(outcome: &Result<(), CommandError>) -> bool {
    matches!(
        outcome,
        Err(CommandError::Edit {
            source: Error::Protected(Refusal::Restricted(_) | Refusal::EncryptedSource),
            ..
        })
    )
}

fn texts(document: &mut Document) -> Vec<String> {
    (0..document.page_count())
        .map(|index| {
            document
                .page_text(index)
                .expect("text extracts")
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect()
}

fn rotations(document: &mut Document) -> Vec<Option<i64>> {
    let count = document.page_count();
    let current = document.structure().expect("the current document");
    (0..count)
        .map(|index| {
            current
                .page(index)
                .expect("page")
                .dict
                .get(b"Rotate")
                .and_then(onionskin_cos::Object::as_integer)
        })
        .collect()
}

fn has_labels(document: &mut Document) -> bool {
    document
        .structure()
        .expect("current")
        .catalog()
        .expect("catalog")
        .get(b"PageLabels")
        .is_some()
}

fn undo_steps(document: &mut Document) -> usize {
    let (edit, base) = document.edit_mut();
    let mut steps = 0;
    while edit.undo(base).expect("undo") {
        steps += 1;
    }
    for _ in 0..steps {
        edit.redo(base).expect("redo");
    }
    steps
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("onionskin-organize-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn numbered(count: usize) -> Document {
    Document::open_bytes(numbered_pdf(&vec![[0.0, 0.0, 612.0, 792.0]; count])).expect("opens")
}

/// One page per box, each saying "Page n" in Helvetica.
fn numbered_pdf(boxes: &[[f64; 4]]) -> Vec<u8> {
    build(boxes, "")
}

/// Three pages labelled i, ii, iii.
fn labelled() -> Vec<u8> {
    build(
        &[[0.0, 0.0, 612.0, 792.0]; 3],
        " /PageLabels << /Nums [0 << /S /r >>] >>",
    )
}

fn build(boxes: &[[f64; 4]], catalog_extra: &str) -> Vec<u8> {
    let count = boxes.len();
    let font = 3 + count;
    let first_content = font + 1;
    let kids: Vec<String> = (0..count)
        .map(|index| format!("{} 0 R", 3 + index))
        .collect();
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R{catalog_extra} >>"),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} /Resources << /Font << /F1 {font} 0 R >> >> >>",
            kids.join(" ")
        ),
    ];
    for (index, media_box) in boxes.iter().enumerate() {
        let [x0, y0, x1, y1] = media_box;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [{x0} {y0} {x1} {y1}] /Contents {} 0 R >>",
            first_content + index
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());
    for index in 0..count {
        let text = format!("BT /F1 24 Tf 20 20 Td (Page {}) Tj ET", index + 1);
        objects.push(format!(
            "<< /Length {} >>\nstream\n{text}\nendstream",
            text.len()
        ));
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
