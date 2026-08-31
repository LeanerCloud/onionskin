use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use gpui::{point, px, Modifiers as GpuiModifiers, Pixels, Point, RenderImage};
use onionskin_core::{
    Attachment, Document, FitMode, GeometryError, Layer, ObjRef, OutlineItem, PageAlignment,
    PageGeometry, PageGeometryResponse, PageIndex, PageLayoutMode, PagePlacement, PagePoint,
    PageQuad, PageRect, RenderRequest, RenderResponse, SearchOptions, SearchState, SignatureField,
    ThumbnailResponse, ViewHistory, ViewPoint, ViewRect, ViewRotation, ViewSize, Viewport,
    ViewportError,
};
use onionskin_plugin_api::{
    ExportError, ExportRequest, ExportedFile, Overlay, PageRange, PluginRegistry, PointerInput,
    ToolCtx,
};
use onionskin_render::{BaseRaster, Tile, TileStore, TILE_SIZE};
use smallvec::smallvec;

use super::input::{
    pointer_input, validate_pressure, DragKind, DragUpdate, InputError, InputState,
};

const PAGE_GAP: f32 = 12.0;
const VIEW_HISTORY_CAPACITY: NonZeroUsize = NonZeroUsize::new(100).unwrap();
/// How long the canvas keeps polling a request nothing has answered.
///
/// The worker answers a page in milliseconds, and every answer restarts the
/// clock, so this only expires on a request that will never be answered: a
/// stopped worker, or a response the canvas dropped. Without it the poll had
/// no exit but an answer, and a request that never came back left the app
/// waking every 16 ms for the rest of the process.
const PENDING_WORK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CanvasViewState {
    pub(super) current_page: PageIndex,
    pub(super) page_count: usize,
    pub(super) zoom: f32,
    pub(super) zoom_policy: onionskin_core::ZoomPolicy,
    pub(super) layout_mode: PageLayoutMode,
    pub(super) show_cover: bool,
    pub(super) rotation: ViewRotation,
    pub(super) can_previous_view: bool,
    pub(super) can_next_view: bool,
}

impl CanvasViewState {
    pub(super) fn is_actual_size(self) -> bool {
        self.zoom_policy == onionskin_core::ZoomPolicy::Fixed
            && (self.zoom - 1.0).abs() < f32::EPSILON
    }

    pub(super) fn fit_mode(self) -> Option<FitMode> {
        match self.zoom_policy {
            onionskin_core::ZoomPolicy::Fit(mode) => Some(mode),
            onionskin_core::ZoomPolicy::Fixed => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ViewAction {
    PreviousView,
    NextView,
    FirstPage,
    PreviousPage,
    NextPage,
    LastPage,
    GoToPage(PageIndex),
    RotateClockwise,
    ActualSize,
    ZoomOut,
    ZoomIn,
    Fit(FitMode),
    SetLayout(PageLayoutMode),
    SetShowCover(bool),
}

#[derive(Debug)]
pub enum CanvasError {
    EmptyDocument,
    GenerationExhausted,
    Core(onionskin_core::Error),
    Viewport(ViewportError),
    Input(InputError),
    Render(onionskin_render::RenderError),
    InvalidImageBuffer {
        expected: usize,
        actual: usize,
    },
    InvalidImageCrop {
        width: u32,
        height: u32,
        crop_width: u32,
        crop_height: u32,
    },
    ToolOutOfRange {
        index: usize,
        count: usize,
    },
    /// The build has no codec for the format a menu entry asked for, because
    /// the plugin that owns it was compiled out.
    UnknownCodec(&'static str),
    Export(ExportError),
    Geometry(GeometryError),
    SnapshotUnrendered {
        page: PageIndex,
    },
    SnapshotEmpty {
        page: PageIndex,
    },
    SnapshotEncode(String),
    WorkerSilent {
        pages: Vec<PageIndex>,
        waited: Duration,
    },
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDocument => write!(f, "the PDF has no pages"),
            Self::GenerationExhausted => write!(f, "the render generation counter is exhausted"),
            Self::Core(error) => write!(f, "{error}"),
            Self::Viewport(error) => write!(f, "{error}"),
            Self::Input(error) => write!(f, "{error}"),
            Self::Render(error) => write!(f, "{error}"),
            Self::InvalidImageBuffer { expected, actual } => {
                write!(f, "tile image needs {expected} RGBA bytes, got {actual}")
            }
            Self::InvalidImageCrop {
                width,
                height,
                crop_width,
                crop_height,
            } => write!(
                f,
                "tile crop {crop_width}x{crop_height} is outside {width}x{height}"
            ),
            Self::Geometry(error) => write!(f, "{error}"),
            Self::SnapshotUnrendered { page } => {
                write!(
                    f,
                    "page {page} is not on screen yet, so it cannot be copied"
                )
            }
            Self::SnapshotEmpty { page } => {
                write!(f, "the snapshot region on page {page} covers no pixels")
            }
            Self::SnapshotEncode(error) => write!(f, "cannot encode the snapshot: {error}"),
            Self::ToolOutOfRange { index, count } => {
                write!(f, "tool {index} is outside a {count}-tool registry")
            }
            Self::UnknownCodec(id) => write!(f, "no {id} codec is installed"),
            Self::Export(error) => write!(f, "{error}"),
            Self::WorkerSilent { pages, waited } => write!(
                f,
                "no answer for {pages:?} after {} seconds; the render worker stopped answering",
                waited.as_secs()
            ),
        }
    }
}

impl std::error::Error for CanvasError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::Viewport(error) => Some(error),
            Self::Input(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Export(error) => Some(error),
            Self::Geometry(error) => Some(error),
            Self::EmptyDocument
            | Self::GenerationExhausted
            | Self::InvalidImageBuffer { .. }
            | Self::InvalidImageCrop { .. }
            | Self::ToolOutOfRange { .. }
            | Self::UnknownCodec(_)
            | Self::SnapshotUnrendered { .. }
            | Self::SnapshotEmpty { .. }
            | Self::SnapshotEncode(_)
            | Self::WorkerSilent { .. } => None,
        }
    }
}

impl From<onionskin_core::Error> for CanvasError {
    fn from(error: onionskin_core::Error) -> Self {
        Self::Core(error)
    }
}

impl From<ViewportError> for CanvasError {
    fn from(error: ViewportError) -> Self {
        Self::Viewport(error)
    }
}

impl From<InputError> for CanvasError {
    fn from(error: InputError) -> Self {
        Self::Input(error)
    }
}

impl From<GeometryError> for CanvasError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
    }
}

