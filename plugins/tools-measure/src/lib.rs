//! Acrobat's Measure toolset: distance, perimeter and area, read against
//! the document's scale, snapped to the page's line art, and kept on the
//! page as measurement comments.

use onionskin_plugin_api::{PluginManifest, PluginRegistry};

mod settings;
mod snap;
mod tool;

pub use settings::{Settings, Shared, SCALES};
pub use snap::{SnapKind, SnapOptions, Snapper};
pub use tool::MeasureTool;

pub struct MeasureToolsPlugin;

impl PluginManifest for MeasureToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-measure"
    }

    fn name(&self) -> &'static str {
        "Measure"
    }

    /// The three tools share one set of settings, so a scale chosen for one
    /// is the scale of all three.
    fn register(&self, registry: &mut PluginRegistry) {
        let settings = Shared::default();
        registry.register_tool(Box::new(MeasureTool::distance(settings.clone())));
        registry.register_tool(Box::new(MeasureTool::perimeter(settings.clone())));
        registry.register_tool(Box::new(MeasureTool::area(settings)));
    }
}

#[cfg(test)]
pub(crate) mod testing {
    /// A PDF from its objects, numbered from 1, the first the catalog.
    pub(crate) fn pdf(objects: &[String]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
        }
        let xref = out.len();
        let size = objects.len() + 1;
        out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n")
                .as_bytes(),
        );
        out
    }
}
