//! Acrobat's Comment toolset: highlight, underline, strikeout, sticky
//! note, ink with stylus pressure, text box, stamps and
//! attach-as-comment. Every one of them is an annotation appended in an
//! incremental section, so commenting never rewrites the original.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct CommentToolsPlugin;

impl PluginManifest for CommentToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-comment"
    }

    fn name(&self) -> &'static str {
        "Comment"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
