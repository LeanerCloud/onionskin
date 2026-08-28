//! Kernel: the shared, GPUI-free document session.
//!
//! M2's viewer session owns the original bytes, the repairing COS document,
//! provenance, bounded page caches, selection, and find state. Edit graph,
//! history, save, and shell integration land in later milestones.

mod page;
mod render;
mod selection;
mod session;

pub use onionskin_content::SearchOptions;
pub use onionskin_cos::Provenance;
pub use page::{
    DeviceQuad, GeometryError, Modifiers, PageGeometry, PageIndex, PagePoint, PageQuad, PageRect,
};
pub use render::{PagePlaceholder, RenderRequest, RenderResponse, WorkerError};
pub use selection::{SearchMatch, SearchState, Selection};
pub use session::{Document, Error, Result};