impl From<onionskin_render::RenderError> for CanvasError {
    fn from(error: onionskin_render::RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<ExportError> for CanvasError {
    fn from(error: ExportError) -> Self {
        Self::Export(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasStatus {
    Warning {
        page: PageIndex,
        message: String,
    },
    Error {
        page: Option<PageIndex>,
        message: String,
    },
}

pub struct PagePaint {
    pub page: PageIndex,
    pub rect: ViewRect,
}

pub struct TilePaint {
    pub page: PageIndex,
    pub rect: ViewRect,
    pub clip_rect: ViewRect,
    pub source_zoom: f32,
    pub image: Arc<RenderImage>,
}

/// A tool overlay in canvas coordinates, ready to paint.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayPaint {
    /// Filled translucent polygons: a text selection.
    Quads(Vec<[ViewPoint; 4]>),
    /// Dashed outline: a marquee in progress.
    AntsRect(ViewRect),
}

/// One search hit's box on a visible page. An overlay in the same sense: it is
/// painted over the tiles rather than rendered into them, so highlighting
/// costs no re-render.
pub struct HighlightPaint {
    pub rect: ViewRect,
    /// The hit next/previous last landed on, drawn differently from the rest.
    pub current: bool,
}

#[derive(Default)]
pub struct PaintList {
    pub pages: Vec<PagePaint>,
    pub tiles: Vec<TilePaint>,
    pub overlays: Vec<OverlayPaint>,
    pub highlights: Vec<HighlightPaint>,
}

#[derive(Clone, PartialEq, Eq)]
struct RenderSignature {
    pages: Vec<PageIndex>,
    zoom_bits: u32,
}

#[derive(Clone, Copy)]
enum ToolPointerPhase {
    Down,
    Move,
    Up,
}

pub struct CanvasModel {
    document: Document,
    viewport: Viewport,
    view_history: ViewHistory,
    registry: PluginRegistry,
    active_tool: Option<usize>,
    input: InputState,
    tiles: TileStore,
    sources: BTreeMap<PageIndex, BaseRaster>,
    geometry_requests: BTreeSet<PageIndex>,
    failed_geometry: BTreeSet<PageIndex>,
    requests: BTreeMap<PageIndex, RenderRequest>,
    failed_renders: BTreeSet<PageIndex>,
    placeholders: BTreeSet<PageIndex>,
    generation: u64,
    signature: Option<RenderSignature>,
    canvas_origin: ViewPoint,
    image_cache: TileImageCache,
    status: Option<CanvasStatus>,
    /// How many responses had been applied when the current wait started, and
    /// when that was. Only [`CanvasModel::poll_again`] reads it.
    waiting: Option<(u64, Instant)>,
    /// Responses applied since the canvas opened. Read only as "did this
    /// change", which is what separates a slow answer from no answer.
    responses: u64,
    /// A hit waiting to be scrolled to. Set when the hit is chosen, applied
    /// once its page has been measured, which is usually a later frame.
    pending_reveal: Option<(PageIndex, Vec<PageQuad>)>,
    /// Thumbnails the pane asked for and the worker has not answered yet.
    /// Keeps the poll loop awake, the way an outstanding render does, so a
    /// picture that arrives between frames still reaches the pane.
    pending_thumbnails: BTreeSet<PageIndex>,
    /// Thumbnails answered and not yet collected. The pane takes them,
    /// because turning a raster into an image the window can paint is the
    /// shell's job and not the model's.
    ready_thumbnails: Vec<(PageIndex, BaseRaster)>,
}

impl CanvasModel {
    pub fn new(
        mut document: Document,
        mut registry: PluginRegistry,
        size: ViewSize,
    ) -> Result<Self, CanvasError> {
        if document.page_count() == 0 {
            return Err(CanvasError::EmptyDocument);
        }
        let page_count = document.page_count();
        let first = document.page_geometry(0)?.clone();
        let mut viewport = Viewport::new(page_count, size, PAGE_GAP)?;
        viewport.measure_page(first)?;
        viewport.fit(FitMode::Page)?;
        let active_tool = registry.tools().next().map(|_| 0);
        if let Some(index) = active_tool {
            registry
                .tool_mut(index)
                .expect("the initial tool remains registered")
                .on_activate(&mut ToolCtx {
                    doc: &mut document,
                    viewport: &mut viewport,
                });
        }
        Ok(Self {
            document,
            viewport,
            view_history: ViewHistory::new(VIEW_HISTORY_CAPACITY),
            registry,
            active_tool,
            input: InputState::default(),
            tiles: TileStore::new(),
            sources: BTreeMap::new(),
            geometry_requests: BTreeSet::new(),
            failed_geometry: BTreeSet::new(),
            requests: BTreeMap::new(),
            failed_renders: BTreeSet::new(),
            placeholders: BTreeSet::new(),
            generation: 0,
            signature: None,
            canvas_origin: ViewPoint::default(),
            image_cache: TileImageCache::default(),
            status: None,
            waiting: None,
            responses: 0,
            pending_reveal: None,
            pending_thumbnails: BTreeSet::new(),
            ready_thumbnails: Vec::new(),
        })
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    /// True when this build has the codec a menu entry would run.
    pub fn has_codec(&self, id: &str) -> bool {
        self.registry.codec(id).is_some()
    }

    /// Export the whole document through the named codec.
    ///
    /// The registry and the document are paired here for the same reason
    /// tool gestures are: the chrome should not have to hold both and get
    /// their lifetimes right. Nothing is written to disk by this call, so a
    /// caller that fails to save has still not left a half-written export
    /// behind.
    pub fn export(
        &mut self,
        codec: &'static str,
        dpi: f32,
    ) -> Result<Vec<ExportedFile>, CanvasError> {
        let request = ExportRequest {
            pages: PageRange::whole(self.document.page_count())?,
            dpi,
        };
        let codec = self
            .registry
            .codec(codec)
            .ok_or(CanvasError::UnknownCodec(codec))?;
        Ok(codec.export(&mut self.document, &request)?)
    }

    /// The text the current selection covers, which is what the context
    /// menu's Copy puts on the clipboard.
    pub fn selection_text(&self) -> Option<&str> {
        self.document
            .selection()
            .text()
            .map(|selection| selection.text.as_str())
    }

    pub fn active_tool(&self) -> Option<usize> {
        self.active_tool
    }

    // ---- navigation panes ---------------------------------------------

    /// The document readers the left panes list. Each is read once by the
    /// session and handed back by value: a pane holds a snapshot rather than
    /// a borrow of the document the canvas is drawing from.
    pub fn outline(&mut self) -> Result<Vec<OutlineItem>, CanvasError> {
        Ok(self.document.outline()?.to_vec())
    }

    pub fn attachments(&mut self) -> Result<Vec<Attachment>, CanvasError> {
        Ok(self.document.attachments()?.to_vec())
    }

    pub fn attachment_bytes(&mut self, index: usize) -> Result<Vec<u8>, CanvasError> {
        Ok(self.document.attachment_bytes(index)?)
    }

    pub fn signatures(&mut self) -> Result<Vec<SignatureField>, CanvasError> {
        Ok(self.document.signatures()?.to_vec())
    }

    pub fn layers(&mut self) -> Result<Vec<Layer>, CanvasError> {
        Ok(self.document.layers()?.to_vec())
    }

    /// Show or hide one optional content group, and drop every pixel that
    /// predates the change.
    ///
    /// The store is keyed by page and zoom, not by the options the raster was
    /// produced with, so nothing in it would be rebuilt on its own. Clearing
    /// it is not enough either: `sources` holds the same rasters for
    /// placeholders and would put the old layer state straight back on
    /// screen, and the signature has not changed, so without resetting it the
    /// generation would not advance and the visible pages would never be
    /// asked for again.
    pub fn set_layer_visible(&mut self, layer: ObjRef, visible: bool) -> Result<bool, CanvasError> {
        if !self.document.set_layer_visible(layer, visible)? {
            return Ok(false);
        }
        self.invalidate_rendered_pixels();
        Ok(true)
    }

    /// Forget every cached raster and make the visible pages be rendered
    /// again, because the render options they were produced with have
    /// changed.
    fn invalidate_rendered_pixels(&mut self) {
        self.tiles.clear();
        self.sources.clear();
        self.requests.clear();
        self.failed_renders.clear();
        self.placeholders.clear();
        // The next update compares the visible set against `None`, advances
        // the generation, and re-requests every page. The advance is also
        // what makes the answers already in flight, which were rendered with
        // the old options, be dropped rather than painted.
        self.signature = None;
    }

    /// Put every optional content group back to the visibility the file's
    /// own default configuration gives it, dropping the pixels that were
    /// produced under the overrides.
    pub fn reset_layer_visibility(&mut self) -> Result<bool, CanvasError> {
        if !self.document.reset_layer_visibility()? {
            return Ok(false);
        }
        self.invalidate_rendered_pixels();
        Ok(true)
    }

    /// Queue a thumbnail of `page`, unless one is already outstanding.
    pub fn request_thumbnail(&mut self, page: PageIndex, zoom: f32) -> Result<(), CanvasError> {
        if !self.pending_thumbnails.insert(page) {
            return Ok(());
        }
        if let Err(error) = self.document.request_thumbnail(page, zoom) {
            self.pending_thumbnails.remove(&page);
            return Err(error.into());
        }
        Ok(())
    }

    /// The thumbnails answered since the last call, taken rather than
    /// borrowed: the pane converts each one to an image and owns it from
    /// there, so the model never holds two copies of the same picture.
    pub fn take_thumbnails(&mut self) -> Vec<(PageIndex, BaseRaster)> {
        std::mem::take(&mut self.ready_thumbnails)
    }

    /// Whether `page` has a thumbnail on the way, so the pane does not ask
    /// again on every frame while it waits.
    pub fn thumbnail_pending(&self, page: PageIndex) -> bool {
        self.pending_thumbnails.contains(&page)
    }

    fn drain_thumbnail_responses(&mut self) -> Result<(), CanvasError> {
        while let Some(response) = self.document.try_thumbnail_response()? {
            self.pending_thumbnails.remove(&response.page());
            self.responses += 1;
            match response {
                ThumbnailResponse::Ready { page, render } => {
                    self.ready_thumbnails.push((page, render.raster));
                }
                // Reported on the page it belongs to, like a failed render:
                // a blank row with no reason is a pane that looks broken.
                ThumbnailResponse::Failed { page, error } => {
                    self.status = Some(CanvasStatus::Error {
                        page: Some(page),
                        message: format!("page {page} thumbnail: {error}"),
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) fn view_state(&self) -> CanvasViewState {
        CanvasViewState {
            current_page: self.viewport.current_page(),
            page_count: self.viewport.page_count(),
            zoom: self.viewport.zoom(),
            zoom_policy: self.viewport.zoom_policy(),
            layout_mode: self.viewport.mode(),
            show_cover: self.viewport.show_cover(),
            rotation: self.viewport.rotation(),
            can_previous_view: self.can_previous_view(),
            can_next_view: self.can_next_view(),
        }
    }

    pub fn can_previous_view(&self) -> bool {
        self.view_history.can_previous()
    }

    pub fn can_next_view(&self) -> bool {
        self.view_history.can_next()
    }

    pub fn go_to_page(&mut self, page: PageIndex) -> Result<bool, CanvasError> {
        self.apply_view_change(|viewport| viewport.go_to_page(page, PageAlignment::Start))
    }

    pub fn first_page(&mut self) -> Result<bool, CanvasError> {
        self.apply_view_change(Viewport::first_page)
    }

    pub fn previous_page(&mut self) -> Result<bool, CanvasError> {
        self.apply_view_change(Viewport::previous_page)
    }

    pub fn next_page(&mut self) -> Result<bool, CanvasError> {
        self.apply_view_change(Viewport::next_page)
    }

    pub fn last_page(&mut self) -> Result<bool, CanvasError> {
        self.apply_view_change(Viewport::last_page)
    }

    pub fn zoom_in(&mut self) -> Result<bool, CanvasError> {
        let anchor = self.viewport_center();
        self.apply_view_change(|viewport| viewport.zoom_in(anchor))
    }

    pub fn zoom_out(&mut self) -> Result<bool, CanvasError> {
        let anchor = self.viewport_center();
        self.apply_view_change(|viewport| viewport.zoom_out(anchor))
    }

    pub fn zoom_to(&mut self, zoom: f32) -> Result<bool, CanvasError> {
        let anchor = self.viewport_center();
        self.apply_view_change(|viewport| viewport.zoom_to(zoom, anchor))
    }

    pub fn fit(&mut self, mode: FitMode) -> Result<bool, CanvasError> {
        self.apply_view_change(|viewport| viewport.fit(mode))
    }

    pub fn actual_size(&mut self) -> Result<bool, CanvasError> {
        self.apply_view_change(Viewport::actual_size)
    }

    pub fn set_rotation(&mut self, rotation: ViewRotation) -> Result<bool, CanvasError> {
        self.apply_view_change(|viewport| viewport.set_rotation(rotation))
    }

    pub fn rotate_clockwise(&mut self) -> Result<bool, CanvasError> {
        let rotation = match self.viewport.rotation() {
            ViewRotation::None => ViewRotation::Clockwise90,
            ViewRotation::Clockwise90 => ViewRotation::HalfTurn,
            ViewRotation::HalfTurn => ViewRotation::Clockwise270,
            ViewRotation::Clockwise270 => ViewRotation::None,
        };
        self.set_rotation(rotation)
    }

    pub fn set_layout_mode(&mut self, mode: PageLayoutMode) -> Result<bool, CanvasError> {
        self.apply_view_change(|viewport| viewport.set_mode(mode))
    }

    pub fn set_show_cover(&mut self, show_cover: bool) -> Result<bool, CanvasError> {
        self.apply_view_change(|viewport| viewport.set_show_cover(show_cover))
    }

    pub fn previous_view(&mut self) -> Result<bool, CanvasError> {
        let current = self.viewport.snapshot();
        let Some(target) = self.view_history.previous(current) else {
            return Ok(false);
        };
        if let Err(error) = self.viewport.restore(target) {
            let restored = self.view_history.next(target);
            debug_assert_eq!(restored, Some(current));
            return Err(error.into());
        }
        Ok(true)
    }

    pub fn next_view(&mut self) -> Result<bool, CanvasError> {
        let current = self.viewport.snapshot();
        let Some(target) = self.view_history.next(current) else {
            return Ok(false);
        };
        if let Err(error) = self.viewport.restore(target) {
            let restored = self.view_history.previous(target);
            debug_assert_eq!(restored, Some(current));
            return Err(error.into());
        }
        Ok(true)
    }

    pub fn search(&self) -> &SearchState {
        self.document.search()
    }

    /// Starts a document-wide find at the page in view, so the nearest hits
    /// arrive first. Results stream in through [`CanvasModel::update`].
    pub fn start_search(
        &mut self,
        needle: &str,
        options: SearchOptions,
    ) -> Result<bool, CanvasError> {
        self.pending_reveal = None;
        let start_page = self.viewport.current_page();
        Ok(self.document.start_search(needle, options, start_page)?)
    }

    pub fn cancel_search(&mut self) {
        self.pending_reveal = None;
        self.document.cancel_search();
    }

    pub fn select_next_match(&mut self) -> Result<bool, CanvasError> {
        if !self.document.select_next_match() {
            return Ok(false);
        }
        self.reveal_as_navigation()?;
        // The cursor moved even when the view did not: the hit drawn as the
        // current one changed, and that is a repaint.
        Ok(true)
    }

    /// Make one particular hit current, which is what a click in the search
    /// results pane does. Takes the same route as stepping to it, so
    /// Previous View comes back from the jump.
    pub fn select_match(&mut self, page: PageIndex, index: usize) -> Result<bool, CanvasError> {
        if !self.document.select_match(page, index) {
            return Ok(false);
        }
        self.reveal_as_navigation()?;
        Ok(true)
    }

    pub fn select_previous_match(&mut self) -> Result<bool, CanvasError> {
        if !self.document.select_previous_match() {
            return Ok(false);
        }
        self.reveal_as_navigation()?;
        Ok(true)
    }

    /// Stepping to a hit is a navigation the user asked for, so Previous View
    /// comes back from it, and the whole reveal counts as the one entry. The
    /// reveal a streaming result triggers records nothing: a walk restarts on
    /// every keystroke, and those would bury the view the user typed from
    /// under one entry per character.
    fn reveal_as_navigation(&mut self) -> Result<(), CanvasError> {
        let before = self.viewport.snapshot();
        self.reveal_current_match()?;
        // What the viewport did, not what the reveal asked for: a pan that
        // clamped against the end of the document moved nothing, and Previous
        // View should not offer to return to where the user already is.
        if self.viewport.snapshot() != before {
            self.view_history.record(before);
        }
        Ok(())
    }

    /// Applies whatever the search worker produced, and scrolls to the first
    /// hit of a fresh walk as soon as it arrives. Only the first page to
    /// report a hit places the cursor, so a cursor that moved here means that
    /// page has just landed.
    fn poll_search(&mut self) -> Result<(), CanvasError> {
        let before = self.document.search().cursor();
        if self.document.poll_search() && self.document.search().cursor() != before {
            self.reveal_current_match()?;
        }
        Ok(())
    }

    /// Puts the current hit on screen. A hit on a page the viewport is already
    /// showing is panned to, so the page does not jump under a user who can
    /// see the hit already; only a hit somewhere else is worth the jump, and
    /// the pan onto it then waits for that page to be measured.
    ///
    /// A hit with no quads, which is what a match on the separator between two
    /// runs is, takes the same route: it has no bounds, so it goes to its page
    /// and the pan onto it finds nothing to do.
    ///
    /// Nothing here records view history. Whether the view ended up somewhere
    /// worth returning to is the caller's question, and only the caller knows
    /// whether the user asked for the move.
    fn reveal_current_match(&mut self) -> Result<(), CanvasError> {
        self.pending_reveal = None;
        let Some(hit) = self.document.search().current() else {
            return Ok(());
        };
        let page = hit.page;
        let quads = hit.quads.clone();
        if let Some(bounds) = self.hit_bounds(page, &quads)? {
            return self.pan_onto(bounds);
        }
        self.pending_reveal = Some((page, quads));
        self.viewport.go_to_page(page, PageAlignment::Start)?;
        self.apply_pending_reveal()
    }

    /// The pan the jump above could not do yet, once the page it jumped to has
    /// been measured. Dropped rather than held when the page failed to measure
    /// or the user has scrolled it off screen in the meantime.
    fn apply_pending_reveal(&mut self) -> Result<(), CanvasError> {
        let Some((page, quads)) = self.pending_reveal.take() else {
            return Ok(());
        };
        if self.failed_geometry.contains(&page) {
            return Ok(());
        }
        if self.viewport.page_geometry(page).is_none() {
            if self.is_visible(page)? {
                self.pending_reveal = Some((page, quads));
            }
            return Ok(());
        }
        let Some(bounds) = self.hit_bounds(page, &quads)? else {
            return Ok(());
        };
        self.pan_onto(bounds)
    }

    /// Where a hit's quads sit in the viewport, or `None` when the page is not
    /// laid out or not measured and the hit has nowhere to land yet.
    fn hit_bounds(
        &self,
        page: PageIndex,
        quads: &[PageQuad],
    ) -> Result<Option<ViewRect>, CanvasError> {
        Ok(union_rect(&self.viewport.page_quad_rects(page, quads)?))
    }

    fn pan_onto(&mut self, bounds: ViewRect) -> Result<(), CanvasError> {
        let delta = scroll_delta_into_view(bounds, self.viewport.size());
        if delta == ViewPoint::default() {
            return Ok(());
        }
        self.viewport.pan_by(delta)?;
        Ok(())
    }

    fn is_visible(&self, page: PageIndex) -> Result<bool, CanvasError> {
        Ok(self
            .viewport
            .visible_pages()?
            .iter()
            .any(|placement| placement.page == page))
    }

    fn apply_view_change(
        &mut self,
        change: impl FnOnce(&mut Viewport) -> Result<(), ViewportError>,
    ) -> Result<bool, CanvasError> {
        let before = self.viewport.snapshot();
        change(&mut self.viewport)?;
        if self.viewport.snapshot() == before {
            return Ok(false);
        }
        self.view_history.record(before);
        Ok(true)
    }

    fn viewport_center(&self) -> ViewPoint {
        let size = self.viewport.size();
        ViewPoint {
            x: size.width / 2.0,
            y: size.height / 2.0,
        }
    }

    pub fn activate_tool(&mut self, index: usize) -> Result<bool, CanvasError> {
        let count = self.registry.tools().count();
        if index >= count {
            return Err(CanvasError::ToolOutOfRange { index, count });
        }
        if self.active_tool == Some(index) {
            return Ok(false);
        }

        self.cancel_pointer_gesture();
        if let Some(active) = self.active_tool {
            self.registry
                .tool_mut(active)
                .expect("the active tool remains registered")
                .on_deactivate(&mut ToolCtx {
                    doc: &mut self.document,
                    viewport: &mut self.viewport,
                });
        }
        self.active_tool = Some(index);
        self.registry
            .tool_mut(index)
            .expect("validated tools remain registered")
            .on_activate(&mut ToolCtx {
                doc: &mut self.document,
                viewport: &mut self.viewport,
            });
        Ok(true)
    }

    pub fn canvas_origin(&self) -> ViewPoint {
        self.canvas_origin
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn status(&self) -> Option<&CanvasStatus> {
        self.status.as_ref()
    }

    pub fn has_pending_render(&self) -> bool {
        !self.requests.is_empty()
    }

    pub fn has_pending_work(&self) -> bool {
        self.has_pending_pages()
            || self.document.search().is_running()
            || !self.pending_thumbnails.is_empty()
    }

    /// Work a page worker owes an answer for. Separate from the search and
    /// from the thumbnails, which are pending work of their own and answer on
    /// channels of their own; the deadline below watches page work only.
    fn has_pending_pages(&self) -> bool {
        !self.geometry_requests.is_empty() || self.has_pending_render()
    }

    /// Whether the poll loop should wake again, given the clock.
    ///
    /// False once there is nothing outstanding, and false again once the
    /// outstanding set has gone [`PENDING_WORK_TIMEOUT`] without a single
    /// response, which is recorded as a [`CanvasStatus::Error`] naming the
    /// pages nobody answered. Any response restarts the wait, so a document
    /// that keeps the worker busy for hours never trips it.
    ///
    /// The deadline watches page work only. A search walking a long document
    /// keeps the loop awake without a page outstanding and without bumping
    /// the response count, so watching it here would report the render worker
    /// silent, name no pages, and stop polling the results still arriving. A
    /// search that stops answering ends its own walk instead, and says so in
    /// the find bar.
    pub fn poll_again(&mut self, now: Instant) -> bool {
        if !self.has_pending_work() {
            self.waiting = None;
            return false;
        }
        if !self.has_pending_pages() {
            self.waiting = None;
            return true;
        }
        let (responses, since) = *self.waiting.get_or_insert((self.responses, now));
        if responses != self.responses {
            self.waiting = Some((self.responses, now));
            return true;
        }
        let waited = now.saturating_duration_since(since);
        if waited < PENDING_WORK_TIMEOUT {
            return true;
        }
        self.waiting = None;
        let pages = self
            .geometry_requests
            .iter()
            .copied()
            .chain(self.requests.keys().copied())
            .collect();
        self.record_error(CanvasError::WorkerSilent { pages, waited });
        false
    }

    pub fn resize(&mut self, origin: ViewPoint, size: ViewSize) -> Result<(), CanvasError> {
        self.canvas_origin = origin;
        if self.viewport.size() != size {
            self.viewport.resize(size)?;
        }
        Ok(())
    }

    pub fn scroll(
        &mut self,
        delta: ViewPoint,
        zooming: bool,
        at: ViewPoint,
    ) -> Result<(), CanvasError> {
        self.viewport.scroll(delta, zooming, at)?;
        Ok(())
    }

    /// Drop a pinch the platform reports with a nonsense factor, and zoom by
    /// anything else. The filter lives here, at the OS event boundary, rather
    /// than in the viewport, which treats a non-positive factor as the error
    /// it is.
    pub fn pinch(&mut self, factor: f32, at: ViewPoint) -> Result<bool, CanvasError> {
        if !(factor.is_finite() && factor > 0.0) {
            return Ok(false);
        }
        self.viewport.pinch(factor, at)?;
        Ok(true)
    }

    pub fn pointer_down(
        &mut self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<bool, CanvasError> {
        validate_pressure(pressure)?;
        if self.active_tool.is_none() {
            self.input.begin_pan(window_point(position));
            return Ok(true);
        }

        let Some(input) = self.map_pointer(position, pressure, modifiers)? else {
            return Ok(false);
        };
        self.input.begin_tool();
        self.dispatch_tool(ToolPointerPhase::Down, input);
        Ok(true)
    }

    pub fn pointer_move(
        &mut self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
        left_button_pressed: bool,
    ) -> Result<bool, CanvasError> {
        let update = self
            .input
            .move_to(window_point(position), left_button_pressed);
        if let Err(error) = validate_pressure(pressure) {
            let cancelled_tool = matches!(update, Some(DragUpdate::CancelTool))
                || matches!(self.input.cancel(), Some(DragUpdate::CancelTool));
            if cancelled_tool {
                self.cancel_active_tool();
            }
            return Err(error.into());
        }
        match update {
            Some(DragUpdate::PanBy(delta)) => {
                self.viewport.pan_by(delta)?;
                Ok(true)
            }
            Some(DragUpdate::ToolMove) => {
                let input = match self.map_pointer(position, pressure, modifiers) {
                    Ok(Some(input)) => input,
                    Ok(None) => {
                        self.input.cancel();
                        self.cancel_active_tool();
                        return Ok(true);
                    }
                    Err(error) => {
                        self.input.cancel();
                        self.cancel_active_tool();
                        return Err(error);
                    }
                };
                self.dispatch_tool(ToolPointerPhase::Move, input);
                Ok(true)
            }
            Some(DragUpdate::CancelTool) => {
                self.cancel_active_tool();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn pointer_up(
        &mut self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<bool, CanvasError> {
        let drag = self.input.end();
        if let Err(error) = validate_pressure(pressure) {
            if drag == Some(DragKind::Tool) {
                self.cancel_active_tool();
            }
            return Err(error.into());
        }
        match drag {
            Some(DragKind::Pan) => Ok(true),
            Some(DragKind::Tool) => {
                match self.map_pointer(position, pressure, modifiers) {
                    Ok(Some(input)) => self.dispatch_tool(ToolPointerPhase::Up, input),
                    Ok(None) => self.cancel_active_tool(),
                    Err(error) => {
                        self.cancel_active_tool();
                        return Err(error);
                    }
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn cancel_pointer_gesture(&mut self) -> bool {
        match self.input.end() {
            Some(DragKind::Tool) => {
                self.cancel_active_tool();
                true
            }
            Some(DragKind::Pan) => true,
            None => false,
        }
    }

    pub fn update(&mut self) -> Result<(), CanvasError> {
        self.poll_search()?;
        self.drain_thumbnail_responses()?;
        self.drain_geometry_responses()?;
        self.queue_visible_geometry()?;
        self.drain_geometry_responses()?;
        self.apply_pending_reveal()?;

        let visible = self.viewport.visible_pages()?;
        self.update_signature(&visible)?;
        self.retain_visible_state(&visible);
        // Everything the store hands out from here until `paint_list` ends is
        // this frame's, and exempt from eviction: the exact-zoom cache, and
        // the other-zoom cache a rescaled placeholder paints from. Draining
        // inside the frame matters when several rasters land at once: four of
        // them at 6x are 266 MiB against a 202 MiB budget, and unframed they
        // would evict each other as they arrived.
        self.tiles.begin_frame();
        self.drain_render_responses()?;
        self.schedule_visible_renders(&visible)?;
        self.drain_render_responses()?;
        Ok(())
    }

    pub fn drain_geometry_responses(&mut self) -> Result<usize, CanvasError> {
        let mut drained = 0;
        while let Some(response) = self.document.try_page_geometry_response()? {
            drained += 1;
            self.apply_geometry_response(response)?;
        }
        Ok(drained)
    }

    pub fn drain_render_responses(&mut self) -> Result<usize, CanvasError> {
        self.sync_signature()?;
        let mut drained = 0;
        while let Some(response) = self.document.try_render_response()? {
            drained += usize::from(self.apply_render_response(response));
        }
        Ok(drained)
    }

    pub fn paint_list(&mut self) -> Result<PaintList, CanvasError> {
        let visible = self.viewport.visible_pages()?;
        let mut paint = PaintList {
            pages: visible
                .iter()
                .map(|placement| PagePaint {
                    page: placement.page,
                    rect: placement.rect,
                })
                .collect(),
            tiles: Vec::new(),
            overlays: Vec::new(),
            highlights: self.highlights(&visible)?,
        };
        let rotation = self.viewport.rotation();
        let exact_zoom = self.viewport.zoom();
        let viewport_size = self.viewport.size();
        let mut displayed_images = BTreeSet::new();

        for placement in visible.into_iter().filter(|page| page.measured) {
            let Some(source_zoom) = self.paint_source(placement.page, exact_zoom)? else {
                continue;
            };
            let source = RasterPaintSource {
                page: placement.page,
                zoom: source_zoom,
                rect: placement.rect,
                rotation,
            };
            let raw_tiles = collect_tiles(&mut self.tiles, source, viewport_size);
            for raw in raw_tiles {
                let key = TileImageKey::new(
                    placement.page,
                    source_zoom,
                    raw.region.col,
                    raw.region.row,
                    rotation,
                );
                let image = self.image_cache.image_for(
                    key,
                    &raw.tile,
                    raw.region.width,
                    raw.region.height,
                    rotation,
                )?;
                displayed_images.insert(key);
                let (content_width, content_height) =
                    rotated_size(raw.region.width, raw.region.height, rotation);
                paint.tiles.push(TilePaint {
                    page: placement.page,
                    rect: atlas_image_rect(raw.rect, content_width, content_height),
                    clip_rect: raw.rect,
                    source_zoom,
                    image,
                });
            }
        }
        self.image_cache.retain_keys(&displayed_images);
        self.tiles.end_frame();
        paint.overlays = self.overlay_paints();
        Ok(paint)
    }

    /// Fulfil a pending snapshot request, as PNG bytes ready for the
    /// clipboard, or `Ok(None)` when no tool has raised one.
    ///
    /// This crops the page raster the canvas is already painting, at the
    /// zoom it was rasterized at, and turns it by the rotation it is shown
    /// under, rather than asking the renderer for the region again: the
    /// point of a snapshot is the pixels the user drew a marquee around.
    /// Overlays are painted separately and are not in that raster, so the
    /// snapshot is the page alone. Acrobat's includes annotations, which is
    /// a gap to close when there are annotations to include.
    pub fn take_snapshot_png(&mut self) -> Result<Option<Vec<u8>>, CanvasError> {
        let Some(request) = self.document.take_snapshot_request() else {
            return Ok(None);
        };
        let page = request.region.page;
        let (Some(source), Some(geometry)) =
            (self.sources.get(&page), self.viewport.page_geometry(page))
        else {
            return Err(CanvasError::SnapshotUnrendered { page });
        };
        let crop = raster_crop(geometry, request.region, source)?;
        encode_snapshot(source, crop, self.viewport.rotation()).map(Some)
    }

    /// Every hit on the pages currently on screen. Highlight-all is drawn from
    /// the results found so far, so a walk still running highlights the pages
    /// it has already reported.
    fn highlights(&self, visible: &[PagePlacement]) -> Result<Vec<HighlightPaint>, CanvasError> {
        let cursor = self.document.search().cursor();
        let mut highlights = Vec::new();
        // One mapping call per page, not per hit: each one re-walks the
        // layout, and a page can carry hundreds of hits.
        let mut quads: Vec<PageQuad> = Vec::new();
        let mut is_current: Vec<bool> = Vec::new();
        for placement in visible.iter().filter(|placement| placement.measured) {
            let page = placement.page;
            quads.clear();
            is_current.clear();
            for (index, hit) in self.document.search().matches_on(page).iter().enumerate() {
                quads.extend(hit.quads.iter().copied());
                is_current.resize(quads.len(), cursor == Some((page, index)));
            }
            highlights.extend(
                self.viewport
                    .page_quad_rects(page, &quads)?
                    .into_iter()
                    .zip(is_current.iter().copied())
                    .map(|(rect, current)| HighlightPaint { rect, current }),
            );
        }
        Ok(highlights)
    }

    pub fn record_error(&mut self, error: impl fmt::Display) -> bool {
        let status = CanvasStatus::Error {
            page: None,
            message: error.to_string(),
        };
        if self.status.as_ref() == Some(&status) {
            return false;
        }
        self.status = Some(status);
        true
    }

    /// Requests geometry for every visible page that has none, and records
    /// what it waits on.
    ///
    /// A page is recorded only when the session says the request went out.
    /// `false` means the session is already holding one, so either this canvas
    /// already recorded it or the two disagree; inventing a wait in the second
    /// case is what left `has_pending_work` true with nothing on the way.
    fn queue_visible_geometry(&mut self) -> Result<usize, CanvasError> {
        let mut queued = 0;
        for placement in self.viewport.visible_pages()? {
            if placement.measured
                || self.failed_geometry.contains(&placement.page)
                || self.geometry_requests.contains(&placement.page)
            {
                continue;
            }
            if self.document.request_page_geometry(placement.page)? {
                self.geometry_requests.insert(placement.page);
                queued += 1;
            }
        }
        Ok(queued)
    }

    fn apply_geometry_response(
        &mut self,
        response: PageGeometryResponse,
    ) -> Result<(), CanvasError> {
        self.geometry_requests.remove(&response.page());
        self.responses += 1;
        match response {
            PageGeometryResponse::Ready(geometry) => {
                self.failed_geometry.remove(&geometry.index);
                self.viewport.measure_page(geometry)?;
            }
            PageGeometryResponse::Failed { page, error } => {
                self.failed_geometry.insert(page);
                self.status = Some(CanvasStatus::Error {
                    page: Some(page),
                    message: error.to_string(),
                });
            }
        }
        Ok(())
    }

    fn update_signature(&mut self, visible: &[PagePlacement]) -> Result<bool, CanvasError> {
        let signature = RenderSignature {
            pages: visible.iter().map(|page| page.page).collect(),
            zoom_bits: self.viewport.zoom().to_bits(),
        };
        if self.signature.as_ref() == Some(&signature) {
            return Ok(false);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(CanvasError::GenerationExhausted)?;
        self.signature = Some(signature);
        self.requests.clear();
        self.failed_renders.clear();
        // Geometry failures go with them. Nothing re-requests a page the
        // canvas is holding as failed, so the only response that could clear
        // it never arrives, and a page that failed once stayed blank for the
        // life of the process.
        self.failed_geometry.clear();
        self.placeholders.clear();
        Ok(true)
    }

    fn sync_signature(&mut self) -> Result<bool, CanvasError> {
        let visible = self.viewport.visible_pages()?;
        self.update_signature(&visible)
    }

    fn retain_visible_state(&mut self, visible: &[PagePlacement]) {
        let pages: BTreeSet<_> = visible.iter().map(|page| page.page).collect();
        self.sources.retain(|page, _| pages.contains(page));
    }

    fn schedule_visible_renders(
        &mut self,
        visible: &[PagePlacement],
    ) -> Result<usize, CanvasError> {
        let zoom = self.viewport.zoom();
        let mut queued = 0;
        for placement in visible.iter().filter(|page| page.measured) {
            let request = RenderRequest {
                page: placement.page,
                zoom,
                generation: self.generation,
            };
            if self.tiles.get(placement.page, zoom).is_some()
                || self.requests.get(&placement.page) == Some(&request)
                || self.failed_renders.contains(&placement.page)
            {
                continue;
            }
            let geometry = self
                .viewport
                .page_geometry(placement.page)
                .expect("a measured placement retains geometry")
                .clone();
            let source = self.sources.get(&placement.page);
            self.document
                .request_render_with_geometry(request, &geometry, source)?;
            self.requests.insert(placement.page, request);
            queued += 1;
        }
        Ok(queued)
    }

    fn apply_render_response(&mut self, response: RenderResponse) -> bool {
        let request = response.request();
        if self.requests.get(&request.page) != Some(&request) {
            return false;
        }
        self.responses += 1;

        match response {
            RenderResponse::Placeholder(placeholder) => {
                if let Some(source) = placeholder.source() {
                    self.sources
                        .entry(request.page)
                        .or_insert_with(|| source.clone());
                }
                self.placeholders.insert(request.page);
            }
            RenderResponse::Raster { render, .. } => {
                self.requests.remove(&request.page);
                self.failed_renders.remove(&request.page);
                self.placeholders.remove(&request.page);
                let raster = render.raster;
                self.sources.insert(request.page, raster.clone());
                self.tiles.insert(request.page, raster);
                if render.warnings.is_empty() {
                    if matches!(
                        self.status.as_ref(),
                        Some(CanvasStatus::Error {
                            page: Some(page),
                            ..
                        }) if *page == request.page
                    ) {
                        self.status = None;
                    }
                } else {
                    self.status = Some(CanvasStatus::Warning {
                        page: request.page,
                        message: format!("{:?}", render.warnings),
                    });
                }
            }
            RenderResponse::Failed { error, .. } => {
                self.requests.remove(&request.page);
                self.failed_renders.insert(request.page);
                self.placeholders.remove(&request.page);
                self.status = Some(CanvasStatus::Error {
                    page: Some(request.page),
                    message: format!("page {} at {}x: {error}", request.page, request.zoom),
                });
            }
        }
        true
    }

    /// The zoom whose cached raster this page paints from, or `None` when it
    /// has none yet.
    ///
    /// Only the zoom, because that plus the page is the store's key and the
    /// store owns the raster's dimensions. Reporting a size here as well gave
    /// the paint two sources for one fact, and the one it reported came from
    /// `sources` while the tiles it cut came from the store.
    fn paint_source(
        &mut self,
        page: PageIndex,
        exact_zoom: f32,
    ) -> Result<Option<f32>, CanvasError> {
        if let Some(cache) = self.tiles.get(page, exact_zoom) {
            let source = cache.base().clone();
            self.sources.insert(page, source);
            return Ok(Some(exact_zoom));
        }

        let Some(source) = self.sources.get(&page).cloned() else {
            return Ok(None);
        };
        let source_zoom = source.zoom();
        if self.tiles.get(page, source_zoom).is_none() {
            self.tiles.insert(page, source);
        }
        Ok(Some(source_zoom))
    }

    /// What the active tool wants drawn this frame, in canvas coordinates.
    ///
    /// An overlay this canvas cannot draw yet is reported rather than
    /// dropped: a tool that asks for one is asking for something the user
    /// would otherwise never see.
    fn overlay_paints(&mut self) -> Vec<OverlayPaint> {
        let overlays = match self
            .active_tool
            .and_then(|index| self.registry.tools().nth(index))
        {
            Some(tool) => tool.overlays(&self.document),
            None => return Vec::new(),
        };
        let mut paints = Vec::new();
        let mut unpaintable = None;
        for overlay in overlays {
            match self.map_overlay(&overlay) {
                Ok(Some(paint)) => paints.push(paint),
                Ok(None) => {}
                Err(kind) => unpaintable = unpaintable.or(Some(kind)),
            }
        }
        if let Some(kind) = unpaintable {
            self.record_error(format!("the canvas cannot draw a {kind} overlay yet"));
        }
        paints
    }

    /// `Ok(None)` for an overlay the viewport cannot place; `Err` names an
    /// overlay shape with no painter yet.
    fn map_overlay(&self, overlay: &Overlay) -> Result<Option<OverlayPaint>, &'static str> {
        match overlay {
            Overlay::Quads(quads) => {
                let mapped: Vec<[ViewPoint; 4]> = quads
                    .iter()
                    .filter_map(|quad| self.map_quad(*quad))
                    .collect();
                Ok((!mapped.is_empty()).then_some(OverlayPaint::Quads(mapped)))
            }
            Overlay::AntsRect(rect) => Ok(self
                .map_quad((*rect).into())
                .map(|corners| OverlayPaint::AntsRect(bounding_rect(corners)))),
            Overlay::Rect(_) => Err("rectangle"),
            Overlay::Polyline(_) => Err("polyline"),
            Overlay::Line { .. } => Err("line"),
            Overlay::Circle { .. } => Err("circle"),
        }
    }

    /// `None` when any corner cannot be placed: the page is not laid out in
    /// the current mode, or has not been measured yet. Both are ordinary
    /// frames, so neither is worth a status the user has to read.
    fn map_quad(&self, quad: PageQuad) -> Option<[ViewPoint; 4]> {
        let mut corners = [ViewPoint::default(); 4];
        for (corner, (x, y)) in corners.iter_mut().zip(quad.corners) {
            *corner = self
                .viewport
                .view_point_for(PagePoint {
                    page: quad.page,
                    x,
                    y,
                })
                .ok()
                .flatten()?;
        }
        Some(corners)
    }

    fn map_pointer(
        &self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<Option<PointerInput>, CanvasError> {
        Ok(pointer_input(
            &self.viewport,
            position,
            point(px(self.canvas_origin.x), px(self.canvas_origin.y)),
            pressure,
            modifiers,
        )?)
    }

    fn dispatch_tool(&mut self, phase: ToolPointerPhase, input: PointerInput) {
        let index = self
            .active_tool
            .expect("tool dispatch requires an active tool");
        let document = &mut self.document;
        let viewport = &mut self.viewport;
        let tool = self
            .registry
            .tool_mut(index)
            .expect("the active tool remains registered");
        let mut context = ToolCtx {
            doc: document,
            viewport,
        };
        match phase {
            ToolPointerPhase::Down => tool.on_pointer_down(&mut context, input),
            ToolPointerPhase::Move => tool.on_pointer_move(&mut context, input),
            ToolPointerPhase::Up => tool.on_pointer_up(&mut context, input),
        }
    }

    fn cancel_active_tool(&mut self) {
        let Some(index) = self.active_tool else {
            return;
        };
        let document = &mut self.document;
        let viewport = &mut self.viewport;
        let tool = self
            .registry
            .tool_mut(index)
            .expect("the active tool remains registered");
        tool.on_cancel(&mut ToolCtx {
            doc: document,
            viewport,
        });
    }
}

/// The box around a hit's quads. A hit that wraps two lines is revealed as the
/// one region it occupies, not as its first quad.
fn union_rect(rects: &[ViewRect]) -> Option<ViewRect> {
    let first = rects.first()?;
    let mut left = first.origin.x;
    let mut top = first.origin.y;
    let mut right = first.origin.x + first.size.width;
    let mut bottom = first.origin.y + first.size.height;
    for rect in &rects[1..] {
        left = left.min(rect.origin.x);
        top = top.min(rect.origin.y);
        right = right.max(rect.origin.x + rect.size.width);
        bottom = bottom.max(rect.origin.y + rect.size.height);
    }
    Some(ViewRect {
        origin: ViewPoint { x: left, y: top },
        size: ViewSize {
            width: right - left,
            height: bottom - top,
        },
    })
}

/// How far to pan so `rect` is on screen, centring it on whichever axis it
/// runs off. Zero when it is already visible: a hit the user can see does not
/// move the page under them.
fn scroll_delta_into_view(rect: ViewRect, viewport: ViewSize) -> ViewPoint {
    ViewPoint {
        x: axis_delta(rect.origin.x, rect.size.width, viewport.width),
        y: axis_delta(rect.origin.y, rect.size.height, viewport.height),
    }
}

fn axis_delta(origin: f32, extent: f32, viewport: f32) -> f32 {
    if origin >= 0.0 && origin + extent <= viewport {
        return 0.0;
    }
    (viewport - extent) / 2.0 - origin
}

fn window_point(point: Point<Pixels>) -> ViewPoint {
    ViewPoint {
        x: f32::from(point.x),
        y: f32::from(point.y),
    }
}

/// The axis-aligned extent of four mapped corners. A marquee is dragged
/// axis-aligned in the viewport, but it is carried as a page rectangle, so
/// under a rotated view its corners come back in a different order than
/// they went in.
fn bounding_rect(corners: [ViewPoint; 4]) -> ViewRect {
    let (mut left, mut top) = (f32::MAX, f32::MAX);
    let (mut right, mut bottom) = (f32::MIN, f32::MIN);
    for corner in corners {
        left = left.min(corner.x);
        right = right.max(corner.x);
        top = top.min(corner.y);
        bottom = bottom.max(corner.y);
    }
    ViewRect {
        origin: ViewPoint { x: left, y: top },
        size: ViewSize {
            width: right - left,
            height: bottom - top,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TileImageKey {
    page: PageIndex,
    zoom_bits: u32,
    col: u32,
    row: u32,
    rotation: u8,
}

impl TileImageKey {
    fn new(page: PageIndex, zoom: f32, col: u32, row: u32, rotation: ViewRotation) -> Self {
        Self {
            page,
            zoom_bits: zoom.to_bits(),
            col,
            row,
            rotation: rotation_code(rotation),
        }
    }
}

struct TileImageEntry {
    tile: Weak<Tile>,
    image: Arc<RenderImage>,
}

#[derive(Default)]
struct TileImageCache {
    entries: BTreeMap<TileImageKey, TileImageEntry>,
}

impl TileImageCache {
    fn image_for(
        &mut self,
        key: TileImageKey,
        tile: &Arc<Tile>,
        width: u32,
        height: u32,
        rotation: ViewRotation,
    ) -> Result<Arc<RenderImage>, CanvasError> {
        if let Some(entry) = self.entries.get(&key) {
            if let Some(cached) = entry.tile.upgrade() {
                if Arc::ptr_eq(&cached, tile) {
                    return Ok(Arc::clone(&entry.image));
                }
            }
        }

        let (width, height, bgra) =
            atlas_tile_bgra(tile.rgba(), TILE_SIZE, TILE_SIZE, width, height, rotation)?;
        let buffer = image::RgbaImage::from_raw(width, height, bgra)
            .expect("tile conversion returns exactly width*height*4 bytes");
        let image = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        self.entries.insert(
            key,
            TileImageEntry {
                tile: Arc::downgrade(tile),
                image: Arc::clone(&image),
            },
        );
        Ok(image)
    }

    fn retain_keys(&mut self, keys: &BTreeSet<TileImageKey>) {
        self.entries.retain(|key, _| keys.contains(key));
    }
}

struct RawTile {
    region: TileRegion,
    rect: ViewRect,
    tile: Arc<Tile>,
}

#[derive(Clone, Copy)]
struct TileRegion {
    col: u32,
    row: u32,
    width: u32,
    height: u32,
}

/// A rectangle of raster *pixels*, unlike `TileRegion` next to it whose
/// `col` and `row` are tile-grid indices that `tile_rect` multiplies by
/// `TILE_SIZE`. Separate types because the two are otherwise identical
/// and mixing them up is silent.
#[derive(Clone, Copy)]
struct RasterCrop {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
struct RasterPaintSource {
    page: PageIndex,
    zoom: f32,
    rect: ViewRect,
    rotation: ViewRotation,
}

/// The visible tiles of one page's cached raster.
///
/// The raster's dimensions come from the cache being cut, never from the
/// caller: `cols` and `rows` are derived from them, so `col * TILE_SIZE` is
/// below `raster_width` by construction and the remainder below cannot
/// underflow. A caller that passed its own size could disagree with the store
/// and did not have to be right.
fn collect_tiles(
    store: &mut TileStore,
    source: RasterPaintSource,
    viewport_size: ViewSize,
) -> Vec<RawTile> {
    let cache = store
        .get(source.page, source.zoom)
        .expect("the selected paint source has a tile cache");
    let (raster_width, raster_height) = (cache.base().width(), cache.base().height());
    let mut tiles = Vec::with_capacity((cache.cols() * cache.rows()) as usize);
    for row in 0..cache.rows() {
        for col in 0..cache.cols() {
            let region = TileRegion {
                col,
                row,
                width: TILE_SIZE.min(raster_width - col * TILE_SIZE),
                height: TILE_SIZE.min(raster_height - row * TILE_SIZE),
            };
            let rect = tile_rect(
                source.rect,
                region,
                (raster_width, raster_height),
                source.rotation,
            );
            if !rect_intersects_viewport(rect, viewport_size) {
                continue;
            }
            tiles.push(RawTile {
                region,
                rect,
                tile: cache.tile(col, row),
            });
        }
    }
    tiles
}

fn rect_intersects_viewport(rect: ViewRect, viewport: ViewSize) -> bool {
    rect.origin.x < viewport.width
        && rect.origin.y < viewport.height
        && rect.origin.x + rect.size.width > 0.0
        && rect.origin.y + rect.size.height > 0.0
}

/// A whole page raster as an image the window can paint, with the size it
/// came out at.
///
/// The thumbnails pane's one step out of pixels. It goes through the same
/// BGRA conversion the tiles do, cropping nothing and rotating nothing: a
/// thumbnail is the whole page, and the page's own `/Rotate` is already in
/// the raster the worker produced.
pub(super) fn raster_image(
    raster: &BaseRaster,
) -> Result<(Arc<RenderImage>, u32, u32), CanvasError> {
    let (width, height) = (raster.width(), raster.height());
    let (width, height, bgra) = tile_bgra(
        raster.rgba(),
        width,
        height,
        width,
        height,
        ViewRotation::None,
    )?;
    let buffer = image::RgbaImage::from_raw(width, height, bgra)
        .expect("the conversion returns exactly width*height*4 bytes");
    Ok((
        Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)])),
        width,
        height,
    ))
}

fn tile_bgra(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
    crop_width: u32,
    crop_height: u32,
    rotation: ViewRotation,
) -> Result<(u32, u32, Vec<u8>), CanvasError> {
    let expected = source_width as usize * source_height as usize * 4;
    if rgba.len() != expected {
        return Err(CanvasError::InvalidImageBuffer {
            expected,
            actual: rgba.len(),
        });
    }
    if crop_width == 0
        || crop_height == 0
        || crop_width > source_width
        || crop_height > source_height
    {
        return Err(CanvasError::InvalidImageCrop {
            width: source_width,
            height: source_height,
            crop_width,
            crop_height,
        });
    }

    let (output_width, output_height) = rotated_size(crop_width, crop_height, rotation);
    let mut output = vec![0; output_width as usize * output_height as usize * 4];
    for y in 0..crop_height {
        for x in 0..crop_width {
            let source = (y as usize * source_width as usize + x as usize) * 4;
            let pixel = unpremultiplied_bgra(&rgba[source..source + 4]);
            let (dx, dy) = rotate_pixel(x, y, crop_width, crop_height, rotation);
            let destination = (dy as usize * output_width as usize + dx as usize) * 4;
            output[destination..destination + 4].copy_from_slice(&pixel);
        }
    }
    Ok((output_width, output_height, output))
}

fn atlas_tile_bgra(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
    crop_width: u32,
    crop_height: u32,
    rotation: ViewRotation,
) -> Result<(u32, u32, Vec<u8>), CanvasError> {
    let (width, height, pixels) = tile_bgra(
        rgba,
        source_width,
        source_height,
        crop_width,
        crop_height,
        rotation,
    )?;
    let output_width = width + 2;
    let output_height = height + 2;
    let mut output = vec![0; output_width as usize * output_height as usize * 4];

    // GPUI linearly samples to exact atlas-allocation edges. Keep visible
    // samples inside duplicate pixels so adjacent atlas entries cannot bleed.
    for y in 0..output_height {
        let source_y = y.saturating_sub(1).min(height - 1);
        for x in 0..output_width {
            let source_x = x.saturating_sub(1).min(width - 1);
            let source = (source_y as usize * width as usize + source_x as usize) * 4;
            let destination = (y as usize * output_width as usize + x as usize) * 4;
            output[destination..destination + 4].copy_from_slice(&pixels[source..source + 4]);
        }
    }

    Ok((output_width, output_height, output))
}

fn unpremultiplied_bgra(rgba: &[u8]) -> [u8; 4] {
    let [red, green, blue, alpha] = unpremultiplied_rgba(rgba);
    [blue, green, red, alpha]
}

/// The rasters are premultiplied; PNG is not, and neither is what a paste
/// target expects.
fn unpremultiplied_rgba(rgba: &[u8]) -> [u8; 4] {
    let alpha = rgba[3];
    if alpha == 0 {
        return [0, 0, 0, 0];
    }
    let straight = |channel: u8| ((u16::from(channel) * 255 / u16::from(alpha)).min(255)) as u8;
    [
        straight(rgba[0]),
        straight(rgba[1]),
        straight(rgba[2]),
        alpha,
    ]
}

/// Where a page-space region lands in a raster's pixels, clipped to it.
///
/// The transform is exact but the drag is not, so a corner may sit a
/// fraction outside the page; the region is rounded outwards first so a
/// thin selection still covers the pixels it touches.
fn raster_crop(
    geometry: &PageGeometry,
    region: PageRect,
    source: &BaseRaster,
) -> Result<RasterCrop, CanvasError> {
    let quad = geometry.user_to_device(region.into(), source.zoom())?;
    // `f64::min` and `f64::max` ignore a NaN operand, so a partly non-finite
    // quad would silently crop from whichever corners survived.
    if quad
        .corners
        .iter()
        .any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return Err(CanvasError::SnapshotEmpty { page: region.page });
    }
    let (left, right) = device_span(quad.corners.map(|(x, _)| x), source.width());
    let (top, bottom) = device_span(quad.corners.map(|(_, y)| y), source.height());
    if right <= left || bottom <= top {
        return Err(CanvasError::SnapshotEmpty { page: region.page });
    }
    Ok(RasterCrop {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn device_span(values: [f64; 4], limit: u32) -> (u32, u32) {
    let (min, max) = values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), value| {
            (min.min(*value), max.max(*value))
        });
    let clamp = |value: f64| value.clamp(0.0, f64::from(limit)) as u32;
    (clamp(min.floor()), clamp(max.ceil()))
}

/// The cropped region, turned by the view rotation and encoded as PNG: the
/// one image format gpui's clipboard entry and every paste target agree on.
fn encode_snapshot(
    source: &BaseRaster,
    crop: RasterCrop,
    rotation: ViewRotation,
) -> Result<Vec<u8>, CanvasError> {
    let (width, height) = rotated_size(crop.width, crop.height, rotation);
    let mut pixels = vec![0; width as usize * height as usize * 4];
    let rgba = source.rgba();
    let stride = source.width() as usize;
    for y in 0..crop.height {
        for x in 0..crop.width {
            let read = ((crop.y + y) as usize * stride + (crop.x + x) as usize) * 4;
            let pixel = unpremultiplied_rgba(&rgba[read..read + 4]);
            let (dx, dy) = rotate_pixel(x, y, crop.width, crop.height, rotation);
            let write = (dy as usize * width as usize + dx as usize) * 4;
            pixels[write..write + 4].copy_from_slice(&pixel);
        }
    }

    let image = image::RgbaImage::from_raw(width, height, pixels)
        .expect("the snapshot buffer is width * height * 4 bytes");
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| CanvasError::SnapshotEncode(error.to_string()))?;
    Ok(png.into_inner())
}

fn rotate_pixel(x: u32, y: u32, width: u32, height: u32, rotation: ViewRotation) -> (u32, u32) {
    match rotation {
        ViewRotation::None => (x, y),
        ViewRotation::Clockwise90 => (height - 1 - y, x),
        ViewRotation::HalfTurn => (width - 1 - x, height - 1 - y),
        ViewRotation::Clockwise270 => (y, width - 1 - x),
    }
}

fn rotated_size(width: u32, height: u32, rotation: ViewRotation) -> (u32, u32) {
    match rotation {
        ViewRotation::None | ViewRotation::HalfTurn => (width, height),
        ViewRotation::Clockwise90 | ViewRotation::Clockwise270 => (height, width),
    }
}

fn tile_rect(
    page: ViewRect,
    tile: TileRegion,
    raster: (u32, u32),
    rotation: ViewRotation,
) -> ViewRect {
    let (raster_width, raster_height) = raster;
    let TileRegion {
        col,
        row,
        width,
        height,
    } = tile;
    let x = col * TILE_SIZE;
    let y = row * TILE_SIZE;
    let (x, y, width, height) = match rotation {
        ViewRotation::None => (x, y, width, height),
        ViewRotation::Clockwise90 => (raster_height - y - height, x, height, width),
        ViewRotation::HalfTurn => (
            raster_width - x - width,
            raster_height - y - height,
            width,
            height,
        ),
        ViewRotation::Clockwise270 => (y, raster_width - x - width, height, width),
    };
    let (output_width, output_height) = rotated_size(raster_width, raster_height, rotation);
    let scale_x = page.size.width / output_width as f32;
    let scale_y = page.size.height / output_height as f32;
    ViewRect {
        origin: ViewPoint {
            x: page.origin.x + x as f32 * scale_x,
            y: page.origin.y + y as f32 * scale_y,
        },
        size: ViewSize {
            width: width as f32 * scale_x,
            height: height as f32 * scale_y,
        },
    }
}

fn atlas_image_rect(clip: ViewRect, content_width: u32, content_height: u32) -> ViewRect {
    let gutter_width = clip.size.width / content_width as f32;
    let gutter_height = clip.size.height / content_height as f32;
    ViewRect {
        origin: ViewPoint {
            x: clip.origin.x - gutter_width,
            y: clip.origin.y - gutter_height,
        },
        size: ViewSize {
            width: clip.size.width + 2.0 * gutter_width,
            height: clip.size.height + 2.0 * gutter_height,
        },
    }
}

fn rotation_code(rotation: ViewRotation) -> u8 {
    match rotation {
        ViewRotation::None => 0,
        ViewRotation::Clockwise90 => 1,
        ViewRotation::HalfTurn => 2,
        ViewRotation::Clockwise270 => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use onionskin_core::{Error as CoreError, PageAlignment, PageLayoutMode};
    use onionskin_plugin_api::{ToolCtx, ToolPlugin};
    use onionskin_render::{InterpreterWarning, PageRender, RenderError};

    use super::*;

    const VIEWPORT: ViewSize = ViewSize {
        width: 800.0,
        height: 600.0,
    };

    fn model() -> CanvasModel {
        model_with_registry(PluginRegistry::new())
    }

    fn model_with_registry(registry: PluginRegistry) -> CanvasModel {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
        CanvasModel::new(
            Document::open_path(&path).expect("seed opens"),
            registry,
            VIEWPORT,
        )
        .expect("canvas starts")
    }

    fn assert_view_change_matches(
        model: &mut CanvasModel,
        direct: &mut CanvasModel,
        model_change: impl FnOnce(&mut CanvasModel) -> Result<bool, CanvasError>,
        direct_change: impl FnOnce(&mut Viewport) -> Result<(), ViewportError>,
    ) {
        let before = direct.viewport.snapshot();
        direct_change(&mut direct.viewport).unwrap();
        let expected_changed = direct.viewport.snapshot() != before;

        assert_eq!(model_change(model).unwrap(), expected_changed);
        assert_eq!(model.viewport.snapshot(), direct.viewport.snapshot());
    }

    fn direct_viewport_center(viewport: &Viewport) -> ViewPoint {
        let size = viewport.size();
        ViewPoint {
            x: size.width / 2.0,
            y: size.height / 2.0,
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum RecordedToolEvent {
        Down(PointerInput),
        Move(PointerInput),
        Up(PointerInput),
        Cancel,
    }

    struct RecordingTool {
        events: Arc<Mutex<Vec<RecordedToolEvent>>>,
    }

    impl ToolPlugin for RecordingTool {
        fn id(&self) -> &'static str {
            "recording"
        }

        fn name(&self) -> &'static str {
            "Recording"
        }

        fn icon(&self) -> &'static str {
            "recording"
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
            self.events
                .lock()
                .unwrap()
                .push(RecordedToolEvent::Down(input));
        }

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
            self.events
                .lock()
                .unwrap()
                .push(RecordedToolEvent::Move(input));
        }

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
            self.events
                .lock()
                .unwrap()
                .push(RecordedToolEvent::Up(input));
        }

        fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
            self.events.lock().unwrap().push(RecordedToolEvent::Cancel);
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum LifecycleEvent {
        Activate(&'static str),
        Down(&'static str),
        Cancel(&'static str),
        Deactivate(&'static str),
    }

    struct LifecycleTool {
        id: &'static str,
        events: Arc<Mutex<Vec<LifecycleEvent>>>,
    }

    impl LifecycleTool {
        fn record(&self, event: LifecycleEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    impl ToolPlugin for LifecycleTool {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            self.id
        }

        fn icon(&self) -> &'static str {
            self.id
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {
            self.record(LifecycleEvent::Down(self.id));
        }

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_activate(&mut self, _ctx: &mut ToolCtx) {
            self.record(LifecycleEvent::Activate(self.id));
        }

        fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
            self.record(LifecycleEvent::Cancel(self.id));
        }

        fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
            self.record(LifecycleEvent::Deactivate(self.id));
        }
    }

    fn lifecycle_model() -> (CanvasModel, Arc<Mutex<Vec<LifecycleEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut registry = PluginRegistry::new();
        for id in ["first", "second"] {
            registry.register_tool(Box::new(LifecycleTool {
                id,
                events: Arc::clone(&events),
            }));
        }
        (model_with_registry(registry), events)
    }

    fn page_center(model: &CanvasModel) -> Point<Pixels> {
        let page = model.viewport().visible_pages().unwrap()[0].rect;
        point(
            px(page.origin.x + page.size.width / 2.0),
            px(page.origin.y + page.size.height / 2.0),
        )
    }

    fn prepare_request(model: &mut CanvasModel) -> RenderRequest {
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.tiles.begin_frame();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        *model.requests.get(&0).expect("page zero is requested")
    }

    /// The canvas half of the P4 review's layer note. The store is keyed by
    /// page and zoom, not by the render options, so nothing in it would be
    /// rebuilt on its own; and `sources` holds the same rasters for
    /// placeholders, so leaving them would put the old layer state back on
    /// screen the moment the page was scrolled.
    #[test]
    fn toggling_a_layer_drops_every_cached_pixel_and_asks_for_the_pages_again() {
        let mut model = CanvasModel::new(
            Document::open_bytes(crate::shell::fixtures::optional_content_pdf())
                .expect("the fixture opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts");
        let request = prepare_request(&mut model);
        assert!(model.apply_render_response(RenderResponse::Raster {
            request,
            render: PageRender {
                raster: raster(&model, request.page, request.zoom, [10, 10, 10, 255]),
                warnings: Vec::new(),
            },
        }));
        assert_eq!(model.tiles.len(), 1, "there is a cached raster to lose");
        assert!(!model.sources.is_empty());
        let generation = model.generation;
        let layer = model.layers().expect("the layers read")[0].clone();

        assert!(model
            .set_layer_visible(layer.id, false)
            .expect("an unlocked layer toggles"));

        assert_eq!(model.tiles.len(), 0, "the cached composites are stale");
        assert!(
            model.sources.is_empty(),
            "a placeholder built from the old raster would show the old layers again"
        );
        assert!(model.requests.is_empty());
        assert!(
            model.signature.is_none(),
            "the visible set did not change, so only a cleared signature makes the next frame re-request it"
        );

        // The next frame advances the generation, which is what drops the
        // answers still in flight from before the toggle, and asks for the
        // page again.
        let visible = model.viewport.visible_pages().unwrap();
        assert!(model.update_signature(&visible).unwrap());
        assert!(model.generation > generation);
        model.tiles.begin_frame();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
    }

    /// A toggle that changes nothing costs nothing: the pixels stay, because
    /// re-rendering them would produce the same picture.
    #[test]
    fn a_toggle_to_the_state_a_layer_is_in_keeps_the_cached_pixels() {
        let mut model = CanvasModel::new(
            Document::open_bytes(crate::shell::fixtures::optional_content_pdf())
                .expect("the fixture opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts");
        let request = prepare_request(&mut model);
        assert!(model.apply_render_response(RenderResponse::Raster {
            request,
            render: PageRender {
                raster: raster(&model, request.page, request.zoom, [10, 10, 10, 255]),
                warnings: Vec::new(),
            },
        }));
        let layer = model.layers().expect("the layers read")[0].clone();

        assert!(!model
            .set_layer_visible(layer.id, layer.visible)
            .expect("a no-op toggle is not an error"));

        assert_eq!(model.tiles.len(), 1);
        assert!(model.signature.is_some());
    }

    fn raster(model: &CanvasModel, page: PageIndex, zoom: f32, rgba: [u8; 4]) -> BaseRaster {
        let geometry = model.viewport.page_geometry(page).unwrap();
        let (width, height) = onionskin_render::raster_size(
            geometry.render_size.0 as f32,
            geometry.render_size.1 as f32,
            zoom,
        )
        .unwrap();
        BaseRaster::new(
            u32::from(width),
            u32::from(height),
            zoom,
            rgba.repeat(usize::from(width) * usize::from(height)),
        )
    }

    fn raster_response(request: RenderRequest, raster: BaseRaster) -> RenderResponse {
        RenderResponse::Raster {
            request,
            render: PageRender {
                raster,
                warnings: Vec::new(),
            },
        }
    }

    struct OverlayTool {
        overlays: Vec<Overlay>,
    }

    impl ToolPlugin for OverlayTool {
        fn id(&self) -> &'static str {
            "overlay"
        }

        fn name(&self) -> &'static str {
            "Overlay"
        }

        fn icon(&self) -> &'static str {
            "overlay"
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
            self.overlays.clone()
        }
    }

    fn overlay_model(overlays: Vec<Overlay>) -> CanvasModel {
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(OverlayTool { overlays }));
        let mut model = model_with_registry(registry);
        model.update().expect("the first frame runs");
        model
    }

    fn selection_quad() -> PageQuad {
        PageQuad {
            page: 0,
            corners: [(72.0, 720.0), (144.0, 720.0), (72.0, 700.0), (144.0, 700.0)],
        }
    }

    #[test]
    fn a_text_selection_overlay_is_mapped_corner_by_corner_through_the_page_transform() {
        let mut model = overlay_model(vec![Overlay::Quads(vec![selection_quad()])]);

        let overlays = model.paint_list().expect("the frame paints").overlays;

        let expected: Vec<ViewPoint> = selection_quad()
            .corners
            .iter()
            .map(|&(x, y)| {
                model
                    .viewport
                    .view_point_for(PagePoint { page: 0, x, y })
                    .expect("page zero is measured")
                    .expect("page zero is laid out")
            })
            .collect();
        assert_eq!(
            overlays,
            vec![OverlayPaint::Quads(vec![[
                expected[0],
                expected[1],
                expected[2],
                expected[3]
            ]])]
        );
        assert!(model.status().is_none());
    }

    /// The quad is axis-aligned in page space, so a quarter turn has to swap
    /// the extent it covers on screen. Painting the page-space extent under a
    /// rotated view would leave the highlight off the glyphs it selects.
    #[test]
    fn a_selection_overlay_follows_the_view_rotation() {
        let mut model = overlay_model(vec![Overlay::Quads(vec![selection_quad()])]);
        let upright = overlay_bounds(&mut model);

        model
            .set_rotation(ViewRotation::Clockwise90)
            .expect("the view rotates");
        model.update().expect("the rotated frame runs");
        let turned = overlay_bounds(&mut model);

        // The quarter turn refits the page, so the overlay changes size as
        // well as orientation; what has to invert is its aspect.
        let upright_aspect = upright.size.width / upright.size.height;
        let turned_aspect = turned.size.height / turned.size.width;
        assert!(
            upright_aspect > 1.0,
            "the selection is wider than it is tall"
        );
        assert!((upright_aspect - turned_aspect).abs() < 0.01);
    }

    #[test]
    fn a_marquee_overlay_becomes_the_bounding_rectangle_of_its_mapped_corners() {
        let region = PageRect {
            page: 0,
            x0: 72.0,
            y0: 700.0,
            x1: 144.0,
            y1: 720.0,
        };
        let mut model = overlay_model(vec![Overlay::AntsRect(region)]);
        model
            .set_rotation(ViewRotation::Clockwise90)
            .expect("the view rotates");
        model.update().expect("the rotated frame runs");

        let overlays = model.paint_list().expect("the frame paints").overlays;

        let corners: Vec<ViewPoint> = PageQuad::from(region)
            .corners
            .iter()
            .map(|&(x, y)| {
                model
                    .viewport
                    .view_point_for(PagePoint { page: 0, x, y })
                    .expect("page zero is measured")
                    .expect("page zero is laid out")
            })
            .collect();
        let expected = bounding_rect([corners[0], corners[1], corners[2], corners[3]]);
        assert_eq!(overlays, vec![OverlayPaint::AntsRect(expected)]);
        assert!(expected.size.width > 0.0 && expected.size.height > 0.0);
    }

    /// Pages measure lazily and a layout mode shows only some of them, so an
    /// overlay the viewport cannot place yet is a normal frame, not an error.
    #[test]
    fn an_overlay_on_a_page_the_layout_cannot_place_is_dropped_quietly() {
        let mut model = overlay_model(vec![Overlay::Quads(vec![PageQuad {
            page: 9,
            corners: [(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)],
        }])]);

        let overlays = model.paint_list().expect("the frame paints").overlays;

        assert!(overlays.is_empty());
        assert!(model.status().is_none());
    }

    /// The plugin API has six overlay shapes and this canvas paints two. The
    /// four with no painter are reported, because a tool asking for one is
    /// asking for something the user would otherwise never see.
    #[test]
    fn an_overlay_shape_the_canvas_cannot_paint_is_reported() {
        let mut model = overlay_model(vec![Overlay::Circle {
            center: PagePoint {
                page: 0,
                x: 100.0,
                y: 700.0,
            },
            radius: 4.0,
        }]);

        let overlays = model.paint_list().expect("the frame paints").overlays;

        assert!(overlays.is_empty());
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { message, .. }) if message.contains("circle")
        ));
    }

    fn overlay_bounds(model: &mut CanvasModel) -> ViewRect {
        match model
            .paint_list()
            .expect("the frame paints")
            .overlays
            .as_slice()
        {
            [OverlayPaint::Quads(quads)] => bounding_rect(quads[0]),
            other => panic!("expected one selection overlay, got {other:?}"),
        }
    }

    /// A model with page zero rendered and painted once, which is what puts
    /// a raster in `sources` for the snapshot path to crop.
    fn painted_model(rgba: [u8; 4]) -> CanvasModel {
        let mut model = model();
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, rgba);
        assert!(model.apply_render_response(raster_response(request, rendered)));
        model.paint_list().expect("the frame paints");
        model
    }

    /// Well inside the seed's 200x100 pt page, and wider than it is tall so
    /// a quarter turn is visible in the encoded dimensions.
    fn snapshot_region() -> PageRect {
        PageRect {
            page: 0,
            x0: 20.0,
            y0: 20.0,
            x1: 120.0,
            y1: 60.0,
        }
    }

    fn decode(png: &[u8]) -> image::RgbaImage {
        image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .expect("the snapshot is a PNG")
            .to_rgba8()
    }

    #[test]
    fn a_snapshot_request_crops_the_raster_the_canvas_is_painting() {
        let mut model = painted_model([128, 0, 0, 128]);
        model.document.request_snapshot(snapshot_region());

        let png = model
            .take_snapshot_png()
            .expect("the snapshot is produced")
            .expect("a request was pending");

        let source = model.sources.get(&0).expect("page zero is rendered");
        let geometry = model.viewport.page_geometry(0).unwrap().clone();
        let expected = raster_crop(&geometry, snapshot_region(), source).unwrap();
        let decoded = decode(&png);
        assert_eq!(decoded.dimensions(), (expected.width, expected.height));
        // The rasters are premultiplied and PNG is not, so a half-opaque
        // dark red comes back as the colour it was drawn in.
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 128]);
    }

    /// Acrobat's snapshot copies the page as it is displayed, so a quarter
    /// turn has to reach the clipboard as a turned image.
    #[test]
    fn a_snapshot_turns_with_the_view() {
        let mut model = painted_model([255, 255, 255, 255]);
        model.document.request_snapshot(snapshot_region());
        let upright = decode(&model.take_snapshot_png().unwrap().unwrap()).dimensions();

        model.set_rotation(ViewRotation::Clockwise90).unwrap();
        model.update().expect("the rotated frame runs");
        model.document.request_snapshot(snapshot_region());
        let turned = decode(&model.take_snapshot_png().unwrap().unwrap()).dimensions();

        assert_eq!(turned, (upright.1, upright.0));
        assert!(upright.0 > upright.1);
    }

    #[test]
    fn a_snapshot_of_a_page_that_is_not_on_screen_fails_loudly() {
        let mut model = painted_model([255, 255, 255, 255]);
        model.document.request_snapshot(PageRect {
            page: 1,
            ..snapshot_region()
        });

        assert!(matches!(
            model.take_snapshot_png(),
            Err(CanvasError::SnapshotUnrendered { page: 1 })
        ));
    }

    /// Half off the page is the case a real drag produces most often, and
    /// the only one that exercises the clamp: what survives is the part
    /// that covers pixels, not an error and not the whole region.
    #[test]
    fn a_snapshot_region_hanging_off_the_page_keeps_the_part_that_covers_pixels() {
        let mut model = painted_model([255, 255, 255, 255]);
        let inside = decode(&{
            model.document.request_snapshot(snapshot_region());
            model.take_snapshot_png().unwrap().unwrap()
        })
        .dimensions();

        // The same rectangle slid left so its left half hangs off the page.
        model.document.request_snapshot(PageRect {
            x0: -50.0,
            x1: 50.0,
            ..snapshot_region()
        });
        let clipped = decode(&model.take_snapshot_png().unwrap().unwrap()).dimensions();

        assert_eq!(clipped.1, inside.1, "the vertical span is untouched");
        assert!(clipped.0 < inside.0, "the overhanging half is dropped");
        assert!(clipped.0 > 0);
    }

    /// A region entirely off the page would crop nothing, and an empty PNG
    /// on the clipboard is worse than a message saying why there is none.
    #[test]
    fn a_snapshot_region_off_the_page_fails_loudly() {
        let mut model = painted_model([255, 255, 255, 255]);
        model.document.request_snapshot(PageRect {
            page: 0,
            x0: -400.0,
            y0: 20.0,
            x1: -300.0,
            y1: 60.0,
        });

        assert!(matches!(
            model.take_snapshot_png(),
            Err(CanvasError::SnapshotEmpty { page: 0 })
        ));
    }

    #[test]
    fn a_frame_with_no_pending_request_produces_no_snapshot() {
        let mut model = painted_model([255, 255, 255, 255]);

        assert!(model.take_snapshot_png().unwrap().is_none());
    }

    #[test]
    fn the_snapshot_tool_reaches_the_canvas_through_the_snapshot_request() {
        let mut model = painted_model([255, 255, 255, 255]);
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(onionskin_tools_basic::SnapshotTool::new()));
        model.registry = registry;
        model.activate_tool(0).expect("the snapshot tool activates");

        let start = page_center(&model);
        model
            .pointer_down(start, 1.0, GpuiModifiers::default())
            .unwrap();
        let end = point(start.x + px(60.0), start.y + px(40.0));
        model
            .pointer_move(end, 1.0, GpuiModifiers::default(), true)
            .unwrap();
        model
            .pointer_up(end, 1.0, GpuiModifiers::default())
            .unwrap();

        assert!(model.document.selection().region().is_some());
        assert!(model.take_snapshot_png().unwrap().is_some());
    }

    #[test]
    fn the_default_active_tool_receives_page_space_pointer_input() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(RecordingTool {
            events: Arc::clone(&events),
        }));
        let mut model = model_with_registry(registry);
        let origin = ViewPoint { x: 50.0, y: 40.0 };
        model.resize(origin, VIEWPORT).unwrap();
        assert_eq!(model.active_tool(), Some(0));

        let page = model.viewport().visible_pages().unwrap()[0].rect;
        let local = [
            ViewPoint {
                x: page.origin.x + page.size.width * 0.4,
                y: page.origin.y + page.size.height * 0.4,
            },
            ViewPoint {
                x: page.origin.x + page.size.width * 0.5,
                y: page.origin.y + page.size.height * 0.5,
            },
            ViewPoint {
                x: page.origin.x + page.size.width * 0.6,
                y: page.origin.y + page.size.height * 0.6,
            },
        ];
        let positions = local.map(|at| point(px(at.x + origin.x), px(at.y + origin.y)));
        let modifiers = [
            GpuiModifiers {
                shift: true,
                ..Default::default()
            },
            GpuiModifiers {
                alt: true,
                ..Default::default()
            },
            GpuiModifiers {
                platform: true,
                ..Default::default()
            },
        ];
        let pressures = [0.25, 0.5, 0.75];
        let expected: [PointerInput; 3] = std::array::from_fn(|index| {
            pointer_input(
                model.viewport(),
                positions[index],
                point(px(origin.x), px(origin.y)),
                pressures[index],
                modifiers[index],
            )
            .unwrap()
            .unwrap()
        });

        assert!(model
            .pointer_down(positions[0], pressures[0], modifiers[0])
            .unwrap());
        assert!(model
            .pointer_move(positions[1], pressures[1], modifiers[1], true)
            .unwrap());
        assert!(model
            .pointer_up(positions[2], pressures[2], modifiers[2])
            .unwrap());

        assert_eq!(
            *events.lock().unwrap(),
            [
                RecordedToolEvent::Down(expected[0]),
                RecordedToolEvent::Move(expected[1]),
                RecordedToolEvent::Up(expected[2]),
            ]
        );
    }

    #[test]
    fn the_initial_registered_tool_is_activated_exactly_once() {
        let (model, events) = lifecycle_model();

        assert_eq!(model.active_tool(), Some(0));
        assert_eq!(*events.lock().unwrap(), [LifecycleEvent::Activate("first")]);
    }

    #[test]
    fn switching_tools_deactivates_then_activates_and_same_tool_is_a_no_op() {
        let (mut model, events) = lifecycle_model();

        assert!(model.activate_tool(1).unwrap());
        assert_eq!(model.active_tool(), Some(1));
        assert!(!model.activate_tool(1).unwrap());
        assert_eq!(
            *events.lock().unwrap(),
            [
                LifecycleEvent::Activate("first"),
                LifecycleEvent::Deactivate("first"),
                LifecycleEvent::Activate("second"),
            ]
        );
    }

    #[test]
    fn switching_during_a_gesture_cancels_before_deactivation_and_activation() {
        let (mut model, events) = lifecycle_model();
        let position = page_center(&model);
        assert!(model
            .pointer_down(position, 1.0, GpuiModifiers::default())
            .unwrap());

        assert!(model.activate_tool(1).unwrap());

        assert_eq!(
            *events.lock().unwrap(),
            [
                LifecycleEvent::Activate("first"),
                LifecycleEvent::Down("first"),
                LifecycleEvent::Cancel("first"),
                LifecycleEvent::Deactivate("first"),
                LifecycleEvent::Activate("second"),
            ]
        );
    }

    #[test]
    fn selecting_the_active_tool_preserves_its_gesture() {
        let (mut model, events) = lifecycle_model();
        let position = page_center(&model);
        assert!(model
            .pointer_down(position, 1.0, GpuiModifiers::default())
            .unwrap());
        let before = events.lock().unwrap().clone();

        assert!(!model.activate_tool(0).unwrap());
        assert_eq!(*events.lock().unwrap(), before);
        assert!(model.cancel_pointer_gesture());
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [
                LifecycleEvent::Activate("first"),
                LifecycleEvent::Down("first"),
                LifecycleEvent::Cancel("first")
            ]
        ));
    }

    #[test]
    fn out_of_range_activation_preserves_the_active_tool_gesture_and_events() {
        let (mut model, events) = lifecycle_model();
        let position = page_center(&model);
        assert!(model
            .pointer_down(position, 1.0, GpuiModifiers::default())
            .unwrap());
        let before = events.lock().unwrap().clone();

        assert!(matches!(
            model.activate_tool(2),
            Err(CanvasError::ToolOutOfRange { index: 2, count: 2 })
        ));
        assert_eq!(model.active_tool(), Some(0));
        assert_eq!(*events.lock().unwrap(), before);
        assert!(model.cancel_pointer_gesture());
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [
                LifecycleEvent::Activate("first"),
                LifecycleEvent::Down("first"),
                LifecycleEvent::Cancel("first")
            ]
        ));
    }

    #[test]
    fn pointer_dispatch_follows_the_newly_activated_tool() {
        let (mut model, events) = lifecycle_model();
        assert!(model.activate_tool(1).unwrap());

        assert!(model
            .pointer_down(page_center(&model), 1.0, GpuiModifiers::default())
            .unwrap());

        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [
                LifecycleEvent::Activate("first"),
                LifecycleEvent::Deactivate("first"),
                LifecycleEvent::Activate("second"),
                LifecycleEvent::Down("second")
            ]
        ));
    }

    #[test]
    fn leaving_the_window_cancels_an_active_tool_gesture() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(RecordingTool {
            events: Arc::clone(&events),
        }));
        let mut model = model_with_registry(registry);
        let page = model.viewport().visible_pages().unwrap()[0].rect;
        let position = point(
            px(page.origin.x + page.size.width / 2.0),
            px(page.origin.y + page.size.height / 2.0),
        );

        assert!(model
            .pointer_down(position, 1.0, GpuiModifiers::default())
            .unwrap());
        assert!(model.cancel_pointer_gesture());
        assert!(!model.cancel_pointer_gesture());

        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [RecordedToolEvent::Down(_), RecordedToolEvent::Cancel]
        ));
    }

    #[test]
    fn an_empty_registry_drag_pans_through_the_viewport() {
        let mut model = model();
        assert_eq!(model.active_tool(), None);
        let before = model.viewport().offset();

        assert!(model
            .pointer_down(point(px(400.0), px(300.0)), 1.0, GpuiModifiers::default(),)
            .unwrap());
        assert!(model
            .pointer_move(
                point(px(400.0), px(400.0)),
                1.0,
                GpuiModifiers::default(),
                true,
            )
            .unwrap());
        assert!(model
            .pointer_up(point(px(400.0), px(400.0)), 1.0, GpuiModifiers::default(),)
            .unwrap());

        let panned = model.viewport().offset();
        assert_ne!(panned, before);
        model
            .resize(ViewPoint { x: 25.0, y: 15.0 }, VIEWPORT)
            .unwrap();
        assert_eq!(model.viewport().offset(), panned);
        assert!(!model
            .pointer_move(
                point(px(500.0), px(350.0)),
                1.0,
                GpuiModifiers::default(),
                true,
            )
            .unwrap());
    }

    #[test]
    fn estimated_geometry_is_queued_without_a_synchronous_measurement() {
        let mut model = model();
        model.viewport.go_to_page(1, PageAlignment::Start).unwrap();

        assert!(model.viewport.page_geometry(1).is_none());
        assert_eq!(model.queue_visible_geometry().unwrap(), 1);
        assert!(model.viewport.page_geometry(1).is_none());
        assert_eq!(model.queue_visible_geometry().unwrap(), 0);
        assert!(model.has_pending_work());
        assert!(model.geometry_requests.contains(&1));
    }

    #[test]
    fn view_command_wrappers_match_direct_viewport_behavior() {
        let mut subject = model();
        let mut direct = model();

        assert_view_change_matches(
            &mut subject,
            &mut direct,
            |model| model.go_to_page(1),
            |viewport| viewport.go_to_page(1, PageAlignment::Start),
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::first_page,
            Viewport::first_page,
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::last_page,
            Viewport::last_page,
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::previous_page,
            Viewport::previous_page,
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::next_page,
            Viewport::next_page,
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::zoom_in,
            |viewport| {
                let anchor = direct_viewport_center(viewport);
                viewport.zoom_in(anchor)
            },
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::zoom_out,
            |viewport| {
                let anchor = direct_viewport_center(viewport);
                viewport.zoom_out(anchor)
            },
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            |model| model.zoom_to(1.5),
            |viewport| {
                let anchor = direct_viewport_center(viewport);
                viewport.zoom_to(1.5, anchor)
            },
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            |model| model.fit(FitMode::Width),
            |viewport| viewport.fit(FitMode::Width),
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::actual_size,
            Viewport::actual_size,
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            |model| model.set_rotation(ViewRotation::Clockwise90),
            |viewport| viewport.set_rotation(ViewRotation::Clockwise90),
        );
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            CanvasModel::rotate_clockwise,
            |viewport| viewport.set_rotation(ViewRotation::HalfTurn),
        );
        for mode in [
            PageLayoutMode::SinglePage,
            PageLayoutMode::SinglePageContinuous,
            PageLayoutMode::TwoPage,
            PageLayoutMode::TwoPageContinuous,
        ] {
            assert_view_change_matches(
                &mut subject,
                &mut direct,
                |model| model.set_layout_mode(mode),
                |viewport| viewport.set_mode(mode),
            );
        }
        assert_view_change_matches(
            &mut subject,
            &mut direct,
            |model| model.set_show_cover(true),
            |viewport| viewport.set_show_cover(true),
        );
    }

