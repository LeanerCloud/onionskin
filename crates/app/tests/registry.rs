//! Properties every registry entry has to hold, checked against the whole
//! registry at once so a new plugin cannot quietly skip them. A plugin
//! inherits the contract by being registered.

use std::path::{Path, PathBuf};

use onionskin_app::build_registry;
use onionskin_core::{Document, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_plugin_api::{PointerInput, ToolCtx};

#[test]
fn every_plugin_has_a_non_empty_id_and_name() {
    for plugin in build_registry().plugins() {
        assert!(!plugin.id.is_empty(), "{} has an empty id", plugin.name);
        assert!(!plugin.name.is_empty(), "{} has no name", plugin.id);
    }
}

#[test]
fn every_plugin_id_is_unique() {
    let registry = build_registry();
    let mut ids: Vec<&str> = registry.plugins().iter().map(|p| p.id).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate plugin ids");
}

#[test]
fn every_tool_has_a_name_icon_and_group() {
    for tool in build_registry().tools() {
        assert!(!tool.id().is_empty(), "a tool has an empty id");
        assert!(!tool.name().is_empty(), "{} has no name", tool.id());
        assert!(!tool.icon().is_empty(), "{} has no icon", tool.id());
        assert!(!tool.group().is_empty(), "{} has no group", tool.id());
    }
}

#[test]
fn every_tool_id_is_unique() {
    let registry = build_registry();
    let mut ids: Vec<&str> = registry.tools().map(|t| t.id()).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate tool ids");
}

#[test]
fn every_command_has_a_title_and_a_namespaced_id() {
    for command in build_registry().commands() {
        assert!(!command.title.is_empty(), "{} has no title", command.id);
        assert!(
            command.id.contains('.'),
            "{} is not namespaced (expected e.g. pages.rotate)",
            command.id
        );
    }
}

#[test]
fn every_command_id_is_unique() {
    let registry = build_registry();
    let mut ids: Vec<&str> = registry.commands().iter().map(|c| c.id).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate command ids");
}

/// Every tool runs its whole lifecycle against a document with nothing in
/// it. A registry entry that only works on a page with content is a crash
/// waiting for the first empty or zero-page file.
#[test]
fn every_tool_survives_a_degenerate_document() {
    for bytes in [
        std::fs::read(seed("minimal.pdf")).expect("seed is readable"),
        no_pages(),
    ] {
        let mut document = Document::open_bytes(bytes).expect("the document opens");
        let mut viewport = Viewport::new(
            document.page_count(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
            12.0,
        )
        .expect("the viewport is valid");
        for page in 0..document.page_count() {
            let geometry = document
                .page_geometry(page)
                .expect("the page measures")
                .clone();
            viewport
                .measure_page(geometry)
                .expect("the page is measurable");
        }

        let mut registry = build_registry();
        for index in 0..registry.tools().count() {
            let tool = registry.tool_mut(index).expect("the tool stays registered");
            let mut ctx = ToolCtx {
                doc: &mut document,
                viewport: &mut viewport,
            };
            tool.on_activate(&mut ctx);
            for at in [(20.0, 20.0), (60.0, 50.0), (60.0, 50.0)] {
                let input = PointerInput {
                    at: PagePoint {
                        page: 0,
                        x: at.0,
                        y: at.1,
                    },
                    pressure: 1.0,
                    modifiers: Modifiers::default(),
                };
                tool.on_pointer_down(&mut ctx, input);
                tool.on_pointer_move(&mut ctx, input);
                tool.on_pointer_up(&mut ctx, input);
            }
            tool.on_commit(&mut ctx);
            tool.on_cancel(&mut ctx);
            tool.on_deactivate(&mut ctx);
            tool.overlays(ctx.doc);
        }
    }
}

fn seed(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/seeds")
        .join(name)
}

/// A structurally valid document whose page tree is empty. The corpus has
/// no seed for it because no producer writes one on purpose.
fn no_pages() -> Vec<u8> {
    let objects: [&[u8]; 2] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [] /Count 0 >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 3\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Every edit is undoable: applying a tool gesture or command to a
/// document and then undoing it is the identity on the edit graph.
#[test]
#[ignore = "needs core's edit graph and history, M3"]
fn every_edit_is_undoable() {
    unimplemented!("needs Document, the edit graph and undo")
}

/// Every edit serializes: what a tool or command produces saves to an
/// incremental section that a fresh parse of the result accepts.
#[test]
#[ignore = "needs the cos incremental writer, M3"]
fn every_edit_serializes_into_an_acceptable_incremental_section() {
    unimplemented!("needs cos save and re-parse")
}

/// Every edit is deterministic: the same gesture against the same
/// document twice produces the same edit, or previews would disagree with
/// what gets committed.
#[test]
#[ignore = "needs Document and a corpus fixture to replay against, M3"]
fn every_edit_is_deterministic() {
    unimplemented!("needs Document and a fixture to replay")
}
