//! Common codecs: page export to text, PNG, SVG, JPEG and TIFF; PDFs made
//! from PNG, JPEG and TIFF images; and every image out of a document. PDF is
//! itself the interchange format, so the codec surface stays this small.
//!
//! JPEG 2000 is not here. Its essential patents have expired, so the format
//! itself is not the obstacle; the encoder is. There is no pure-Rust JPEG 2000
//! encoder of usable quality, and the working ones (OpenJPEG, Kakadu) are C
//! libraries this project does not link. The row ships `partial` with that
//! reason rather than with a C dependency.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

mod images;
mod import;
mod jpeg;
mod png;
mod raster;
mod svg;
mod text;
mod tiff;

pub use images::{extract_images, ExtractedImage, Extraction, SkippedImage};
pub use jpeg::JpegCodec;
pub use png::PngCodec;
pub use svg::SvgCodec;
pub use text::TextCodec;
pub use tiff::TiffCodec;

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
        registry.register_codec(Box::new(JpegCodec::default()));
        registry.register_codec(Box::new(TiffCodec));
    }
}
