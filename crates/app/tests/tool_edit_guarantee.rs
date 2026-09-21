//! Guarantee test 2: an edit appends one incremental section and nothing else,
//! driven by an edit a **tool** made through `core`.
//!
//! `crates/cos/tests/incremental.rs` proves the same sentence at the layer that
//! writes the bytes, and `crates/core/tests/save.rs` proves it through
//! `EditSession`. Neither can reach a plugin: `crates/core` depends on
//! `content`, `cos` and `render` only, so a test living there cannot drive a
//! tool, and the definition of done asks for exactly that.
//!
//! So this drives the real highlight tool through its real gesture lifecycle,
//! saves through `core`, and asserts all three clauses of the guarantee
//! sentence on the bytes that reach the disk.

use std::path::{Path, PathBuf};

use onionskin_core::{Document, DocumentFile, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_corpus_testing::seed;
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_comment::MarkupTool;

const VIEWPORT: ViewSize = ViewSize {
    width: 800.0,
    height: 600.0,
};

#[test]
fn a_tool_edit_appends_one_section_that_truncates_away() {
    let dir = temp_dir("one-highlight");
    let path = copy_seed(&dir, "hello.pdf");
    let original = std::fs::read(&path).expect("readable");

    let mut file = DocumentFile::open(&path).expect("opens");
    highlight_once(&mut file);
    assert!(
        file.is_dirty(),
        "the tool has to have edited something, or the rest of this proves nothing"
    );

    let outcome = file.save().expect("saves");
    let saved = std::fs::read(&path).expect("readable");

    assert_eq!(
        &saved[..original.len()],
        &original[..],
        "the original bytes must survive an edit made by a tool"
    );
    assert_eq!(
        outcome.sections_appended, 1,
        "a tool's edit must append exactly one incremental section"
    );
    assert_eq!(
        sections(&saved),
        2,
        "a tool's edit must append exactly one incremental section, counted by parsing"
    );
    assert_eq!(
        &saved[..original.len()],
        &original[..],
        "truncating the section must undo the tool's edit"
    );
    assert!(
        onionskin_cos::Document::open(Box::new(onionskin_cos::BytesSource::new(original.clone())))
            .is_ok(),
        "truncating the section must undo the tool's edit, leaving a document that opens"
    );
}

/// The clause the plan calls ambiguous, at the tool level: many gestures, one
/// save, one section.
#[test]
fn ten_tool_edits_and_one_save_are_still_one_section() {
    let dir = temp_dir("ten-highlights");
    let path = copy_seed(&dir, "hello.pdf");

    let mut file = DocumentFile::open(&path).expect("opens");
    for _ in 0..10 {
        highlight_once(&mut file);
    }
    assert!(
        file.edit().history().reach() >= 2,
        "ten gestures have to make more than one undo step, or this is the one-edit test again"
    );

    let outcome = file.save().expect("saves");
    assert_eq!(
        outcome.sections_appended, 1,
        "a tool's edit must append exactly one incremental section, however many edits it took"
    );
    assert_eq!(sections(&std::fs::read(&path).expect("readable")), 2);
}

/// Drive the real tool over the first run of glyphs on page 0.
fn highlight_once(file: &mut DocumentFile) {
    let mut tool = MarkupTool::highlight();
    let document: &mut Document = file;
    let mut viewport = viewport_for(document);

    let (from, to) = {
        let text = document.page_text(0).expect("page text extracts");
        let run = &text.runs[0];
        (
            centre(run.glyphs[0].quad),
            centre(run.glyphs[run.glyphs.len() - 1].quad),
        )
    };
    let input = |at: PagePoint| PointerInput {
        at,
        pressure: 1.0,
        modifiers: Modifiers::default(),
    };
    let mut ctx = ToolCtx {
        doc: document,
        viewport: &mut viewport,
    };
    tool.on_pointer_down(&mut ctx, input(from));
    tool.on_pointer_move(&mut ctx, input(to));
    tool.on_pointer_up(&mut ctx, input(to));
}

fn centre(quad: onionskin_core::PageQuad) -> PagePoint {
    PagePoint {
        page: quad.page,
        x: quad.corners.iter().map(|(x, _)| x).sum::<f64>() / 4.0,
        y: quad.corners.iter().map(|(_, y)| y).sum::<f64>() / 4.0,
    }
}

fn viewport_for(document: &mut Document) -> Viewport {
    let mut viewport =
        Viewport::new(document.page_count(), VIEWPORT, 12.0).expect("viewport is valid");
    for page in 0..document.page_count() {
        let geometry = document.page_geometry(page).expect("page measures").clone();
        viewport.measure_page(geometry).expect("page is measurable");
    }
    viewport.fit(FitMode::Page).expect("the page fits");
    viewport
}

/// Counted by parsing the cross-reference chain, never by scanning for
/// `%%EOF`: an original document may legitimately contain one already.
fn sections(bytes: &[u8]) -> usize {
    onionskin_cos::Document::open(Box::new(onionskin_cos::BytesSource::new(bytes.to_vec())))
        .expect("the saved document reopens")
        .sections()
        .expect("sections")
        .len()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("onionskin-tool-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn copy_seed(dir: &Path, name: &str) -> PathBuf {
    let destination = dir.join(name);
    std::fs::copy(seed(name), &destination).expect("seed copied");
    destination
}
