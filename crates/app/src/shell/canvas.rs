use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use gpui::{Modifiers as GpuiModifiers, Pixels, Point, RenderImage};
use onionskin_core::{
    Attachment, Document, ExportSnapshot, FitMode, GeometryError, Layer, ObjRef, OutlineItem,
    PageAlignment, PageGeometry, PageGeometryResponse, PageIndex, PageLayoutMode, PagePlacement,
    PagePoint, PageQuad, PageRect, PageRenderRect, Provenance, RenderRequest, RenderResponse,
    RunCoverage, SearchOptions, SearchState, ThumbnailRequest, ThumbnailResponse, ViewHistory,
    ViewPoint, ViewRect, ViewRotation, ViewSize, Viewport, ViewportError,
};
use onionskin_plugin_api::{
    CodecPlugin, CommandCtx, CommandError, ExportError, ExportOutputKind, ExportRequest, Overlay,
    PageRange, PluginRegistry, PointerInput, ToolCtx,
};
#[cfg(test)]
use onionskin_render::PageRender;
use onionskin_render::{BaseRaster, RasterBounds, Tile, TileCache, TileStore, TILE_SIZE};
use smallvec::smallvec;

mod auto_scroll;
mod comment_reads;
mod edit_verbs;
mod file_ops;
#[cfg(feature = "tools-form")]
mod forms;

pub(in crate::shell) use auto_scroll::AutoScrollChange;
pub use comment_reads::CommentReads;
pub use edit_verbs::edit_verb_refusal;
pub use file_ops::{rank_offers, HistoryFacts, RecoveryOffer};
#[cfg(feature = "tools-form")]
pub use forms::{Entry, FieldPrompt};

use crate::a11y::structure::Outline;

use super::input::{
    pointer_input, pointer_input_near, validate_pressure, DragKind, DragUpdate, InputError,
    InputState,
};

/// One visible page, as an accessibility tree sees it.
///
/// `rect` is in the canvas's own coordinates, the same space `PaintList` uses,
/// so the caller adds `canvas_origin` to reach window coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct PageOutline {
    pub page: PageIndex,
    pub rect: ViewRect,
    /// Whether the layout has this page's real size yet.
    ///
    /// An unmeasured page is still announced, at its placeholder rectangle,
    /// rather than being left out of the tree while it loads. Its `text` is
    /// empty because there is nowhere to put the words yet, which is why the
    /// caller has to read this to tell "still loading" from "no text".
    pub measured: bool,
    /// The page's text, or why it could not be read. A page whose text failed
    /// is announced as unreadable rather than as an empty page.
    pub text: Result<Vec<TextOutline>, String>,
    /// For a tagged document, the page's structure elements as the nodes a
    /// screen reader navigates, in place of the flat `text`; `None` for an
    /// untagged one, which has only its runs. Like `text` it is empty until
    /// the page is measured and someone is listening.
    pub(crate) structure: Option<Result<Vec<Outline>, String>>,
}

/// One semantic piece of text on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct TextOutline {
    pub text: String,
    /// `None` when the piece's quads name a page the layout is not placing, so
    /// there is no rectangle to give rather than a wrong one.
    pub rect: Option<ViewRect>,
}

const PAGE_GAP: f32 = 12.0;
const VIEW_HISTORY_CAPACITY: NonZeroUsize = NonZeroUsize::new(100).unwrap();
const RGBA_BYTES_PER_PIXEL: usize = 4;
/// How far a tile's pixels are duplicated past its own edge in the atlas.
///
/// GPUI samples linearly right up to an atlas allocation's edge, so the
/// outermost sample of one tile reads its neighbour unless a copy of the edge
/// pixel sits between them. One is enough for linear sampling and is what
/// `fdaf657` shipped to close the tile seams; the paint then has to place the
/// image one gutter outside the clip on every side, so the duplicates land
/// outside the visible rectangle.
const ATLAS_GUTTER_PX: u32 = 1;
pub(super) const SNAPSHOT_RGBA_BYTE_LIMIT: usize = 3840 * 2160 * RGBA_BYTES_PER_PIXEL;
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
    /// Whether Automatically Scroll is running, for the menu's check.
    pub(super) auto_scrolling: bool,
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
    /// An explicit magnification, 1.0 being actual size. Clamped to what the
    /// current page can be rasterized at, the way every other zoom is.
    ZoomTo(f32),
    Fit(FitMode),
    /// Fit the page's marks rather than its media box. The rectangle is not
    /// carried here because only the canvas can read it off the rendered
    /// page; the menu asks for the mode and the canvas supplies the bounds.
    FitVisible,
    SetLayout(PageLayoutMode),
    SetShowCover(bool),
}

