//! Common codecs: creating a PDF from images, and PNG or SVG page
//! export. PDF is itself the interchange format, so the codec surface
//! stays this small.
//!
//! M2 registers the three export formats the parity scoreboard puts in this
//! milestone: plain text, PNG and SVG. Creating a PDF from images is import,
//! which needs M3's edit graph.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

mod png;
mod svg;
mod text;

pub use png::PngCodec;
pub use svg::SvgCodec;
pub use text::TextCodec;

pub struct CommonCodecsPlugin;

impl PluginManifest for CommonCodecsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.codecs-common"
    }

    fn name(&self) -> &'static str {
        "Common Codecs"
    }

    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_codec(Box::new(TextCodec));
        registry.register_codec(Box::new(PngCodec));
        registry.register_codec(Box::new(SvgCodec));
    }
}
