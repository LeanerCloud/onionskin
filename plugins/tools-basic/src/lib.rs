//! Basic tools: hand, select (text and region), zoom, snapshot. The
//! viewer's own toolset, and the first plugin the shell exercises.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

mod hand;
mod marquee;
mod select_region;
mod select_text;
mod snapshot;
mod zoom;

pub use hand::HandTool;
pub use select_region::SelectRegionTool;
pub use select_text::SelectTextTool;
pub use snapshot::SnapshotTool;
pub use zoom::ZoomTool;

pub struct BasicToolsPlugin;

impl PluginManifest for BasicToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-basic"
    }

    fn name(&self) -> &'static str {
        "Basic Tools"
    }

    /// Registration order is rail order, and the first tool registered is
    /// the one a document opens with.
    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_tool(Box::new(HandTool::new()));
        registry.register_tool(Box::new(SelectTextTool::new()));
        registry.register_tool(Box::new(SelectRegionTool::new()));
        registry.register_tool(Box::new(ZoomTool::new()));
        registry.register_tool(Box::new(SnapshotTool::new()));
    }
}
