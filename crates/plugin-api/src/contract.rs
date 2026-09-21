//! The contract every registered plugin inherits by being registered.
//!
//! This is the checker; the exhaustive run over the real registry lives in the
//! app's tests, because `build_registry` is the app's and `plugin-api` cannot
//! depend on it. Keeping the rules here means a plugin crate can check itself
//! without the whole app, and means there is one statement of the contract
//! rather than one per test that happens to look for it.
//!
//! **Why exhaustive rather than per-plugin.** A rule written as a test beside
//! one tool is a rule the next tool does not have. Running the same rules over
//! everything the registry holds is what makes "a new plugin inherits the
//! contract" true rather than aspirational.
//!
//! The behavioural half drives each tool's **real gesture lifecycle**
//! (`on_activate`, pointer down/move/up, `on_commit`) rather than a synthetic
//! `DocumentEdit`. A synthetic edit would prove the edit graph works, which is
//! P2's job; only a real gesture proves anything about the tool.

use std::collections::BTreeMap;

use onionskin_core::{Document, PageIndex, PagePoint, Viewport};
use onionskin_cos::PendingEdit;

use crate::{Modifiers, PluginRegistry, PointerInput, ToolCtx};

/// One way a plugin failed the contract. Every variant names the plugin, so a
/// failure says which one rather than that something somewhere is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    /// A tool with no id cannot be selected, keymapped or reported.
    EmptyToolField {
        id: String,
        field: &'static str,
    },
    /// A tool the user can never reach: not on the rail and with no shortcut.
    ToolHasNoHome {
        id: String,
    },
    /// Two tools, or two commands, answering to one id.
    DuplicateId {
        id: String,
        kind: &'static str,
    },
    EmptyCommandField {
        id: String,
        field: &'static str,
    },
    /// A gesture left the document changed after its edits were undone.
    ///
    /// The comparison is on the overlay, not on saved bytes: bytes go through
    /// a serializer that can normalize a difference away, and this is a
    /// statement about the edit graph.
    EditNotUndoable {
        id: String,
    },
    /// A gesture produced a section that will not reopen, or that leaves
    /// dangling references.
    EditDoesNotSerialize {
        id: String,
        detail: String,
    },
    /// The same gesture twice produced two different results.
    NotDeterministic {
        id: String,
    },
    /// A tool panicked or errored on a document it should merely find boring.
    FailsOnDegenerateDocument {
        id: String,
        document: &'static str,
    },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::EmptyToolField { id, field } => {
                write!(f, "tool {id:?} has an empty {field}")
            }
            Violation::ToolHasNoHome { id } => write!(
                f,
                "tool {id:?} is not on the rail and has no shortcut, so nothing can reach it"
            ),
            Violation::DuplicateId { id, kind } => write!(f, "two {kind}s answer to {id:?}"),
            Violation::EmptyCommandField { id, field } => {
                write!(f, "command {id:?} has an empty {field}")
            }
            Violation::EditNotUndoable { id } => write!(
                f,
                "tool {id:?} left the overlay changed after its edits were undone"
            ),
            Violation::EditDoesNotSerialize { id, detail } => {
                write!(f, "tool {id:?}'s edit does not serialize: {detail}")
            }
            Violation::NotDeterministic { id } => {
                write!(
                    f,
                    "tool {id:?} produced two different results from one gesture"
                )
            }
            Violation::FailsOnDegenerateDocument { id, document } => {
                write!(f, "tool {id:?} failed on {document}")
            }
        }
    }
}

/// The metadata half: what every tool and command must declare.
///
/// Cheap, needs no document, and catches the failure a reviewer actually sees:
/// a tool added without a group, which silently takes its own rail slot.
pub fn check_metadata(registry: &PluginRegistry) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut tool_ids: BTreeMap<&str, usize> = BTreeMap::new();

    for tool in registry.tools() {
        let id = tool.id();
        *tool_ids.entry(id).or_default() += 1;
        for (field, value) in [
            ("id", id),
            ("name", tool.name()),
            ("icon", tool.icon()),
            ("group", tool.group()),
        ] {
            if value.trim().is_empty() {
                out.push(Violation::EmptyToolField {
                    id: id.to_string(),
                    field,
                });
            }
        }
        if !tool.in_rail() && tool.shortcut().is_none() {
            out.push(Violation::ToolHasNoHome { id: id.to_string() });
        }
    }
    for (id, count) in tool_ids {
        if count > 1 {
            out.push(Violation::DuplicateId {
                id: id.to_string(),
                kind: "tool",
            });
        }
    }

    let mut command_ids: BTreeMap<&str, usize> = BTreeMap::new();
    for command in registry.commands() {
        *command_ids.entry(command.id).or_default() += 1;
        for (field, value) in [("id", command.id), ("title", command.title)] {
            if value.trim().is_empty() {
                out.push(Violation::EmptyCommandField {
                    id: command.id.to_string(),
                    field,
                });
            }
        }
    }
    for (id, count) in command_ids {
        if count > 1 {
            out.push(Violation::DuplicateId {
                id: id.to_string(),
                kind: "command",
            });
        }
    }
    out
}

