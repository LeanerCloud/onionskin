//! Every registered plugin against the contract, exhaustively.
//!
//! The rules live in `plugin-api`'s `contract` module; this runs them over the
//! real `build_registry()`. That split is deliberate: a plugin crate can check
//! itself without the whole app, and there is one statement of the contract
//! rather than one per test that goes looking for it.
//!
//! **The point of running it over the registry rather than per plugin** is that
//! a tool added tomorrow inherits these rules by being registered. A rule
//! written beside one tool is a rule the next tool does not have.

use std::path::Path;

use onionskin_app::build_registry;
use onionskin_core::{
    add_annotation, read_structure, Annotation, Document, ObjRef, Rect, Subtype, ViewSize, Viewport,
};
use onionskin_plugin_api::contract::{check_edits, check_metadata, Violation};
use onionskin_plugin_api::{Overlay, PluginRegistry, PointerInput, ToolCtx, ToolPlugin};

#[test]
fn every_registered_plugin_declares_what_it_must() {
    let registry = build_registry();
    let violations = check_metadata(&registry);
    assert!(violations.is_empty(), "{}", report(&violations));
}

/// The rules above pass trivially on an empty registry, and guarantee 5 builds
/// exactly that: every plugin compiled out, an app that boots to a workspace
/// which can do nothing. So the non-vacuity check is its own test, gated on a
/// feature that actually contributes tools, rather than an assertion inside the
/// rules that would fail the guarantee-5 build for being correct.
#[cfg(feature = "tools-basic")]
#[test]
fn the_registry_has_something_to_check() {
    let registry = build_registry();
    assert!(
        registry.tools().count() > 0,
        "tools-basic is on, so the registry has tools"
    );
    assert!(
        !registry.commands().is_empty() || registry.tools().count() > 0,
        "a registry with neither tools nor commands checks nothing"
    );
}

/// The suite has to reject something, or it is not checking. A tool with no
/// group is the failure a reviewer actually meets: it silently takes its own
/// rail slot instead of joining the set it belongs to.
#[test]
fn the_contract_rejects_a_tool_that_is_missing_a_group() {
    let mut registry = PluginRegistry::new();
    registry.register_tool(Box::new(Incomplete));
    let violations = check_metadata(&registry);
    assert!(
        violations.contains(&Violation::EmptyToolField {
            id: "incomplete".into(),
            field: "group",
        }),
        "an incomplete tool has to be rejected, got {violations:?}"
    );
}

/// Every tool's real gesture lifecycle, on a document with something to edit.
/// A tool that edits must undo cleanly and must serialize.
#[test]
fn every_tools_edit_is_undoable_and_serializes() {
    let mut registry = build_registry();
    let mut document = Document::open_bytes(one_page()).expect("opens");
    let mut viewport = viewport_for(&document);
    let violations = check_edits(&mut registry, &mut document, &mut viewport);
    assert!(violations.is_empty(), "{}", report(&violations));
}

/// **The clause above is vacuous today and this is what makes it mean
/// something.** No tool M2 shipped edits anything, so `check_edits` finds
/// nothing to check until P8's comment tools land. A synthetic tool that does
/// edit proves the checker works now rather than in three packages' time, and
/// it is the test the "undo is a no-op" mutation has to break.
#[test]
fn the_contract_checks_a_tool_that_really_edits() {
    let mut registry = PluginRegistry::new();
    registry.register_tool(Box::new(Scribble));
    let mut document = Document::open_bytes(one_page()).expect("opens");
    let mut viewport = viewport_for(&document);

    let violations = check_edits(&mut registry, &mut document, &mut viewport);
    assert!(
        violations.is_empty(),
        "an edit made properly through the edit session undoes and serializes: {}",
        report(&violations)
    );
    assert!(
        document.edit().pending_edits().is_empty(),
        "and the checker left the document as it found it"
    );
}

/// Degenerate documents: the shapes a tool is likeliest to be surprised by.
/// Nothing here asserts a tool does anything, only that it survives.
#[test]
fn every_tool_survives_a_degenerate_document() {
    for (what, bytes) in [
        ("a one-page document with no content stream", one_page()),
        ("a page with a zero-size media box", zero_sized_page()),
    ] {
        let mut registry = build_registry();
        let Ok(mut document) = Document::open_bytes(bytes) else {
            panic!("{what} has to open; the fixture is the test's own");
        };
        let mut viewport = viewport_for(&document);
        let violations = check_edits(&mut registry, &mut document, &mut viewport);
        assert!(violations.is_empty(), "{what}: {}", report(&violations));
    }
}

