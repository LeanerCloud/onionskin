//! Acrobat's Fill & Sign toolset: filling flat and interactive forms and
//! placing a drawn or stored signature, without needing the document to
//! carry form fields.
//!
//! - **Add Text** types on the page wherever it is clicked.
//! - **Checkmark, Cross, Dot** put a small mark where they are clicked.
//! - **Circle, Line** draw one, dragged or at a default size.
//! - **Sign** places the signature or the initials kept in the signature
//!   library, which the shell's Add Signature dialog fills from typed text,
//!   a drawing or an image.
//!
//! Everything is an annotation, as Acrobat writes it, so it undoes, moves
//! and deletes like a comment. A signature here is an appearance, not a
//! cryptographic signature: that is `tools-protect`'s.

mod gesture;
mod shapes;
mod sign_tool;
pub mod signature;
mod symbols;
mod text;

use onionskin_core::{add_annotation, Annotation, Document, ObjRef, PageIndex};
use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub use shapes::ShapeTool;
pub use sign_tool::SignTool;
pub use signature::{SignatureKind, SignatureLibrary};
pub use symbols::{Symbol, SymbolTool};
pub use text::FillTextTool;

/// The rail group every Fill & Sign tool shares.
pub(crate) const GROUP: &str = "fill-sign";

pub struct FillSignToolsPlugin;

impl PluginManifest for FillSignToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-fill-sign"
    }

    fn name(&self) -> &'static str {
        "Fill & Sign"
    }

    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_tool(Box::new(FillTextTool::new()));
        for symbol in Symbol::ALL {
            registry.register_tool(Box::new(SymbolTool::new(symbol)));
        }
        registry.register_tool(Box::new(ShapeTool::circle()));
        registry.register_tool(Box::new(ShapeTool::line()));
        registry.register_tool(Box::new(SignTool::new()));
    }
}

/// The page's own object, which an annotation is written onto.
fn page_object(document: &mut Document, page: PageIndex) -> Option<ObjRef> {
    document.structure().ok()?.page(page).ok().map(|p| p.objref)
}

/// Put `annotation` on `page` as one undo step named `label`.
pub(crate) fn place(
    document: &mut Document,
    page: PageIndex,
    label: &'static str,
    annotation: &Annotation,
) {
    let Some(page) = page_object(document, page) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    // A refusal - a document that may not be edited - leaves the page as it
    // was; the shell disables the tools on such a document before this.
    let _ = document.edit_annotations(label, |tx, structure| {
        add_annotation(tx, structure, page, annotation, now).map(|_| ())
    });
}
