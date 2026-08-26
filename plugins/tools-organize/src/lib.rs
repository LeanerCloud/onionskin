//! Acrobat's Organize Pages toolset: rotate, reorder, insert, delete,
//! extract and split. Page-tree edits, which the incremental model
//! expresses as a rewritten page tree appended over the original.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct OrganizeToolsPlugin;

impl PluginManifest for OrganizeToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-organize"
    }

    fn name(&self) -> &'static str {
        "Organize Pages"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
