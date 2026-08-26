//! Acrobat's Edit PDF toolset: line-level text editing backed by
//! `text-engine` font matching, image replace and transform, links,
//! headers and footers, watermarks, Bates numbering and crop. Reflowing
//! text edit is out of scope pre-1.0.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct EditToolsPlugin;

impl PluginManifest for EditToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-edit"
    }

    fn name(&self) -> &'static str {
        "Edit PDF"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
