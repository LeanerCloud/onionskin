//! Acrobat's Measure toolset: distance, perimeter and area, read against
//! the document's scale.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct MeasureToolsPlugin;

impl PluginManifest for MeasureToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-measure"
    }

    fn name(&self) -> &'static str {
        "Measure"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
