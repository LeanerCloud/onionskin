//! Kernel: the shared, GPUI-free document session.
//!
//! M2's viewer session owns the original bytes, the repairing COS document,
//! provenance, bounded page caches, selection, and find state. Edit graph,
//! history, save, and shell integration land in later milestones.

mod history;
mod layout;
mod page;
mod render;
mod search;
mod selection;
mod session;
mod viewport;

pub use history::{ViewHistory, ViewState};
pub use layout::{
    LayoutError, PageAlignment, PageLayoutMode, PagePlacement, PageRenderRect, ViewPoint, ViewRect,
    ViewRotation, ViewSize,
};
pub use onionskin_content::{Glyph, Mapping, MatchMode, PageText, SearchOptions, TextRun};
pub use onionskin_cos::Provenance;
/// Re-exported because [`Document::render_page_now`] and
/// [`Document::page_svg`] hand these back: a caller outside `core` has to be
/// able to name what it received.
pub use onionskin_render::{BaseRaster, PageRender, PageSvg};
pub use page::{
    DeviceQuad, GeometryError, Modifiers, PageGeometry, PageIndex, PagePoint, PageQuad, PageRect,
};
pub use render::{PagePlaceholder, RenderRequest, RenderResponse, WorkerError};
pub use search::{PageFailure, SearchMatch, SearchState, SearchWorkerError};
pub use selection::{Selection, TextSelection};
pub use session::{Document, Error, PageGeometryResponse, Result, SnapshotRequest};
pub use viewport::{FitMode, Viewport, ViewportError, ZoomPolicy};
