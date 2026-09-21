//! Acrobat's Comment toolset: highlight, underline, strikeout, sticky
//! note, ink with stylus pressure, text box, stamps and
//! attach-as-comment. Every one of them is an annotation appended in an
//! incremental section, so commenting never rewrites the original.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

mod freetext;
mod markup;
mod note;
mod place;
mod quads;

pub use freetext::FreeTextTool;
pub use markup::MarkupTool;
pub use note::NoteTool;

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
    }
}
