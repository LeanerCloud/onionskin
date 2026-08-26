//! Core menu commands: Combine files, compress and flatten export,
//! document properties, generation rollback (truncating back to an
//! earlier skin), and print through `crates/print`.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct CoreCommandsPlugin;

impl PluginManifest for CoreCommandsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.commands-core"
    }

    fn name(&self) -> &'static str {
        "Core Commands"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
