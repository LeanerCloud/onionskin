//! Application assembly: builds the plugin registry from the first-party
//! set and reports what it holds. The GPUI shell - window, canvas,
//! thumbnails, panels, keymap, and the translation between GPUI events
//! and `plugin-api` types - lands with M1; until then this crate is
//! windowless, so the assembled registry is testable on every platform.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

/// Assemble the first-party plugin set. Every entry sits behind its own
/// cargo feature: with `--no-default-features` this returns an empty
/// registry and the app still boots, to a workspace that can do nothing.
pub fn build_registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    for manifest in first_party_manifests() {
        registry.install(manifest.as_ref());
    }
    registry
}

/// The first-party plugins in Acrobat's tool-rail order, with the two
/// that are not toolsets last.
fn first_party_manifests() -> Vec<Box<dyn PluginManifest>> {
    vec![
        #[cfg(feature = "tools-basic")]
        Box::new(onionskin_tools_basic::BasicToolsPlugin),
        #[cfg(feature = "tools-comment")]
        Box::new(onionskin_tools_comment::CommentToolsPlugin),
        #[cfg(feature = "tools-edit")]
        Box::new(onionskin_tools_edit::EditToolsPlugin),
        #[cfg(feature = "tools-organize")]
        Box::new(onionskin_tools_organize::OrganizeToolsPlugin),
        #[cfg(feature = "tools-fill-sign")]
        Box::new(onionskin_tools_fill_sign::FillSignToolsPlugin),
        #[cfg(feature = "tools-form")]
        Box::new(onionskin_tools_form::FormToolsPlugin),
        #[cfg(feature = "redact")]
        Box::new(onionskin_redact::RedactPlugin),
        #[cfg(feature = "tools-protect")]
        Box::new(onionskin_tools_protect::ProtectToolsPlugin),
        #[cfg(feature = "tools-accessibility")]
        Box::new(onionskin_tools_accessibility::AccessibilityToolsPlugin),
        #[cfg(feature = "tools-measure")]
        Box::new(onionskin_tools_measure::MeasureToolsPlugin),
        #[cfg(feature = "commands-core")]
        Box::new(onionskin_commands_core::CoreCommandsPlugin),
        #[cfg(feature = "codecs-common")]
        Box::new(onionskin_codecs_common::CommonCodecsPlugin),
    ]
}

/// One-line summary of what an assembled registry holds.
pub fn boot_summary(registry: &PluginRegistry) -> String {
    format!(
        "onionskin: {} plugins, {} tools, {} commands",
        registry.plugins().len(),
        registry.tools().count(),
        registry.commands().len()
    )
}
