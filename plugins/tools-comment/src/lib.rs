//! Acrobat's Comment toolset: highlight, underline, strikeout, sticky
//! note, ink with stylus pressure, text box, stamps and
//! attach-as-comment. Every one of them is an annotation appended in an
//! incremental section, so commenting never rewrites the original.

use onionskin_plugin_api::command_ids::SUMMARIZE_COMMENTS;
use onionskin_plugin_api::{
    Command, CommandCtx, CommandEffect, CommandError, CommandPlugin, PluginManifest, PluginRegistry,
};

mod attach;
mod freetext;
mod ink;
mod markup;
mod note;
mod place;
mod quads;
mod shapes;
mod stamp;
mod summary;
mod text;

pub use attach::AttachFileTool;
pub use freetext::FreeTextTool;
pub use ink::{EraseInkTool, InkTool};
pub use markup::MarkupTool;
pub use note::NoteTool;
pub use shapes::ShapeTool;
pub use stamp::{library_in, Clock, CustomStamp, LibraryError, StampLibrary, StampTool};
pub use summary::{summarize, Summary, SummaryError, SummaryLayout};

pub struct CommentToolsPlugin;

impl PluginManifest for CommentToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-comment"
    }

    fn name(&self) -> &'static str {
        "Comment"
    }

    /// Registration order is rail order within the group, and the first is
    /// the one the shared slot shows before anything has been used.
    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_tool(Box::new(MarkupTool::highlight()));
        registry.register_tool(Box::new(MarkupTool::underline()));
        registry.register_tool(Box::new(MarkupTool::strikethrough()));
        registry.register_tool(Box::new(MarkupTool::insert_text()));
        registry.register_tool(Box::new(MarkupTool::replace_text()));
        registry.register_tool(Box::new(NoteTool::new()));
        registry.register_tool(Box::new(FreeTextTool::typewriter()));
        registry.register_tool(Box::new(FreeTextTool::text_box()));
        registry.register_tool(Box::new(FreeTextTool::callout()));
        registry.register_tool(Box::new(ShapeTool::line()));
        registry.register_tool(Box::new(ShapeTool::arrow()));
        registry.register_tool(Box::new(ShapeTool::rectangle()));
        registry.register_tool(Box::new(ShapeTool::oval()));
        registry.register_tool(Box::new(ShapeTool::polygon()));
        registry.register_tool(Box::new(ShapeTool::connected_lines()));
        registry.register_tool(Box::new(ShapeTool::cloud()));
        registry.register_tool(Box::new(InkTool::new()));
        registry.register_tool(Box::new(EraseInkTool::new()));
        registry.register_tool(Box::new(StampTool::new()));
        registry.register_tool(Box::new(AttachFileTool::new()));
        registry.register_commands(self);
    }
}

impl CommandPlugin for CommentToolsPlugin {
    fn commands(&self) -> Vec<Command> {
        vec![Command {
            id: SUMMARIZE_COMMENTS,
            title: "Summarize Comments",
            keybind: None,
            // Copies the document's content into another file: refused, like
            // extract and split, where the encrypted-source rule protects it.
            effect: CommandEffect::ReadsOut,
            run: Box::new(summarize_beside),
        }]
    }
}

/// The summary that needs no dialog: comments only, beside the document,
/// named after it. Never over an existing file.
fn summarize_beside(ctx: &mut CommandCtx) -> Result<(), CommandError> {
    const LABEL: &str = "Summarize Comments";
    let failed = |reason: String| CommandError::Failed {
        label: LABEL,
        reason,
    };
    let path = ctx
        .doc
        .path()
        .ok_or_else(|| failed("the document has not been saved to a file yet".to_owned()))?
        .to_path_buf();
    let summary = summarize(ctx.doc, SummaryLayout::CommentsOnly)
        .map_err(|error| failed(error.to_string()))?;
    let output = summary_path(&path);
    write_new(&output, &summary.bytes)
        .map_err(|error| failed(format!("{} was not written: {error}", output.display())))
}

/// `report.pdf` summarizes into `report - Comments.pdf`, beside it.
pub fn summary_path(document: &std::path::Path) -> std::path::PathBuf {
    let stem = document
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    document.with_file_name(format!("{stem} - Comments.pdf"))
}

fn write_new(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)
}