    #[test]
    fn previous_and_next_view_restore_every_view_state_field() {
        let mut model = model();
        let mut states = vec![model.viewport.snapshot()];

        assert!(model.set_layout_mode(PageLayoutMode::TwoPage).unwrap());
        states.push(model.viewport.snapshot());
        assert!(model.set_show_cover(true).unwrap());
        states.push(model.viewport.snapshot());
        assert!(model.set_rotation(ViewRotation::Clockwise90).unwrap());
        states.push(model.viewport.snapshot());
        assert!(model.zoom_to(1.5).unwrap());
        states.push(model.viewport.snapshot());
        let target_page = if model.viewport.current_page() == 0 {
            1
        } else {
            0
        };
        assert!(model.go_to_page(target_page).unwrap());
        states.push(model.viewport.snapshot());

        for expected in states.iter().rev().skip(1) {
            assert!(model.can_previous_view());
            assert!(model.previous_view().unwrap());
            assert_eq!(&model.viewport.snapshot(), expected);
        }
        assert!(!model.can_previous_view());

        for expected in states.iter().skip(1) {
            assert!(model.can_next_view());
            assert!(model.next_view().unwrap());
            assert_eq!(&model.viewport.snapshot(), expected);
        }
        assert!(!model.can_next_view());
    }

