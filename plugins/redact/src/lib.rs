//! Redaction: content-stream rewriting, image region scrub and metadata
//! scrub. The one destructive path in the product, saved as a flattening
//! rewrite rather than an incremental section, because redaction under
//! incremental update would be a lie. Ships with a verifier that
//! re-extracts text and images from the output and proves the target is
//! gone; the verifier is part of the feature, not the test suite.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub struct RedactPlugin;

impl PluginManifest for RedactPlugin {
    fn id(&self) -> &'static str {
        "onionskin.redact"
    }

    fn name(&self) -> &'static str {
        "Redact"
    }

    fn register(&self, _registry: &mut PluginRegistry) {}
}
