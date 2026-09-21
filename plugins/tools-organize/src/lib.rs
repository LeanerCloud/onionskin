//! Acrobat's Organize Pages toolset: rotate, reorder, insert, delete, extract,
//! replace and renumber.
//!
//! Every operation is `core::pages`: a new page order handed to the one
//! page-tree transformation, plus, for insert and replace, the transitive
//! importer. This crate translates what the user asked for - "this page",
//! "these pages before that one", "pages 2 to 4 of that file" - into those
//! calls, each inside [`Document::edit_pages`] so each is one undo step with
//! the session's current structure tree.
//!
//! # Two surfaces over one set of functions
//!
//! - **Commands** for what needs no more input than the page the viewport is
//!   on: rotate it, delete it, insert a blank page after it, move it, renumber
//!   the document. They are what the menus, the context menu and the keymap
//!   reach, and they declare [`CommandEffect::Edits`] so an encrypted document
//!   disables them through the shared requirement query.
//! - **Functions** taking an explicit page selection, a source document or a
//!   path: what the page grid (P21) and the insert and extract dialogs call.
//!   A source document is checked against the encrypted-source rule at
//!   execution, inside the importer, because the file the user picks after
//!   invoking the command is one a session-scoped requirement cannot see.

use onionskin_core::{Document, Structure, Transaction};
use onionskin_plugin_api::{
    Command, CommandCtx, CommandEffect, CommandError, CommandPlugin, PluginManifest, PluginRegistry,
};

mod delete;
mod extract;
mod insert;
mod labels;
mod reorder;
mod replace;
mod rotate;

pub use delete::delete_pages;
pub use extract::{extract_pages, extract_pages_to};
pub use insert::{copy_pages_between, insert_blank_pages, insert_pages_from, move_pages_between};
pub use labels::renumber_pages;
pub use reorder::move_pages;
pub use replace::replace_pages_from;
pub use rotate::{rotate_pages, Turn};

pub struct OrganizeToolsPlugin;

impl PluginManifest for OrganizeToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-organize"
    }

    fn name(&self) -> &'static str {
        "Organize Pages"
    }

    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_commands(self);
    }
}

impl CommandPlugin for OrganizeToolsPlugin {
    fn commands(&self) -> Vec<Command> {
        [
            rotate::commands(),
            delete::commands(),
            insert::commands(),
            reorder::commands(),
            labels::commands(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// A command that edits, with no keybinding: page commands in Acrobat have
/// none by default, and a keystroke here would need the window test T9 asks
/// of every bound command.
fn editing(
    id: &'static str,
    title: &'static str,
    run: impl Fn(&mut CommandCtx) -> Result<(), CommandError> + Send + 'static,
) -> Command {
    Command {
        id,
        title,
        keybind: None,
        effect: CommandEffect::Edits,
        run: Box::new(run),
    }
}

/// Run one page operation as one undo step, named `label`.
fn edit<T>(
    doc: &mut Document,
    label: &'static str,
    body: impl FnOnce(&mut Transaction<'_>, &Structure) -> onionskin_core::Result<T>,
) -> Result<T, CommandError> {
    doc.edit_pages(label, body)
        .map_err(|source| CommandError::Edit { label, source })
}