/// A gesture, described the way the canvas would deliver it: three points
/// across the first page, which is a drag rather than a click, because a drag
/// is what the tools that edit are driven by.
fn gesture() -> [PointerInput; 3] {
    let at = |x: f64, y: f64| PointerInput {
        at: PagePoint {
            page: PageIndex::from(0usize),
            x,
            y,
        },
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    };
    [at(20.0, 20.0), at(60.0, 40.0), at(100.0, 60.0)]
}

/// The behavioural half, over one document.
///
/// For each tool: run the real lifecycle, and if it edited, require that
/// undoing returns the overlay to exactly what it was and that what it wrote
/// serializes into a section that reopens with no dangling references.
///
/// A tool that edits nothing passes every clause trivially, which is correct:
/// the contract is about tools that do edit, and a viewer tool is not exempt
/// from it so much as unaffected by it.
pub fn check_edits(
    registry: &mut PluginRegistry,
    document: &mut Document,
    viewport: &mut Viewport,
) -> Vec<Violation> {
    let mut out = Vec::new();
    let count = registry.tools().count();
    for index in 0..count {
        let Some(id) = registry
            .tools()
            .nth(index)
            .map(|tool| tool.id().to_string())
        else {
            continue;
        };
        let before = overlay_of(document);
        let reach_before = document.edit().history().reach();

        run_gesture(registry, index, document, viewport);

        let reach_after = document.edit().history().reach();
        if reach_after == reach_before {
            continue;
        }

        if let Err(detail) = serializes(document) {
            out.push(Violation::EditDoesNotSerialize {
                id: id.clone(),
                detail,
            });
        }

        let mut steps = reach_after - reach_before;
        while steps > 0 {
            let (edit, base) = document.edit_mut();
            match edit.undo(base) {
                Ok(true) => steps -= 1,
                _ => break,
            }
        }
        if overlay_of(document) != before {
            out.push(Violation::EditNotUndoable { id });
        }
    }
    out
}

fn run_gesture(
    registry: &mut PluginRegistry,
    index: usize,
    document: &mut Document,
    viewport: &mut Viewport,
) {
    let inputs = gesture();
    let Some(tool) = registry.tool_mut(index) else {
        return;
    };
    let mut ctx = ToolCtx {
        doc: document,
        viewport,
    };
    tool.on_activate(&mut ctx);
    tool.on_pointer_down(&mut ctx, inputs[0]);
    tool.on_pointer_move(&mut ctx, inputs[1]);
    tool.on_pointer_up(&mut ctx, inputs[2]);
    tool.on_commit(&mut ctx);
    tool.on_deactivate(&mut ctx);
}

fn overlay_of(document: &Document) -> BTreeMap<u32, PendingEdit> {
    document.edit().pending_edits()
}

/// Whether what the session holds would write a section that reopens through
/// the strict parser with no dangling references.
fn serializes(document: &mut Document) -> Result<(), String> {
    let bytes = document
        .preview_bytes(onionskin_core::AnnotationFilter::DocumentAndMarkups)
        .map_err(|error| error.to_string())?;
    let reopened =
        onionskin_cos::Document::open(Box::new(onionskin_cos::BytesSource::from_shared(bytes)))
            .map_err(|error| format!("does not reopen: {error}"))?;
    let dangling = reopened
        .audit_references()
        .map_err(|error| format!("cannot be audited: {error}"))?;
    if dangling.is_empty() {
        Ok(())
    } else {
        Err(format!("{} dangling reference(s)", dangling.len()))
    }
}
