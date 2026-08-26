//! Acrobat's Fill & Sign toolset: filling flat and interactive forms and
//! placing a drawn or stored signature, without needing the document to
//! carry form fields.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct FillSignToolsPlugin;

impl PluginManifest for FillSignToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-fill-sign"
    }

    fn name(&self) -> &'static str {
        "Fill & Sign"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