    #[test]
    fn no_op_view_commands_do_not_create_history() {
        let mut model = model();

        assert!(!model.set_show_cover(false).unwrap());
        assert!(!model.can_previous_view());
        assert!(!model.can_next_view());
    }

    #[test]
    fn failed_view_commands_do_not_create_history() {
        let mut model = model();
        let page_count = model.viewport.page_count();

        assert!(model.go_to_page(page_count).is_err());
        assert!(!model.can_previous_view());
        assert!(!model.can_next_view());
    }

    #[test]
    fn continuous_pan_does_not_add_view_history_entries() {
        let mut model = model();
        let initial = model.viewport.snapshot();

        assert!(model.zoom_to(2.0).unwrap());
        assert!(model
            .pointer_down(point(px(400.0), px(300.0)), 1.0, GpuiModifiers::default())
            .unwrap());
        for y in [320.0, 340.0, 360.0, 380.0, 400.0] {
            assert!(model
                .pointer_move(point(px(400.0), px(y)), 1.0, GpuiModifiers::default(), true,)
                .unwrap());
        }
        assert!(model
            .pointer_up(point(px(400.0), px(400.0)), 1.0, GpuiModifiers::default())
            .unwrap());

        assert!(model.previous_view().unwrap());
        assert_eq!(model.viewport.snapshot(), initial);
        assert!(!model.can_previous_view());
    }

