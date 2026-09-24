//! Kernel: the shared, GPUI-free document session.
//!
//! M2's viewer session owns the original bytes, the repairing COS document,
//! provenance, bounded page caches, selection, and find state. Edit graph,
//! history, save, and shell integration land in later milestones.

mod annots;
mod attachment_search;
mod attachments;
mod autoscroll;
mod edit;
pub mod embedded;
mod file;
pub mod forms;
mod generations;
mod history;
pub mod image_edit;
pub mod images;
mod layers;
mod layout;
pub mod links;
pub mod metadata;
mod outline;
mod page;
pub mod pages;
mod preview;
pub mod protection;
mod recovery;
pub mod redactions;
mod render;
mod save;
mod search;
mod selection;
mod session;
mod signatures;
mod structure;
#[cfg(test)]
mod testpdf;
pub mod text_edit;
pub mod textselect;
mod viewport;

pub use annots::{
    add_annotation, pdf_date, read_annotations, remove_annotation, set_ink_strokes, Annotation,
    AnnotationFilter, BaseFont, BorderEffect, Color, Flags, Intent, LineEnding, Quad,
    ReadAnnotation, Rect, StampArt, Subtype, TextStyle,
};
pub use annots::{properties, review};
pub use attachment_search::{
    AttachmentHit, AttachmentSearch, ATTACHMENT_SEARCH_DEPTH, MAX_ATTACHMENT_HITS,
};
pub use attachments::Attachment;
pub use autoscroll::{
    AutoScroll, AUTO_SCROLL_MAX_STEP, AUTO_SCROLL_RESUME_AFTER, AUTO_SCROLL_SPEEDS,
    DEFAULT_AUTO_SCROLL_LEVEL,
};
pub use edit::{
    Change, DocumentEdit, EditSession, Entry, History, ObjectState, Overlay, TrailerState,
    Transaction, MAX_HISTORY_BYTES,
};
pub use file::DocumentFile;
pub use generations::{Generation, GenerationDetail, RevertRefusal};
pub use history::{ViewHistory, ViewState};
pub use layers::Layer;
pub use layout::{
    LayoutError, PageAlignment, PageLayoutMode, PagePlacement, PageRenderRect, ViewPoint, ViewRect,
    ViewRotation, ViewSize,
};
pub use onionskin_content::placements::ImagePlacement;
pub use onionskin_content::{
    Glyph, LineGlyph, Mapping, MatchMode, PageText, SearchOptions, TextLine, TextRun,
};
/// Re-exported because a [`Layer`] is named by the object its dictionary
/// lives in, and the shell has to be able to name one back.
pub use onionskin_cos::{ObjRef, Provenance};
/// Re-exported because [`Document::render_page_now`] and
/// [`Document::page_svg`] hand these back: a caller outside `core` has to be
/// able to name what it received.
pub use onionskin_render::{BaseRaster, PageRender, PageSvg};
pub use outline::write::{
    add_bookmark, delete_bookmark, move_bookmark, rename_bookmark, set_bookmark_destination,
};
pub use outline::OutlineItem;
pub use page::{
    DeviceQuad, GeometryError, Modifiers, PageGeometry, PageIndex, PagePoint, PageQuad, PageRect,
};
pub use recovery::{Recovered, RecoveryError, RecoveryStore};
pub use render::{
    PagePlaceholder, RenderRequest, RenderResponse, ThumbnailRequest, ThumbnailResponse,
    WorkerError,
};
pub use save::{SaveOutcome, WrittenButNotReloaded};
pub use search::{PageFailure, SearchMatch, SearchState, SearchWorkerError};
pub use selection::{ImageSelection, Selection, TextSelection, TextSpan};
pub use session::{
    Document, Error, ExportSnapshot, FieldRequest, LinkRequest, PageGeometryResponse, RenderView,
    Result, SnapshotRequest,
};
pub use signatures::SignatureField;
pub use structure::{
    attach_annotation, check, read_structure, remove_page, remove_pages, reorder_pages, Element,
    Kid, Maintenance, ParentEntry, Report, Structure, StructureTree, Violation,
};
pub use viewport::{FitMode, Viewport, ViewportError, ZoomPolicy, MAX_ZOOM, MIN_ZOOM};
