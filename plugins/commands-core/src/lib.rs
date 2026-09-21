//! Core menu commands: the ones that act on the open document rather than on
//! the shell around it.
//!
//! M2 registered the two Edit commands. M3's P12 adds the commands that
//! produce new documents: [`combine`] (and Create PDF From Multiple Files,
//! which is Combine from another entry point) and [`split`], both built on
//! `core::pages::Assembly`. Compress, document properties, generation
//! rollback and print land with the packages that own their subsystems.
//!
//! Registering here rather than in `app` is what makes the shell's command
//! surface a query: the palette lists whatever the registry holds, the Edit
//! menu asks for these ids by name, and a build without this plugin says the
//! plugin is missing instead of showing an entry that would do nothing.

use onionskin_plugin_api::command_ids::{REDUCE_FILE_SIZE, SPLIT_DOCUMENT};
use onionskin_plugin_api::{
    Command, CommandCtx, CommandEffect, CommandError, CommandPlugin, PluginManifest,
    PluginRegistry, TextSelection,
};

pub mod combine;
pub mod compress;
pub mod publish;
pub mod split;

pub const SELECT_ALL: &str = "edit.select-all";
pub const DESELECT_ALL: &str = "edit.deselect-all";

pub struct CoreCommandsPlugin;

impl PluginManifest for CoreCommandsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.commands-core"
    }

    fn name(&self) -> &'static str {
        "Core Commands"
    }

    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_commands(self);
    }
}

impl CommandPlugin for CoreCommandsPlugin {
    fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                id: SELECT_ALL,
                title: "Select All",
                keybind: Some("cmd-a"),
                effect: CommandEffect::Reads,
                run: Box::new(select_all),
            },
            Command {
                id: DESELECT_ALL,
                title: "Deselect All",
                keybind: Some("cmd-shift-a"),
                effect: CommandEffect::Reads,
                run: Box::new(deselect_all),
            },
            Command {
                id: SPLIT_DOCUMENT,
                title: "Split Document at Top-Level Bookmarks",
                keybind: None,
                // Copies the document's objects into new files: refused, like
                // extract, on a document the encrypted-source rule protects.
                effect: CommandEffect::ReadsOut,
                run: Box::new(split_at_bookmarks),
            },
            Command {
                id: REDUCE_FILE_SIZE,
                title: "Reduce File Size",
                keybind: None,
                // Rewrites the document's objects into a new file: refused on
                // a document the encrypted-source rule protects.
                effect: CommandEffect::ReadsOut,
                run: Box::new(reduce_beside),
            },
        ]
    }
}

/// Acrobat's Edit > Select All, over the page the viewport is on.
///
/// One page, not the whole document: a `TextSelection` names a page because
/// `content` extracts one page at a time and the quads the selection draws
/// are in that page's user space. A page with no text selects nothing, which
/// also drops whatever region was selected before, exactly as selecting text
/// with the pointer does.
fn select_all(ctx: &mut CommandCtx) -> Result<(), CommandError> {
    let page = ctx.page;
    let text = ctx
        .doc
        .page_text(page)
        .map_err(|source| CommandError::Page { page, source })?;
    // Flattened rather than joined here, so the rules that decide where a
    // space or a line break goes stay in `content`.
    let (flat, spans) = onionskin_core::textselect::styled_text(text);
    let selection = TextSelection {
        page: text.page,
        quads: text
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect(),
        text: flat,
        spans,
    };
    ctx.doc.selection_mut().set_text(selection);
    Ok(())
}

fn deselect_all(ctx: &mut CommandCtx) -> Result<(), CommandError> {
    ctx.doc.selection_mut().clear();
    Ok(())
}

/// The split that needs no dialog: at the top-level bookmarks, into files
/// beside the document, named after it. The dialog offers the others.
fn split_at_bookmarks(ctx: &mut CommandCtx) -> Result<(), CommandError> {
    const LABEL: &str = "Split Document";
    let failed = |reason: String| CommandError::Failed {
        label: LABEL,
        reason,
    };
    let path = ctx
        .doc
        .path()
        .ok_or_else(|| failed("the document has not been saved to a file yet".to_owned()))?
        .to_path_buf();
    let folder = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = path
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    split::split(ctx.doc, split::SplitBy::TopLevelBookmarks, folder, &stem)
        .map(|_| ())
        .map_err(|error| failed(error.to_string()))
}

