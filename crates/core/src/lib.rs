//! Kernel: the shared, GPUI-free document session.
//!
//! M2's viewer session owns the original bytes, the repairing COS document,
//! provenance, bounded page caches, selection, and find state. Edit graph,
//! history, save, and shell integration land in later milestones.

mod attachments;
mod edit;
mod history;
mod layers;
mod layout;
mod outline;
mod page;
mod render;
mod search;
mod selection;
mod session;
mod signatures;
#[cfg(test)]
mod testpdf;
mod viewport;

pub use attachments::Attachment;
pub use edit::{
    Change, DocumentEdit, EditSession, Entry, History, ObjectState, Overlay, TrailerState,
    Transaction, MAX_HISTORY_BYTES,
};
pub use history::{ViewHistory, ViewState};
pub use layers::Layer;
pub use layout::{
    LayoutError, PageAlignment, PageLayoutMode, PagePlacement, PageRenderRect, ViewPoint, ViewRect,
    ViewRotation, ViewSize,
};
pub use onionskin_content::{Glyph, Mapping, MatchMode, PageText, SearchOptions, TextRun};
/// Re-exported because a [`Layer`] is named by the object its dictionary
/// lives in, and the shell has to be able to name one back.
pub use onionskin_cos::{ObjRef, Provenance};
/// Re-exported because [`Document::render_page_now`] and
/// [`Document::page_svg`] hand these back: a caller outside `core` has to be
/// able to name what it received.
pub use onionskin_render::{BaseRaster, PageRender, PageSvg};
pub use outline::OutlineItem;
pub use page::{
    DeviceQuad, GeometryError, Modifiers, PageGeometry, PageIndex, PagePoint, PageQuad, PageRect,
};
pub use render::{
    PagePlaceholder, RenderRequest, RenderResponse, ThumbnailRequest, ThumbnailResponse,
    WorkerError,
};
pub use search::{PageFailure, SearchMatch, SearchState, SearchWorkerError};
pub use selection::{Selection, TextSelection};
pub use session::{Document, Error, ExportSnapshot, PageGeometryResponse, Result, SnapshotRequest};
pub use signatures::SignatureField;
pub use viewport::{FitMode, Viewport, ViewportError, ZoomPolicy, MAX_ZOOM, MIN_ZOOM};