/// The thousand-page file, when it has been generated. Skipped loudly rather
/// than silently when it has not, like every other corpus-dependent test.
#[test]
fn every_tool_survives_the_thousand_page_document() {
    let Some(path) = bench_document() else {
        eprintln!(
            "SKIPPED: corpus/bench/pages-1000.pdf is absent; generate it with corpus/make-bench.py"
        );
        return;
    };
    let mut registry = build_registry();
    let mut document = Document::open_path(&path).expect("the bench file opens");
    let mut viewport = viewport_for(&document);
    let violations = check_edits(&mut registry, &mut document, &mut viewport);
    assert!(violations.is_empty(), "{}", report(&violations));
}

/// The narrowing P7's review risk asks about, asserted on the source.
///
/// A tool is handed `&mut core::Document`. If the save path lived on
/// `Document`, every tool could truncate the user's file mid-gesture. It lives
/// on `DocumentFile` instead, and nothing gets from a `Document` back to the
/// file that owns it. This reads the source because the alternative, a test
/// that tries to call `document.save()` and expects a compile error, needs a
/// compile-fail harness this workspace does not carry.
#[test]
fn the_save_path_is_not_on_what_a_tool_is_handed() {
    let session = std::fs::read_to_string(workspace_root().join("crates/core/src/session.rs"))
        .expect("session.rs is readable");
    for method in [
        "pub fn save(",
        "pub fn save_as(",
        "pub fn revert_to(",
        "pub fn autosave(",
        "pub fn set_recovery(",
    ] {
        assert!(
            !session.contains(method),
            "`{method}` is public on Document, so every tool holding &mut Document can reach it"
        );
    }
    let file = std::fs::read_to_string(workspace_root().join("crates/core/src/file.rs"))
        .expect("file.rs is readable");
    assert!(
        file.contains("pub fn save(") && file.contains("pub fn revert_to("),
        "the file operations have to live somewhere, and that somewhere is DocumentFile"
    );
}

fn report(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(|violation| format!("  {violation}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn viewport_for(document: &Document) -> Viewport {
    Viewport::new(
        document.page_count(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
        12.0,
    )
    .expect("a viewport for the test document")
}

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/app sits two levels under the workspace root")
        .to_path_buf()
}

fn bench_document() -> Option<std::path::PathBuf> {
    let path = workspace_root().join("corpus/bench/pages-1000.pdf");
    path.is_file().then_some(path)
}

/// A tool that declares nothing it should. Never registered anywhere real.
struct Incomplete;

impl ToolPlugin for Incomplete {
    fn id(&self) -> &'static str {
        "incomplete"
    }
    fn name(&self) -> &'static str {
        ""
    }
    fn icon(&self) -> &'static str {
        ""
    }
    fn group(&self) -> &'static str {
        ""
    }
    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        Vec::new()
    }
}

/// A tool that authors one annotation on commit. Never registered anywhere
/// real; it exists so the behavioural clauses have something to be true of.
struct Scribble;

impl ToolPlugin for Scribble {
    fn id(&self) -> &'static str {
        "scribble"
    }
    fn name(&self) -> &'static str {
        "Scribble"
    }
    fn icon(&self) -> &'static str {
        "square"
    }
    fn group(&self) -> &'static str {
        "scribble"
    }
    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, _input: PointerInput) {
        let structure = read_structure(ctx.doc.edit_mut().1).expect("structure reads");
        let (edit, base) = ctx.doc.edit_mut();
        edit.transact(base, "Scribble", |tx| {
            add_annotation(
                tx,
                &structure,
                ObjRef::new(3, 0),
                &Annotation::new(Subtype::Square, Rect::new(10.0, 10.0, 60.0, 60.0)),
                0,
            )
            .map(|_| ())
        })
        .expect("the annotation commits");
    }
    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        Vec::new()
    }
}

fn one_page() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>".to_vec(),
    ])
}

fn zero_sized_page() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 0 0] /Resources << >> >>".to_vec(),
    ])
}

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}
