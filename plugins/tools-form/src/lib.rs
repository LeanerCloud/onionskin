//! Acrobat's Prepare Form toolset: AcroForm fields and their appearance
//! streams, with calculation, validation and formatting driven by
//! `scripting` so real-world forms compute the way Acrobat computes them.
//!
//! - [`fill`]: committing a value, toggling a check box, Clear Form.
//! - [`replay`]: a session recorded in Acrobat made again and compared,
//!   guarantee test 7's harness.

pub mod fill;
pub mod replay;

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct FormToolsPlugin;

impl PluginManifest for FormToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-form"
    }

    fn name(&self) -> &'static str {
        "Prepare Form"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