/// Reduce File Size with no dialog: a smaller copy beside the document,
/// named after it. The dialog asks where instead.
fn reduce_beside(ctx: &mut CommandCtx) -> Result<(), CommandError> {
    const LABEL: &str = "Reduce File Size";
    let failed = |reason: String| CommandError::Failed {
        label: LABEL,
        reason,
    };
    let path = ctx
        .doc
        .path()
        .ok_or_else(|| failed("the document has not been saved to a file yet".to_owned()))?;
    let destination = compress::reduced_path(path);
    match compress::compress(ctx.doc, &compress::CompressOptions::default())
        .map_err(|error| failed(error.to_string()))?
    {
        compress::Compressed::Nothing => Err(failed(compress::NOTHING_TO_COMPRESS.to_owned())),
        compress::Compressed::Smaller { bytes, .. } => {
            publish::publish(&[(destination, bytes)]).map_err(|error| failed(error.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use onionskin_plugin_api::{Document, PageRect};

    use super::*;

    fn seed(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(name)
    }

    fn command(id: &str) -> Command {
        CoreCommandsPlugin
            .commands()
            .into_iter()
            .find(|command| command.id == id)
            .unwrap_or_else(|| panic!("{id} is registered"))
    }

    fn run(id: &str, doc: &mut Document, page: usize) -> Result<(), CommandError> {
        (command(id).run)(&mut CommandCtx { doc, page })
    }

    /// The page the viewport is on, not page zero: a two-page seed is the
    /// only shape that tells the two apart.
    #[test]
    fn select_all_selects_the_text_of_the_page_the_viewport_is_on() {
        let mut doc = Document::open_path(&seed("two-page.pdf")).expect("the seed opens");
        let second = doc.page_text(1).expect("page two has text").flatten().text;
        let first = doc.page_text(0).expect("page one has text").flatten().text;
        let glyphs: usize = doc
            .page_text(1)
            .unwrap()
            .runs
            .iter()
            .map(|r| r.glyphs.len())
            .sum();
        assert!(!second.is_empty());
        assert_ne!(first, second, "the seed's two pages must differ");

        run(SELECT_ALL, &mut doc, 1).expect("page two selects");

        let selection = doc.selection().text().expect("text is selected");
        assert_eq!(selection.page, 1);
        assert_eq!(selection.text, second);
        assert_eq!(selection.quads.len(), glyphs);
        assert!(selection.quads.iter().all(|quad| quad.page == 1));
    }

    #[test]
    fn deselect_all_drops_whatever_was_selected() {
        let mut doc = Document::open_path(&seed("hello.pdf")).expect("the seed opens");
        run(SELECT_ALL, &mut doc, 0).expect("page one selects");
        assert!(doc.selection().text().is_some());

        run(DESELECT_ALL, &mut doc, 0).expect("deselecting cannot fail");

        assert!(doc.selection().text().is_none());
        assert!(doc.selection().region().is_none());
    }

    /// Selecting a page the document does not have reports that page rather
    /// than quietly leaving the old selection in place, which is the whole
    /// reason `run` hands back a `Result`.
    #[test]
    fn select_all_on_a_page_the_document_does_not_have_reports_it() {
        let mut doc = Document::open_path(&seed("hello.pdf")).expect("the seed opens");
        let region = PageRect {
            page: 0,
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        };
        doc.selection_mut().set_region(region);

        let error = run(SELECT_ALL, &mut doc, 7).expect_err("page eight does not exist");

        assert!(
            matches!(error, CommandError::Page { page: 7, .. }),
            "{error:?}"
        );
        assert_eq!(
            doc.selection().region(),
            Some(region),
            "a failed Select All leaves the selection alone"
        );
    }

    /// Both ids are namespaced and both carry Acrobat's default keystroke,
    /// which is what the app's keymap resolves against.
    #[test]
    fn every_command_is_named_bound_and_declares_its_effect() {
        let commands = CoreCommandsPlugin.commands();

        assert_eq!(
            commands
                .iter()
                .map(|command| (command.id, command.title, command.keybind, command.effect))
                .collect::<Vec<_>>(),
            vec![
                (
                    SELECT_ALL,
                    "Select All",
                    Some("cmd-a"),
                    CommandEffect::Reads
                ),
                (
                    DESELECT_ALL,
                    "Deselect All",
                    Some("cmd-shift-a"),
                    CommandEffect::Reads
                ),
                (
                    SPLIT_DOCUMENT,
                    "Split Document at Top-Level Bookmarks",
                    None,
                    CommandEffect::ReadsOut
                ),
                (
                    REDUCE_FILE_SIZE,
                    "Reduce File Size",
                    None,
                    CommandEffect::ReadsOut
                ),
            ]
        );
    }

    #[test]
    fn the_manifest_registers_every_command_into_a_registry() {
        let mut registry = PluginRegistry::new();
        registry.install(&CoreCommandsPlugin);

        assert_eq!(
            registry
                .commands()
                .iter()
                .map(|command| command.id)
                .collect::<Vec<_>>(),
            vec![SELECT_ALL, DESELECT_ALL, SPLIT_DOCUMENT, REDUCE_FILE_SIZE]
        );
    }
}
