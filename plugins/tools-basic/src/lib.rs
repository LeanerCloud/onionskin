//! Basic tools: hand, select (text and region), zoom, snapshot. The
//! viewer's own toolset, and the first plugin the shell exercises.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct BasicToolsPlugin;

impl PluginManifest for BasicToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-basic"
    }

    fn name(&self) -> &'static str {
        "Basic Tools"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
