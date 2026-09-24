//! Acrobat's Accessibility toolset: a rule-based checker, reading-order
//! view and repair, and Read Out Loud through the platform speech APIs.
//! Its checker is also what proves the tag-integrity guarantee, so the
//! feature eats its own dog food.

pub mod checker;

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct AccessibilityToolsPlugin;

impl PluginManifest for AccessibilityToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-accessibility"
    }

    fn name(&self) -> &'static str {
        "Accessibility"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