/// A comment a text tool has just placed, and where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextTarget {
    pub annotation: ObjRef,
    pub page: PageIndex,
    pub rect: onionskin_core::Rect,
    /// A pop-up beside it, for a note; the comment's own box, for free text.
    pub popup: bool,
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
    /// The same, for a command: the entry that offered it asked the registry
    /// first, so reaching this means the registry changed under the menu.
    UnknownCommand(&'static str),
    Command(CommandError),
    Export(ExportError),
    Geometry(GeometryError),
    SnapshotUnrendered {
        page: PageIndex,
    },
    SnapshotEmpty {
        page: PageIndex,
    },
    SnapshotTooLarge {
        width: u32,
        height: u32,
        limit: usize,
    },
    SnapshotEncode(String),
    /// Fit Visible reads the page's marks off its rendered pixels, and this
    /// page has none yet.
    FitVisibleUnrendered {
        page: PageIndex,
    },
    /// The page draws nothing, so there is no visible content to fit.
    FitVisibleBlank {
        page: PageIndex,
    },
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
                write!(f, "RGBA image needs {expected} bytes, got {actual}")
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
            Self::SnapshotTooLarge {
                width,
                height,
                limit,
            } => write!(
                f,
                "snapshot {width}x{height} exceeds the {limit}-byte RGBA clipboard limit"
            ),
            Self::SnapshotEncode(error) => write!(f, "cannot encode the snapshot: {error}"),
            Self::FitVisibleUnrendered { page } => write!(
                f,
                "page {page} has not been rendered yet, so its visible content is not known"
            ),
            Self::FitVisibleBlank { page } => {
                write!(f, "page {page} draws nothing, so it has no content to fit")
            }
            Self::ToolOutOfRange { index, count } => {
                write!(f, "tool {index} is outside a {count}-tool registry")
            }
            Self::UnknownCodec(id) => write!(f, "no {id} codec is installed"),
            Self::UnknownCommand(id) => write!(f, "no plugin registers the {id} command"),
            Self::Command(error) => write!(f, "{error}"),
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
            Self::Command(error) => Some(error),
            Self::Geometry(error) => Some(error),
            Self::EmptyDocument
            | Self::GenerationExhausted
            | Self::InvalidImageBuffer { .. }
            | Self::InvalidImageCrop { .. }
            | Self::ToolOutOfRange { .. }
            | Self::UnknownCodec(_)
            | Self::UnknownCommand(_)
            | Self::SnapshotUnrendered { .. }
            | Self::SnapshotEmpty { .. }
            | Self::SnapshotTooLarge { .. }
            | Self::SnapshotEncode(_)
            | Self::FitVisibleUnrendered { .. }
            | Self::FitVisibleBlank { .. }
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
    /// Something the user should know about the document as a whole, from the
    /// moment it opens: that an encrypted document is read-only, and why. Not
    /// an error - nothing failed - and not a page's warning.
    Notice {
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
    /// Solid outline: the bounds of a text box or a selected annotation.
    Rect(ViewRect),
    /// A stroked path, closed back to its first point when `closed`.
    Polyline {
        points: Vec<ViewPoint>,
        closed: bool,
    },
    /// A single stroked segment: a measurement, a callout's leader.
    Line { from: ViewPoint, to: ViewPoint },
    /// An ellipse inscribed in its bounds.
    Ellipse(ViewRect),
}

/// One search hit's box on a visible page. An overlay in the same sense: it is
/// painted over the tiles rather than rendered into them, so highlighting
/// costs no re-render.
pub struct HighlightPaint {
    pub rect: ViewRect,
    /// The hit next/previous last landed on, drawn differently from the rest.
    pub current: bool,
    /// Not a hit: the content a Tags or Content pane choice points at, in a
    /// colour of its own so a find in progress is not mistaken for it.
    pub structure: bool,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolPointerPhase {
    Down,
    Move,
    Up,
}

/// A document and its file, shared by every window showing it (View > New
/// Window): one session, one overlay, one undo stack.
pub type SharedFile = std::rc::Rc<std::cell::RefCell<onionskin_core::DocumentFile>>;

pub struct CanvasModel {
    /// The document with its file: what Save, Save As, Revert and autosave
    /// act through. Tools and commands are handed only the `Document` inside.
    /// Shared with any other window on the same document; borrowed for each
    /// use and never held across a call that could borrow it again.
    document: SharedFile,
    /// This window's own render queue when it is not the document's first
    /// window. `None` renders through the session's primary queue.
    view: Option<onionskin_core::RenderView>,
    /// Whether the pixels this canvas holds were drawn with hairline strokes.
    hairline_strokes: bool,
    /// Whether Validate All asked for this document's signatures to be
    /// checked, when the preferences do not check them on opening.
    signatures_requested: bool,
    /// Where autosave writes, kept so a Revert can hand it to the document
    /// it reopens.
    recovery: Option<onionskin_core::RecoveryStore>,
    viewport: Viewport,
    view_history: ViewHistory,
    registry: PluginRegistry,
    active_tool: Option<usize>,
    input: InputState,
    /// Every raster the canvas holds, and the only place it holds one. A
    /// second collection beside this one carried the same base rasters under
    /// its own eviction policy, so a page scrolled out of view lost the
    /// raster it would have been scaled from while the store still had it.
    tiles: TileStore,
    geometry_requests: BTreeSet<PageIndex>,
    failed_geometry: BTreeSet<PageIndex>,
    requests: BTreeMap<PageIndex, RenderRequest>,
    failed_renders: BTreeSet<PageIndex>,
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
    /// The thumbnail outstanding for each page, exactly as it was asked
    /// for. Keeps the poll loop awake the way an outstanding render does,
    /// and is what an answer is matched against: a picture nothing is
    /// waiting for is a picture of a document that has since changed.
    pending_thumbnails: BTreeMap<PageIndex, ThumbnailRequest>,
    /// How many times the render options every page goes through have
    /// changed. Carried on a thumbnail request and back on its answer,
    /// because a thumbnail is not part of the interactive queue's generation
    /// and would otherwise have nothing to be stale against.
    thumbnail_epoch: u64,
    /// The document's edit epoch the cached pixels were drawn from. An edit,
    /// an undo or a redo moves the epoch, whichever surface made it: a
    /// tool's pointer-up, a pane, a dialog or a command. The pixels are
    /// checked against it every frame, so none of those has to remember to
    /// say the page changed.
    pixels_epoch: u64,
    /// The click count the next press carries; see [`Self::set_click_count`].
    click_count: u8,
    /// The comment a text tool just placed, waiting to be written in.
    text_target: Option<TextTarget>,
    /// The name comments are signed with, from the tool environment.
    author: Option<String>,
    /// Which comments the user has read this session; never saved.
    comment_reads: CommentReads,
    /// Thumbnails answered and not yet collected. The pane takes them,
    /// because turning a raster into an image the window can paint is the
    /// shell's job and not the model's.
    ready_thumbnails: Vec<(PageIndex, BaseRaster)>,
    /// Each visible page's words and where they sit on the page, for the
    /// accessibility tree. Page space, so scrolling does not invalidate it.
    page_words: BTreeMap<PageIndex, Vec<(String, Vec<PageQuad>)>>,
    /// The structure nodes of a tagged page, kept like `page_words` until the
    /// session's content changes.
    page_structure: BTreeMap<PageIndex, Vec<Outline>>,
    /// The content the Tags or Content pane is pointing at, in page space,
    /// drawn over the page like a search hit.
    structure_highlight: Vec<(PageIndex, [f64; 4])>,
    /// The session state `page_structure` was built for, and why the structure
    /// could not be read in it, so a document whose structure is unreadable
    /// is not parsed again on every frame.
    page_structure_at: (u64, u64),
    structure_error: Option<String>,
    /// View > Page Display > Automatically Scroll, while it runs.
    auto_scroll: Option<onionskin_core::AutoScroll>,
    /// Whether form scripts run, and what filling had to say.
    #[cfg(feature = "tools-form")]
    forms: forms::FormFilling,
    /// `(byte generation, edit epoch)` the layout was last built for, so an
    /// edit made in another window on the same document is followed.
    laid_out_at: (u64, u64),
    /// The document snapshot this canvas last observed for Find.
    find_seen_at: (u64, u64),
}

pub(super) struct PreparedExport {
    pub(super) snapshot: ExportSnapshot,
    pub(super) codec: Arc<dyn CodecPlugin + Send + Sync>,
    pub(super) request: ExportRequest,
    pub(super) output_kind: ExportOutputKind,
    pub(super) page_count: usize,
}

impl CanvasModel {
    pub fn new(
        document: Document,
        registry: PluginRegistry,
        size: ViewSize,
    ) -> Result<Self, CanvasError> {
        let file = std::rc::Rc::new(std::cell::RefCell::new(
            onionskin_core::DocumentFile::from_document(document),
        ));
        Self::over(file, None, registry, size)
    }

    /// View > New Window: another viewport over this window's session. It
    /// edits the same document with the same undo stack, and draws through a
    /// render queue of its own so the two windows scroll and zoom apart.
    pub fn new_window(
        &self,
        registry: PluginRegistry,
        size: ViewSize,
    ) -> Result<Self, CanvasError> {
        let view = self.document.borrow_mut().new_render_view()?;
        let mut model = Self::over(self.shared_file(), Some(view), registry, size)?;
        model.recovery = self.recovery.clone();
        Ok(model)
    }

    /// How many other windows show this window's document.
    pub fn other_windows(&self) -> usize {
        std::rc::Rc::strong_count(&self.document) - 1
    }

    fn over(
        document: SharedFile,
        view: Option<onionskin_core::RenderView>,
        mut registry: PluginRegistry,
        size: ViewSize,
    ) -> Result<Self, CanvasError> {
        let (viewport, active_tool, status, laid_out_at, hairline_strokes) = {
            let mut file = document.borrow_mut();
            if file.page_count() == 0 {
                return Err(CanvasError::EmptyDocument);
            }
            let page_count = file.page_count();
            let first = file.page_geometry(0)?.clone();
            let mut viewport = Viewport::new(page_count, size, PAGE_GAP)?;
            viewport.measure_page(first)?;
            viewport.fit(FitMode::Page)?;
            let active_tool = registry.tools().next().map(|_| 0);
            if let Some(index) = active_tool {
                registry
                    .tool_mut(index)
                    .expect("the initial tool remains registered")
                    .on_activate(&mut ToolCtx {
                        doc: &mut file,
                        viewport: &mut viewport,
                    });
            }
            // Said at open rather than at the first refused edit, so the user
            // knows before starting work they cannot keep.
            let status = file
                .protection_notice()
                .or_else(|| form_notice(&mut file))
                .map(|message| CanvasStatus::Notice { message });
            let laid_out_at = (file.byte_generation(), file.edit().epoch());
            let hairline_strokes = file.hairline_strokes();
            (viewport, active_tool, status, laid_out_at, hairline_strokes)
        };
        Ok(Self {
            document,
            view,
            hairline_strokes,
            signatures_requested: false,
            laid_out_at,
            find_seen_at: laid_out_at,
            recovery: None,
            viewport,
            view_history: ViewHistory::new(VIEW_HISTORY_CAPACITY),
            registry,
            active_tool,
            input: InputState::default(),
            tiles: TileStore::new(),
            geometry_requests: BTreeSet::new(),
            failed_geometry: BTreeSet::new(),
            requests: BTreeMap::new(),
            failed_renders: BTreeSet::new(),
            generation: 0,
            signature: None,
            canvas_origin: ViewPoint::default(),
            image_cache: TileImageCache::default(),
            status,
            waiting: None,
            responses: 0,
            pending_reveal: None,
            pending_thumbnails: BTreeMap::new(),
            thumbnail_epoch: 0,
            pixels_epoch: 0,
            click_count: 1,
            text_target: None,
            author: None,
            comment_reads: CommentReads::default(),
            ready_thumbnails: Vec::new(),
            page_words: BTreeMap::new(),
            page_structure: BTreeMap::new(),
            structure_highlight: Vec::new(),
            page_structure_at: laid_out_at,
            structure_error: None,
            auto_scroll: None,
            #[cfg(feature = "tools-form")]
            forms: forms::FormFilling::default(),
        })
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    /// Hand this tab's tools the shell's environment.
    pub(super) fn configure_tools(&mut self, environment: &onionskin_plugin_api::ToolEnvironment) {
        self.author.clone_from(&environment.author);
        #[cfg(feature = "tools-form")]
        self.set_form_scripts(!environment.javascript_off);
        self.registry.configure_tools(environment);
    }

    /// The name this tab's comments are signed with, as the tools were told.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Set what tool `index` places next. `false` when it has no such choice.
    pub(super) fn choose_tool(&mut self, index: usize, choice: &str) -> bool {
        self.registry
            .tool_mut(index)
            .is_some_and(|tool| tool.choose(choice))
    }

    /// The document itself, for work that reads it out into new files - a
    /// split - and changes nothing the canvas draws.
    pub(super) fn document_mut(&self) -> std::cell::RefMut<'_, Document> {
        std::cell::RefMut::map(self.document.borrow_mut(), |file| file.document_mut())
    }

    /// The session this window shows, to open another window on it.
    pub(super) fn shared_file(&self) -> SharedFile {
        std::rc::Rc::clone(&self.document)
    }

    /// Why the document may not be read out into a new file, or `None`.
    pub(super) fn read_out_refusal(&self) -> Option<onionskin_core::protection::Refusal> {
        self.document.borrow_mut().document().read_out_refusal()
    }

    /// Why the open document may not be edited, as the short reason a disabled
    /// entry shows, or `None` when it may. Asked of `core`, which derives it
    /// from the document; the shell holds no flag of its own.
    pub fn edit_refusal(&self) -> Option<&'static str> {
        self.document
            .borrow_mut()
            .edit_refusal()
            .map(|refusal| refusal.reason())
    }

    /// Both of the document's refusals, as the context menu asks for them.
    pub(super) fn refusals(&self) -> super::context_menu::Refusals {
        // One borrow each, ended before the next: temporaries in a struct
        // literal live to its end.
        let edit = self.edit_refusal();
        let comment = self
            .document
            .borrow()
            .edit_refusal_as(onionskin_core::protection::EditKind::Comments)
            .map(|refusal| refusal.reason());
        let read_out = self
            .document
            .borrow_mut()
            .read_out_refusal()
            .map(|refusal| refusal.reason());
        super::context_menu::Refusals {
            edit,
            comment,
            read_out,
        }
    }

    /// What to tell the user about this document when it opens, if anything.
    pub fn protection_notice(&self) -> Option<String> {
        self.document.borrow_mut().protection_notice()
    }

    /// True when this build has the codec a menu entry would run.
    pub fn has_codec(&self, id: &str) -> bool {
        self.registry.codec(id).is_some()
    }

    /// Capture everything the background export worker needs.
    pub(super) fn prepare_export(
        &self,
        codec: &'static str,
        request: ExportRequest,
    ) -> Result<PreparedExport, CanvasError> {
        let page_count = self.document.borrow_mut().page_count();
        let pages = request.pages.pages();
        PageRange::new(*pages.start(), *pages.end(), page_count)?;
        let codec = self
            .registry
            .codec(codec)
            .ok_or(CanvasError::UnknownCodec(codec))?;
        Ok(PreparedExport {
            snapshot: self.document.borrow_mut().export_snapshot()?,
            output_kind: codec.output_kind(),
            codec,
            request,
            page_count,
        })
    }

    /// How this document was opened: clean, or repaired to make it open.
    /// The shell owes the user a notice for the second case.
    pub fn provenance(&self) -> Provenance {
        self.document.borrow().provenance().clone()
    }

    /// Run a registered command against this document, on the page the
    /// viewport is on.
    ///
    /// Paired here for the same reason `export` is: the chrome should not
    /// have to hold the registry and the document at once and get their
    /// lifetimes right. The command is found by the id the menu entry
    /// carries, which is the id the availability query asked about.
    pub fn run_command(&mut self, id: &'static str) -> Result<(), CanvasError> {
        let command = self
            .registry
            .commands()
            .iter()
            .find(|command| command.id == id)
            .ok_or(CanvasError::UnknownCommand(id))?;
        let before = self.document.borrow_mut().edit().epoch();
        (command.run)(&mut CommandCtx {
            doc: &mut self.document.borrow_mut(),
            page: self.viewport.current_page(),
        })
        .map_err(CanvasError::Command)?;
        if self.document.borrow_mut().edit().epoch() != before {
            self.relayout_after_edit()?;
        }
        Ok(())
    }

    /// Run an edit that takes an explicit page selection, such as the
    /// Organize Pages grid's, and rebuild the layout when it changed the
    /// document, as a registered command does.
    pub fn edit_pages(
        &mut self,
        edit: impl FnOnce(&mut Document) -> Result<(), onionskin_plugin_api::CommandError>,
    ) -> Result<(), CanvasError> {
        let before = self.document.borrow_mut().edit().epoch();
        edit(self.document.borrow_mut().document_mut()).map_err(CanvasError::Command)?;
        if self.document.borrow_mut().edit().epoch() != before {
            self.relayout_after_edit()?;
        }
        Ok(())
    }

    /// Rewrite the line the Edit Text tool picked with `text`, or draw it
    /// where the Add Text tool clicked, as one undo step. A line that says
    /// something else now is refused.
    #[cfg(feature = "tools-edit")]
    pub fn edit_text_line(
        &mut self,
        request: &onionskin_core::TextEditRequest,
        text: &str,
        style: onionskin_core::text_edit::TextStyle,
    ) -> Result<bool, CanvasError> {
        use onionskin_tools_edit::text::{add_styled_text, edit_styled_line};
        self.edit_pages(|doc| match request.line {
            Some(line) => edit_styled_line(doc, request.page, line, &request.text, text, style),
            None => add_styled_text(
                doc,
                request.page,
                (request.bounds[0], request.bounds[1]),
                text,
                style,
            ),
        })?;
        Ok(true)
    }

    /// A command changed the document, and a page command can change what
    /// the layout was built from: how many pages there are, their order, and
    /// their size once turned. So the layout is rebuilt from the document as
    /// edited, keeping what the user chose about the view - mode, cover,
    /// rotation, zoom - and staying on the same page number, or the last page
    /// when that one is gone. Every raster goes, because a page index may now
    /// name a different page.
    fn relayout_after_edit(&mut self) -> Result<(), CanvasError> {
        let count = self.document.borrow_mut().page_count();
        if count == 0 {
            return Err(CanvasError::EmptyDocument);
        }
        let view = self.viewport.snapshot();
        let mut viewport = Viewport::new(count, self.viewport.size(), PAGE_GAP)?;
        // Measured first: every layout call below needs an estimate to work
        // from, which is what the first page's geometry gives it.
        viewport.measure_page(self.document.borrow_mut().page_geometry(0)?.clone())?;
        viewport.set_mode(view.mode)?;
        viewport.set_show_cover(view.show_cover)?;
        viewport.set_rotation(view.rotation)?;
        match view.zoom_policy {
            onionskin_core::ZoomPolicy::Fit(mode) if !matches!(mode, FitMode::Visible(_)) => {
                viewport.fit(mode)?
            }
            _ => viewport.zoom_to(view.zoom, ViewPoint::default())?,
        }
        viewport.go_to_page(view.current_page.min(count - 1), PageAlignment::Start)?;
        self.viewport = viewport;
        // The history's states name page indices of the old pagination.
        self.view_history = ViewHistory::new(VIEW_HISTORY_CAPACITY);
        self.laid_out_at = self.session_stamp();
        self.geometry_requests.clear();
        self.failed_geometry.clear();
        self.page_words.clear();
        self.invalidate_rendered_pixels();
        Ok(())
    }

    /// What identifies the session's current content: its bytes and edits.
    pub fn session_stamp(&self) -> (u64, u64) {
        let file = self.document.borrow();
        (file.byte_generation(), file.edit().epoch())
    }

    /// Follow an edit made to this document, in this window or another:
    /// when it changed how many pages there are or the size of one on
    /// screen, the layout is rebuilt as a page command's is. A comment
    /// changes neither and leaves the view where it is.
    fn follow_session(&mut self) -> Result<(), CanvasError> {
        let stamp = self.session_stamp();
        if stamp == self.laid_out_at {
            return Ok(());
        }
        self.laid_out_at = stamp;
        if self.layout_is_stale()? {
            self.relayout_after_edit()?;
        }
        Ok(())
    }

    fn layout_is_stale(&self) -> Result<bool, CanvasError> {
        let mut file = self.document.borrow_mut();
        if file.page_count() != self.viewport.page_count() {
            return Ok(true);
        }
        for placement in self.viewport.visible_pages()? {
            if let Some(measured) = self.viewport.page_geometry(placement.page) {
                if file.page_geometry(placement.page)?.render_size != measured.render_size {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// The text the current selection covers, which is what the context
    /// menu's Copy puts on the clipboard.
    /// The selected text with its faces, for Export Selection As.
    pub fn text_selection(&self) -> Option<onionskin_core::TextSelection> {
        self.document.borrow().selection().text().cloned()
    }

    /// The image the Edit Image tool selected, for Save Image As.
    pub fn image_selection(&self) -> Option<onionskin_core::ImageSelection> {
        self.document.borrow().selection().image().cloned()
    }

    pub fn selection_text(&self) -> Option<String> {
        self.document
            .borrow()
            .selection()
            .text()
            .map(|selection| selection.text.clone())
    }

    /// The visible pages and their text, in the canvas's own coordinates.
    ///
    /// What the accessibility tree needs and nothing else: `paint_list`
    /// answers the same question in pixels, this one answers it in words.
    ///
    /// Extracting a page's text parses its content stream, which is not free,
    /// but the session caches what it extracts, so a page is parsed once
    /// rather than once per frame, and only the pages on screen are parsed at
    /// all. An unmeasured page has no layout to place its words in yet, so it
    /// is described without them and picks them up when it is measured.
    ///
    /// `with_text` false leaves every page's `text` empty and parses nothing:
    /// the caller knows nothing is listening, and the first frame each page
    /// is visible for is otherwise the one that pays for its content stream,
    /// on the thread that draws.
    pub fn accessible_pages(&mut self, with_text: bool) -> Result<Vec<PageOutline>, CanvasError> {
        let placements = self.viewport.visible_pages()?;
        // Bounded to what is on screen: a long scroll would otherwise keep
        // every page it passed for the life of the process.
        self.page_words
            .retain(|page, _| placements.iter().any(|placement| placement.page == *page));
        self.page_structure
            .retain(|page, _| placements.iter().any(|placement| placement.page == *page));
        let mut pages = Vec::with_capacity(placements.len());
        for placement in placements {
            let text = if with_text && placement.measured {
                self.page_runs(placement.page)
            } else {
                Ok(Vec::new())
            };
            let structure = if with_text && placement.measured {
                self.page_structure_nodes(placement.page).transpose()
            } else {
                None
            };
            pages.push(PageOutline {
                page: placement.page,
                rect: placement.rect,
                measured: placement.measured,
                text,
                structure,
            });
        }
        Ok(pages)
    }

    /// The page's structure nodes if the document is tagged, `Ok(None)` if it is
    /// not. The first call on a tagged document reads every page the structure
    /// names content on, which the session then keeps until the bytes change.
    ///
    /// Kept per page for the session state it was built in, so an edit from
    /// anywhere, not only this window's, replaces it. A tagged page none of
    /// whose content is in the structure has no nodes to give and reads as
    /// runs, as an untagged one does.
    fn page_structure_nodes(&mut self, page: PageIndex) -> Result<Option<Vec<Outline>>, String> {
        let stamp = self.session_stamp();
        if self.page_structure_at != stamp {
            self.page_structure.clear();
            self.structure_error = None;
            self.page_structure_at = stamp;
        }
        if let Some(error) = &self.structure_error {
            return Err(error.clone());
        }
        let blocks = match self.document.borrow_mut().reading_blocks() {
            Ok(Some(blocks)) => blocks,
            Ok(None) => return Ok(None),
            Err(error) => {
                let error = error.to_string();
                self.structure_error = Some(error.clone());
                return Err(error);
            }
        };
        let nodes = self
            .page_structure
            .entry(page)
            .or_insert_with(|| crate::a11y::structure::outline(&blocks, page));
        Ok((!nodes.is_empty()).then(|| nodes.clone()))
    }

    /// Where a box in page space is in the view, for placing a structure node.
    /// `None` when the layout is not placing that page.
    pub fn structure_rect(&self, page: PageIndex, bounds: [f64; 4]) -> Option<ViewRect> {
        let quad = PageQuad {
            page,
            corners: [
                (bounds[0], bounds[3]),
                (bounds[2], bounds[3]),
                (bounds[0], bounds[1]),
                (bounds[2], bounds[1]),
            ],
        };
        let rects = self.viewport.page_quad_rects(page, &[quad]).ok()?;
        union_rect(&rects)
    }

    /// One entry per semantic text piece on a page, with the rectangle it occupies.
    ///
    /// A page is not one label: a screen reader that gets the whole page as a
    /// single string cannot navigate it. Pieces preserve replacement text and
    /// member geometry, so they are the unit here too.
    ///
    /// The words and their page-space quads are cached until the page text is
    /// invalidated, and this runs on every frame; only the mapping into the
    /// view is redone, because that is what scrolling changes.
    fn page_runs(&mut self, page: PageIndex) -> Result<Vec<TextOutline>, String> {
        if !self.page_words.contains_key(&page) {
            let words = {
                let mut document = self.document.borrow_mut();
                let text = document
                    .page_text(page)
                    .map_err(|error| error.to_string())?;
                let flat = text.flatten();
                flat.pieces()
                    .iter()
                    .filter_map(|piece| {
                        let label = &flat.text[piece.range.clone()];
                        if label.trim().is_empty() {
                            return None;
                        }
                        let quads = piece
                            .coverage_for(text, piece.range.clone())
                            .into_iter()
                            .flat_map(|covered| match covered.coverage {
                                RunCoverage::Decoded(local) => covered.run.quads_for_decoded(local),
                                RunCoverage::WholeActualText => {
                                    covered.run.glyphs.iter().map(|glyph| glyph.quad).collect()
                                }
                            })
                            .collect();
                        Some((label.to_owned(), quads))
                    })
                    .collect()
            };
            self.page_words.insert(page, words);
        }
        // Split from the insert above so the borrow of `document` ends before
        // the viewport is asked to map the quads.
        let words = &self.page_words[&page];
        words
            .iter()
            .map(|(text, quads)| {
                let rects = self
                    .viewport
                    .page_quad_rects(page, quads)
                    .map_err(|error| error.to_string())?;
                Ok(TextOutline {
                    text: text.clone(),
                    rect: union_rect(&rects),
                })
            })
            .collect()
    }

    /// How many pages this session has parsed the text of, for the test that
    /// the shell parses none of them while nothing is listening.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn extracted_pages(&self) -> usize {
        self.page_words.len()
    }

    pub fn active_tool(&self) -> Option<usize> {
        self.active_tool
    }

    // ---- navigation panes ---------------------------------------------

    /// The document readers the left panes list. Each is read once by the
    /// session and handed back by value: a pane holds a snapshot rather than
    /// a borrow of the document the canvas is drawing from.
    pub fn outline(&mut self) -> Result<Vec<OutlineItem>, CanvasError> {
        Ok(self.document.borrow_mut().outline()?.to_vec())
    }

    /// The tagged structure as blocks in reading order, or `None` for a
    /// document with no structure tree: what the Tags pane lists.
    pub fn structure_blocks(
        &mut self,
    ) -> Result<Option<Arc<Vec<onionskin_core::Block>>>, CanvasError> {
        Ok(self.document.borrow_mut().reading_blocks()?)
    }

    /// What page `page` draws, each piece with its marked-content sequence:
    /// what the Content pane lists.
    pub fn marked_page(
        &mut self,
        page: PageIndex,
    ) -> Result<onionskin_core::MarkedPage, CanvasError> {
        Ok(self.document.borrow_mut().marked_page(page)?)
    }

    /// Box the content of a structure element or a content entry on the view,
    /// replacing what was boxed before. Page-space boxes, as content reports
    /// them. Empty clears it.
    pub fn set_structure_highlight(&mut self, boxes: Vec<(PageIndex, [f64; 4])>) {
        self.structure_highlight = boxes;
    }

    pub fn attachments(&mut self) -> Result<Vec<Attachment>, CanvasError> {
        Ok(self.document.borrow_mut().attachments()?.to_vec())
    }

    pub fn attachment_bytes(&mut self, index: usize) -> Result<Vec<u8>, CanvasError> {
        Ok(self.document.borrow_mut().attachment_bytes(index)?)
    }

    /// The signature fields, each with what validating it found and
    /// whether its signer is trusted, by `trust`. Unchecked until asked
    /// when `trust` does not check on opening. A document whose signatures
    /// cannot be validated as a whole still lists its fields, unchecked.
    pub(in crate::shell) fn signatures(
        &mut self,
        trust: &super::panes::signatures::SignatureTrust,
    ) -> Result<Vec<super::panes::signatures::SignatureRow>, CanvasError> {
        let mut document = self.document.borrow_mut();
        let fields = document.signatures()?.to_vec();
        let validations = if trust.verify_on_open || self.signatures_requested {
            document.validate_signatures().unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(super::panes::signatures::rows(fields, validations, trust))
    }

    /// Validate All: check this document's signatures from now on.
    pub(in crate::shell) fn request_signature_validation(&mut self) {
        self.signatures_requested = true;
    }

    /// The document as the signature on `field` signed it, when the field
    /// is signed and its signature covers a known length of the file.
    pub(in crate::shell) fn signed_version(&mut self, field: &str) -> Option<Vec<u8>> {
        let mut document = self.document.borrow_mut();
        let validations = document.validate_signatures().ok()?;
        let validation = validations.iter().find(|found| found.field == field)?;
        document.signed_version(validation)
    }

    pub fn layers(&mut self) -> Result<Vec<Layer>, CanvasError> {
        Ok(self.document.borrow_mut().layers()?.to_vec())
    }

    /// Every annotation the edited document carries, for the Comments pane.
    pub fn annotations(&mut self) -> Result<Vec<onionskin_core::ReadAnnotation>, CanvasError> {
        Ok(self.document.borrow_mut().document_mut().annotations()?)
    }

    /// Show or hide one optional content group, and drop every pixel that
    /// predates the change.
    ///
    /// The store is keyed by page and zoom, not by the options the raster was
    /// produced with, so nothing in it would be rebuilt on its own. Clearing
    /// it is not enough either: the signature has not changed, so without
    /// resetting it the generation would not advance and the visible pages
    /// would never be asked for again.
    pub fn set_layer_visible(&mut self, layer: ObjRef, visible: bool) -> Result<bool, CanvasError> {
        if !self
            .document
            .borrow_mut()
            .set_layer_visible(layer, visible)?
        {
            return Ok(false);
        }
        self.invalidate_rendered_pixels();
        Ok(true)
    }

    /// Every layer at the file's own defaults, for Layer Properties.
    pub(in crate::shell) fn layer_defaults(&mut self) -> Result<Vec<Layer>, CanvasError> {
        Ok(self.document.borrow_mut().layer_defaults()?)
    }

    /// Layer Properties' Apply, as one undoable step; every raster is drawn
    /// again, since the defaults the renderer starts from changed.
    pub(in crate::shell) fn set_layer_properties(
        &mut self,
        layer: ObjRef,
        properties: &onionskin_core::LayerProperties,
    ) -> Result<(), CanvasError> {
        self.document
            .borrow_mut()
            .set_layer_properties(layer, properties)?;
        self.invalidate_rendered_pixels();
        Ok(())
    }

    /// A number that changes with every edit, undo and redo of the
    /// document, for a view that has to follow them.
    pub fn edit_epoch(&self) -> u64 {
        self.document.borrow_mut().edit().epoch()
    }

    /// Drop the cached pixels when the document has been edited since they
    /// were drawn. A new comment is on the page the moment it is committed,
    /// not the next time the zoom changes.
    fn drop_pixels_older_than_the_document(&mut self) {
        let epoch = self.document.borrow_mut().edit().epoch();
        if epoch != self.pixels_epoch {
            self.pixels_epoch = epoch;
            self.invalidate_rendered_pixels();
        }
    }

    /// Forget every cached raster and make the visible pages be rendered
    /// again, because the render options they were produced with have
    /// changed.
    fn invalidate_rendered_pixels(&mut self) {
        // Every thumbnail asked for under the old options is now a picture
        // of a document nobody is showing. Advancing the epoch is what makes
        // the answers already in flight be dropped when they arrive; wrapping
        // is harmless, since it would take an epoch's worth of toggles to
        // reach a number still outstanding.
        self.thumbnail_epoch = self.thumbnail_epoch.wrapping_add(1);
        self.pending_thumbnails.clear();
        self.ready_thumbnails.clear();
        self.tiles.clear();
        self.requests.clear();
        self.failed_renders.clear();
        // The next update compares the visible set against `None`, advances
        // the generation, and re-requests every page. The advance is also
        // what makes the answers already in flight, which were rendered with
        // the old options, be dropped rather than painted.
        self.signature = None;
    }

    /// Draw strokes one pixel wide (`on`) or at their own widths: View >
    /// Show/Hide > Line Weights, off and on. The session's render queues all
    /// take it; this canvas drops the pixels it drew the other way. Tracked
    /// per canvas rather than read from the session, because the first of
    /// two windows to hear of the change sets the session's flag and the
    /// second still has to redraw.
    pub fn set_hairline_strokes(&mut self, on: bool) -> Result<bool, CanvasError> {
        if self.hairline_strokes == on {
            return Ok(false);
        }
        self.document.borrow_mut().set_hairline_strokes(on)?;
        self.hairline_strokes = on;
        self.invalidate_rendered_pixels();
        Ok(true)
    }

    /// Whether this canvas draws strokes one pixel wide.
    pub fn hairline_strokes(&self) -> bool {
        self.hairline_strokes
    }

    /// Put every optional content group back to the visibility the file's
    /// own default configuration gives it, dropping the pixels that were
    /// produced under the overrides.
    pub fn reset_layer_visibility(&mut self) -> Result<bool, CanvasError> {
        if !self.document.borrow_mut().reset_layer_visibility()? {
            return Ok(false);
        }
        self.invalidate_rendered_pixels();
        Ok(true)
    }

    /// Queue a thumbnail of `page` at `zoom`, unless the same picture is
    /// already outstanding.
    ///
    /// "The same picture" is the page, the size and the options epoch
    /// together. A request that differs in any of them replaces the
    /// outstanding one, which is what makes the answer to the old one
    /// arrive to nothing waiting for it and be dropped.
    pub fn request_thumbnail(&mut self, page: PageIndex, zoom: f32) -> Result<(), CanvasError> {
        let request = ThumbnailRequest {
            page,
            zoom,
            epoch: self.thumbnail_epoch,
        };
        if self.pending_thumbnails.get(&page) == Some(&request) {
            return Ok(());
        }
        let replaced = self.pending_thumbnails.insert(page, request);
        if let Err(error) = self.queue_thumbnail(request) {
            match replaced {
                Some(previous) => self.pending_thumbnails.insert(page, previous),
                None => self.pending_thumbnails.remove(&page),
            };
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

    /// Whether a thumbnail is on the way for `page`.
    pub fn thumbnail_pending(&self, page: PageIndex) -> bool {
        self.pending_thumbnails.contains_key(&page)
    }

    /// Take one answer, if it is still the answer to something outstanding.
    ///
    /// An answer that does not match what is pending was rendered under
    /// options or at a size this canvas has moved on from, and keeping it
    /// would show the document as it was: the pane has already dropped its
    /// pictures, and a picture it did not ask for would stop it asking
    /// again. Returns whether it was kept.
    fn accept_thumbnail(&mut self, response: ThumbnailResponse) -> bool {
        let request = response.request();
        if self.pending_thumbnails.get(&request.page) != Some(&request) {
            return false;
        }
        self.pending_thumbnails.remove(&request.page);
        self.responses += 1;
        match response {
            ThumbnailResponse::Ready { render, .. } => {
                self.ready_thumbnails.push((request.page, render.raster));
            }
            // Reported on the page it belongs to, like a failed render: a
            // blank row with no reason is a pane that looks broken.
            ThumbnailResponse::Failed { error, .. } => {
                self.status = Some(CanvasStatus::Error {
                    page: Some(request.page),
                    message: format!("page {} thumbnail: {error}", request.page),
                });
            }
        }
        true
    }

    fn drain_thumbnail_responses(&mut self) -> Result<(), CanvasError> {
        while let Some(response) = self.next_thumbnail_response()? {
            self.accept_thumbnail(response);
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
            auto_scrolling: self.auto_scrolling(),
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

    pub fn fit_visible(&mut self) -> Result<bool, CanvasError> {
        let bounds = self.visible_content_bounds()?;
        self.fit(FitMode::Visible(bounds))
    }

    /// The rectangle around everything the current page draws, in the page's
    /// unrotated render space, which is what [`FitMode::Visible`] is
    /// expressed in.
    ///
    /// Read off the raster the canvas already holds for the page, so it
    /// covers whatever the renderer put on the paper: images and vector art
    /// as well as text. There is no cheaper source that is not also a
    /// narrower one.
    fn visible_content_bounds(&self) -> Result<PageRenderRect, CanvasError> {
        let page = self.viewport.current_page();
        let (Some(source), Some(geometry)) =
            (self.tiles.base(page), self.viewport.page_geometry(page))
        else {
            return Err(CanvasError::FitVisibleUnrendered { page });
        };
        let marks = source
            .content_bounds()
            .ok_or(CanvasError::FitVisibleBlank { page })?;
        content_rect(
            page,
            marks,
            (source.width(), source.height()),
            ViewSize {
                width: geometry.render_size.0 as f32,
                height: geometry.render_size.1 as f32,
            },
        )
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

    pub fn search(&self) -> std::cell::Ref<'_, SearchState> {
        std::cell::Ref::map(self.document.borrow(), |file| file.search())
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
        let started = self
            .document
            .borrow_mut()
            .start_search(needle, options, start_page)?;
        self.find_seen_at = self.session_stamp();
        Ok(started)
    }

    pub fn cancel_search(&mut self) {
        self.pending_reveal = None;
        self.document.borrow_mut().cancel_search();
    }

    pub fn select_next_match(&mut self) -> Result<bool, CanvasError> {
        if !self.document.borrow_mut().select_next_match() {
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
        if !self.document.borrow_mut().select_match(page, index) {
            return Ok(false);
        }
        self.reveal_as_navigation()?;
        Ok(true)
    }

    pub fn select_previous_match(&mut self) -> Result<bool, CanvasError> {
        if !self.document.borrow_mut().select_previous_match() {
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
        let stamp = self.session_stamp();
        if stamp != self.find_seen_at {
            self.find_seen_at = stamp;
            self.pending_reveal = None;
            let (needle, options) = {
                let search = self.document.borrow();
                (
                    search.search().needle().to_owned(),
                    search.search().options(),
                )
            };
            if !needle.is_empty() {
                self.start_search(&needle, options)?;
            }
        }
        let before = self.document.borrow_mut().search().cursor();
        if self.document.borrow_mut().poll_search()
            && self.document.borrow_mut().search().cursor() != before
        {
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
        let Some((page, quads)) = self
            .document
            .borrow()
            .search()
            .current()
            .map(|hit| (hit.page, hit.quads.clone()))
        else {
            return Ok(());
        };
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
                    doc: &mut self.document.borrow_mut(),
                    viewport: &mut self.viewport,
                });
        }
        self.active_tool = Some(index);
        self.registry
            .tool_mut(index)
            .expect("validated tools remain registered")
            .on_activate(&mut ToolCtx {
                doc: &mut self.document.borrow_mut(),
                viewport: &mut self.viewport,
            });
        Ok(true)
    }

    pub fn canvas_origin(&self) -> ViewPoint {
        self.canvas_origin
    }

    /// A window point in the canvas's own coordinates.
    ///
    /// The one place the canvas origin is subtracted. Hit testing, panning
    /// and the zoom anchors used to do it in three places across two modules,
    /// and one of the three left it out: a pan tracked raw window points,
    /// which agree with these only while the origin holds still. When it
    /// moves mid-drag, the canvas-local delta is the one that keeps the
    /// content the pointer grabbed under the pointer, because the content
    /// moved with the origin and the pointer did not.
    fn canvas_point(&self, window: Point<Pixels>) -> ViewPoint {
        ViewPoint {
            x: f32::from(window.x) - self.canvas_origin.x,
            y: f32::from(window.y) - self.canvas_origin.y,
        }
    }

    pub fn status(&self) -> Option<&CanvasStatus> {
        self.status.as_ref()
    }

    fn has_pending_render(&self) -> bool {
        !self.requests.is_empty()
    }

    pub fn has_pending_work(&self) -> bool {
        self.has_pending_pages()
            || self.document.borrow_mut().search().is_running()
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

    pub(in crate::shell) fn reset_worker_wait(&mut self) {
        self.waiting = None;
    }

    #[cfg(test)]
    pub(in crate::shell) fn seed_worker_wait_for_test(&mut self, page: PageIndex, now: Instant) {
        self.geometry_requests.insert(page);
        assert!(self.poll_again(now));
    }

    #[cfg(test)]
    pub(in crate::shell) fn has_worker_wait_for_test(&self) -> bool {
        self.waiting.is_some()
    }

    pub fn resize(&mut self, origin: ViewPoint, size: ViewSize) -> Result<(), CanvasError> {
        self.canvas_origin = origin;
        if self.viewport.size() != size {
            self.viewport.resize(size)?;
        }
        Ok(())
    }

    /// `at` is the zoom anchor in window coordinates, as the platform
    /// reports it; `delta` is a displacement, which no origin applies to.
    pub fn scroll(
        &mut self,
        delta: ViewPoint,
        zooming: bool,
        at: Point<Pixels>,
    ) -> Result<(), CanvasError> {
        let at = self.canvas_point(at);
        self.pause_auto_scroll(Instant::now());
        self.viewport.scroll(delta, zooming, at)?;
        Ok(())
    }

    /// Drop a pinch the platform reports with a nonsense factor, and zoom by
    /// anything else. The filter lives here, at the OS event boundary, rather
    /// than in the viewport, which treats a non-positive factor as the error
    /// it is.
    pub fn pinch(&mut self, factor: f32, at: Point<Pixels>) -> Result<bool, CanvasError> {
        if !(factor.is_finite() && factor > 0.0) {
            return Ok(false);
        }
        let at = self.canvas_point(at);
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
        self.pause_auto_scroll(Instant::now());
        let at = self.canvas_point(position);
        if self.active_tool.is_none() {
            self.input.begin_pan(at);
            return Ok(true);
        }

        let Some(mut input) = self.map_pointer(at, pressure, modifiers)? else {
            return Ok(false);
        };
        input.clicks = std::mem::replace(&mut self.click_count, 1).max(1);
        self.input.begin_tool();
        self.dispatch_tool(ToolPointerPhase::Down, input);
        Ok(true)
    }

    /// The platform's click count for the press about to be reported, which
    /// is how a double click reaches a tool.
    pub fn set_click_count(&mut self, clicks: usize) {
        self.click_count = u8::try_from(clicks).unwrap_or(u8::MAX);
    }

    pub fn pointer_move(
        &mut self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
        left_button_pressed: bool,
    ) -> Result<bool, CanvasError> {
        let at = self.canvas_point(position);
        let update = self.input.move_to(at, left_button_pressed);
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
                let input = match self.map_pointer(at, pressure, modifiers) {
                    Ok(Some(input)) => input,
                    Ok(None) => match self.map_pointer_off_page(at, pressure, modifiers)? {
                        Some(input) => input,
                        None => {
                            self.input.cancel();
                            self.cancel_active_tool();
                            return Ok(true);
                        }
                    },
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
        let at = self.canvas_point(position);
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
                match self.map_pointer(at, pressure, modifiers) {
                    Ok(Some(input)) => self.dispatch_tool(ToolPointerPhase::Up, input),
                    Ok(None) => match self.map_pointer_off_page(at, pressure, modifiers)? {
                        Some(input) => self.dispatch_tool(ToolPointerPhase::Up, input),
                        None => self.cancel_active_tool(),
                    },
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
        self.follow_session()?;
        self.poll_search()?;
        self.drain_thumbnail_responses()?;
        self.drain_geometry_responses()?;
        self.queue_visible_geometry()?;
        self.drain_geometry_responses()?;
        self.apply_pending_reveal()?;

        self.drop_pixels_older_than_the_document();
        let visible = self.viewport.visible_pages()?;
        self.update_signature(&visible)?;
        // The rasters this drains are exempt from eviction until the frame
        // closes below. That matters when several land at once: four of them
        // at 6x are 266 MiB against a 202 MiB budget, and unframed they would
        // evict each other as they arrived.
        //
        // The frame closes here rather than in `paint_list`, so an update
        // that is not followed by a paint, which is every pointer move and
        // every poll tick, leaves nothing open behind it, on the error path
        // as well. Its pages stay exempt for one more frame, which is what
        // carries them into the paint.
        self.tiles.begin_frame();
        let framed = self.drain_and_schedule(&visible);
        self.tiles.end_frame();
        framed
    }

    /// The part of an update that has to run inside a store frame: applying
    /// the rasters that have arrived and asking for the ones that have not.
    ///
    /// Split out so its errors cannot escape past the `end_frame` that
    /// matches this frame's `begin_frame`.
    fn drain_and_schedule(&mut self, visible: &[PagePlacement]) -> Result<(), CanvasError> {
        self.drain_render_responses()?;
        self.schedule_visible_renders(visible)?;
        self.drain_render_responses()?;
        Ok(())
    }

    /// This window's render queue: its own view's when it is not the
    /// document's first window, the session's primary otherwise. Each
    /// borrows the session for the one call.
    fn queue_render(
        document: &SharedFile,
        view: &mut Option<onionskin_core::RenderView>,
        request: RenderRequest,
        geometry: &onionskin_core::PageGeometry,
        source: Option<&onionskin_render::BaseRaster>,
    ) -> Result<(), onionskin_core::Error> {
        let mut file = document.borrow_mut();
        match view.as_mut() {
            Some(view) => file.request_render_with_geometry_in(view, request, geometry, source),
            None => file.request_render_with_geometry(request, geometry, source),
        }
    }

    fn next_render_response(&mut self) -> Result<Option<RenderResponse>, onionskin_core::Error> {
        let mut file = self.document.borrow_mut();
        match self.view.as_mut() {
            Some(view) => file.try_render_response_in(view),
            None => file.try_render_response(),
        }
    }

    fn queue_page_geometry(&mut self, page: PageIndex) -> Result<bool, onionskin_core::Error> {
        let mut file = self.document.borrow_mut();
        match self.view.as_mut() {
            Some(view) => file.request_page_geometry_in(view, page),
            None => file.request_page_geometry(page),
        }
    }

    fn next_geometry_response(
        &mut self,
    ) -> Result<Option<onionskin_core::PageGeometryResponse>, onionskin_core::Error> {
        let mut file = self.document.borrow_mut();
        match self.view.as_mut() {
            Some(view) => file.try_page_geometry_response_in(view),
            None => file.try_page_geometry_response(),
        }
    }

    fn queue_thumbnail(
        &mut self,
        request: onionskin_core::ThumbnailRequest,
    ) -> Result<(), onionskin_core::Error> {
        let mut file = self.document.borrow_mut();
        match self.view.as_mut() {
            Some(view) => file.request_thumbnail_in(view, request),
            None => file.request_thumbnail(request),
        }
    }

    fn next_thumbnail_response(
        &mut self,
    ) -> Result<Option<onionskin_core::ThumbnailResponse>, onionskin_core::Error> {
        let mut file = self.document.borrow_mut();
        match self.view.as_mut() {
            Some(view) => file.try_thumbnail_response_in(view),
            None => file.try_thumbnail_response(),
        }
    }

    fn drain_geometry_responses(&mut self) -> Result<usize, CanvasError> {
        let mut drained = 0;
        while let Some(response) = self.next_geometry_response()? {
            drained += 1;
            self.apply_geometry_response(response)?;
        }
        Ok(drained)
    }

    /// Apply every render answer waiting, dropping the ones whose generation
    /// the current signature has moved past.
    ///
    /// The signature is the caller's to refresh. This used to re-derive it
    /// per drain, which re-walked the layout for a visible set `update` had
    /// computed moments earlier and could not have changed since.
    fn drain_render_responses(&mut self) -> Result<usize, CanvasError> {
        let mut drained = 0;
        while let Some(response) = self.next_render_response()? {
            drained += usize::from(self.apply_render_response(response));
        }
        Ok(drained)
    }

    /// What to draw this frame, in canvas coordinates.
    ///
    /// Opens a store frame of its own. `update` pins the cache each visible
    /// page has at the exact zoom; the paint may instead fall back to a cache
    /// at another zoom, and that one has to be exempt from eviction too while
    /// the rest of the frame is cut. Everything that can fail before a page
    /// is asked for happens first, so a frame that opens here also closes
    /// here.
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

        self.tiles.begin_frame();
        let cut = self.cut_visible_tiles(&visible, rotation, exact_zoom, viewport_size);
        self.tiles.end_frame();
        paint.tiles = cut?;
        paint.overlays = self.overlay_paints();
        Ok(paint)
    }

    /// The tiles of every measured visible page, as images the window can
    /// paint.
    ///
    /// Split out of [`Self::paint_list`] so the store frame it runs inside is
    /// closed whatever this returns. The conversion below can fail on a tile
    /// whose buffer disagrees with its own dimensions, and an error escaping
    /// past the `end_frame` would leave the frame open, which is the state the
    /// frame boundary exists to prevent.
    fn cut_visible_tiles(
        &mut self,
        visible: &[PagePlacement],
        rotation: ViewRotation,
        exact_zoom: f32,
        viewport_size: ViewSize,
    ) -> Result<Vec<TilePaint>, CanvasError> {
        let mut tiles = Vec::new();
        let mut displayed_images = BTreeSet::new();
        for placement in visible.iter().filter(|page| page.measured) {
            // One lookup, so the raster the tiles are cut from and the zoom
            // they are reported at cannot come from two places and disagree.
            let Some(cache) = self.tiles.paint_source(placement.page, exact_zoom) else {
                continue;
            };
            let source_zoom = cache.base().zoom();
            let raw_tiles = collect_tiles(cache, placement.rect, rotation, viewport_size);
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
                tiles.push(TilePaint {
                    page: placement.page,
                    rect: atlas_image_rect(raw.rect, content_width, content_height),
                    clip_rect: raw.rect,
                    source_zoom,
                    image,
                });
            }
        }
        self.image_cache.retain_keys(&displayed_images);
        Ok(tiles)
    }

    /// Fulfil a pending snapshot request as owned pixels ready to encode, or
    /// `Ok(None)` when no tool has raised one.
    ///
    /// This crops the page raster the canvas is already painting, at the
    /// zoom it was rasterized at, and turns it by the rotation it is shown
    /// under, rather than asking the renderer for the region again: the
    /// point of a snapshot is the pixels the user drew a marquee around.
    /// Overlays are painted separately and are not in that raster, so the
    /// snapshot is the page alone. Acrobat's includes annotations, which is
    /// a gap to close when there are annotations to include.
    pub(super) fn take_snapshot_pixels(&mut self) -> Result<Option<SnapshotPixels>, CanvasError> {
        let Some(request) = self.document.borrow_mut().take_snapshot_request() else {
            return Ok(None);
        };
        let page = request.region.page;
        let (Some(source), Some(geometry)) =
            (self.tiles.base(page), self.viewport.page_geometry(page))
        else {
            return Err(CanvasError::SnapshotUnrendered { page });
        };
        let crop = raster_crop(geometry, request.region, source)?;
        prepare_snapshot_pixels(source, crop, self.viewport.rotation()).map(Some)
    }

    #[cfg(test)]
    pub(in crate::shell) fn request_snapshot_for_test(&mut self, region: PageRect) {
        self.document.borrow_mut().request_snapshot(region);
    }

    /// Whether the current page already has a rendered raster.
    ///
    /// The render worker runs in a test too, so a test that needs a raster
    /// has to say "seed one unless the real one already arrived" rather than
    /// race the worker for the right to supply it.
    #[cfg(test)]
    pub(in crate::shell) fn has_rendered_current_page_for_test(&self) -> bool {
        self.tiles.base(self.viewport.current_page()).is_some()
    }

    #[cfg(test)]
    pub(in crate::shell) fn seed_visible_raster_for_test(
        &mut self,
        rgba: [u8; 4],
    ) -> Result<(), CanvasError> {
        let visible = self.viewport.visible_pages()?;
        self.update_signature(&visible)?;
        self.tiles.begin_frame();
        self.schedule_visible_renders(&visible)?;
        let page = visible
            .first()
            .expect("a non-empty document has a visible page")
            .page;
        let request = *self
            .requests
            .get(&page)
            .expect("the visible page is requested");
        let geometry = self
            .viewport
            .page_geometry(page)
            .expect("the visible page is measured");
        let (width, height) = onionskin_render::raster_size(
            geometry.render_size.0 as f32,
            geometry.render_size.1 as f32,
            request.zoom,
        )
        .expect("the visible page raster fits");
        let raster = BaseRaster::new(
            u32::from(width),
            u32::from(height),
            request.zoom,
            rgba.repeat(usize::from(width) * usize::from(height)),
        );
        assert!(self.apply_render_response(RenderResponse::Raster {
            request,
            render: PageRender {
                raster,
                warnings: Vec::new(),
            },
        }));
        self.paint_list()?;
        Ok(())
    }

    /// Every hit on the pages currently on screen. Highlight-all is drawn from
    /// the results found so far, so a walk still running highlights the pages
    /// it has already reported.
    fn highlights(&self, visible: &[PagePlacement]) -> Result<Vec<HighlightPaint>, CanvasError> {
        let cursor = self.document.borrow_mut().search().cursor();
        let mut highlights = Vec::new();
        // One mapping call per page, not per hit: each one re-walks the
        // layout, and a page can carry hundreds of hits.
        let mut quads: Vec<PageQuad> = Vec::new();
        let mut is_current: Vec<bool> = Vec::new();
        for placement in visible.iter().filter(|placement| placement.measured) {
            let page = placement.page;
            quads.clear();
            is_current.clear();
            for (index, hit) in self
                .document
                .borrow_mut()
                .search()
                .matches_on(page)
                .iter()
                .enumerate()
            {
                quads.extend(hit.quads.iter().copied());
                is_current.resize(quads.len(), cursor == Some((page, index)));
            }
            highlights.extend(
                self.viewport
                    .page_quad_rects(page, &quads)?
                    .into_iter()
                    .zip(is_current.iter().copied())
                    .map(|(rect, current)| HighlightPaint {
                        rect,
                        current,
                        structure: false,
                    }),
            );
        }
        for (page, bounds) in &self.structure_highlight {
            // `page_quad_rects` walks the whole layout for each call, and a
            // box for every page of a long document is a box for every frame:
            // only the pages on screen are asked about.
            if !visible.iter().any(|placement| placement.page == *page) {
                continue;
            }
            let quad = PageQuad {
                page: *page,
                corners: [
                    (bounds[0], bounds[3]),
                    (bounds[2], bounds[3]),
                    (bounds[0], bounds[1]),
                    (bounds[2], bounds[1]),
                ],
            };
            highlights.extend(
                self.viewport
                    .page_quad_rects(*page, &[quad])?
                    .into_iter()
                    .map(|rect| HighlightPaint {
                        rect,
                        current: false,
                        structure: true,
                    }),
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
            if self.queue_page_geometry(placement.page)? {
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
        Ok(true)
    }

    /// Ask for a render of every visible page that has no raster at the
    /// current zoom yet.
    ///
    /// An unmeasured page is skipped by having no geometry rather than by
    /// reading `PagePlacement::measured`: the layout answers both from the
    /// same map, so asking for the geometry directly says what the render
    /// needs and cannot disagree with the flag. It used to be a filter on the
    /// flag and an `expect` on the geometry, which is one invariant asserted
    /// twice.
    ///
    /// Claiming the raster is why this runs for every visible page and not
    /// only the ones it goes on to ask for. The store is the canvas's only
    /// raster owner, so a page's cache survives the frame only if the frame
    /// claims it; a zoom change has no cache at the exact zoom, and asking
    /// for one would claim nothing and let the close of the frame evict the
    /// raster the paint was about to scale.
    fn schedule_visible_renders(
        &mut self,
        visible: &[PagePlacement],
    ) -> Result<usize, CanvasError> {
        let zoom = self.viewport.zoom();
        let mut queued = 0;
        for placement in visible {
            let Some(geometry) = self.viewport.page_geometry(placement.page) else {
                continue;
            };
            // Whatever this page will paint from, the cache at this zoom or
            // the one the paint scales instead, is this frame's. It is also
            // what the worker scales into the placeholder it answers with
            // immediately.
            let resident = self
                .tiles
                .paint_source(placement.page, zoom)
                .map(TileCache::base);
            let request = RenderRequest {
                page: placement.page,
                zoom,
                generation: self.generation,
            };
            if resident.is_some_and(|base| base.zoom().to_bits() == zoom.to_bits())
                || self.requests.get(&placement.page) == Some(&request)
                || self.failed_renders.contains(&placement.page)
            {
                continue;
            }
            Self::queue_render(&self.document, &mut self.view, request, geometry, resident)?;
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
            // Nothing to record. The worker echoes back the raster the
            // request carried, which is the one the store already holds and
            // the one the placeholder is drawn by scaling, and the page stays
            // in `requests`, which is what keeps the poll armed until a
            // terminal answer arrives.
            RenderResponse::Placeholder(_) => {}
            RenderResponse::Raster { render, .. } => {
                self.requests.remove(&request.page);
                self.failed_renders.remove(&request.page);
                self.tiles.insert(request.page, render.raster);
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
                self.status = Some(CanvasStatus::Error {
                    page: Some(request.page),
                    message: format!("page {} at {}x: {error}", request.page, request.zoom),
                });
            }
        }
        true
    }

    /// What the active tool wants drawn this frame, in canvas coordinates.
    ///
    /// Every `Overlay` variant has a painter, so there is nothing left to
    /// report: `map_overlay` returns an `Option` for a placement the viewport
    /// cannot make this frame, and a variant with no painter is a compile
    /// error rather than a status the user has to read.
    fn overlay_paints(&mut self) -> Vec<OverlayPaint> {
        let overlays = match self
            .active_tool
            .and_then(|index| self.registry.tools().nth(index))
        {
            Some(tool) => tool.overlays(&self.document.borrow()),
            None => return Vec::new(),
        };
        overlays
            .iter()
            .filter_map(|overlay| self.map_overlay(overlay))
            .collect()
    }

    /// `None` for an overlay the viewport cannot place this frame.
    ///
    /// There is no error arm. A variant added to `Overlay` without a painter
    /// here fails to compile, which is the only form of exhaustiveness that
    /// survives someone adding a variant in a hurry.
    fn map_overlay(&self, overlay: &Overlay) -> Option<OverlayPaint> {
        match overlay {
            Overlay::Quads(quads) => {
                let mapped: Vec<[ViewPoint; 4]> = quads
                    .iter()
                    .filter_map(|quad| self.map_quad(*quad))
                    .collect();
                (!mapped.is_empty()).then_some(OverlayPaint::Quads(mapped))
            }
            Overlay::AntsRect(rect) => self
                .map_quad((*rect).into())
                .map(|corners| OverlayPaint::AntsRect(bounding_rect(corners))),
            Overlay::Rect(rect) => self
                .map_quad((*rect).into())
                .map(|corners| OverlayPaint::Rect(bounding_rect(corners))),
            // An unplaceable point drops the whole path rather than being
            // skipped: a polyline missing one vertex is a different shape, and
            // drawing a different shape is worse than drawing none.
            Overlay::Polyline { points, closed } => {
                let mapped = points
                    .iter()
                    .map(|point| self.map_point(*point))
                    .collect::<Option<Vec<ViewPoint>>>()?;
                (mapped.len() >= 2).then_some(OverlayPaint::Polyline {
                    points: mapped,
                    closed: *closed,
                })
            }
            Overlay::Line { from, to } => Some(OverlayPaint::Line {
                from: self.map_point(*from)?,
                to: self.map_point(*to)?,
            }),
            Overlay::Ellipse { bounds } => self
                .map_quad((*bounds).into())
                .map(|corners| OverlayPaint::Ellipse(bounding_rect(corners))),
        }
    }

    /// One page point in canvas coordinates, or `None` when the page is not
    /// laid out in the current mode or has not been measured yet.
    fn map_point(&self, at: PagePoint) -> Option<ViewPoint> {
        self.viewport.view_point_for(at).ok().flatten()
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
        at: ViewPoint,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<Option<PointerInput>, CanvasError> {
        Ok(pointer_input(&self.viewport, at, pressure, modifiers)?)
    }

    /// The pointer expressed against the nearest visible page, for a tool
    /// that must keep hearing about it after it leaves the page.
    ///
    /// `None` for every other tool, which keeps the cancel-on-leave rule:
    /// for a marquee or a text selection, the pointer leaving the page is
    /// the user leaving the gesture. For a tool that zooms continuously it
    /// is the gesture working, because zooming out shrinks the page away
    /// from a pointer that is still on screen and still held down. Asked of
    /// the tool's declared capability rather than its id, the way every
    /// other shell surface finds a tool.
    fn map_pointer_off_page(
        &self,
        at: ViewPoint,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<Option<PointerInput>, CanvasError> {
        let follows_the_pointer_off_page = self
            .active_tool
            .and_then(|index| self.registry.tools().nth(index))
            .is_some_and(|tool| {
                tool.capabilities()
                    .contains(&onionskin_plugin_api::ToolCapability::DynamicZoom)
            });
        if !follows_the_pointer_off_page {
            return Ok(None);
        }
        Ok(pointer_input_near(&self.viewport, at, pressure, modifiers)?)
    }

    fn dispatch_tool(&mut self, phase: ToolPointerPhase, input: PointerInput) {
        let index = self
            .active_tool
            .expect("tool dispatch requires an active tool");
        // A tool that places something to write in: note what was on the page
        // before the release, so what the release added can be found and a
        // text field opened on it.
        let takes_text = phase == ToolPointerPhase::Up
            && self
                .registry
                .tool(index)
                .is_some_and(|tool| tool.takes_text());
        let before = if takes_text {
            self.document.borrow_mut().document_mut().annotations().ok()
        } else {
            None
        };
        self.dispatch_tool_inner(phase, input, index);
        if let Some(before) = before {
            self.text_target = self.newly_placed(&before);
        }
    }

    /// What a text tool's release just placed: the comment to write in. A
    /// Replace Text writes a strike-out and a note answering it, and the note
    /// is where the replacement is typed, so a note or free text is preferred
    /// over the markup that came with it.
    fn newly_placed(&mut self, before: &[onionskin_core::ReadAnnotation]) -> Option<TextTarget> {
        let after = self
            .document
            .borrow_mut()
            .document_mut()
            .annotations()
            .ok()?;
        let fresh: Vec<_> = after
            .into_iter()
            .filter(|annotation| !before.iter().any(|old| old.objref == annotation.objref))
            .collect();
        let chosen = fresh
            .iter()
            .find(|annotation| {
                matches!(
                    annotation.subtype,
                    Some(onionskin_core::Subtype::Text | onionskin_core::Subtype::FreeText)
                )
            })
            .or_else(|| fresh.first())?;
        Some(TextTarget {
            annotation: chosen.objref,
            page: chosen.page,
            rect: chosen.rect,
            popup: chosen.subtype != Some(onionskin_core::Subtype::FreeText),
        })
    }

    /// The comment a text tool just placed, waiting for its text, if any.
    pub fn text_target(&self) -> Option<TextTarget> {
        self.text_target
    }

    /// Where the text field for [`Self::text_target`] goes, in canvas
    /// coordinates: over a free text's own box, or as a pop-up beside a note,
    /// where Acrobat opens its pop-up note.
    pub fn text_target_rect(&self) -> Option<(ViewPoint, f32, f32)> {
        let target = self.text_target?;
        let rect = [
            target.rect.x0,
            target.rect.y0,
            target.rect.x1,
            target.rect.y1,
        ];
        let (at, width, height) = self.view_rect(target.page, rect)?;
        let (left, right, top, bottom) = (at.x, at.x + width, at.y, at.y + height);
        if target.popup {
            Some((
                ViewPoint {
                    x: right + 8.0,
                    y: top,
                },
                260.0,
                32.0,
            ))
        } else {
            Some((
                ViewPoint { x: left, y: top },
                (right - left).max(160.0),
                (bottom - top).max(28.0),
            ))
        }
    }

    /// Where page rectangle `rect` on `page` is, in canvas coordinates: its
    /// top-left corner, width and height, whatever the page's rotation.
    pub fn view_rect(
        &self,
        page: PageIndex,
        [x0, y0, x1, y1]: [f64; 4],
    ) -> Option<(ViewPoint, f32, f32)> {
        let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
            .map(|(x, y)| self.map_point(PagePoint { page, x, y }));
        let mut points = Vec::with_capacity(4);
        for corner in corners {
            points.push(corner?);
        }
        let left = points.iter().map(|at| at.x).fold(f32::INFINITY, f32::min);
        let right = points
            .iter()
            .map(|at| at.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let top = points.iter().map(|at| at.y).fold(f32::INFINITY, f32::min);
        let bottom = points
            .iter()
            .map(|at| at.y)
            .fold(f32::NEG_INFINITY, f32::max);
        Some((ViewPoint { x: left, y: top }, right - left, bottom - top))
    }

    /// Write `text` into the waiting comment, as one undoable step, and stop
    /// waiting. Nothing is written for an empty text: the comment stays as
    /// placed, which Undo can take away.
    pub fn finish_text(&mut self, text: &str) -> Result<bool, CanvasError> {
        let Some(target) = self.text_target.take() else {
            return Ok(false);
        };
        if text.trim().is_empty() {
            return Ok(false);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        self.document
            .borrow_mut()
            .document_mut()
            .edit_document("Edit Comment Text", |tx| {
                onionskin_core::review::set_contents(tx, target.annotation, text, now)
            })?;
        Ok(true)
    }

    fn dispatch_tool_inner(&mut self, phase: ToolPointerPhase, input: PointerInput, index: usize) {
        let mut file = self.document.borrow_mut();
        let document = file.document_mut();
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

    /// Enter on the canvas: the active tool finishes what it has pending,
    /// which is how a polygon or a connected line built by clicking ends.
    pub fn commit_active_tool(&mut self) -> bool {
        let Some(index) = self.active_tool else {
            return false;
        };
        let mut file = self.document.borrow_mut();
        let document = file.document_mut();
        let viewport = &mut self.viewport;
        let tool = self
            .registry
            .tool_mut(index)
            .expect("the active tool remains registered");
        tool.on_commit(&mut ToolCtx {
            doc: document,
            viewport,
        });
        true
    }

    /// Whether the active tool is part way through something: it is drawing
    /// a preview of it. Escape abandons that before it does anything else.
    pub fn tool_has_pending_gesture(&self) -> bool {
        self.active_tool
            .and_then(|index| self.registry.tool(index))
            .is_some_and(|tool| !tool.overlays(&self.document.borrow()).is_empty())
    }

    /// Escape on the canvas: drop the active tool's pending gesture.
    pub fn cancel_tool_gesture(&mut self) -> bool {
        if self.active_tool.is_none() {
            return false;
        }
        self.cancel_active_tool();
        true
    }

    /// The active tool's name and how it is used, for the side panel.
    pub fn active_tool_help(&self) -> Option<(&'static str, Option<&'static str>)> {
        let tool = self.registry.tool(self.active_tool?)?;
        Some((tool.name(), tool.hint()))
    }

    /// What the active tool is reading off the page, for the side panel.
    pub(super) fn active_tool_readings(&self) -> Vec<onionskin_plugin_api::Reading> {
        self.active_tool
            .and_then(|index| self.registry.tool(index))
            .map(|tool| tool.readings())
            .unwrap_or_default()
    }

    /// The active tool's settings, each with whether it is on.
    pub(super) fn active_tool_settings(&self) -> Vec<(onionskin_plugin_api::ToolChoice, bool)> {
        let Some(tool) = self.active_tool.and_then(|index| self.registry.tool(index)) else {
            return Vec::new();
        };
        tool.settings()
            .into_iter()
            .map(|setting| {
                let on = tool.picked(&setting.id);
                (setting, on)
            })
            .collect()
    }

    /// Choose or turn over one of the active tool's settings.
    pub(super) fn choose_active_tool_setting(&mut self, id: &str) -> bool {
        self.active_tool
            .is_some_and(|index| self.choose_tool(index, id))
    }

    fn cancel_active_tool(&mut self) {
        let Some(index) = self.active_tool else {
            return;
        };
        let mut document = self.document.borrow_mut();
        let viewport = &mut self.viewport;
        let tool = self
            .registry
            .tool_mut(index)
            .expect("the active tool remains registered");
        tool.on_cancel(&mut ToolCtx {
            doc: &mut document,
            viewport,
        });
    }
}

/// The box around a hit's quads. A hit that wraps two lines is revealed as the
/// one region it occupies, not as its first quad.
/// What the document's form needs said when it opens, if anything.
fn form_notice(file: &mut onionskin_core::DocumentFile) -> Option<String> {
    let form = file.document_mut().form().ok()?;
    form.notice().map(str::to_owned)
}

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
    rotation: ViewRotation,
}

impl TileImageKey {
    fn new(page: PageIndex, zoom: f32, col: u32, row: u32, rotation: ViewRotation) -> Self {
        Self {
            page,
            zoom_bits: zoom.to_bits(),
            col,
            row,
            rotation,
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

/// The visible tiles of one page's cached raster.
///
/// The raster's dimensions come from the cache being cut, never from the
/// caller: `cols` and `rows` are derived from them, so `col * TILE_SIZE` is
/// below `raster_width` by construction and the remainder below cannot
/// underflow. A caller that passed its own size could disagree with the store
/// and did not have to be right.
fn collect_tiles(
    cache: &TileCache,
    page_rect: ViewRect,
    rotation: ViewRotation,
    viewport_size: ViewSize,
) -> Vec<RawTile> {
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
            let rect = tile_rect(page_rect, region, (raster_width, raster_height), rotation);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotPixels {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
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
    let output_width = width + 2 * ATLAS_GUTTER_PX;
    let output_height = height + 2 * ATLAS_GUTTER_PX;
    let mut output = vec![0; output_width as usize * output_height as usize * 4];

    // Every gutter pixel repeats the tile edge nearest to it, so a sample that
    // runs past the tile reads the tile's own colour instead of a neighbour's.
    for y in 0..output_height {
        let source_y = y.saturating_sub(ATLAS_GUTTER_PX).min(height - 1);
        for x in 0..output_width {
            let source_x = x.saturating_sub(ATLAS_GUTTER_PX).min(width - 1);
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
/// A rectangle of raster pixels as a rectangle of the page's unrotated
/// render space.
///
/// Scaled by the raster's own pixel count rather than by its zoom: the
/// renderer floors the pixel count, so dividing by the zoom can place the far
/// edge a fraction outside the page, and `PageRenderRect::new` refuses that
/// rather than fitting a rectangle the page does not contain.
fn content_rect(
    page: PageIndex,
    marks: RasterBounds,
    raster: (u32, u32),
    page_size: ViewSize,
) -> Result<PageRenderRect, CanvasError> {
    let scale_x = page_size.width / raster.0 as f32;
    let scale_y = page_size.height / raster.1 as f32;
    let origin = ViewPoint {
        x: marks.x as f32 * scale_x,
        y: marks.y as f32 * scale_y,
    };
    let size = ViewSize {
        width: (marks.width as f32 * scale_x).min(page_size.width - origin.x),
        height: (marks.height as f32 * scale_y).min(page_size.height - origin.y),
    };
    PageRenderRect::new(page, origin, size, page_size)
        .map_err(|error| ViewportError::from(error).into())
}

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

fn snapshot_buffer_len(width: u32, height: u32) -> Result<usize, CanvasError> {
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(RGBA_BYTES_PER_PIXEL))
        .ok_or(CanvasError::SnapshotTooLarge {
            width,
            height,
            limit: SNAPSHOT_RGBA_BYTE_LIMIT,
        })?;
    if bytes > SNAPSHOT_RGBA_BYTE_LIMIT {
        return Err(CanvasError::SnapshotTooLarge {
            width,
            height,
            limit: SNAPSHOT_RGBA_BYTE_LIMIT,
        });
    }
    Ok(bytes)
}

/// The cropped region, turned by the view rotation, as straight RGBA pixels.
fn prepare_snapshot_pixels(
    source: &BaseRaster,
    crop: RasterCrop,
    rotation: ViewRotation,
) -> Result<SnapshotPixels, CanvasError> {
    let (width, height) = rotated_size(crop.width, crop.height, rotation);
    let mut pixels = vec![0; snapshot_buffer_len(width, height)?];
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
    Ok(SnapshotPixels {
        width,
        height,
        rgba: pixels,
    })
}

/// The one image format GPUI's clipboard entry and every paste target agree on.
pub(super) fn encode_snapshot_png(snapshot: SnapshotPixels) -> Result<Vec<u8>, CanvasError> {
    let mut png = std::io::Cursor::new(Vec::new());
    write_snapshot_png(snapshot, &mut png)?;
    Ok(png.into_inner())
}

fn write_snapshot_png(
    snapshot: SnapshotPixels,
    writer: &mut (impl std::io::Write + std::io::Seek),
) -> Result<(), CanvasError> {
    let expected = snapshot_buffer_len(snapshot.width, snapshot.height)?;
    if snapshot.rgba.len() != expected {
        return Err(CanvasError::InvalidImageBuffer {
            expected,
            actual: snapshot.rgba.len(),
        });
    }
    let image = image::RgbaImage::from_raw(snapshot.width, snapshot.height, snapshot.rgba)
        .expect("the snapshot buffer is width * height * 4 bytes");
    image
        .write_to(writer, image::ImageFormat::Png)
        .map_err(|error| CanvasError::SnapshotEncode(error.to_string()))?;
    Ok(())
}

/// Where one pixel of a `width` by `height` image lands after the turn.
///
/// This is `ViewRotation::rotate_rect_within` applied to a one-by-one rect,
/// exactly, and deliberately not written as a call to it: both callers run it
/// once per pixel inside a double loop over a whole tile or crop, so building
/// a `ViewRect` and a `ViewSize` per pixel would be real work on the paint
/// path for a result that is the same by construction.
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

/// Where one tile of a page's raster lands on screen.
///
/// The turn is `ViewRotation`'s own: a tile inside its raster turns exactly
/// as a rectangle inside a page does, and the app had written those four arms
/// out a second time. What is left here is the part core cannot know, scaling
/// the turned raster onto the rectangle the layout gave the page.
fn tile_rect(
    page: ViewRect,
    tile: TileRegion,
    raster: (u32, u32),
    rotation: ViewRotation,
) -> ViewRect {
    let (raster_width, raster_height) = raster;
    let turned = rotation.rotate_rect_within(
        ViewRect {
            origin: ViewPoint {
                x: (tile.col * TILE_SIZE) as f32,
                y: (tile.row * TILE_SIZE) as f32,
            },
            size: ViewSize {
                width: tile.width as f32,
                height: tile.height as f32,
            },
        },
        ViewSize {
            width: raster_width as f32,
            height: raster_height as f32,
        },
    );
    let (output_width, output_height) = rotated_size(raster_width, raster_height, rotation);
    let scale_x = page.size.width / output_width as f32;
    let scale_y = page.size.height / output_height as f32;
    ViewRect {
        origin: ViewPoint {
            x: page.origin.x + turned.origin.x * scale_x,
            y: page.origin.y + turned.origin.y * scale_y,
        },
        size: ViewSize {
            width: turned.size.width * scale_x,
            height: turned.size.height * scale_y,
        },
    }
}

/// Where to paint a guttered tile image so its content lands exactly on
/// `clip`, which is the rectangle the content alone occupies.
///
/// `content_width` and `content_height` are the tile without its gutter, so
/// the ratio is what one gutter pixel is worth on screen at this zoom.
fn atlas_image_rect(clip: ViewRect, content_width: u32, content_height: u32) -> ViewRect {
    let gutter = ATLAS_GUTTER_PX as f32;
    let gutter_width = gutter * clip.size.width / content_width as f32;
    let gutter_height = gutter * clip.size.height / content_height as f32;
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use gpui::{point, px};
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

    /// A scroll-zoom and a pinch both anchor on a point the platform reports
    /// in window coordinates, so both have to subtract the canvas origin
    /// before the viewport sees it. Anchoring on the raw window point zooms
    /// about a place the chrome's width and height away from the pointer.
    #[test]
    fn zoom_anchors_are_read_in_canvas_coordinates() {
        let origin = ViewPoint { x: 50.0, y: 40.0 };
        let window = point(px(150.0), px(140.0));
        let canvas_local = point(px(100.0), px(100.0));

        let gestures: [fn(&mut CanvasModel, Point<Pixels>); 2] = [
            |model, at| {
                model
                    .scroll(ViewPoint { x: 0.0, y: 30.0 }, true, at)
                    .expect("the view zooms");
            },
            |model, at| {
                assert!(model.pinch(1.4, at).expect("the view zooms"));
            },
        ];
        for gesture in gestures {
            let mut offset = model();
            offset.resize(origin, VIEWPORT).expect("the canvas resizes");
            gesture(&mut offset, window);

            let mut flush = model();
            flush
                .resize(ViewPoint::default(), VIEWPORT)
                .expect("the canvas resizes");
            gesture(&mut flush, canvas_local);

            assert_ne!(
                offset.viewport.snapshot(),
                model().viewport.snapshot(),
                "the gesture did nothing, so it would prove nothing"
            );
            assert_eq!(
                offset.viewport.snapshot(),
                flush.viewport.snapshot(),
                "the anchor was read in window coordinates, not canvas ones"
            );
        }
    }

    /// The canvas origin moves whenever the chrome around it changes size,
    /// and a pan can be in progress across that move. The grabbed content
    /// moves with the origin while the pointer does not, so the viewport has
    /// to follow by the same amount to keep the two together. Tracking raw
    /// window points instead reports no movement at all, and the page stays
    /// where the origin left it.
    #[test]
    fn a_pan_follows_the_canvas_origin_when_it_moves_mid_drag() {
        let mut model = model();
        assert!(
            model.active_tool.is_none(),
            "a pointer press pans only when no tool has it"
        );
        let second = model
            .document
            .borrow_mut()
            .page_geometry(1)
            .unwrap()
            .clone();
        model.viewport.measure_page(second).unwrap();
        // Zoomed in and away from the ends, so the pan has room in both
        // directions and cannot be clamped into looking like a no-op.
        model
            .viewport
            .zoom_to(4.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        model.go_to_page(1).unwrap();
        model.resize(ViewPoint::default(), VIEWPORT).unwrap();

        let grab = point(px(100.0), px(100.0));
        assert!(model
            .pointer_down(grab, 1.0, GpuiModifiers::default())
            .unwrap());
        let before = model.viewport.offset();

        // The chrome above the canvas grows by 50px. The pointer has not
        // moved; the document under it has.
        model
            .resize(ViewPoint { x: 0.0, y: 50.0 }, VIEWPORT)
            .unwrap();
        assert!(model
            .pointer_move(grab, 1.0, GpuiModifiers::default(), true)
            .unwrap());

        assert_eq!(
            model.viewport.offset(),
            ViewPoint {
                x: before.x,
                y: before.y + 50.0
            },
            "the view did not follow the origin, so the grabbed content slid \
             out from under the pointer"
        );
    }

    /// A page command changes what the layout was built from. Run through the
    /// registry, the way the menus run it, the canvas follows: one page fewer
    /// in the viewport, the view kept, and every raster dropped because page
    /// index 0 now names a different page.
    #[cfg(feature = "tools-organize")]
    #[test]
    fn a_page_command_rebuilds_the_layout_from_the_edited_document() {
        use onionskin_plugin_api::command_ids::{DELETE_PAGE, ROTATE_PAGE_CLOCKWISE};

        let mut model = model_with_registry(crate::build_registry());
        model.update().expect("the first frame runs");
        assert_eq!(model.viewport().page_count(), 2);
        let mode = model.viewport().mode();

        model.run_command(DELETE_PAGE).expect("deletes");
        assert_eq!(model.viewport().page_count(), 1, "the layout lost the page");
        assert_eq!(model.viewport().mode(), mode, "and kept the view's mode");
        assert!(
            model.tiles.is_empty(),
            "no raster of the old page 0 survives"
        );
        model
            .update()
            .expect("the next frame runs over the new layout");

        let before = model
            .viewport()
            .page_geometry(0)
            .expect("measured")
            .render_size;
        model.run_command(ROTATE_PAGE_CLOCKWISE).expect("rotates");
        let after = model
            .viewport()
            .page_geometry(0)
            .expect("measured")
            .render_size;
        assert_eq!(
            (after.0, after.1),
            (before.1, before.0),
            "a quarter turn swaps the page's width and height in the layout"
        );
    }

    /// A command that changes nothing leaves the layout alone.
    #[cfg(feature = "tools-organize")]
    #[test]
    fn a_page_command_that_changes_nothing_keeps_the_layout() {
        use onionskin_plugin_api::command_ids::MOVE_PAGE_EARLIER;

        let mut model = model_with_registry(crate::build_registry());
        model.update().expect("the first frame runs");
        model.go_to_page(1).expect("navigates");
        model.go_to_page(0).expect("navigates");
        assert!(
            model.view_history.can_previous(),
            "there is history to lose"
        );
        model
            .run_command(MOVE_PAGE_EARLIER)
            .expect("a no-op on page 1");
        assert!(
            model.view_history.can_previous(),
            "no relayout, so the view history survives"
        );
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

    /// P1b's open-time notice: an encrypted document says it is read-only,
    /// and why, from the first frame - before the user starts work they could
    /// not keep - and a plain one says nothing.
    #[test]
    fn an_encrypted_document_opens_with_a_notice_and_a_plain_one_without() {
        let encrypted = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/encrypted/r6-aes-256-print-only.pdf");
        let mut model = CanvasModel::new(
            Document::open_path(&encrypted).expect("an encrypted document opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts");
        model.update().expect("the first frame runs");
        let Some(CanvasStatus::Notice { message }) = model.status() else {
            panic!("no open-time notice: {:?}", model.status());
        };
        assert!(message.contains("does not allow changes"), "{message}");
        assert!(message.contains("permissions password"), "{message}");
        assert_eq!(
            model.edit_refusal(),
            Some(
                onionskin_core::protection::Refusal::Restricted(
                    onionskin_core::protection::EditKind::Content
                )
                .reason()
            )
        );

        let plain = model_with_registry(PluginRegistry::new());
        assert!(plain.status().is_none());
        assert_eq!(plain.edit_refusal(), None);
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

    /// Found by hand: nothing sent Enter to the tool, so a polygon built by
    /// clicking could never be finished. Enter now commits it, and Escape
    /// abandons one part built.
    #[cfg(feature = "tools-comment")]
    #[test]
    fn enter_finishes_a_polygon_and_escape_abandons_one() {
        let path = onionskin_corpus_testing::seed("hello.pdf");
        let mut model = CanvasModel::new(
            Document::open_path(&path).expect("seed opens"),
            crate::build_registry(),
            VIEWPORT,
        )
        .expect("canvas starts");
        let polygon = model
            .registry()
            .tools()
            .position(|tool| tool.id() == "polygon")
            .expect("the polygon tool is installed");
        model.activate_tool(polygon).expect("activates");
        let (_, hint) = model.active_tool_help().expect("a tool is active");
        assert!(hint.expect("a hint").contains("Double-click"));

        let centre = page_center(&model);
        let click = |model: &mut CanvasModel, dx: f32, dy: f32| {
            let at = point(centre.x + px(dx), centre.y + px(dy));
            model
                .pointer_down(at, 1.0, GpuiModifiers::default())
                .unwrap();
            model.pointer_up(at, 1.0, GpuiModifiers::default()).unwrap();
        };
        click(&mut model, 0.0, 0.0);
        click(&mut model, 60.0, 0.0);
        assert!(model.tool_has_pending_gesture());
        assert!(model.cancel_tool_gesture());
        assert!(!model.tool_has_pending_gesture(), "Escape abandoned it");
        assert!(!model.document.borrow_mut().is_dirty());

        click(&mut model, 0.0, 0.0);
        click(&mut model, 60.0, 0.0);
        click(&mut model, 30.0, 50.0);
        assert!(
            !model.document.borrow_mut().is_dirty(),
            "nothing is written until Enter"
        );
        assert!(model.commit_active_tool());
        assert!(
            model.document.borrow_mut().is_dirty(),
            "Enter wrote the polygon"
        );
    }

    /// Every tool on the rail says how it is used: the rail draws a glyph,
    /// and the side panel's sentence is the only place that says what a
    /// click or a drag will do. A new tool without one fails here.
    #[test]
    fn every_rail_tool_carries_a_hint() {
        let registry = crate::build_registry();
        let missing: Vec<_> = registry
            .tools()
            .filter(|tool| tool.in_rail() && tool.hint().is_none())
            .map(|tool| tool.name())
            .collect();
        assert_eq!(missing, Vec::<&str>::new());
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

    fn optional_content_model() -> CanvasModel {
        CanvasModel::new(
            Document::open_bytes(crate::shell::fixtures::optional_content_pdf())
                .expect("the fixture opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts")
    }

    /// The canvas half of the P4 review's layer note. The store is keyed by
    /// page and zoom, not by the render options, so nothing in it would be
    /// rebuilt on its own; and it is also what the next placeholder is scaled
    /// from, so a raster left in it would put the old layer state back on
    /// screen the moment the page was scrolled.
    #[test]
    fn toggling_a_layer_drops_every_cached_pixel_and_asks_for_the_pages_again() {
        let mut model = optional_content_model();
        let request = prepare_request(&mut model);
        assert!(model.apply_render_response(RenderResponse::Raster {
            request,
            render: PageRender {
                raster: raster(&model, request.page, request.zoom, [10, 10, 10, 255]),
                warnings: Vec::new(),
            },
        }));
        assert_eq!(model.tiles.len(), 1, "there is a cached raster to lose");
        assert!(model.tiles.base(0).is_some());
        let generation = model.generation;
        let layer = model.layers().expect("the layers read")[0].clone();

        assert!(model
            .set_layer_visible(layer.id, false)
            .expect("an unlocked layer toggles"));

        assert_eq!(model.tiles.len(), 0, "the cached composites are stale");
        assert!(
            model.tiles.base(0).is_none(),
            "a placeholder scaled from the old raster would show the old layers again"
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

    /// Found by hand on macOS: a rectangle drawn with the Rectangle tool was
    /// in the document and not on the screen until the zoom changed, because
    /// the page's cached raster was keyed by page and zoom only. Any edit
    /// now drops the pixels on the next frame and asks for the page again.
    #[test]
    fn an_edit_drops_the_cached_pixels_and_asks_for_the_page_again() {
        let mut model = optional_content_model();
        let request = prepare_request(&mut model);
        assert!(model.apply_render_response(RenderResponse::Raster {
            request,
            render: PageRender {
                raster: raster(&model, request.page, request.zoom, [10, 10, 10, 255]),
                warnings: Vec::new(),
            },
        }));
        model.drop_pixels_older_than_the_document();
        assert!(
            model.tiles.base(0).is_some(),
            "no edit, so the raster stays"
        );

        model
            .document_mut()
            .edit_document("Add Bookmark", |tx| {
                onionskin_core::add_bookmark(tx, &[], None, "edited", None)
            })
            .expect("edits");
        model.drop_pixels_older_than_the_document();

        assert!(model.tiles.base(0).is_none(), "the old raster is gone");
        let visible = model.viewport.visible_pages().unwrap();
        assert!(model.update_signature(&visible).unwrap());
        model.tiles.begin_frame();
        assert_eq!(
            model.schedule_visible_renders(&visible).unwrap(),
            1,
            "and the page is asked for again"
        );
    }

    fn thumbnail_response(model: &CanvasModel, page: PageIndex, zoom: f32) -> ThumbnailResponse {
        ThumbnailResponse::Ready {
            request: *model
                .pending_thumbnails
                .get(&page)
                .expect("the page has a thumbnail outstanding"),
            render: PageRender {
                raster: raster(model, page, zoom, [40, 40, 40, 255]),
                warnings: Vec::new(),
            },
        }
    }

    /// A thumbnail rendered before a layer toggle must not become the
    /// picture after it.
    ///
    /// The sequence that leaked: the pane drops its pictures on the toggle,
    /// the raster already in flight arrives, and because a picture now
    /// exists the page is never asked for again. The row would show the old
    /// layers for as long as the document stayed open.
    #[test]
    fn a_thumbnail_rendered_before_a_layer_toggle_is_dropped_when_it_arrives() {
        let mut model = optional_content_model();
        model
            .request_thumbnail(0, 0.18)
            .expect("the thumbnail is queued");
        let in_flight = thumbnail_response(&model, 0, 0.18);
        let layer = model.layers().expect("the layers read")[0].clone();

        assert!(model
            .set_layer_visible(layer.id, false)
            .expect("an unlocked layer toggles"));

        assert!(
            !model.accept_thumbnail(in_flight),
            "the picture predates the toggle and has to be dropped"
        );
        assert!(
            model.take_thumbnails().is_empty(),
            "nothing stale reaches the pane"
        );
        assert!(
            !model.thumbnail_pending(0),
            "the toggle dropped what was outstanding, so the pane asks again"
        );

        // Asking again under the new options is a fresh request, and its
        // answer is the one that is kept.
        model
            .request_thumbnail(0, 0.18)
            .expect("the thumbnail is queued again");
        let after = thumbnail_response(&model, 0, 0.18);
        assert!(model.accept_thumbnail(after));
        assert_eq!(model.take_thumbnails().len(), 1);
    }

    /// The same mechanism at a different size: Reduce and Enlarge Page
    /// Thumbnails change the zoom every row is rendered at, and the answer to
    /// the size before must not be kept as the picture of the size now.
    #[test]
    fn a_thumbnail_rendered_at_the_previous_size_is_dropped_when_it_arrives() {
        let mut model = optional_content_model();
        model
            .request_thumbnail(0, 0.18)
            .expect("the thumbnail is queued");
        let smaller = thumbnail_response(&model, 0, 0.18);

        model
            .request_thumbnail(0, 0.36)
            .expect("the larger thumbnail is queued");

        assert!(
            !model.accept_thumbnail(smaller),
            "the picture is of the size the pane no longer shows"
        );
        assert!(
            model.thumbnail_pending(0),
            "the request at the new size is still outstanding"
        );
        let larger = thumbnail_response(&model, 0, 0.36);
        assert!(model.accept_thumbnail(larger));
        assert_eq!(model.take_thumbnails().len(), 1);
    }

    /// Asking for a picture already outstanding costs nothing, which is what
    /// lets the pane ask on every frame without queueing a render each time.
    #[test]
    fn asking_again_for_the_thumbnail_already_outstanding_queues_nothing() {
        let mut model = optional_content_model();
        model
            .request_thumbnail(0, 0.18)
            .expect("the thumbnail is queued");
        let outstanding = *model
            .pending_thumbnails
            .get(&0)
            .expect("the page has one outstanding");

        model
            .request_thumbnail(0, 0.18)
            .expect("asking again is not an error");

        assert_eq!(model.pending_thumbnails.get(&0), Some(&outstanding));
    }

    /// A toggle that changes nothing costs nothing: the pixels stay, because
    /// re-rendering them would produce the same picture.
    #[test]
    fn a_toggle_to_the_state_a_layer_is_in_keeps_the_cached_pixels() {
        let mut model = optional_content_model();
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

    /// Every shape the plugin API has, painted, with its view-space geometry
    /// checked against the viewport's own mapping.
    ///
    /// Exhaustive by the compiler rather than by this test remembering the
    /// list: `map_overlay` has no catch-all arm and no error arm, so a variant
    /// added to `Overlay` without a painter does not build. What this adds is
    /// that each painter places its shape where the viewport says, and that
    /// **no overlay produces a status** - four of the six used to report "the
    /// canvas cannot draw a {kind} overlay yet", which is what a user saw
    /// instead of their own ink.
    #[test]
    fn every_overlay_shape_paints_where_the_viewport_puts_it() {
        let at = |x: f64, y: f64| PagePoint { page: 0, x, y };
        let region = PageRect {
            page: 0,
            x0: 72.0,
            y0: 700.0,
            x1: 144.0,
            y1: 720.0,
        };

        for overlay in [
            Overlay::Quads(vec![selection_quad()]),
            Overlay::AntsRect(region),
            Overlay::Rect(region),
            Overlay::Polyline {
                points: vec![at(72.0, 700.0), at(100.0, 720.0), at(144.0, 700.0)],
                closed: false,
            },
            Overlay::Polyline {
                points: vec![at(72.0, 700.0), at(100.0, 720.0), at(144.0, 700.0)],
                closed: true,
            },
            Overlay::Line {
                from: at(72.0, 700.0),
                to: at(144.0, 720.0),
            },
            Overlay::Ellipse { bounds: region },
        ] {
            let mut model = overlay_model(vec![overlay.clone()]);
            let painted = model.paint_list().expect("the frame paints").overlays;
            let mapped = |x: f64, y: f64| {
                model
                    .viewport
                    .view_point_for(at(x, y))
                    .expect("page zero is measured")
                    .expect("page zero is laid out")
            };
            let corners = || bounding_rect(model.map_quad(region.into()).expect("region maps"));

            match (&overlay, painted.as_slice()) {
                (Overlay::Quads(_), [OverlayPaint::Quads(quads)]) => assert_eq!(quads.len(), 1),
                (Overlay::AntsRect(_), [OverlayPaint::AntsRect(rect)]) => {
                    assert_eq!(*rect, corners())
                }
                (Overlay::Rect(_), [OverlayPaint::Rect(rect)]) => assert_eq!(*rect, corners()),
                (
                    Overlay::Polyline { closed, .. },
                    [OverlayPaint::Polyline { points, closed: c }],
                ) => {
                    assert_eq!(c, closed, "the closing edge survives the mapping");
                    assert_eq!(
                        points.as_slice(),
                        [
                            mapped(72.0, 700.0),
                            mapped(100.0, 720.0),
                            mapped(144.0, 700.0)
                        ]
                    );
                }
                (Overlay::Line { .. }, [OverlayPaint::Line { from, to }]) => {
                    assert_eq!((*from, *to), (mapped(72.0, 700.0), mapped(144.0, 720.0)));
                }
                (Overlay::Ellipse { .. }, [OverlayPaint::Ellipse(rect)]) => {
                    assert_eq!(*rect, corners())
                }
                (overlay, painted) => {
                    panic!("{overlay:?} painted as {painted:?}")
                }
            }
            assert!(
                model.status().is_none(),
                "{overlay:?} put a status on screen: {:?}",
                model.status()
            );
        }
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
    /// a raster in the store for the snapshot path to crop.
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

    fn take_snapshot_png(model: &mut CanvasModel) -> Result<Option<Vec<u8>>, CanvasError> {
        model
            .take_snapshot_pixels()?
            .map(encode_snapshot_png)
            .transpose()
    }

    #[test]
    fn a_snapshot_request_crops_the_raster_the_canvas_is_painting() {
        let mut model = painted_model([128, 0, 0, 128]);
        model
            .document
            .borrow_mut()
            .request_snapshot(snapshot_region());

        let png = take_snapshot_png(&mut model)
            .expect("the snapshot is produced")
            .expect("a request was pending");

        let source = model.tiles.base(0).expect("page zero is rendered").clone();
        let geometry = model.viewport.page_geometry(0).unwrap().clone();
        let expected = raster_crop(&geometry, snapshot_region(), &source).unwrap();
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
        model
            .document
            .borrow_mut()
            .request_snapshot(snapshot_region());
        let upright = decode(&take_snapshot_png(&mut model).unwrap().unwrap()).dimensions();

        model.set_rotation(ViewRotation::Clockwise90).unwrap();
        model.update().expect("the rotated frame runs");
        model
            .document
            .borrow_mut()
            .request_snapshot(snapshot_region());
        let turned = decode(&take_snapshot_png(&mut model).unwrap().unwrap()).dimensions();

        assert_eq!(turned, (upright.1, upright.0));
        assert!(upright.0 > upright.1);
    }

    #[test]
    fn a_snapshot_of_a_page_that_is_not_on_screen_fails_loudly() {
        let mut model = painted_model([255, 255, 255, 255]);
        model.document.borrow_mut().request_snapshot(PageRect {
            page: 1,
            ..snapshot_region()
        });

        assert!(matches!(
            model.take_snapshot_pixels(),
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
            model
                .document
                .borrow_mut()
                .request_snapshot(snapshot_region());
            take_snapshot_png(&mut model).unwrap().unwrap()
        })
        .dimensions();

        // The same rectangle slid left so its left half hangs off the page.
        model.document.borrow_mut().request_snapshot(PageRect {
            x0: -50.0,
            x1: 50.0,
            ..snapshot_region()
        });
        let clipped = decode(&take_snapshot_png(&mut model).unwrap().unwrap()).dimensions();

        assert_eq!(clipped.1, inside.1, "the vertical span is untouched");
        assert!(clipped.0 < inside.0, "the overhanging half is dropped");
        assert!(clipped.0 > 0);
    }

    /// A region entirely off the page would crop nothing, and an empty PNG
    /// on the clipboard is worse than a message saying why there is none.
    #[test]
    fn a_snapshot_region_off_the_page_fails_loudly() {
        let mut model = painted_model([255, 255, 255, 255]);
        model.document.borrow_mut().request_snapshot(PageRect {
            page: 0,
            x0: -400.0,
            y0: 20.0,
            x1: -300.0,
            y1: 60.0,
        });

        assert!(matches!(
            model.take_snapshot_pixels(),
            Err(CanvasError::SnapshotEmpty { page: 0 })
        ));
    }

    #[test]
    fn a_frame_with_no_pending_request_produces_no_snapshot() {
        let mut model = painted_model([255, 255, 255, 255]);

        assert!(model.take_snapshot_pixels().unwrap().is_none());
    }

    #[test]
    fn an_oversized_snapshot_is_refused_before_pixel_allocation() {
        assert!(matches!(
            snapshot_buffer_len(3841, 2160),
            Err(CanvasError::SnapshotTooLarge {
                width: 3841,
                height: 2160,
                limit: SNAPSHOT_RGBA_BYTE_LIMIT,
            })
        ));
    }

    #[test]
    fn snapshot_encoder_write_errors_are_reported() {
        struct FailingWriter;

        impl std::io::Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("sink closed"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl std::io::Seek for FailingWriter {
            fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
                Ok(0)
            }
        }

        let mut writer = FailingWriter;
        let result = write_snapshot_png(
            SnapshotPixels {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            },
            &mut writer,
        );

        assert!(matches!(
            result,
            Err(CanvasError::SnapshotEncode(message)) if message.contains("sink closed")
        ));
    }

    // Names a tools-basic type, so it only exists when that plugin is compiled
    // in. Without the guard, --no-default-features --features shell builds the
    // app but fails to build its tests.
    #[cfg(feature = "tools-basic")]
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

        assert!(model.document.borrow_mut().selection().region().is_some());
        assert!(model.take_snapshot_pixels().unwrap().is_some());
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
                ViewPoint {
                    x: f32::from(positions[index].x) - origin.x,
                    y: f32::from(positions[index].y) - origin.y,
                },
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
        let generation = model.generation;

        assert!(model.zoom_to(2.0).unwrap());
        model.update().unwrap();

        assert!(model.generation > generation);
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

    #[test]
    fn resetting_worker_wait_restarts_the_deadline_for_later_page_work() {
        let mut model = model();
        let start = Instant::now();
        model.geometry_requests.insert(1);
        assert!(model.poll_again(start));

        model.reset_worker_wait();
        model.geometry_requests.clear();
        model.geometry_requests.insert(0);

        let restarted = start + PENDING_WORK_TIMEOUT;
        assert!(model.poll_again(restarted));
        assert!(model.status().is_none());

        assert!(!model.poll_again(restarted + PENDING_WORK_TIMEOUT));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { page: None, message })
                if message.contains("[0]") && !message.contains("[1]")
        ));
    }

    /// The paint used to read the raster's size from a collection beside the
    /// store while cutting its tiles from the store's own cache. The store's
    /// `cols` and `rows` come from its base raster, so a disagreement made
    /// `raster_width - col * TILE_SIZE` underflow and took the window down.
    /// One lookup makes the disagreement unconstructible; what is left to pin
    /// is that the tile count follows the raster the store holds.
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

        let paint = model.paint_list().expect("the page paints");

        let tiles: Vec<_> = paint.tiles.iter().filter(|tile| tile.page == 0).collect();
        assert_eq!(tiles.len(), 2, "the store holds a raster two tiles wide");
        assert!(tiles.iter().all(|tile| tile.source_zoom == source_zoom));
    }

    /// The store is the only raster owner now, so the frame that is about to
    /// paint has to claim what it will paint from. A zoom change misses the
    /// exact-zoom cache, so nothing else does: the frame would close having
    /// pinned nothing, eviction would run with nothing exempt, and the page
    /// on screen would lose the raster it was about to be scaled from.
    ///
    /// The budget has to be small enough for eviction to actually run. At the
    /// default 202 MiB nothing is ever evicted and this passes vacuously.
    #[test]
    fn a_zoom_change_keeps_the_raster_the_paint_will_scale() {
        let mut model = model();
        model.viewport.set_mode(PageLayoutMode::SinglePage).unwrap();
        model.first_page().unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        assert_eq!(
            visible.iter().map(|page| page.page).collect::<Vec<_>>(),
            [0],
            "page zero alone is on screen"
        );

        let raster = || BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]);
        // Room for one raster, so whatever the frame does not claim goes.
        model.tiles = TileStore::with_budget(raster().rgba().len());
        // Staged inside a frame so the setup does not evict one of them:
        // page zero is on screen, page one is a page scrolled away.
        model.tiles.begin_frame();
        model.tiles.insert(0, raster());
        model.tiles.insert(1, raster());
        assert_eq!(model.tiles.len(), 2, "both rasters are resident to start");

        model
            .viewport
            .zoom_to(3.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        model.update().expect("the zoom-change frame runs");

        assert!(
            model.tiles.base(0).is_some(),
            "the frame evicted the raster the page on screen paints from"
        );
        let paint = model.paint_list().expect("the frame paints");
        let tiles: Vec<_> = paint.tiles.iter().filter(|tile| tile.page == 0).collect();
        assert!(
            !tiles.is_empty(),
            "the page on screen painted no tiles at the new zoom"
        );
        assert!(
            tiles.iter().all(|tile| tile.source_zoom == 1.0),
            "the resident 1x raster is what the 3x view scales"
        );
    }

    /// The third of the three early exits. A page whose render has failed is
    /// never asked for again under this signature, but it goes on painting
    /// whatever raster it has, so it has to keep claiming it.
    ///
    /// Driven through `schedule_visible_renders` rather than `update`,
    /// because `update_signature` clears `failed_renders` whenever the
    /// signature moves: the exit is only reachable while the view holds
    /// still, which is exactly when a render fails under it.
    #[test]
    fn a_page_whose_render_failed_still_claims_the_raster_it_paints() {
        let mut model = model();
        model.viewport.set_mode(PageLayoutMode::SinglePage).unwrap();
        model.first_page().unwrap();
        let raster = || BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]);
        model.tiles = TileStore::with_budget(raster().rgba().len());
        model.tiles.begin_frame();
        model.tiles.insert(0, raster());
        model.tiles.insert(1, raster());
        model.tiles.end_frame();

        model
            .viewport
            .zoom_to(3.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.failed_renders.insert(0);

        model.tiles.begin_frame();
        assert_eq!(
            model.schedule_visible_renders(&visible).unwrap(),
            0,
            "a page whose render failed is not asked for again"
        );
        model.tiles.end_frame();

        assert!(
            model.tiles.base(0).is_some(),
            "a failed page lost the raster it was still painting"
        );
    }

    /// The claim buys a page the frame it is on screen for and the grace
    /// window after it, and nothing more. A page scrolled away has to become
    /// evictable again, or claiming would be a permanent pin and the store
    /// would fill with every page ever looked at.
    ///
    /// The store is a byte budget, not a visible-set policy, so leaving the
    /// visible set is not on its own a reason to drop a page: this asserts
    /// that the page goes when the memory is actually wanted. That is the
    /// distinction HARD-CAN-004 traded a second collection for.
    #[test]
    fn a_claim_expires_once_the_page_is_no_longer_on_screen() {
        let mut model = model();
        model.viewport.set_mode(PageLayoutMode::SinglePage).unwrap();
        let second = model
            .document
            .borrow_mut()
            .page_geometry(1)
            .unwrap()
            .clone();
        model.viewport.measure_page(second).unwrap();
        model.first_page().unwrap();
        let raster = || BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]);
        model.tiles = TileStore::with_budget(raster().rgba().len());
        model.tiles.begin_frame();
        model.tiles.insert(0, raster());
        model.tiles.insert(1, raster());
        model.tiles.end_frame();

        model.update().expect("the frame showing page zero runs");
        assert!(
            model.tiles.base(0).is_some(),
            "page zero was on screen and should have been claimed"
        );

        // Page zero leaves, and two frames pass, which spends its grace
        // window. It is still resident because the store fits its budget.
        model.go_to_page(1).unwrap();
        model.update().expect("the first frame away runs");
        model.update().expect("the second frame away runs");

        // Now the memory is wanted. An unclaimed page goes first.
        model.tiles.insert(9, raster());
        assert!(
            model.tiles.base(0).is_none(),
            "a page nothing is showing survived the pressure, so the claim never expires"
        );
        assert!(
            model.tiles.base(9).is_some(),
            "the raster that arrived should be the one kept"
        );
    }

    /// The claim has to be made for every visible page, not only the ones the
    /// frame goes on to ask for. A page whose render is already outstanding
    /// takes an early exit on the next frame, and a claim made after that
    /// exit would be skipped exactly when the page is still being scaled.
    #[test]
    fn a_page_already_awaiting_its_render_still_claims_its_raster() {
        let mut model = model();
        model.viewport.set_mode(PageLayoutMode::SinglePage).unwrap();
        model.first_page().unwrap();
        let raster = || BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]);
        model.tiles = TileStore::with_budget(raster().rgba().len());
        model.tiles.begin_frame();
        model.tiles.insert(0, raster());
        model.tiles.insert(1, raster());

        model
            .viewport
            .zoom_to(3.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();

        model.tiles.begin_frame();
        assert_eq!(
            model.schedule_visible_renders(&visible).unwrap(),
            1,
            "the new zoom is asked for"
        );
        model.tiles.end_frame();

        // A raster for some other page lands between the frames, so the store
        // has something newer than the one page zero is still scaling.
        model.tiles.insert(2, raster());

        // The next frame: same zoom and same generation, so the request
        // stands and page zero takes the early exit.
        model.tiles.begin_frame();
        assert_eq!(
            model.schedule_visible_renders(&visible).unwrap(),
            0,
            "the outstanding request is not made twice"
        );
        model.tiles.end_frame();

        assert!(
            model.tiles.base(0).is_some(),
            "the frame closed over the raster page zero is still being scaled from"
        );
    }

    /// Decision 11 asks a page that comes back into view to be shown scaled
    /// from whatever raster is still resident, rather than blank until the
    /// re-render lands. A collection beside the store used to drop the page's
    /// raster the moment it left the visible set, so the return trip found
    /// nothing to scale even though the store still held the raster.
    #[test]
    fn a_page_returning_to_view_at_a_new_zoom_paints_the_resident_raster() {
        let mut model = model();
        let anchor = ViewPoint { x: 400.0, y: 300.0 };
        // One page on screen at a time, so leaving page zero really leaves it.
        model.viewport.set_mode(PageLayoutMode::SinglePage).unwrap();
        let second = model
            .document
            .borrow_mut()
            .page_geometry(1)
            .unwrap()
            .clone();
        model.viewport.measure_page(second).unwrap();
        model.viewport.zoom_to(1.0, anchor).unwrap();
        model.first_page().unwrap();
        assert_eq!(
            model.viewport.visible_pages().unwrap().len(),
            1,
            "single-page layout shows one page"
        );
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, [40, 40, 40, 255]);
        assert!(model.apply_render_response(raster_response(request, rendered)));

        // Page zero leaves the visible set, and the zoom moves while it is
        // away, so it comes back needing a raster it was never rendered at.
        model.go_to_page(1).unwrap();
        let away = model.viewport.visible_pages().unwrap();
        assert!(away.iter().all(|placement| placement.page != 0));
        model.update().expect("the frame away from page zero runs");
        model.viewport.zoom_to(3.0, anchor).unwrap();
        model.go_to_page(0).unwrap();
        model.update().expect("the returning frame runs");

        let paint = model.paint_list().expect("the returning frame paints");
        let tiles: Vec<_> = paint.tiles.iter().filter(|tile| tile.page == 0).collect();
        assert!(
            !tiles.is_empty(),
            "page zero came back to a resident raster and still painted nothing"
        );
        assert!(
            tiles.iter().all(|tile| tile.source_zoom == 1.0),
            "the resident raster is the 1x one, scaled to the 3x view"
        );
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
        assert!(model.tiles.is_empty());
    }

    #[test]
    fn polling_refreshes_the_signature_before_accepting_a_response() {
        let mut model = model();
        let stale = prepare_request(&mut model);
        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();

        model.update().expect("the frame runs");
        assert!(
            model.tiles.is_empty(),
            "the raster answering the pre-zoom request was accepted"
        );
        assert_ne!(
            model.requests.get(&stale.page),
            Some(&stale),
            "the pre-zoom request is still the one outstanding"
        );
        assert_ne!(model.generation, stale.generation);
    }

    /// Every pointer move and every poll tick runs an update that no paint
    /// follows. The store exempts a frame's pages from eviction, so an update
    /// that left its frame open would keep exempting whatever arrived next:
    /// the store's own note calls that a document delivering pages faster
    /// than it repaints filling memory with caches eviction may not take.
    #[test]
    fn an_update_that_paints_nothing_leaves_no_frame_open() {
        let mut model = model();
        model.tiles = TileStore::with_budget(1);
        model.update().expect("the frame runs");

        // Two pages nothing is showing, so only an open frame could exempt
        // them. The store never evicts the entry a caller just asked for, so
        // the second survives either way and the first is the witness.
        model
            .tiles
            .insert(7, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));
        model
            .tiles
            .insert(8, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));

        assert!(
            model.tiles.base(7).is_none(),
            "the update's frame is still open, so its pages cannot be evicted"
        );
        assert_eq!(model.tiles.len(), 1);
    }

    /// The paint opens a store frame, so it has to close one. A frame left
    /// open goes on exempting whatever the store is handed next, which is the
    /// unbounded case `TileStore::begin_frame` documents.
    #[test]
    fn the_paint_closes_the_frame_it_opened() {
        let mut model = model();
        model.tiles = TileStore::with_budget(1);
        model.paint_list().expect("the frame paints");

        // Two pages nothing is showing, so only an open frame could exempt
        // them. The store never evicts the entry just handed to a caller, so
        // the second survives either way and the first is the witness.
        model
            .tiles
            .insert(7, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));
        model
            .tiles
            .insert(8, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));

        assert!(
            model.tiles.base(7).is_none(),
            "the paint's frame is still open, so its pages cannot be evicted"
        );
        assert_eq!(model.tiles.len(), 1);
    }

    /// `update` pins the cache each visible page has at the exact zoom. A
    /// page painting from a scaled raster instead, because its exact-zoom
    /// render has not landed, is pinned by nothing that update did, so the
    /// paint has to declare its own frame: compositing the page's tiles is
    /// what puts the store over its budget, and the next thing to ask it for
    /// anything would otherwise take the cache the frame just painted from.
    #[test]
    fn the_paint_declares_the_pages_it_painted() {
        let mut model = model();
        let source_zoom = 0.5_f32;
        assert_ne!(model.viewport.zoom().to_bits(), source_zoom.to_bits());
        let raster = || BaseRaster::new(1, 1, source_zoom, vec![255, 255, 255, 255]);
        // Room for the base raster and nothing more, so the tile the paint
        // composites is what carries the store over.
        model.tiles = TileStore::with_budget(raster().rgba().len());
        model.tiles.insert(0, raster());
        assert_eq!(
            model.tiles.over_budget(),
            0,
            "the setup starts under budget"
        );

        let paint = model.paint_list().expect("the frame paints");

        assert!(
            !paint.tiles.is_empty(),
            "page zero painted from the scaled raster"
        );
        model.tiles.insert(9, raster());
        assert!(
            model.tiles.base(0).is_some(),
            "the raster the frame painted from was evicted by the next insert"
        );
    }

    #[test]
    fn a_placeholder_keeps_polling_armed_until_a_terminal_response() {
        let mut model = model();
        let request = prepare_request(&mut model);
        let placeholder = model
            .document
            .borrow_mut()
            .try_render_response()
            .unwrap()
            .expect("placeholder is immediate");
        assert!(matches!(placeholder, RenderResponse::Placeholder(_)));
        assert!(model.apply_render_response(placeholder));
        assert!(model.has_pending_render());

        assert!(model.apply_render_response(RenderResponse::Failed {
            request,
            error: RenderError::UnrenderableSize {
                width: 100_000.0,
                height: 100_000.0,
            },
        }));
        assert!(!model.has_pending_render());
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
            .borrow_mut()
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
        assert_eq!(model.tiles.base(0).map(BaseRaster::zoom), Some(1.0));

        model.viewport.zoom_to(3.0, anchor).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.tiles.begin_frame();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let placeholder = model
            .document
            .borrow_mut()
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
        let second = model
            .document
            .borrow_mut()
            .page_geometry(1)
            .unwrap()
            .clone();
        model.viewport.measure_page(second).unwrap();
        model.viewport.set_mode(PageLayoutMode::TwoPage).unwrap();
        model.viewport.fit(FitMode::Page).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        assert_eq!(visible.len(), 2);
        model.tiles = TileStore::with_budget(1);
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

    fn assert_quad_bounds(quads: &[PageQuad], expected: &[(f64, f64, f64, f64)]) {
        assert_eq!(quads.len(), expected.len());
        for (quad, &(x0, x1, y0, y1)) in quads.iter().zip(expected.iter()) {
            let (actual_x0, actual_x1) = quad
                .corners
                .iter()
                .map(|(x, _)| *x)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), x| {
                    (min.min(x), max.max(x))
                });
            let (actual_y0, actual_y1) = quad
                .corners
                .iter()
                .map(|(_, y)| *y)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), y| {
                    (min.min(y), max.max(y))
                });
            for (actual, expected) in [
                (actual_x0, x0),
                (actual_x1, x1),
                (actual_y0, y0),
                (actual_y1, y1),
            ] {
                assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
            }
        }
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

    /// The pixels are turned on the way into the image, so the view rotation
    /// is part of what identifies one. Keying without it would hand a rotated
    /// frame the image built for the upright one, because the tile behind it
    /// is the same `Arc` and nothing else about the key has changed.
    #[test]
    fn the_same_tile_under_two_rotations_gets_two_images() {
        let mut store = TileStore::new();
        let mut images = TileImageCache::default();
        let tile = store
            .insert(
                0,
                BaseRaster::new(2, 1, 1.0, vec![255, 0, 0, 255, 0, 0, 255, 255]),
            )
            .tile(0, 0);

        let upright = images
            .image_for(
                TileImageKey::new(0, 1.0, 0, 0, ViewRotation::None),
                &tile,
                2,
                1,
                ViewRotation::None,
            )
            .unwrap();
        let turned = images
            .image_for(
                TileImageKey::new(0, 1.0, 0, 0, ViewRotation::Clockwise90),
                &tile,
                2,
                1,
                ViewRotation::Clockwise90,
            )
            .unwrap();

        assert_eq!(images.entries.len(), 2, "one image per rotation");
        assert_ne!(upright.id, turned.id);
        assert_ne!(
            upright.as_bytes(0).unwrap(),
            turned.as_bytes(0).unwrap(),
            "the turned image is the upright one over again"
        );
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

    #[test]
    fn edited_search_refreshes_after_local_relayout() {
        let mut model = model_from(assembled_pdf(
            &[
                "BT /F1 12 Tf 20 100 Td (alpha) Tj ET",
                "BT /F1 12 Tf 20 100 Td (beta) Tj ET",
            ],
            2,
        ));
        find(&mut model, "beta", SearchOptions::default());
        model.pending_reveal = Some((1, Vec::new()));

        model
            .document
            .borrow_mut()
            .document_mut()
            .edit_pages("Delete", |tx, structure| {
                onionskin_core::pages::delete_pages(tx, structure, &[0]).map(|_| ())
            })
            .expect("delete succeeds");
        model.relayout_after_edit().expect("relayout succeeds");
        model.update().expect("update succeeds");
        drain_search(&mut model);

        assert_eq!(model.search().current().map(|hit| hit.page), Some(0));
        assert!(model.pending_reveal.is_none());
        assert_eq!(model.viewport.current_page(), 0);

        model.cancel_search();
        model
            .document
            .borrow_mut()
            .document_mut()
            .edit_document("Touch", |tx| {
                tx.set_trailer(
                    onionskin_cos::Name::new("SearchTest"),
                    Some(onionskin_cos::Object::Name(onionskin_cos::Name::new(
                        "touch",
                    ))),
                )
            })
            .expect("touch succeeds");
        model
            .document
            .borrow_mut()
            .document_mut()
            .undo()
            .expect("undo succeeds");
        model.update().expect("closed search update succeeds");
        assert_eq!(model.search().needle(), "");
        assert_eq!(model.search().len(), 0);
        assert!(!model.search().is_running());
    }

    #[test]
    fn edited_search_other_window_clears_pending_reveal_without_restarting_results() {
        let mut a = model_from(assembled_pdf(
            &[
                "BT /F1 12 Tf 20 100 Td (alpha) Tj ET",
                "BT /F1 12 Tf 20 100 Td (beta beta Beta) Tj ET",
            ],
            2,
        ));
        let mut b = a
            .new_window(PluginRegistry::new(), VIEWPORT)
            .expect("second window opens");
        let options = SearchOptions {
            case_sensitive: true,
            ..SearchOptions::default()
        };
        find(&mut a, "beta", options);
        assert_eq!(a.search().len(), 2);
        let old_hit = a.search().current().expect("a hit exists").clone();
        b.pending_reveal = Some((1, old_hit.quads.clone()));

        a.document
            .borrow_mut()
            .document_mut()
            .edit_pages("Delete", |tx, structure| {
                onionskin_core::pages::delete_pages(tx, structure, &[0]).map(|_| ())
            })
            .expect("delete succeeds");
        a.relayout_after_edit().expect("relayout succeeds");
        a.update().expect("first window updates");
        drain_search(&mut a);
        assert!(a.select_match(0, 1).expect("second hit selects"));
        let before = a.search().clone();

        b.update().expect("second window updates");
        assert!(b.pending_reveal.is_none());
        assert_eq!(*b.search(), before);
    }

    #[test]
    fn edited_search_rotation_preserves_user_quads_and_updates_geometry() {
        let mut model = model_from(assembled_pdf(&["BT /F1 12 Tf 20 100 Td (Page) Tj ET"], 1));
        find(&mut model, "Page", SearchOptions::default());
        let original = model.search().matches_on(0)[0].quads.clone();

        model
            .document
            .borrow_mut()
            .document_mut()
            .edit_pages("Rotate", |tx, _structure| {
                onionskin_core::pages::rotate_pages(tx, &[0], 1)
            })
            .expect("rotation succeeds");
        model
            .relayout_after_edit()
            .expect("rotated relayout succeeds");
        model.update().expect("rotated update succeeds");
        drain_search(&mut model);
        settle_geometry(&mut model);

        assert_eq!(model.viewport.page_geometry(0).unwrap().rotate, 90);
        assert_eq!(model.search().matches_on(0)[0].quads, original);
        let expected = model
            .document
            .borrow_mut()
            .search_page(0, "Page", SearchOptions::default())
            .expect("per-page search succeeds");
        assert_eq!(model.search().matches_on(0), expected.as_slice());
        let hit_bounds = model
            .hit_bounds(0, &original)
            .expect("hit bounds succeed")
            .expect("the hit has bounds");
        let mapped = model.viewport.page_quad_rects(0, &original).unwrap();
        assert_eq!(
            hit_bounds,
            union_rect(&mapped).expect("mapped quads have bounds")
        );
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
            .map(|page| {
                model
                    .search()
                    .matches_on(*page)
                    .iter()
                    .map(|hit| hit.quads.len())
                    .sum::<usize>()
            })
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
        let search = model.search();
        let current = search
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
            crate::shell::find_bar::FindSummary::new(&model.search(), model.viewport.page_count());
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

        let search = model.search();
        let hit = search.current().expect("the separator is a hit");
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

    fn seed_model(name: &str) -> CanvasModel {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(name);
        let mut model = CanvasModel::new(
            Document::open_path(&path).expect("seed opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts");
        model.update().expect("the first frame runs");
        model
    }

    fn tagged_model() -> CanvasModel {
        let mut model = CanvasModel::new(
            Document::open_bytes(crate::shell::fixtures::tagged_pdf())
                .expect("the tagged page opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts");
        model.update().expect("the first frame runs");
        model
    }

    /// A tagged page is described by its structure, with the heading's level
    /// and each node placed in the view; an untagged one keeps its flat runs,
    /// and nothing is parsed for a structure while nobody is listening.
    #[test]
    fn a_tagged_page_is_described_by_its_structure_and_an_untagged_one_is_not() {
        use accesskit::Role;

        let mut model = tagged_model();
        let described = model.accessible_pages(true).expect("the pages describe");
        let nodes = described[0]
            .structure
            .as_ref()
            .expect("a tagged page has a structure")
            .as_ref()
            .expect("the structure reads");
        assert_eq!(nodes[0].role, Role::Document);
        let heading = &nodes[0].children[0];
        assert_eq!((heading.role, heading.level), (Role::Heading, Some(1)));
        assert_eq!(heading.label, "Title", "the node is named by its words");
        assert!(heading.children.is_empty());
        assert_eq!(nodes[0].children[1].role, Role::Paragraph);
        let figure = &nodes[0].children[4];
        assert_eq!((figure.role, figure.label.as_str()), (Role::Image, "A cat"));
        assert_eq!(nodes[0].language.as_deref(), Some("en"));
        let bounds = heading.bounds.expect("the title has a box");
        assert!(
            model.structure_rect(0, bounds).is_some(),
            "a measured page places its nodes in the view"
        );

        let quiet = model.accessible_pages(false).expect("the pages describe");
        assert!(quiet[0].structure.is_none(), "nobody is listening");

        let mut untagged = seed_model("hello.pdf");
        let flat = untagged.accessible_pages(true).expect("the pages describe");
        assert!(flat[0].structure.is_none());
    }

    /// The structure is kept per page, and an edit that changes what a page
    /// says has to change what the next frame describes: deleting the first
    /// page leaves the second where the first was.
    #[test]
    fn the_structure_of_a_page_follows_an_edit_to_the_document() {
        use onionskin_core::pages::delete_pages;

        let mut model = CanvasModel::new(
            Document::open_bytes(crate::shell::fixtures::tagged_pages_pdf(2)).expect("opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts");
        model.update().expect("the first frame runs");
        let heading_of = |model: &mut CanvasModel| {
            let described = model.accessible_pages(true).expect("the pages describe");
            let nodes = described[0]
                .structure
                .clone()
                .expect("tagged")
                .expect("reads");
            nodes[0].children[0].label.clone()
        };
        assert_eq!(heading_of(&mut model), "Page 1");

        model
            .document
            .borrow_mut()
            .edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
            .expect("the delete commits");
        model.update().expect("the frame after the edit runs");
        assert_eq!(heading_of(&mut model), "Page 2");
    }

    fn model_of(bytes: Vec<u8>) -> CanvasModel {
        let mut model = CanvasModel::new(
            Document::open_bytes(bytes).expect("opens"),
            PluginRegistry::new(),
            VIEWPORT,
        )
        .expect("canvas starts");
        model.update().expect("the first frame runs");
        model
    }

    /// A change to the document the canvas was not told about, an edit from
    /// another window, still replaces what the next frame says: the cache is
    /// kept for a session state, not until this canvas relays out.
    #[test]
    fn the_structure_follows_an_edit_the_canvas_did_not_make_itself() {
        use onionskin_core::pages::delete_pages;

        let mut model = model_of(crate::shell::fixtures::tagged_pages_pdf(2));
        let first = |model: &mut CanvasModel| {
            let described = model.accessible_pages(true).expect("the pages describe");
            described[0]
                .structure
                .clone()
                .expect("tagged")
                .expect("reads")[0]
                .children[0]
                .label
                .clone()
        };
        assert_eq!(first(&mut model), "Page 1");
        model
            .document
            .borrow_mut()
            .edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
            .expect("the delete commits");
        // No `update`: nothing has relaid this canvas out.
        assert_eq!(first(&mut model), "Page 2");
    }

    /// A box on a page that is not on screen is not drawn, and it is when the
    /// view gets there.
    #[test]
    fn a_structure_box_is_drawn_only_on_a_page_that_is_on_screen() {
        let mut model = model_of(crate::shell::fixtures::tagged_pages_pdf(40));
        model.set_structure_highlight(vec![(30, [10.0, 10.0, 50.0, 50.0])]);
        let drawn = |model: &mut CanvasModel| {
            model.update().expect("a frame runs");
            model.paint_list().expect("paints").highlights.len()
        };
        assert_eq!(drawn(&mut model), 0, "page 30 is far below the view");
        model.go_to_page(30).expect("goes there");
        // The page is drawn once its size is known, which the worker supplies.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut count = 0;
        while count == 0 && Instant::now() < deadline {
            count = drawn(&mut model);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(count, 1);
    }

    /// A failure is kept for the session state it happened in; the next state
    /// reads the structure again.
    #[test]
    fn a_kept_structure_failure_does_not_outlive_the_state_it_was_found_in() {
        use onionskin_core::pages::delete_pages;

        let mut model = model_of(crate::shell::fixtures::tagged_pages_pdf(2));
        model.page_structure_nodes(0).expect("reads");
        model.structure_error = Some("stale".to_owned());
        assert_eq!(model.page_structure_nodes(0), Err("stale".to_owned()));

        model
            .document
            .borrow_mut()
            .edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
            .expect("the delete commits");
        assert!(
            model
                .page_structure_nodes(0)
                .expect("reads again")
                .is_some(),
            "the failure belonged to the state before the edit"
        );
    }

    /// A structure that cannot be read does not take the words with it, and
    /// the failure is kept so the document is not parsed again every frame.
    #[test]
    fn an_unreadable_structure_is_remembered_and_leaves_the_runs_in_place() {
        let mut model = model_of(crate::shell::fixtures::unreadable_structure_pdf());
        let described = model.accessible_pages(true).expect("the pages describe");
        assert!(
            matches!(described[0].structure, Some(Err(_))),
            "{:?}",
            described[0].structure
        );
        assert!(
            !described[0]
                .text
                .as_ref()
                .expect("the text reads")
                .is_empty(),
            "the words are still there"
        );
        assert!(model.structure_error.is_some(), "the failure is kept");
    }

    /// A tagged page with no content in its structure reads as runs, as an
    /// untagged one does, rather than as an empty page.
    #[test]
    fn a_tagged_page_the_structure_marks_nothing_of_reads_as_runs() {
        let mut model = model_of(crate::shell::fixtures::tagged_without_marked_content_pdf());
        let described = model.accessible_pages(true).expect("the pages describe");
        assert!(described[0].structure.is_none());
        assert!(!described[0]
            .text
            .as_ref()
            .expect("the text reads")
            .is_empty());
    }

    /// A measured page is described at the rectangle it is painted at. The
    /// two lists can differ in length, because the paint list drops
    /// unmeasured pages and the description keeps them, so this compares the
    /// measured ones.
    #[test]
    fn a_measured_page_is_described_at_the_rectangle_it_is_painted_at() {
        let mut model = seed_model("two-page.pdf");

        let painted = model.paint_list().expect("the frame paints").pages;
        let described = model.accessible_pages(true).expect("the pages describe");

        assert!(!painted.is_empty());
        for page in &painted {
            let outline = described
                .iter()
                .find(|outline| outline.page == page.page)
                .unwrap_or_else(|| panic!("page {} is painted but not described", page.page));
            assert_eq!(outline.rect, page.rect);
        }
    }

    /// A page the layout has not measured yet has nowhere to put its words,
    /// so it is described without them. That has to be tellable apart from a
    /// page with no text on it, which is what `measured` is for.
    ///
    /// `two-page.pdf` measures page 0 to lay anything out at all and leaves
    /// page 1 for the geometry worker, so one frame in it has both states.
    #[test]
    fn an_unmeasured_page_is_described_without_words_and_says_so() {
        let mut model = seed_model("two-page.pdf");

        let described = model.accessible_pages(true).expect("the pages describe");

        let measured = described
            .iter()
            .find(|page| page.measured)
            .expect("no page was measured");
        assert!(!measured.text.as_ref().expect("no failure").is_empty());

        let waiting = described
            .iter()
            .find(|page| !page.measured)
            .expect("no page was left unmeasured");
        assert_eq!(waiting.text.as_ref().expect("no failure"), &Vec::new());
    }

    /// The page's own words, one node per run rather than one string for the
    /// page, and each with the rectangle it occupies so a screen reader can
    /// put its cursor on it.
    #[test]
    fn a_page_reports_its_text_as_runs_with_the_rectangles_they_occupy() {
        let mut model = seed_model("hello.pdf");

        let described = model.accessible_pages(true).expect("the pages describe");
        let runs = described[0].text.as_ref().expect("the text reads");

        assert!(!runs.is_empty(), "the page reported no text");
        assert!(
            runs.iter().any(|run| run.text.contains("Hello Onionskin")),
            "the page's words were not reported: {runs:?}"
        );
        let placed = runs.iter().find(|run| run.text.contains("Hello")).unwrap();
        let rect = placed.rect.expect("the run has no rectangle");
        assert!(rect.size.width > 0.0 && rect.size.height > 0.0);
        // The run sits inside the page it belongs to.
        let page = described[0].rect;
        assert!(rect.origin.x >= page.origin.x - 1.0);
        assert!(rect.origin.y >= page.origin.y - 1.0);
    }

    #[test]
    fn actual_text_accessibility_uses_all_member_geometry() {
        let mut model = model_from(assembled_pdf(
            &["BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (AB) Tj 1 0 0 1 100 50 Tm (CD) Tj EMC ET"],
            1,
        ));
        settle_geometry(&mut model);
        let page = model
            .document
            .borrow_mut()
            .page_text(0)
            .expect("page text reads")
            .clone();
        assert_eq!(page.runs.len(), 2);
        assert!(page.runs.iter().all(|run| run.glyphs.len() == 2));
        assert_eq!(
            page.runs
                .iter()
                .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
                .collect::<Vec<_>>(),
            vec![65, 66, 67, 68]
        );
        assert_eq!(page.runs[0].decoded_text, "AB");
        assert_eq!(page.runs[1].decoded_text, "CD");
        assert!(page.runs[0].actual_text.is_some());
        assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
        let source_quads: Vec<_> = page
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect();
        let expected_bounds = [
            (10.0, 16.67, 97.5, 107.5),
            (16.67, 23.34, 97.5, 107.5),
            (100.0, 107.22, 47.5, 57.5),
            (107.22, 114.44, 47.5, 57.5),
        ];
        assert_quad_bounds(&source_quads, &expected_bounds);

        let described = model.accessible_pages(true).expect("the pages describe");
        assert_eq!(described.len(), 1);
        let labels = described[0].text.as_ref().expect("the text reads");
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].text, "XY");
        let cached = model.page_words.get(&0).expect("page text is cached");
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].0, "XY");
        assert_eq!(cached[0].1, source_quads);
        let expected_rect = union_rect(
            &model
                .viewport
                .page_quad_rects(0, &cached[0].1)
                .expect("source quads map"),
        )
        .expect("source quads have bounds");
        assert_rect_close(
            labels[0].rect.expect("the label has a rectangle"),
            expected_rect,
        );
    }

    #[test]
    fn actual_text_accessibility_keeps_repeated_form_occurrences_separate() {
        let form_content =
            "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj (B) Tj EMC ET";
        let form_stream = format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Length {} >>\nstream\n{}\nendstream",
            form_content.len(),
            form_content
        );
        let page_content = "q /Fm Do Q q 1 0 0 1 0 -30 cm /Fm Do Q";
        let page_stream = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            page_content.len(),
            page_content
        );
        let mut model = model_from(crate::shell::fixtures::pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Fm 6 0 R >> >> /Contents 4 0 R >>",
            page_stream.as_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
            form_stream.as_bytes(),
        ]));
        settle_geometry(&mut model);
        let page = model
            .document
            .borrow_mut()
            .page_text(0)
            .expect("page text reads")
            .clone();
        assert_eq!(page.runs.len(), 4);
        assert!(page.runs.iter().all(|run| run.glyphs.len() == 1));
        assert_eq!(
            page.runs
                .iter()
                .map(|run| run.glyphs[0].code)
                .collect::<Vec<_>>(),
            vec![65, 66, 65, 66]
        );
        assert_eq!(
            page.runs
                .iter()
                .map(|run| run.decoded_text.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B", "A", "B"]
        );
        assert!(page.runs.iter().all(|run| run.actual_text.is_some()));
        assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
        assert_eq!(page.runs[2].actual_text, page.runs[3].actual_text);
        assert_ne!(page.runs[0].actual_text, page.runs[2].actual_text);
        assert_eq!(page.runs[0].provenance, page.runs[2].provenance);
        assert_eq!(page.runs[1].provenance, page.runs[3].provenance);
        let source_quads: Vec<_> = page
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect();
        let expected_bounds = [
            (10.0, 16.0, 97.5, 107.5),
            (16.0, 22.0, 97.5, 107.5),
            (10.0, 16.0, 67.5, 77.5),
            (16.0, 22.0, 67.5, 77.5),
        ];
        assert_quad_bounds(&source_quads, &expected_bounds);

        let described = model.accessible_pages(true).expect("the pages describe");
        assert_eq!(described.len(), 1);
        let labels = described[0].text.as_ref().expect("the text reads");
        assert_eq!(labels.len(), 2);
        assert!(labels.iter().all(|label| label.text == "XY"));
        assert_ne!(labels[0].rect, labels[1].rect);
        let cached = model.page_words.get(&0).expect("page text is cached");
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].0, "XY");
        assert_eq!(cached[1].0, "XY");
        assert_eq!(cached[0].1, source_quads[0..2].to_vec());
        assert_eq!(cached[1].1, source_quads[2..4].to_vec());
        for (label, quads) in labels.iter().zip([&cached[0].1, &cached[1].1]) {
            let expected_rect = union_rect(
                &model
                    .viewport
                    .page_quad_rects(0, quads)
                    .expect("source quads map"),
            )
            .expect("source quads have bounds");
            assert_rect_close(
                label.rect.expect("the label has a rectangle"),
                expected_rect,
            );
        }
    }

    /// A page whose text will not read reports why, rather than reading as a
    /// page with no words on it.
    #[test]
    fn a_page_whose_text_cannot_be_read_reports_the_reason() {
        let mut model = seed_model("two-page.pdf");

        let refused = model
            .page_runs(model.viewport.page_count())
            .expect_err("a page the document does not have has no text");

        assert!(!refused.is_empty());
    }

    /// A model showing the one page a fresh canvas has measured, which is
    /// the only page a raster can be built for without a render worker.
    fn model_on_a_measured_page() -> CanvasModel {
        let mut model = model();
        model.first_page().expect("the first page is reachable");
        assert!(model.viewport.page_geometry(0).is_some());
        assert_eq!(model.viewport.current_page(), 0);
        model
    }

    /// A tool that reports the capabilities it is given and records what the
    /// canvas hands it, under an id of its own choosing.
    struct CapabilityTool {
        id: &'static str,
        capabilities: &'static [onionskin_plugin_api::ToolCapability],
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ToolPlugin for CapabilityTool {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            self.id
        }

        fn icon(&self) -> &'static str {
            self.id
        }

        fn capabilities(&self) -> &'static [onionskin_plugin_api::ToolCapability] {
            self.capabilities
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {
            self.events.lock().unwrap().push("down");
        }

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {
            self.events.lock().unwrap().push("move");
        }

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {
            self.events.lock().unwrap().push("up");
        }

        fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
            self.events.lock().unwrap().push("cancel");
        }
    }

    /// The canvas keeps a pointer alive off the page for a tool that
    /// declares `ToolCapability::DynamicZoom`, and for no other tool.
    ///
    /// This is a different lookup from the one the View menu entry runs, and
    /// only the menu's was pinned. Resolving this one to `id() ==
    /// "dynamic-zoom"` instead passes every other test in the workspace,
    /// which makes it exactly the shortcut a conflict resolution reaches
    /// for; it would give a third-party tool the menu entry and not the
    /// exemption, putting the frozen-drag bug back silently. Both directions
    /// are asserted here, so neither the id nor the capability can stand in
    /// for the other.
    #[test]
    fn the_off_page_exemption_follows_the_capability_rather_than_a_tool_id() {
        use onionskin_plugin_api::ToolCapability;

        for (id, capabilities, exempt) in [
            // The capability under an id the first-party tool does not use.
            (
                "third-party-magnifier",
                &[ToolCapability::DynamicZoom][..],
                true,
            ),
            // The first-party id without the capability behind it.
            ("dynamic-zoom", &[][..], false),
        ] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let mut registry = PluginRegistry::new();
            registry.register_tool(Box::new(CapabilityTool {
                id,
                capabilities,
                events: Arc::clone(&events),
            }));
            let mut model = model_with_registry(registry);
            model.first_page().expect("the first page is reachable");
            model.activate_tool(0).expect("the tool activates");

            let page = model.viewport.visible_pages().unwrap()[0].rect;
            let off_page = point(
                px(page.origin.x + page.size.width / 2.0),
                px(page.origin.y + page.size.height + 5.0),
            );
            assert!(
                model
                    .viewport
                    .page_point_at(ViewPoint {
                        x: f32::from(off_page.x),
                        y: f32::from(off_page.y),
                    })
                    .expect("the point is mappable")
                    .is_none(),
                "{id}: the target is still on a page, so nothing is being tested"
            );

            model
                .pointer_down(page_center(&model), 1.0, GpuiModifiers::default())
                .expect("the press maps onto a page");
            model
                .pointer_move(off_page, 1.0, GpuiModifiers::default(), true)
                .expect("the move is handled");

            let seen = events.lock().unwrap().clone();
            if exempt {
                assert_eq!(
                    seen,
                    vec!["down", "move"],
                    "{id} carries the capability and should have kept the pointer"
                );
                assert_eq!(model.active_tool(), Some(0), "{id} was cancelled");
            } else {
                assert_eq!(
                    seen,
                    vec!["down", "cancel"],
                    "{id} carries no capability and should have been cancelled"
                );
            }
        }
    }

    /// Dragging down with the dynamic zoom tool shrinks the page under a
    /// pointer that is moving the other way, so the pointer leaves the page
    /// within a couple of events. That is the gesture working, not ending:
    /// the canvas used to answer an off-page pointer by cancelling the tool,
    /// which froze the zoom-out half of the drag after about 80 pixels while
    /// the button was still held.
    #[cfg(feature = "tools-basic")]
    #[test]
    fn dragging_a_dynamic_zoom_off_the_page_keeps_zooming_out() {
        let mut model = model_with_registry(crate::build_registry());
        // Every page measured, so an off-page pointer is off a page the
        // layout can still place rather than one it has never seen.
        for page in 0..model.viewport.page_count() {
            let geometry = model
                .document
                .borrow_mut()
                .page_geometry(page)
                .expect("the seed measures")
                .clone();
            model.viewport.measure_page(geometry).expect("it applies");
        }
        model.viewport.fit(FitMode::Page).expect("the page fits");
        let index = onionskin_plugin_api::PluginRegistry::tools(model.registry())
            .position(|tool| {
                tool.capabilities()
                    .contains(&onionskin_plugin_api::ToolCapability::DynamicZoom)
            })
            .expect("tools-basic registers a dynamic zoom tool");
        assert!(model.activate_tool(index).expect("the tool activates"));

        let anchor = point(px(400.0), px(300.0));
        model
            .pointer_down(anchor, 1.0, GpuiModifiers::default())
            .expect("the press maps onto a page");

        let mut zooms = vec![model.viewport.zoom()];
        for step in 1..=8u8 {
            let at = point(anchor.x, anchor.y + px(40.0 * f32::from(step)));
            model
                .pointer_move(at, 1.0, GpuiModifiers::default(), true)
                .expect("the move is handled");
            zooms.push(model.viewport.zoom());
        }

        for pair in zooms.windows(2) {
            assert!(
                pair[1] < pair[0],
                "the zoom stopped falling mid-drag: {zooms:?}"
            );
        }
        // 320 pixels down is 2^(-320/240) of where it started.
        let expected = zooms[0] * 2.0_f32.powf(-320.0 / 240.0);
        let last = *zooms.last().expect("the drag reported zooms");
        assert!(
            (last / expected - 1.0).abs() < 1e-3,
            "{last} is not 2^(-320/240) of {}: {zooms:?}",
            zooms[0]
        );
        assert_eq!(
            model.active_tool(),
            Some(index),
            "the drag cancelled the tool"
        );
    }

    /// A raster of `page` at zoom 1, all paper except for `marks`.
    fn paper_with_marks(model: &CanvasModel, page: PageIndex, marks: RasterBounds) -> BaseRaster {
        let geometry = model
            .viewport
            .page_geometry(page)
            .expect("the page is measured");
        let (width, height) = onionskin_render::raster_size(
            geometry.render_size.0 as f32,
            geometry.render_size.1 as f32,
            1.0,
        )
        .expect("the page raster fits");
        let (width, height) = (u32::from(width), u32::from(height));
        let mut rgba = vec![255u8; width as usize * height as usize * 4];
        for y in marks.y..marks.y + marks.height {
            for x in marks.x..marks.x + marks.width {
                let start = (y as usize * width as usize + x as usize) * 4;
                rgba[start..start + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        BaseRaster::new(width, height, 1.0, rgba)
    }

    fn page_render_size(model: &CanvasModel, page: PageIndex) -> ViewSize {
        let geometry = model
            .viewport
            .page_geometry(page)
            .expect("the page is measured");
        ViewSize {
            width: geometry.render_size.0 as f32,
            height: geometry.render_size.1 as f32,
        }
    }

    /// The arithmetic Fit Visible rests on, including the far edge: the
    /// renderer floors a page's pixel count, so a box that reaches the last
    /// pixel has to land on the page edge and not past it.
    #[test]
    fn content_rect_maps_raster_pixels_onto_the_page() {
        // A page 1.25 points to the raster pixel, so the scale is exact and
        // the expected numbers are the arithmetic rather than its rounding.
        let page_size = ViewSize {
            width: 250.0,
            height: 125.0,
        };

        let quarter = content_rect(
            3,
            RasterBounds {
                x: 40,
                y: 20,
                width: 80,
                height: 40,
            },
            (200, 100),
            page_size,
        )
        .expect("a rectangle inside the page maps");
        assert_eq!(quarter.page(), 3);
        assert_eq!(quarter.origin(), ViewPoint { x: 50.0, y: 25.0 });
        assert_eq!(
            quarter.size(),
            ViewSize {
                width: 100.0,
                height: 50.0
            }
        );

        // A page whose axes scale differently, so an implementation that
        // used one scale for both would place the box somewhere else.
        let squat = ViewSize {
            width: 250.0,
            height: 50.0,
        };
        let stretched = content_rect(
            0,
            RasterBounds {
                x: 40,
                y: 20,
                width: 80,
                height: 40,
            },
            (200, 100),
            squat,
        )
        .expect("a rectangle inside a page of another shape maps");
        assert_eq!(stretched.origin(), ViewPoint { x: 50.0, y: 10.0 });
        assert_eq!(
            stretched.size(),
            ViewSize {
                width: 100.0,
                height: 20.0
            },
            "the two axes scale by the page, not by one of them"
        );

        // The renderer floors a page's pixel count, so a page whose size is
        // not a whole number of pixels scales back to a hair over its own
        // edge in f32. The far edge has to land on the page, not past it, or
        // `PageRenderRect::new` refuses the rectangle outright. These two
        // numbers are chosen because they overshoot on both axes; a pair
        // that round-trips exactly would leave the clamp dead code.
        let fractional = ViewSize {
            width: 200.5,
            height: 100.6,
        };
        let raster = (200u32, 100u32);
        assert!(
            200.0 * (fractional.width / raster.0 as f32) > fractional.width,
            "the width does not overshoot, so the clamp is not being tested"
        );
        assert!(
            100.0 * (fractional.height / raster.1 as f32) > fractional.height,
            "the height does not overshoot, so the clamp is not being tested"
        );
        let whole = content_rect(
            0,
            RasterBounds {
                x: 0,
                y: 0,
                width: raster.0,
                height: raster.1,
            },
            raster,
            fractional,
        )
        .expect("a rectangle covering the raster maps");
        assert_eq!(whole.origin(), ViewPoint { x: 0.0, y: 0.0 });
        assert_eq!(whole.size(), fractional, "the far edge is the page edge");
    }

    /// The whole point of the row: Fit Visible fits the marks, so a page that
    /// draws in one corner ends up zoomed further in than Fit Page, and the
    /// policy it holds names the rectangle the raster reported.
    #[test]
    fn fit_visible_fits_the_marks_rather_than_the_media_box() {
        let mut model = model_on_a_measured_page();
        let page = model.viewport.current_page();
        let marks = RasterBounds {
            x: 10,
            y: 20,
            width: 60,
            height: 40,
        };
        let raster = paper_with_marks(&model, page, marks);
        let expected = content_rect(
            page,
            marks,
            (raster.width(), raster.height()),
            page_render_size(&model, page),
        )
        .expect("the marks map onto the page");
        model.tiles.insert(page, raster);

        model.fit(FitMode::Page).expect("the page fits");
        let page_zoom = model.viewport.zoom();
        assert!(model.fit_visible().expect("the marks fit"));

        assert_eq!(
            model.viewport.zoom_policy(),
            onionskin_core::ZoomPolicy::Fit(FitMode::Visible(expected))
        );
        assert!(
            model.viewport.zoom() > page_zoom,
            "fitting a corner of the page should zoom past fitting all of it: {} is not more than {page_zoom}",
            model.viewport.zoom()
        );
    }

    /// Both refusals are named rather than silently doing nothing: a page
    /// still rendering and a page that draws nothing are different answers,
    /// and neither may be reported as a fit that happened.
    #[test]
    fn fit_visible_says_why_it_has_no_content_to_fit() {
        let mut model = model_on_a_measured_page();
        let page = model.viewport.current_page();

        let unrendered = model.fit_visible().expect_err("no raster has arrived");
        assert!(
            matches!(unrendered, CanvasError::FitVisibleUnrendered { page: p } if p == page),
            "{unrendered:?}"
        );
        assert!(unrendered.to_string().contains("has not been rendered"));

        let blank = paper_with_marks(
            &model,
            page,
            RasterBounds {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
        );
        model.tiles.insert(page, blank);

        let empty = model.fit_visible().expect_err("the page draws nothing");
        assert!(
            matches!(empty, CanvasError::FitVisibleBlank { page: p } if p == page),
            "{empty:?}"
        );
        assert!(empty.to_string().contains("draws nothing"));
    }
}
