//! Common codecs: creating a PDF from images, and PNG or SVG page
//! export. PDF is itself the interchange format, so the codec surface
//! stays this small.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct CommonCodecsPlugin;

impl PluginManifest for CommonCodecsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.codecs-common"
    }

    fn name(&self) -> &'static str {
        "Common Codecs"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
