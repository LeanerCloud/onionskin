//! Acrobat's Protect toolset: passwords and permissions, plus the
//! signature UX, including platform keystores (Keychain, CNG, PKCS#11)
//! for signing identities. Signing and counter-signing stay legal
//! because a save appends rather than rewrites.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct ProtectToolsPlugin;

impl PluginManifest for ProtectToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-protect"
    }

    fn name(&self) -> &'static str {
        "Protect"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