    #[test]
    fn successful_view_commands_reuse_render_generation_scheduling() {
        let mut model = model();
        model.update().unwrap();
        let generation = model.generation();

        assert!(model.zoom_to(2.0).unwrap());
        model.update().unwrap();

        assert!(model.generation() > generation);
        assert!(model.has_pending_render());
    }

    /// Fails page 1's geometry with the view already pinned, so nothing in
    /// the test clears the failure on its own.
    fn model_with_failed_geometry() -> CanvasModel {
        let mut model = model();
        model.viewport.go_to_page(1, PageAlignment::Start).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.geometry_requests.insert(1);
        model
            .apply_geometry_response(PageGeometryResponse::Failed {
                page: 1,
                error: CoreError::NoSuchPage { page: 1, count: 1 },
            })
            .unwrap();
        model
    }

    #[test]
    fn geometry_failure_is_visible() {
        let mut model = model_with_failed_geometry();

        assert!(!model.geometry_requests.contains(&1));
        assert!(model.failed_geometry.contains(&1));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { page: Some(1), message })
                if message.contains("outside a 1-page document")
        ));
        assert_eq!(model.queue_visible_geometry().unwrap(), 0);
        let visible = model.viewport.visible_pages().unwrap();
        assert!(!model.update_signature(&visible).unwrap());
        assert!(!model.geometry_requests.contains(&1));
        assert!(model.failed_geometry.contains(&1));
    }

    /// A failed render is retried when the view changes. Geometry was not, and
    /// nothing else could retry it: the only thing that cleared
    /// `failed_geometry` was a `Ready` response, and a page held as failed is
    /// never requested again, so one transient failure blanked that page for
    /// the life of the process.
    #[test]
    fn a_view_change_retries_a_page_whose_geometry_failed() {
        let mut model = model_with_failed_geometry();
        assert!(model.failed_geometry.contains(&1));

        assert!(model.zoom_to(2.0).unwrap());
        let visible = model.viewport.visible_pages().unwrap();
        assert!(model.update_signature(&visible).unwrap());

        assert!(!model.failed_geometry.contains(&1));
        assert_eq!(model.queue_visible_geometry().unwrap(), 1);
    }

    /// `request_page_geometry` answers `false` when the session already holds
    /// a request for that page. Recording a wait anyway means recording one
    /// this canvas did not issue, and `has_pending_work` then reports work
    /// that no response will ever clear, which is a 60 Hz poll with no end.
    #[test]
    fn only_a_geometry_request_that_went_out_is_waited_on() {
        let mut model = model();
        model.viewport.go_to_page(1, PageAlignment::Start).unwrap();
        assert_eq!(model.queue_visible_geometry().unwrap(), 1);
        assert!(model.geometry_requests.contains(&1));

        // The two records disagreeing is the state to survive, so make them.
        model.geometry_requests.clear();

        assert_eq!(model.queue_visible_geometry().unwrap(), 0);
        assert!(
            model.geometry_requests.is_empty(),
            "the canvas is waiting on a request it did not issue"
        );
        assert!(!model.has_pending_work());
    }

    /// The poll loop's only exit used to be an answer, so a request nobody
    /// answers woke the app every 16 ms for the rest of the process.
    #[test]
    fn a_wait_nothing_answers_ends_in_an_error_rather_than_a_permanent_poll() {
        let mut model = model();
        model.geometry_requests.insert(1);
        let start = Instant::now();

        assert!(model.poll_again(start));
        assert!(model.poll_again(start + PENDING_WORK_TIMEOUT / 2));
        assert!(!model.poll_again(start + PENDING_WORK_TIMEOUT));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { page: None, message })
                if message.contains("[1]") && message.contains("stopped answering")
        ));
    }

    /// A worker that keeps answering is not a worker that has stopped, however
    /// long the queue stays occupied.
    #[test]
    fn a_wait_that_keeps_getting_answers_never_times_out() {
        let mut model = model();
        let start = Instant::now();
        for step in 0..4 {
            model.geometry_requests.insert(step);
            let now = start + PENDING_WORK_TIMEOUT * step as u32;
            assert!(model.poll_again(now), "gave up at step {step}");
            model
                .apply_geometry_response(PageGeometryResponse::Failed {
                    page: step,
                    error: CoreError::NoSuchPage {
                        page: step,
                        count: 1,
                    },
                })
                .unwrap();
        }
        assert!(!model.has_pending_work());
        assert!(!model.poll_again(start + PENDING_WORK_TIMEOUT * 4));
    }

    /// A walk keeps the poll loop awake, and the loop's deadline is the render
    /// worker's. Watching the search with it reported "no answer for [] after
    /// 30 seconds", stopped the loop, and froze the results mid-stream on any
    /// document big enough to take that long.
    #[test]
    fn a_long_walk_keeps_the_loop_awake_without_tripping_the_render_deadline() {
        let mut model = search_model();
        settle_geometry(&mut model);
        // Nothing is owed by a page worker, which is the state a fully drawn
        // view sits in while a find walks the rest of the document.
        model.geometry_requests.clear();
        model.requests.clear();
        assert!(!model.has_pending_pages());

        model
            .start_search("Page", SearchOptions::default())
            .expect("the search starts");
        assert!(model.search().is_running());
        assert!(model.has_pending_work());

        let start = Instant::now();
        assert!(model.poll_again(start));
        assert!(
            model.poll_again(start + PENDING_WORK_TIMEOUT * 4),
            "the walk still has pages to report"
        );
        assert!(
            model.status().is_none(),
            "a walk in progress is not a silent render worker"
        );
    }

    /// `paint_source` and `collect_tiles` used to take the raster's size from
    /// different places for the same `(page, zoom)`: the first from `sources`,
    /// the second from the tile store. The store's `cols` and `rows` come from
    /// its own base raster, so a disagreement made `raster_width - col *
    /// TILE_SIZE` underflow and took the window down.
    ///
    /// No reachable sequence produces the disagreement in the current code, so
    /// it is constructed here directly.
    #[test]
    fn the_tiles_of_a_page_are_cut_to_the_raster_the_store_holds() {
        let mut model = model();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        let source_zoom = 0.5_f32;
        assert_ne!(model.viewport.zoom().to_bits(), source_zoom.to_bits());
        model.tiles.begin_frame();
        model.tiles.insert(
            0,
            BaseRaster::new(
                TILE_SIZE * 2,
                1,
                source_zoom,
                vec![255; (TILE_SIZE * 2 * 4) as usize],
            ),
        );
        // A stale raster of a different size under the same key.
        model
            .sources
            .insert(0, BaseRaster::new(8, 1, source_zoom, vec![255; 32]));

        let paint = model.paint_list().expect("the page paints");

        let tiles = paint.tiles.iter().filter(|tile| tile.page == 0).count();
        assert_eq!(tiles, 2, "the store holds a raster two tiles wide");
    }

    #[test]
    fn a_resident_exact_raster_is_not_requested_twice() {
        let mut model = model();
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, [255, 255, 255, 255]);
        assert!(model.apply_render_response(raster_response(request, rendered)));

        let visible = model.viewport.visible_pages().unwrap();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 0);
        assert!(!model.has_pending_render());
    }

    #[test]
    fn a_stale_generation_cannot_replace_the_current_page() {
        let mut model = model();
        let stale = prepare_request(&mut model);
        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        assert!(model.update_signature(&visible).unwrap());
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);

        let old_raster = raster(&model, stale.page, stale.zoom, [255, 0, 0, 255]);
        assert!(!model.apply_render_response(raster_response(stale, old_raster)));
        assert!(model.sources.is_empty());
    }

    #[test]
    fn polling_refreshes_the_signature_before_accepting_a_response() {
        let mut model = model();
        let stale = prepare_request(&mut model);
        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();

        assert_eq!(model.drain_render_responses().unwrap(), 0);
        assert!(model.sources.is_empty());
        assert!(!model.requests.contains_key(&stale.page));
        assert_ne!(model.generation, stale.generation);
    }

    #[test]
    fn a_placeholder_keeps_polling_armed_until_a_terminal_response() {
        let mut model = model();
        let request = prepare_request(&mut model);
        let placeholder = model
            .document
            .try_render_response()
            .unwrap()
            .expect("placeholder is immediate");
        assert!(matches!(placeholder, RenderResponse::Placeholder(_)));
        assert!(model.apply_render_response(placeholder));
        assert!(model.has_pending_render());
        assert!(model.placeholders.contains(&request.page));

        assert!(model.apply_render_response(RenderResponse::Failed {
            request,
            error: RenderError::UnrenderableSize {
                width: 100_000.0,
                height: 100_000.0,
            },
        }));
        assert!(!model.has_pending_render());
        assert!(!model.placeholders.contains(&request.page));
    }

    #[test]
    fn a_render_failure_is_terminal_for_its_signature() {
        let mut model = model();
        let request = prepare_request(&mut model);
        assert!(model.apply_render_response(RenderResponse::Failed {
            request,
            error: RenderError::UnrenderableSize {
                width: 100_000.0,
                height: 100_000.0,
            },
        }));

        let visible = model.viewport.visible_pages().unwrap();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 0);
        assert!(model.failed_renders.contains(&request.page));
        assert!(!model.requests.contains_key(&request.page));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { page: Some(page), .. }) if *page == request.page
        ));

        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        assert!(model.update_signature(&visible).unwrap());
        assert!(!model.failed_renders.contains(&request.page));
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);

        let retry = *model.requests.get(&request.page).unwrap();
        let rendered = raster(&model, retry.page, retry.zoom, [255, 255, 255, 255]);
        assert!(model.apply_render_response(raster_response(retry, rendered)));
        assert!(model.status().is_none());
    }

    #[test]
    fn a_previous_raster_scales_while_the_new_zoom_is_pending() {
        let mut model = model();
        let first = prepare_request(&mut model);
        let previous = raster(&model, first.page, first.zoom, [220, 220, 220, 255]);
        assert!(model.apply_render_response(raster_response(first, previous)));

        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.tiles.begin_frame();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let placeholder = model
            .document
            .try_render_response()
            .unwrap()
            .expect("new placeholder is immediate");
        assert!(matches!(
            &placeholder,
            RenderResponse::Placeholder(value)
                if value.source().is_some_and(|source| source.zoom() == first.zoom)
        ));
        assert!(model.apply_render_response(placeholder));

        let paint = model.paint_list().unwrap();
        assert!(!paint.tiles.is_empty());
        assert!(paint
            .tiles
            .iter()
            .all(|tile| tile.source_zoom == first.zoom));
    }

    #[test]
    fn a_revisited_resident_raster_becomes_the_next_placeholder_source() {
        let mut model = model();
        let anchor = ViewPoint { x: 400.0, y: 300.0 };
        model.viewport.zoom_to(1.0, anchor).unwrap();
        let at_one = prepare_request(&mut model);
        let one = raster(&model, at_one.page, at_one.zoom, [100, 100, 100, 255]);
        assert!(model.apply_render_response(raster_response(at_one, one)));

        model.viewport.zoom_to(2.0, anchor).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.tiles.begin_frame();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let at_two = *model.requests.get(&0).expect("2x raster is requested");
        let two = raster(&model, at_two.page, at_two.zoom, [200, 200, 200, 255]);
        assert!(model.apply_render_response(raster_response(at_two, two)));

        model.viewport.zoom_to(1.0, anchor).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.tiles.begin_frame();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 0);
        assert!(!model.paint_list().unwrap().tiles.is_empty());
        assert_eq!(model.sources.get(&0).map(BaseRaster::zoom), Some(1.0));

        model.viewport.zoom_to(3.0, anchor).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.tiles.begin_frame();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let placeholder = model
            .document
            .try_render_response()
            .unwrap()
            .expect("3x placeholder is immediate");
        assert!(matches!(
            placeholder,
            RenderResponse::Placeholder(value)
                if value.source().is_some_and(|source| source.zoom() == 1.0)
        ));
    }

    #[test]
    fn render_failures_and_interpreter_warnings_are_visible() {
        let mut model = model();
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, [255, 255, 255, 255]);
        assert!(model.apply_render_response(RenderResponse::Raster {
            request,
            render: PageRender {
                raster: rendered,
                warnings: vec![InterpreterWarning::ImageDecodeFailure],
            },
        }));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Warning { page: 0, message })
                if message.contains("ImageDecodeFailure")
        ));

        let visible = model.viewport.visible_pages().unwrap();
        model.tiles.clear();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let request = *model.requests.get(&0).unwrap();
        assert!(model.apply_render_response(RenderResponse::Failed {
            request,
            error: RenderError::UnrenderableSize {
                width: 100_000.0,
                height: 100_000.0,
            },
        }));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { page: Some(0), message })
                if message.contains("100000")
        ));
    }

    #[test]
    fn generation_exhaustion_fails_instead_of_wrapping() {
        let mut model = model();
        model.generation = u64::MAX;
        model.signature = Some(RenderSignature {
            pages: Vec::new(),
            zoom_bits: 0,
        });
        let visible = model.viewport.visible_pages().unwrap();

        assert!(matches!(
            model.update_signature(&visible),
            Err(CanvasError::GenerationExhausted)
        ));
        assert_eq!(model.generation, u64::MAX);
    }

    #[test]
    fn visible_rasters_are_pinned_even_when_they_exceed_the_budget() {
        let mut model = model();
        let second = model.document.page_geometry(1).unwrap().clone();
        model.viewport.measure_page(second).unwrap();
        model.viewport.set_mode(PageLayoutMode::TwoPage).unwrap();
        model.viewport.fit(FitMode::Page).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        assert_eq!(visible.len(), 2);
        model.tiles = TileStore::with_budget(1);
        for page in 0..2 {
            model
                .sources
                .insert(page, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));
        }
        model.tiles.begin_frame();
        for page in 0..2 {
            model
                .tiles
                .insert(page, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));
        }

        assert_eq!(model.tiles.len(), 2);
        assert!(model.tiles.over_budget() > 0);
    }

    #[test]
    fn pinning_a_placeholder_also_protects_a_resident_exact_raster() {
        let mut model = model();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        let exact_zoom = model.viewport.zoom();
        let source_zoom = 1.0_f32;
        assert_ne!(exact_zoom.to_bits(), source_zoom.to_bits());
        model.sources.insert(
            0,
            BaseRaster::new(1, 1, source_zoom, vec![200, 200, 200, 255]),
        );
        model.tiles = TileStore::with_budget(1);
        model.tiles.begin_frame();
        model.tiles.insert(
            0,
            BaseRaster::new(1, 1, exact_zoom, vec![255, 255, 255, 255]),
        );
        model.tiles.insert(
            0,
            BaseRaster::new(1, 1, source_zoom, vec![200, 200, 200, 255]),
        );

        model.tiles.begin_frame();
        assert_eq!(model.tiles.len(), 2);
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 0);
        assert!(!model.has_pending_render());
    }

    #[test]
    fn transparent_and_partial_pixels_become_straight_bgra() {
        let (_, _, output) = tile_bgra(
            &[10, 20, 30, 0, 64, 32, 16, 128],
            2,
            1,
            2,
            1,
            ViewRotation::None,
        )
        .unwrap();

        assert_eq!(output, [0, 0, 0, 0, 31, 63, 127, 128]);
    }

    #[test]
    fn an_edge_tile_is_cropped_before_conversion() {
        let mut source = Vec::new();
        for value in 1..=9 {
            source.extend_from_slice(&[value, 0, 0, 255]);
        }
        let (width, height, output) = tile_bgra(&source, 3, 3, 2, 2, ViewRotation::None).unwrap();

        assert_eq!((width, height), (2, 2));
        assert_eq!(red_values(&output), vec![1, 2, 4, 5]);
    }

    #[test]
    fn cropped_pixels_rotate_in_all_four_orientations() {
        let mut source = Vec::new();
        for value in 1..=6 {
            source.extend_from_slice(&[value, 0, 0, 255]);
        }
        let cases = [
            (ViewRotation::None, (2, 3), vec![1, 2, 3, 4, 5, 6]),
            (ViewRotation::Clockwise90, (3, 2), vec![5, 3, 1, 6, 4, 2]),
            (ViewRotation::HalfTurn, (2, 3), vec![6, 5, 4, 3, 2, 1]),
            (ViewRotation::Clockwise270, (3, 2), vec![2, 4, 6, 1, 3, 5]),
        ];
        for (rotation, size, expected) in cases {
            let (width, height, output) = tile_bgra(&source, 2, 3, 2, 3, rotation).unwrap();
            assert_eq!((width, height), size, "{rotation:?}");
            assert_eq!(red_values(&output), expected, "{rotation:?}");
        }
    }

    #[test]
    fn tile_images_keep_visible_samples_one_pixel_inside_the_gpui_atlas() {
        let (width, height, output) = atlas_tile_bgra(
            &[255, 255, 255, 255, 0, 0, 0, 255],
            2,
            1,
            2,
            1,
            ViewRotation::None,
        )
        .unwrap();

        assert_eq!((width, height), (4, 3));
        let guarded_row = [
            255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255,
        ];
        assert_eq!(output, guarded_row.repeat(3));

        let clip = ViewRect {
            origin: ViewPoint { x: 10.0, y: 20.0 },
            size: ViewSize {
                width: 20.0,
                height: 10.0,
            },
        };
        assert_eq!(
            atlas_image_rect(clip, 2, 1),
            ViewRect {
                origin: ViewPoint { x: 0.0, y: 10.0 },
                size: ViewSize {
                    width: 40.0,
                    height: 30.0,
                },
            }
        );
    }

    #[test]
    fn replacing_a_tile_replaces_its_gpui_image() {
        let mut store = TileStore::new();
        let key = TileImageKey::new(0, 1.0, 0, 0, ViewRotation::None);
        let mut images = TileImageCache::default();

        let first_tile = store
            .insert(0, BaseRaster::new(1, 1, 1.0, vec![255, 0, 0, 255]))
            .tile(0, 0);
        let first = images
            .image_for(key, &first_tile, 1, 1, ViewRotation::None)
            .unwrap();
        let second_tile = store
            .insert(0, BaseRaster::new(1, 1, 1.0, vec![0, 0, 255, 255]))
            .tile(0, 0);
        let second = images
            .image_for(key, &second_tile, 1, 1, ViewRotation::None)
            .unwrap();

        assert_ne!(first.id, second.id);
        assert_eq!(first.as_bytes(0).unwrap(), [0, 0, 255, 255].repeat(9));
        assert_eq!(second.as_bytes(0).unwrap(), [255, 0, 0, 255].repeat(9));
    }

    #[test]
    fn paint_composites_and_caches_only_tiles_inside_the_viewport() {
        let mut model = model();
        model
            .viewport
            .zoom_to(10.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, [255, 255, 255, 255]);
        assert!(model.apply_render_response(raster_response(request, rendered)));

        let paint = model.paint_list().unwrap();
        let cache = model.tiles.get(request.page, request.zoom).unwrap();
        let page_tiles = cache.cols() * cache.rows();
        assert!(!paint.tiles.is_empty());
        assert!((paint.tiles.len() as u32) < page_tiles);
        assert_eq!(cache.composites(), paint.tiles.len() as u64);
        assert_eq!(model.image_cache.entries.len(), paint.tiles.len());
    }

    #[test]
    fn tile_destinations_rotate_with_their_pixels() {
        let page = ViewRect {
            origin: ViewPoint { x: 10.0, y: 20.0 },
            size: ViewSize {
                width: 500.0,
                height: 300.0,
            },
        };
        let region = TileRegion {
            col: 1,
            row: 0,
            width: 44,
            height: 100,
        };
        let none = tile_rect(page, region, (300, 500), ViewRotation::None);
        assert_rect_close(
            none,
            ViewRect {
                origin: ViewPoint {
                    x: 436.66666,
                    y: 20.0,
                },
                size: ViewSize {
                    width: 73.333336,
                    height: 60.0,
                },
            },
        );

        let clockwise = tile_rect(page, region, (300, 500), ViewRotation::Clockwise90);
        assert_rect_close(
            clockwise,
            ViewRect {
                origin: ViewPoint { x: 410.0, y: 276.0 },
                size: ViewSize {
                    width: 100.0,
                    height: 44.0,
                },
            },
        );

        let half_turn = tile_rect(page, region, (300, 500), ViewRotation::HalfTurn);
        assert_rect_close(
            half_turn,
            ViewRect {
                origin: ViewPoint { x: 10.0, y: 260.0 },
                size: ViewSize {
                    width: 73.333336,
                    height: 60.0,
                },
            },
        );

        let counterclockwise = tile_rect(page, region, (300, 500), ViewRotation::Clockwise270);
        assert_rect_close(
            counterclockwise,
            ViewRect {
                origin: ViewPoint { x: 10.0, y: 20.0 },
                size: ViewSize {
                    width: 100.0,
                    height: 44.0,
                },
            },
        );
    }

    /// Highlights are drawn in the same space as the page they sit on, so one
    /// that escapes its page is a transform error rather than a stray glyph.
    fn encloses(page: ViewRect, hit: ViewRect) -> bool {
        const SLACK: f32 = 0.5;
        hit.origin.x >= page.origin.x - SLACK
            && hit.origin.y >= page.origin.y - SLACK
            && hit.origin.x + hit.size.width <= page.origin.x + page.size.width + SLACK
            && hit.origin.y + hit.size.height <= page.origin.y + page.size.height + SLACK
    }

    fn assert_rect_close(actual: ViewRect, expected: ViewRect) {
        for (actual, expected) in [
            (actual.origin.x, expected.origin.x),
            (actual.origin.y, expected.origin.y),
            (actual.size.width, expected.size.width),
            (actual.size.height, expected.size.height),
        ] {
            assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
        }
    }

    /// A PDF assembled from content streams, for the two states the corpus
    /// seeds cannot reach: a page tree that promises a page it does not have,
    /// and a page whose runs sit far enough apart for the flattener to put a
    /// separator between them.
    ///
    /// `crates/core/src/search.rs` and `crates/core/tests/search.rs` build the
    /// same shape for core's own tests. Neither is visible from another crate,
    /// and a fixture crate for three call sites is more machinery than the
    /// duplication costs, so each copy names the others.
    fn assembled_pdf(streams: &[&str], counted: usize) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            Vec::new(), // 2: the page tree, once the kids are numbered
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ];
        let mut kids = Vec::new();
        for stream in streams {
            kids.push(format!("{} 0 R", objects.len() + 1));
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                     /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                    objects.len() + 2
                )
                .into_bytes(),
            );
            let mut body = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
            body.extend_from_slice(stream.as_bytes());
            body.extend_from_slice(b"\nendstream");
            objects.push(body);
        }
        objects[1] = format!(
            "<< /Type /Pages /Kids [{}] /Count {counted} >>",
            kids.join(" ")
        )
        .into_bytes();

        let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        let size = objects.len() + 1;
        out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
        out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }

    fn model_from(bytes: Vec<u8>) -> CanvasModel {
        CanvasModel::new(
            Document::open_bytes(bytes).expect("fixture opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts")
    }

    fn search_model() -> CanvasModel {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
        CanvasModel::new(
            Document::open_path(&path).expect("seed opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts")
    }

    /// The walk runs on its own thread, so a test has to drive `update` until
    /// it reports the walk finished rather than assume one call is enough.
    fn drain_search(model: &mut CanvasModel) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while model.search().is_running() {
            assert!(
                std::time::Instant::now() < deadline,
                "the walk never finished"
            );
            model.update().expect("update succeeds while searching");
        }
    }

    fn find(model: &mut CanvasModel, needle: &str, options: SearchOptions) {
        model.start_search(needle, options).expect("search starts");
        drain_search(model);
    }

    /// Runs updates until every visible page has been measured. Measurement
    /// re-lays out and re-anchors the viewport, so a test that cares where the
    /// view sits has to let that finish before it puts the view there.
    fn settle_geometry(model: &mut CanvasModel) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            model.update().expect("update succeeds while measuring");
            let waiting = !model.geometry_requests.is_empty()
                || model
                    .viewport
                    .visible_pages()
                    .expect("the viewport is laid out")
                    .iter()
                    .any(|placement| !placement.measured);
            if !waiting {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the geometry never arrived"
            );
        }
    }

    fn measured_visible(model: &CanvasModel) -> Vec<PageIndex> {
        model
            .viewport
            .visible_pages()
            .expect("the viewport is laid out")
            .into_iter()
            .filter(|placement| placement.measured)
            .map(|placement| placement.page)
            .collect()
    }

    #[test]
    fn every_hit_on_a_visible_page_becomes_a_highlight_and_the_current_one_is_marked() {
        let mut model = search_model();
        // The seed's two pages both fit on screen, so the walk has to start
        // somewhere named rather than wherever the initial fit landed.
        model.go_to_page(0).expect("the seed has a first page");
        find(&mut model, "Page", SearchOptions::default());
        model.update().expect("update succeeds");

        let visible = measured_visible(&model);
        let expected: usize = visible
            .iter()
            .flat_map(|page| model.search().matches_on(*page))
            .map(|hit| hit.quads.len())
            .sum();
        let paint = model.paint_list().expect("paint list builds");

        assert_eq!(model.search().len(), 2, "one hit per page of the seed");
        assert!(expected > 0, "the visible pages carry hits to highlight");
        assert_eq!(paint.highlights.len(), expected);
        let placements = model
            .viewport
            .visible_pages()
            .expect("the viewport is laid out");
        for highlight in &paint.highlights {
            assert!(
                highlight.rect.size.width > 0.0 && highlight.rect.size.height > 0.0,
                "a highlight with no area highlights nothing: {:?}",
                highlight.rect
            );
            assert!(
                placements
                    .iter()
                    .any(|placement| encloses(placement.rect, highlight.rect)),
                "a highlight landed off every page: {:?}",
                highlight.rect
            );
        }
        let current = model
            .search()
            .current()
            .expect("the walk reported a hit to be current");
        assert_eq!(
            paint
                .highlights
                .iter()
                .filter(|highlight| highlight.current)
                .count(),
            current.quads.len(),
            "the current hit marks every quad it owns and no others"
        );
    }

    /// The page the tree promises and does not have fails extraction, and the
    /// find bar has to name it. The mapping from the search state to what the
    /// bar renders was the one link in that chain no test crossed.
    #[test]
    fn a_page_that_cannot_be_read_reaches_the_find_bar_by_name() {
        let mut model = model_from(assembled_pdf(
            &["BT /F1 12 Tf 20 100 Td (alpha) Tj ET"],
            2, // one kid, and a /Count that claims two
        ));
        find(&mut model, "alpha", SearchOptions::default());

        assert_eq!(
            model.search().len(),
            1,
            "the page that is there is searched"
        );
        assert_eq!(model.search().failures().len(), 1);

        let summary =
            crate::shell::find_bar::FindSummary::new(model.search(), model.viewport.page_count());
        let label = summary
            .failure_label()
            .expect("a page that could not be read is reported");
        assert!(
            label.contains("page 2"),
            "the label does not name the page: {label}"
        );
    }

    /// A hit whose match is entirely the separator the flattener puts between
    /// two runs owns no glyphs, so it has no quads to scroll to. It still
    /// names a page, and going there beats a Next that only moves the count.
    #[test]
    fn a_hit_with_no_quads_still_goes_to_its_page() {
        let mut model = model_from(assembled_pdf(
            &[
                "BT /F1 12 Tf 20 100 Td (alpha) Tj ET",
                "BT /F1 12 Tf 20 150 Td (beta) Tj 0 -40 Td (gamma) Tj ET",
            ],
            2,
        ));
        settle_geometry(&mut model);
        model.go_to_page(0).expect("the fixture has a first page");
        assert_eq!(model.viewport.current_page(), 0);

        // Only the second page has two runs, and only between them is there a
        // separator to match.
        find(&mut model, "\n", SearchOptions::default());

        let hit = model.search().current().expect("the separator is a hit");
        assert_eq!(hit.page, 1);
        assert!(
            hit.quads.is_empty(),
            "a separator belongs to no run, so it has no glyphs"
        );
        assert_eq!(
            model.viewport.current_page(),
            1,
            "the reveal left the reader on the page the hit is not drawn on"
        );
    }

    #[test]
    fn a_closed_find_leaves_no_highlights_behind() {
        let mut model = search_model();
        find(&mut model, "Page", SearchOptions::default());
        model.update().expect("update succeeds");
        assert!(!model
            .paint_list()
            .expect("paint list builds")
            .highlights
            .is_empty());

        model.cancel_search();
        model.update().expect("update succeeds");

        assert!(model
            .paint_list()
            .expect("paint list builds")
            .highlights
            .is_empty());
        assert_eq!(model.search().len(), 0);
    }

    #[test]
    fn next_and_previous_walk_the_hits_and_wrap_at_both_ends() {
        let mut model = search_model();
        model.go_to_page(0).expect("the seed has a first page");
        assert_eq!(model.viewport.current_page(), 0);
        find(&mut model, "Page", SearchOptions::default());

        // The walk started on page 0, so its hit is the one in hand.
        assert_eq!(model.search().current().map(|hit| hit.page), Some(0));
        assert!(model.select_next_match().unwrap());
        assert_eq!(model.search().current().map(|hit| hit.page), Some(1));
        assert!(model.select_next_match().unwrap());
        assert_eq!(model.search().current().map(|hit| hit.page), Some(0));
        assert!(model.select_previous_match().unwrap());
        assert_eq!(model.search().current().map(|hit| hit.page), Some(1));
    }

    #[test]
    fn a_hit_already_on_screen_leaves_the_view_where_it_was() {
        let mut model = search_model();
        model.go_to_page(0).expect("the seed has a first page");
        settle_geometry(&mut model);
        // Scrolled past the top of the page, with the hit still on screen:
        // anything that navigates to the page rather than to the hit snaps
        // back to the page origin from here, and this test says so.
        model
            .viewport
            .pan_by(ViewPoint { x: 0.0, y: -100.0 })
            .expect("the seed page is taller than the pan");
        let before = model.viewport.snapshot();
        assert!(
            before.offset.y > 0.0,
            "the page origin has to be off screen for this to prove anything"
        );

        find(&mut model, "Page", SearchOptions::default());

        assert_eq!(model.search().current().map(|hit| hit.page), Some(0));
        assert_eq!(model.viewport.snapshot(), before);
    }

    #[test]
    fn streaming_results_leave_no_view_history_but_stepping_to_a_hit_does() {
        let mut model = search_model();
        settle_geometry(&mut model);
        let before_navigating = model.viewport.snapshot();
        model.go_to_page(0).expect("the seed has a first page");
        let on_page_one = model.viewport.snapshot();
        assert_ne!(
            on_page_one, before_navigating,
            "the navigation has to move the view for its history entry to mean anything"
        );

        // Only the second page says "two", so each of these walks starts on
        // page 1, wraps, and reveals a hit the view is not showing. Typing the
        // word out restarts the walk on every keystroke.
        for needle in ["t", "tw", "two"] {
            find(&mut model, needle, SearchOptions::default());
        }
        assert_eq!(model.search().current().map(|hit| hit.page), Some(1));
        assert_ne!(
            model.viewport.snapshot(),
            on_page_one,
            "the reveal has to have moved the view for this to prove anything"
        );

        // Previous View returns to where the user was before the navigation
        // they asked for, not to a view a streamed result scrolled them to.
        assert!(model.previous_view().unwrap());
        assert_eq!(model.viewport.snapshot(), before_navigating);
    }

    #[test]
    fn stepping_to_a_hit_is_a_view_to_come_back_from() {
        let mut model = search_model();
        settle_geometry(&mut model);
        let before_navigating = model.viewport.snapshot();
        model.go_to_page(0).expect("the seed has a first page");
        find(&mut model, "two", SearchOptions::default());
        let on_the_hit = model.viewport.snapshot();
        assert_ne!(
            on_the_hit, before_navigating,
            "the reveal has to land somewhere else for the depth check to mean anything"
        );

        // One hit, so next wraps back onto it and moves nothing.
        assert!(model.select_next_match().unwrap());
        assert_eq!(model.viewport.snapshot(), on_the_hit);
        // Depth, not presence: a step that moved nothing recording the view it
        // did not move from would leave an entry that returns to where the
        // user already is, and one Previous View would spend itself on it.
        assert!(model.previous_view().unwrap());
        assert_eq!(
            model.viewport.snapshot(),
            before_navigating,
            "Previous View landed on an entry the reveal or the step left behind"
        );

        model.go_to_page(0).expect("the seed has a first page");
        find(&mut model, "Page", SearchOptions::default());
        let before_step = model.viewport.snapshot();
        assert!(model.select_next_match().unwrap());
        assert_ne!(
            model.viewport.snapshot(),
            before_step,
            "the other page's hit is not on screen, so the step moves"
        );
        assert!(model.previous_view().unwrap());
        assert_eq!(model.viewport.snapshot(), before_step);
    }

    #[test]
    fn navigation_without_a_query_reports_nothing_rather_than_moving_the_view() {
        let mut model = search_model();
        let before = model.viewport.snapshot();

        assert!(!model.select_next_match().unwrap());
        assert!(!model.select_previous_match().unwrap());
        assert_eq!(model.viewport.snapshot(), before);
    }

    #[test]
    fn case_sensitivity_and_whole_word_cut_the_count_the_way_they_imply() {
        let mut model = search_model();

        find(&mut model, "page", SearchOptions::default());
        assert_eq!(
            model.search().len(),
            2,
            "the seed pages read \"Page one\" and \"Page two\""
        );

        find(
            &mut model,
            "page",
            SearchOptions {
                case_sensitive: true,
                ..SearchOptions::default()
            },
        );
        assert_eq!(model.search().len(), 0, "the document capitalises Page");

        find(&mut model, "Pag", SearchOptions::default());
        assert_eq!(model.search().len(), 2);

        find(
            &mut model,
            "Pag",
            SearchOptions {
                whole_word: true,
                ..SearchOptions::default()
            },
        );
        assert_eq!(model.search().len(), 0, "Pag is only ever part of Page");
    }

    #[test]
    fn any_of_the_words_finds_more_than_the_phrase_it_was_typed_as() {
        let mut model = search_model();

        find(&mut model, "one two", SearchOptions::default());
        assert_eq!(model.search().len(), 0, "no page carries that phrase");

        find(
            &mut model,
            "one two",
            SearchOptions {
                mode: onionskin_core::MatchMode::AnyWord,
                ..SearchOptions::default()
            },
        );
        assert_eq!(
            model.search().len(),
            2,
            "one on the first page, two on the second"
        );
    }

    #[test]
    fn the_walk_starts_at_the_page_in_view_and_scrolls_to_the_hit_it_finds() {
        let mut model = search_model();
        model.go_to_page(1).expect("the seed has a second page");
        model.update().expect("update succeeds");

        find(&mut model, "two", SearchOptions::default());

        assert_eq!(model.search().len(), 1);
        assert_eq!(model.search().current().map(|hit| hit.page), Some(1));
        assert_eq!(model.viewport.current_page(), 1);
    }

    #[test]
    fn a_hit_on_another_page_scrolls_that_page_into_view() {
        let mut model = search_model();
        model.go_to_page(0).expect("the seed has a first page");
        model.update().expect("update succeeds");
        assert_eq!(model.viewport.current_page(), 0);

        find(&mut model, "two", SearchOptions::default());

        assert_eq!(model.search().current().map(|hit| hit.page), Some(1));
        assert_eq!(model.viewport.current_page(), 1);
        assert!(measured_visible(&model).contains(&1));
    }

    #[test]
    fn a_visible_hit_does_not_move_the_page_under_the_user() {
        let viewport = ViewSize {
            width: 100.0,
            height: 100.0,
        };
        let inside = ViewRect {
            origin: ViewPoint { x: 10.0, y: 10.0 },
            size: ViewSize {
                width: 20.0,
                height: 5.0,
            },
        };

        assert_eq!(
            scroll_delta_into_view(inside, viewport),
            ViewPoint::default()
        );
    }

    #[test]
    fn a_hit_off_screen_is_centred_on_the_axis_it_ran_off() {
        let viewport = ViewSize {
            width: 100.0,
            height: 100.0,
        };
        let below = ViewRect {
            origin: ViewPoint { x: 10.0, y: 400.0 },
            size: ViewSize {
                width: 20.0,
                height: 10.0,
            },
        };

        let delta = scroll_delta_into_view(below, viewport);

        assert_eq!(delta.x, 0.0, "the hit was already visible across");
        // pan_by subtracts the delta from the offset, so a hit below the
        // viewport asks for a negative one.
        assert_eq!(delta.y, 45.0 - 400.0);
        assert_eq!(below.origin.y + delta.y + below.size.height / 2.0, 50.0);
    }

    #[test]
    fn a_hit_spanning_two_quads_is_revealed_as_one_region() {
        let first = ViewRect {
            origin: ViewPoint { x: 10.0, y: 20.0 },
            size: ViewSize {
                width: 30.0,
                height: 5.0,
            },
        };
        let second = ViewRect {
            origin: ViewPoint { x: 5.0, y: 40.0 },
            size: ViewSize {
                width: 10.0,
                height: 5.0,
            },
        };

        assert_eq!(union_rect(&[]), None);
        assert_eq!(union_rect(&[first]), Some(first));
        assert_eq!(
            union_rect(&[first, second]),
            Some(ViewRect {
                origin: ViewPoint { x: 5.0, y: 20.0 },
                size: ViewSize {
                    width: 35.0,
                    height: 25.0,
                },
            })
        );
    }

    fn red_values(bgra: &[u8]) -> Vec<u8> {
        bgra.as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[2])
            .collect()
    }
}
