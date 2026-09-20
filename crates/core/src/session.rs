use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use onionskin_content as content;
use onionskin_cos::{BytesSource, Provenance};

use crate::render::WorkerHandle;
use crate::search::{DocumentSearch, SearchUpdate};
use crate::{
    attachments, layers, outline, signatures, Attachment, Layer, ObjRef, OutlineItem, PageGeometry,
    PageIndex, PageRect, PageRender, PageSvg, RenderRequest, RenderResponse, SearchMatch,
    SearchOptions, SearchState, SearchWorkerError, Selection, SignatureField, ThumbnailRequest,
    ThumbnailResponse,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    EncryptedUnsupported,
    NoSuchPage {
        page: PageIndex,
        count: usize,
    },
    NoSuchAttachment {
        index: usize,
        count: usize,
    },
    NoSuchLayer {
        layer: ObjRef,
    },
    LayerLocked {
        name: String,
    },
    GeometryPageMismatch {
        request: PageIndex,
        geometry: PageIndex,
    },
    /// The trailer has no `/Root`, or it does not name an object. An edit
    /// to the catalog has nowhere to land.
    NoCatalog,
    /// A save was asked for on a session that was opened from bytes and has no
    /// file to write to.
    NoPath,
    /// The file was written and is correct on disk, but the session could not
    /// reopen it. The overlay is intact and the saved mark has not moved.
    WrittenButNotReloaded(crate::save::WrittenButNotReloaded),
    /// A revert was refused, with the reason.
    RevertRefused(crate::generations::RevertRefusal),
    /// An edit addressed an object that is not a dictionary. Writing a key
    /// into it would discard whatever it actually held.
    NotADictionary {
        number: u32,
    },
    Cos(onionskin_cos::Error),
    Content(content::Error),
    Worker(crate::WorkerError),
    SearchWorker(SearchWorkerError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::EncryptedUnsupported => {
                write!(f, "encrypted PDFs are not viewable in M2")
            }
            Error::NoSuchPage { page, count } => {
                write!(f, "page {page} is outside a {count}-page document")
            }
            Error::NoSuchAttachment { index, count } => write!(
                f,
                "attachment {index} is outside a document with {count} attachments"
            ),
            Error::NoSuchLayer { layer } => write!(
                f,
                "object {} {} is not an optional content group in this document",
                layer.number, layer.generation
            ),
            Error::LayerLocked { name } => write!(
                f,
                "layer {name:?} is locked by the document's default configuration"
            ),
            Error::GeometryPageMismatch { request, geometry } => write!(
                f,
                "render request is for page {request}, but its geometry is for page {geometry}"
            ),
            Error::Cos(e) => write!(f, "{e}"),
            Error::Content(e) => write!(f, "{e}"),
            Error::Worker(e) => write!(f, "{e}"),
            Error::NoCatalog => write!(f, "the trailer names no catalog"),
            Error::NoPath => write!(f, "this document has no file to save to"),
            Error::WrittenButNotReloaded(written) => write!(
                f,
                "{} was written and is correct, but this session could not reload it: {}",
                written.path.display(),
                written.cause
            ),
            Error::RevertRefused(why) => write!(f, "{why}"),
            Error::NotADictionary { number } => {
                write!(f, "object {number} is not a dictionary")
            }
            Error::SearchWorker(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Cos(e) => Some(e),
            Error::Content(e) => Some(e),
            Error::Worker(e) => Some(e),
            Error::SearchWorker(e) => Some(e),
            Error::EncryptedUnsupported
            | Error::NoSuchPage { .. }
            | Error::NoSuchAttachment { .. }
            | Error::NoSuchLayer { .. }
            | Error::LayerLocked { .. }
            | Error::GeometryPageMismatch { .. }
            | Error::NoCatalog
            | Error::NoPath
            | Error::RevertRefused(_)
            | Error::NotADictionary { .. } => None,
            Error::WrittenButNotReloaded(written) => Some(&*written.cause),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<onionskin_cos::Error> for Error {
    fn from(e: onionskin_cos::Error) -> Self {
        match e {
            onionskin_cos::Error::Encrypted => Error::EncryptedUnsupported,
            other => Error::Cos(other),
        }
    }
}

impl From<content::Error> for Error {
    fn from(e: content::Error) -> Self {
        match e {
            content::Error::Cos(onionskin_cos::Error::Encrypted) => Error::EncryptedUnsupported,
            other => Error::Content(other),
        }
    }
}

impl From<crate::WorkerError> for Error {
    fn from(e: crate::WorkerError) -> Self {
        Error::Worker(e)
    }
}

impl From<SearchWorkerError> for Error {
    fn from(e: SearchWorkerError) -> Self {
        Error::SearchWorker(e)
    }
}

#[derive(Debug)]
pub enum PageGeometryResponse {
    Ready(PageGeometry),
    Failed { page: PageIndex, error: Error },
}

impl PageGeometryResponse {
    pub fn page(&self) -> PageIndex {
        match self {
            Self::Ready(geometry) => geometry.index,
            Self::Failed { page, .. } => *page,
        }
    }
}

/// A page region the snapshot tool asked the shell to copy as an image.
///
/// The tool raises this instead of holding a render handle, so the plugin
/// surface stays document and viewport only and the clipboard stays in
/// `app`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapshotRequest {
    pub region: PageRect,
}

/// The immutable document state an export worker reopens away from the UI.
pub struct ExportSnapshot {
    bytes: Arc<Vec<u8>>,
    layer_visibility_differences: Vec<(ObjRef, bool)>,
}

impl ExportSnapshot {
    /// Reopen the original bytes and restore the session's live layer state.
    pub fn open(self) -> Result<Document> {
        let mut document = Document::open_shared(self.bytes)?;
        for (layer, visible) in self.layer_visibility_differences {
            document.set_layer_visible(layer, visible)?;
        }
        Ok(document)
    }
}

const PAGE_CACHE_LIMIT: usize = 128;
const TEXT_CACHE_LIMIT: usize = 16;

pub struct Document {
    bytes: Arc<Vec<u8>>,
    cos: onionskin_cos::Document,
    provenance: Provenance,
    page_count: usize,
    render: WorkerHandle,
    pending_geometry: BTreeSet<PageIndex>,
    geometry: PageCache<PageGeometry>,
    text: PageCache<content::PageText>,
    selection: Selection,
    search: SearchState,
    snapshot: Option<SnapshotRequest>,
    /// Spawned by the first find, so a document nobody searches never pays for
    /// the worker's own parse of the bytes.
    search_worker: Option<DocumentSearch>,
    /// The navigation panes' readers, each run at most once per document.
    /// `None` is "nobody has opened that pane", which is the state most
    /// documents stay in: no pane, no walk.
    outline: Option<Vec<OutlineItem>>,
    attachments: Option<Vec<Attachment>>,
    signatures: Option<Vec<SignatureField>>,
    /// Kept rather than re-read, because a toggle lives here: this is what
    /// the pane shows and what the render worker was last told.
    layers: Option<Vec<Layer>>,
    /// What this session has changed, and the stack that can take it back.
    /// Empty for a document nobody edits, so a reader pays nothing for it.
    edit: crate::EditSession,
    /// Where a save writes. `None` for a session opened from bytes, which is
    /// what `Save As` exists to fill in.
    path: Option<PathBuf>,
    /// Bumped whenever the bytes a reader should see change: a committed edit,
    /// a save, a revert. The preview cache and the render worker's staleness
    /// discipline both key on it.
    byte_generation: u64,
    /// The unfiltered preview for the current generation, and the document
    /// every structural read goes through. Persistent across reads within a
    /// generation.
    preview: Option<crate::preview::PreviewBuffer>,
    /// One transient slot for a filtered preview, keyed by `(generation,
    /// filter)`. See `preview.rs` for why it is one and not one per mode: a
    /// filtered preview is built for a render, used, and dropped, and keeping
    /// it apart from `preview` stops the print dialog's mode changes from
    /// evicting the buffer every structural read depends on.
    filtered: Option<crate::preview::PreviewBuffer>,
    /// The edit epoch the cached views were built at. When the session's
    /// epoch moves past it, every view is stale.
    seen_epoch: u64,
}

impl Document {
    pub fn open_path(path: &Path) -> Result<Self> {
        let mut document = Self::open_bytes(std::fs::read(path)?)?;
        document.path = Some(path.to_path_buf());
        Ok(document)
    }

    pub fn open_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::open_shared(Arc::new(bytes))
    }

    pub fn open_shared(bytes: Arc<Vec<u8>>) -> Result<Self> {
        let (cos, provenance) = onionskin_cos::Document::open_repairing(Box::new(
            BytesSource::from_shared(Arc::clone(&bytes)),
        ))?;
        let page_count = content::page_count(&cos)?;
        let edit = crate::EditSession::for_base(&cos);
        let render = WorkerHandle::spawn(Arc::clone(&bytes))?;
        Ok(Document {
            bytes,
            cos,
            provenance,
            page_count,
            render,
            pending_geometry: BTreeSet::new(),
            geometry: PageCache::new(PAGE_CACHE_LIMIT),
            text: PageCache::new(TEXT_CACHE_LIMIT),
            selection: Selection::default(),
            search: SearchState::default(),
            snapshot: None,
            search_worker: None,
            outline: None,
            attachments: None,
            signatures: None,
            layers: None,
            edit,
            path: None,
            byte_generation: 0,
            preview: None,
            filtered: None,
            seen_epoch: 0,
        })
    }

    /// What this session has changed. Read-only: the only way to move the
    /// overlay is through [`Document::edit_mut`], which records the step.
    pub fn edit(&self) -> &crate::EditSession {
        &self.edit
    }

    /// The edit session together with the base it captures `before` values
    /// from. Handing both out at once is what keeps a caller from capturing
    /// against the wrong document.
    pub fn edit_mut(&mut self) -> (&mut crate::EditSession, &onionskin_cos::Document) {
        (&mut self.edit, &self.cos)
    }

    /// Whether the document differs from its last save.
    pub fn is_dirty(&self) -> bool {
        self.edit.is_dirty()
    }

    /// Where a save writes, if anywhere.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Bumped whenever the bytes a reader should see change.
    pub fn byte_generation(&self) -> u64 {
        self.byte_generation
    }

    /// Write the overlay to the file this session was opened from.
    pub fn save(&mut self) -> Result<crate::SaveOutcome> {
        let path = self.path.clone().ok_or(Error::NoPath)?;
        self.write_to(&path, false)
    }

    /// The same, to a different file. There is no truncation relationship to
    /// the original: the new file gets the original bytes plus one section, and
    /// the session's generations list from here on describes the new file.
    pub fn save_as(&mut self, path: &Path) -> Result<crate::SaveOutcome> {
        self.write_to(path, true)
    }

    fn write_to(&mut self, path: &Path, saved_as: bool) -> Result<crate::SaveOutcome> {
        let (bytes, cos, appended) =
            crate::save::write_and_reopen(&self.bytes, &self.cos, &self.edit, path)?;

        // Only past the reopen is any of this touched, so a failed reopen
        // leaves the session exactly as it was.
        self.bytes = bytes;
        self.cos = cos;
        self.edit.rebase(&self.cos);
        self.path = Some(path.to_path_buf());
        self.bump_generation();
        Ok(crate::SaveOutcome {
            sections_appended: appended,
            saved_as,
        })
    }

    /// Everything a reader should see right now: the original bytes plus at
    /// most one section, with `filter` applied.
    pub fn preview_bytes(&mut self, filter: crate::AnnotationFilter) -> Result<Arc<Vec<u8>>> {
        self.sync_epoch();
        if !filter.hides_anything() {
            self.ensure_unfiltered()?;
            return Ok(self.preview.as_ref().expect("just built").bytes());
        }
        if !self
            .filtered
            .as_ref()
            .is_some_and(|buffer| buffer.matches(self.byte_generation, filter))
        {
            self.filtered = Some(self.build_preview(filter)?);
        }
        Ok(self.filtered.as_ref().expect("just built").bytes())
    }

    /// The document every structural read goes through, rather than the
    /// document as it was opened.
    ///
    /// **Adding a reader to `core` means routing it through here.** The
    /// counterpart rule on the write side is that adding a field to the overlay
    /// means adding a row to its table; both exist because the previous version
    /// of each was a list that sampled.
    pub fn structure(&mut self) -> Result<&onionskin_cos::Document> {
        self.ensure_unfiltered()?;
        self.preview.as_mut().expect("just built").structure()
    }

    fn ensure_unfiltered(&mut self) -> Result<()> {
        self.sync_epoch();
        let unfiltered = crate::AnnotationFilter::DocumentAndMarkups;
        if self
            .preview
            .as_ref()
            .is_some_and(|buffer| buffer.matches(self.byte_generation, unfiltered))
        {
            return Ok(());
        }
        self.preview = Some(self.build_preview(unfiltered)?);
        Ok(())
    }

    fn build_preview(
        &self,
        filter: crate::AnnotationFilter,
    ) -> Result<crate::preview::PreviewBuffer> {
        crate::preview::build(
            Arc::clone(&self.bytes),
            &self.cos,
            &self.edit,
            self.page_count,
            self.byte_generation,
            filter,
        )
    }

    /// Call after anything that changes the bytes underneath the session: a
    /// save, a revert. An edit does not need it; see `sync_epoch`.
    pub fn bump_generation(&mut self) {
        self.byte_generation += 1;
        self.geometry.clear();
        self.invalidate_views();
    }

    /// Drop every view built from the previous state of the edits, if the
    /// edits have moved since.
    ///
    /// Called at the top of every read that caches, which is what makes an
    /// edit through `edit_mut` visible to the next read without the caller
    /// doing anything.
    fn sync_epoch(&mut self) {
        let epoch = self.edit.epoch();
        if epoch != self.seen_epoch {
            self.seen_epoch = epoch;
            self.invalidate_views();
        }
    }

    fn invalidate_views(&mut self) {
        self.preview = None;
        self.filtered = None;
        self.text.clear();
        self.outline = None;
        self.attachments = None;
        self.signatures = None;
        self.layers = None;
    }

    /// The file's own history, oldest first.
    pub fn generations(&self) -> Result<Vec<crate::Generation>> {
        crate::generations::generations(&self.cos)
    }

    /// Truncate the file back to the end of `target` and reopen.
    ///
    /// The generation is bumped **before** the truncation, so anything the
    /// render worker has in flight against the old bytes is already stale by
    /// the time the file changes. The reverse order leaves exactly the window
    /// this ordering exists to close.
    pub fn revert_to(&mut self, target: usize) -> Result<()> {
        let path = self.path.clone().ok_or(Error::NoPath)?;
        let generations = self.generations()?;
        let point =
            crate::generations::truncation_point(&generations, target, self.edit.is_dirty())
                .map_err(Error::RevertRefused)?;

        self.bump_generation();

        let file = std::fs::OpenOptions::new().write(true).open(&path)?;
        file.set_len(point)?;
        file.sync_all()?;
        drop(file);

        let bytes = Arc::new(std::fs::read(&path)?);
        let cos =
            onionskin_cos::Document::open(Box::new(BytesSource::from_shared(Arc::clone(&bytes))))?;
        self.bytes = bytes;
        self.cos = cos;
        self.page_count = content::page_count(&self.cos)?;
        // The stack described bytes that no longer exist, so it goes with them.
        self.edit.forget(&self.cos);
        Ok(())
    }

    pub fn bytes(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.bytes)
    }

    /// Capture the original bytes and layer changes for a background export.
    pub fn export_snapshot(&self) -> Result<ExportSnapshot> {
        let layer_visibility_differences = match &self.layers {
            None => Vec::new(),
            Some(live_layers) => {
                let defaults = layers::read(&self.cos)?;
                live_layers
                    .iter()
                    .filter_map(|live| {
                        let default = defaults
                            .iter()
                            .find(|default| default.id == live.id)
                            .expect("live layers came from this document");
                        (live.visible != default.visible).then_some((live.id, live.visible))
                    })
                    .collect()
            }
        };
        Ok(ExportSnapshot {
            bytes: self.bytes(),
            layer_visibility_differences,
        })
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn page_count(&self) -> usize {
        self.page_count
    }

    pub fn page_geometry(&mut self, index: PageIndex) -> Result<&PageGeometry> {
        let cos = &self.cos;
        let render = &self.render;
        self.geometry.get_or_try_insert_with(index, || {
            let page = content::page(cos, index)?;
            let rendered = render.page_geometry(index)?;
            Ok(PageGeometry::new(&page, rendered))
        })
    }

    pub fn request_page_geometry(&mut self, index: PageIndex) -> Result<bool> {
        self.check_page(index)?;
        if !self.pending_geometry.insert(index) {
            return Ok(false);
        }
        if let Err(error) = self.render.request_page_geometry(index) {
            self.pending_geometry.remove(&index);
            return Err(error.into());
        }
        Ok(true)
    }

    pub fn try_page_geometry_response(&mut self) -> Result<Option<PageGeometryResponse>> {
        let Some((index, result)) = self.render.try_geometry_response()? else {
            return Ok(None);
        };
        self.pending_geometry.remove(&index);
        let rendered = match result {
            Ok(rendered) => rendered,
            Err(error) => {
                return Ok(Some(PageGeometryResponse::Failed {
                    page: index,
                    error: Error::Worker(error),
                }));
            }
        };
        let page = match content::page(&self.cos, index) {
            Ok(page) => page,
            Err(error) => {
                return Ok(Some(PageGeometryResponse::Failed {
                    page: index,
                    error: error.into(),
                }));
            }
        };
        let geometry = PageGeometry::new(&page, rendered);
        self.geometry.insert(index, geometry.clone());
        Ok(Some(PageGeometryResponse::Ready(geometry)))
    }

    pub fn page_text(&mut self, index: PageIndex) -> Result<&content::PageText> {
        self.ensure_unfiltered()?;
        let structure = self.preview.as_mut().expect("just built").structure()?;
        self.text
            .get_or_try_insert_with(index, || Ok(content::extract_page(structure, index)?))
    }

    pub fn request_render(
        &mut self,
        request: RenderRequest,
        source: Option<&onionskin_render::BaseRaster>,
    ) -> Result<()> {
        self.render.validate_request(request)?;
        let geometry = self.page_geometry(request.page)?.clone();
        self.render.request_render(request, &geometry, source)?;
        Ok(())
    }

    pub fn request_render_with_geometry(
        &mut self,
        request: RenderRequest,
        geometry: &PageGeometry,
        source: Option<&onionskin_render::BaseRaster>,
    ) -> Result<()> {
        self.render.validate_request(request)?;
        self.check_page(request.page)?;
        if geometry.index != request.page {
            return Err(Error::GeometryPageMismatch {
                request: request.page,
                geometry: geometry.index,
            });
        }
        self.render.request_render(request, geometry, source)?;
        Ok(())
    }

    pub fn try_render_response(&mut self) -> Result<Option<RenderResponse>> {
        Ok(self.render.try_response()?)
    }

    /// Rasterize one page and wait for it, on the same worker, cache and
    /// render options the canvas draws through.
    ///
    /// The interactive path is a queue whose answers arrive out of
    /// [`Document::try_render_response`] when the worker gets to them, which
    /// an export cannot use: it needs every page it asked for, in order, and
    /// it needs to know which one failed. Both go to the same renderer, so a
    /// PNG export and the pixels on screen are the same rasterizer at the
    /// same zoom.
    pub fn render_page_now(&mut self, page: PageIndex, zoom: f32) -> Result<PageRender> {
        self.check_page(page)?;
        Ok(self.render.render_page_now(page, zoom)?)
    }

    /// Convert one page to SVG on the same worker and render options.
    pub fn page_svg(&mut self, page: PageIndex) -> Result<PageSvg> {
        self.check_page(page)?;
        Ok(self.render.page_svg(page)?)
    }

    fn check_page(&self, page: PageIndex) -> Result<()> {
        if page >= self.page_count {
            return Err(Error::NoSuchPage {
                page,
                count: self.page_count,
            });
        }
        Ok(())
    }

    /// The document outline, read once and kept.
    pub fn outline(&mut self) -> Result<&[OutlineItem]> {
        self.sync_epoch();
        if self.outline.is_none() {
            let page_count = self.page_count;
            let items = outline::read(self.structure()?, page_count)?;
            self.outline = Some(items);
        }
        Ok(self.outline.as_deref().expect("the outline was just read"))
    }

    /// The embedded files, read once and kept.
    pub fn attachments(&mut self) -> Result<&[Attachment]> {
        self.sync_epoch();
        if self.attachments.is_none() {
            let items = attachments::read(self.structure()?)?;
            self.attachments = Some(items);
        }
        Ok(self
            .attachments
            .as_deref()
            .expect("the attachments were just read"))
    }

    /// The decoded bytes of one attachment, by its position in
    /// [`Document::attachments`].
    ///
    /// By position rather than by name, because names repeat: two entries in
    /// one name tree may both be `notes.txt`, and extracting "the one called
    /// notes.txt" would then be a coin toss. Nothing is written here; the
    /// caller decides where the bytes go.
    pub fn attachment_bytes(&mut self, index: usize) -> Result<Vec<u8>> {
        let count = self.attachments()?.len();
        let attachment = self
            .attachments
            .as_ref()
            .expect("the attachments were just read")
            .get(index)
            .ok_or(Error::NoSuchAttachment { index, count })?
            .clone();
        // The listing came from `structure()`, so the bytes do too: an
        // attachment the session added exists only there.
        attachments::read_bytes(self.structure()?, &attachment)
    }

    /// The signature fields, read once and kept. Listing only: nothing here
    /// says whether a signature is valid, which is M6's answer to give.
    pub fn signatures(&mut self) -> Result<&[SignatureField]> {
        self.sync_epoch();
        if self.signatures.is_none() {
            let items = signatures::read(self.structure()?)?;
            self.signatures = Some(items);
        }
        Ok(self
            .signatures
            .as_deref()
            .expect("the signature fields were just read"))
    }

    /// The optional content groups, at the visibility the session is showing
    /// them: the file's default configuration until something toggles one.
    pub fn layers(&mut self) -> Result<&[Layer]> {
        self.sync_epoch();
        if self.layers.is_none() {
            let items = layers::read(self.structure()?)?;
            self.layers = Some(items);
        }
        Ok(self.layers.as_deref().expect("the layers were just read"))
    }

    /// Show or hide one optional content group.
    ///
    /// Returns whether the visibility changed, so a caller that has to
    /// re-render can tell a real toggle from a click on the state it was
    /// already in. A group the document locked is refused rather than
    /// silently ignored: the renderer would honour the override, and the
    /// file said the user may not set one.
    ///
    /// Every later render uses the new map. The rasters already cached do
    /// not, so the caller owns dropping them; `Document` holds no pixels.
    pub fn set_layer_visible(&mut self, layer: ObjRef, visible: bool) -> Result<bool> {
        self.layers()?;
        let layers = self.layers.as_mut().expect("the layers were just read");
        let found = layers
            .iter_mut()
            .find(|candidate| candidate.id == layer)
            .ok_or(Error::NoSuchLayer { layer })?;
        if found.locked {
            return Err(Error::LayerLocked {
                name: found.name.clone(),
            });
        }
        if found.visible == visible {
            return Ok(false);
        }
        found.visible = visible;
        let overrides = layers::overrides(layers);
        self.render.set_layer_visibility(overrides)?;
        Ok(true)
    }

    /// Put every optional content group back to the visibility the file's
    /// own default configuration gives it.
    ///
    /// Re-read rather than remembered: the initial state is a fact about the
    /// file, and keeping a second copy of it beside the live one is a second
    /// thing that can drift.
    pub fn reset_layer_visibility(&mut self) -> Result<bool> {
        // Through `structure()`, like `layers()`: comparing a routed read
        // against an unrouted one would call an edited document's own layers
        // a change from its defaults.
        let initial = layers::read(self.structure()?)?;
        if self.layers.as_ref() == Some(&initial) {
            return Ok(false);
        }
        let overrides = layers::overrides(&initial);
        self.layers = Some(initial);
        self.render.set_layer_visibility(overrides)?;
        Ok(true)
    }

    /// Queue a thumbnail of `page` at `zoom`.
    ///
    /// Queued behind every interactive render, so a pane asking for a screen
    /// of thumbnails never delays the page being read. The picture arrives
    /// through [`Document::try_thumbnail_response`].
    pub fn request_thumbnail(&mut self, request: ThumbnailRequest) -> Result<()> {
        self.check_page(request.page)?;
        Ok(self.render.request_thumbnail(request)?)
    }

    pub fn try_thumbnail_response(&mut self) -> Result<Option<ThumbnailResponse>> {
        Ok(self.render.try_thumbnail_response()?)
    }

    pub fn search_page(
        &mut self,
        index: PageIndex,
        needle: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchMatch>> {
        let text = self.page_text(index)?;
        Ok(content::search(text, needle, options)
            .into_iter()
            .map(SearchMatch::from)
            .collect())
    }

    /// Starts a document-wide walk for `needle`, beginning at `start_page` and
    /// wrapping, on the search worker. Returns false when the query is the one
    /// already running or already answered, whose results stay as they are.
    ///
    /// The call returns as soon as the worker has the request; results arrive
    /// through [`Document::poll_search`], one page at a time.
    pub fn start_search(
        &mut self,
        needle: &str,
        options: SearchOptions,
        start_page: PageIndex,
    ) -> Result<bool> {
        if start_page >= self.page_count {
            return Err(Error::NoSuchPage {
                page: start_page,
                count: self.page_count,
            });
        }
        // A walk that died is worth repeating even when the query has not
        // changed: the state below would otherwise report the loss forever,
        // and the only way out would be editing the needle and editing it back.
        let died = self.search.stopped().is_some();
        if !self.search.set_query(needle, options) && !died {
            return Ok(false);
        }
        if needle.is_empty() {
            self.cancel_search();
            return Ok(false);
        }
        let worker = match &mut self.search_worker {
            Some(worker) => worker,
            slot => slot.insert(DocumentSearch::spawn(Arc::clone(&self.bytes))?),
        };
        if let Err(error) = worker.start(needle, options, start_page, self.page_count) {
            // The worker died since the last poll. Same policy as polling: the
            // find is lost and says so, the handle goes, and the next query
            // starts a fresh one, which a stopped walk allows even unchanged.
            self.search_worker = None;
            self.search.record_stopped(error.to_string());
            return Ok(false);
        }
        self.search.begin();
        Ok(true)
    }

    /// Applies whatever the search worker has produced since the last call.
    /// Returns whether anything was applied, so a caller can decide to repaint.
    ///
    /// A worker that died takes the find down with it and nothing else: the
    /// loss is recorded on the state the find bar reads, the handle is dropped
    /// so the next query can start a fresh worker, and the caller, which is a
    /// viewer drawing pages, is not handed an error it would have to survive
    /// every frame from here on.
    pub fn poll_search(&mut self) -> bool {
        let Some(worker) = &mut self.search_worker else {
            return false;
        };
        let mut applied = false;
        loop {
            let update = match worker.try_update() {
                Ok(Some(update)) => update,
                Ok(None) => return applied,
                Err(error) => {
                    self.search_worker = None;
                    self.search.record_stopped(error.to_string());
                    return true;
                }
            };
            applied = true;
            match update {
                SearchUpdate::Page { page, matches } => {
                    self.search.insert_page(page, matches);
                }
                SearchUpdate::PageFailed { page, message } => {
                    self.search.record_failure(page, message);
                }
                SearchUpdate::Finished => self.search.finish(),
            }
        }
    }

    /// Abandons the walk in flight and forgets the query, which is what closing
    /// the find bar does.
    pub fn cancel_search(&mut self) {
        if let Some(worker) = &mut self.search_worker {
            worker.cancel();
        }
        self.search = SearchState::default();
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        &mut self.selection
    }

    /// Select `region` and ask the shell to copy it as an image. Raising a
    /// request the shell has not taken yet replaces it: the user asked for
    /// the region they drew last.
    pub fn request_snapshot(&mut self, region: PageRect) {
        self.selection.set_region(region);
        self.snapshot = Some(SnapshotRequest { region });
    }

    pub fn take_snapshot_request(&mut self) -> Option<SnapshotRequest> {
        self.snapshot.take()
    }

    pub fn search(&self) -> &SearchState {
        &self.search
    }

    /// Moves the cursor to the next hit found so far, wrapping. Returns
    /// whether there was one to move to.
    ///
    /// The state is handed out immutably: a caller with `&mut SearchState`
    /// could reset the query the worker is still filling, and the results of
    /// the walk in flight would land under the new needle.
    pub fn select_next_match(&mut self) -> bool {
        self.search.select_next().is_some()
    }

    pub fn select_previous_match(&mut self) -> bool {
        self.search.select_previous().is_some()
    }

    /// Moves the cursor to one particular hit, by the page it sits on and its
    /// position among that page's hits. Returns whether there was one there.
    pub fn select_match(&mut self, page: PageIndex, index: usize) -> bool {
        self.search.select(page, index)
    }
}

struct PageCache<T> {
    limit: usize,
    items: BTreeMap<PageIndex, T>,
    order: VecDeque<PageIndex>,
}

impl<T> PageCache<T> {
    /// Drop everything. A generation bump invalidates every page, because an
    /// edit can move any of them.
    fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
    }

    fn new(limit: usize) -> Self {
        PageCache {
            limit: limit.max(1),
            items: BTreeMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get_or_try_insert_with<E>(
        &mut self,
        index: PageIndex,
        load: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<&T, E> {
        if self.items.contains_key(&index) {
            self.touch(index);
            return Ok(self.items.get(&index).expect("cache hit remains present"));
        }

        let value = load()?;
        Ok(self.insert(index, value))
    }

    fn insert(&mut self, index: PageIndex, value: T) -> &T {
        if let Some(existing) = self.items.get_mut(&index) {
            *existing = value;
            self.touch(index);
            return self
                .items
                .get(&index)
                .expect("replaced cache item is present");
        }
        while self.items.len() >= self.limit {
            if let Some(oldest) = self.order.pop_front() {
                self.items.remove(&oldest);
            }
        }
        self.items.insert(index, value);
        self.order.push_back(index);
        self.items
            .get(&index)
            .expect("inserted cache item is present")
    }

    fn touch(&mut self, index: PageIndex) {
        if let Some(position) = self.order.iter().position(|seen| *seen == index) {
            self.order.remove(position);
        }
        self.order.push_back(index);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn a_negative_root_count_does_not_open_an_empty_session() {
        use crate::testpdf::{dict, pdf};

        let result = Document::open_bytes(pdf(&[
            dict("<< /Type /Catalog /Pages 2 0 R >>"),
            dict("<< /Type /Pages /Kids [3 0 R] /Count -1 >>"),
            dict("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>"),
        ]));
        let Err(error) = result else {
            panic!("a negative root count must not create a zero-page session");
        };
        assert!(
            error.to_string().contains("page count"),
            "unexpected error: {error}"
        );
    }

    fn seed() -> Document {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        Document::open_path(&path).expect("seed opens")
    }

    fn drain(doc: &mut Document) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while doc.search().is_running() {
            assert!(std::time::Instant::now() < deadline, "the walk never ended");
            doc.poll_search();
        }
    }

    /// The viewer polls the search inside the same update that paints pages,
    /// so a dead worker must not be an error it has to survive every frame.
    #[test]
    fn a_dead_worker_ends_the_walk_and_the_next_query_starts_a_new_one() {
        let mut doc = seed();
        assert!(doc
            .start_search("Onionskin", SearchOptions::default(), 0)
            .expect("the search worker starts"));

        doc.search_worker
            .as_mut()
            .expect("the first query spawned a worker")
            .kill();

        assert!(doc.poll_search(), "the loss is something to repaint for");
        assert!(!doc.search().is_running());
        assert!(doc.search().stopped().is_some());
        assert!(
            doc.search_worker.is_none(),
            "the dead handle is dropped so the next query can replace it"
        );

        // The same query, which the state still holds, runs again on a fresh
        // worker and finishes.
        assert!(doc
            .start_search("Onionskin", SearchOptions::default(), 0)
            .expect("a second worker starts"));
        drain(&mut doc);

        assert_eq!(doc.search().stopped(), None);
        assert_eq!(doc.search().len(), 1);
        assert_eq!(doc.search().searched_pages(), 1);
    }

    /// Starting a find on a handle that went stale between polls follows the
    /// same policy: reported, dropped, and replaced on the next try.
    #[test]
    fn a_query_on_a_dead_handle_is_reported_rather_than_raised() {
        let mut doc = seed();
        assert!(doc
            .start_search("Onionskin", SearchOptions::default(), 0)
            .expect("the search worker starts"));
        doc.search_worker.as_mut().expect("a worker exists").kill();

        assert!(!doc
            .start_search("Hello", SearchOptions::default(), 0)
            .expect("a dead worker is not an error the viewer has to handle"));

        assert!(doc.search().stopped().is_some());
        assert!(doc.search_worker.is_none());
        assert!(doc
            .start_search("Hello", SearchOptions::default(), 0)
            .expect("the retry spawns a live worker"));
        drain(&mut doc);
        assert_eq!(doc.search().stopped(), None);
        assert_eq!(doc.search().len(), 1);
    }

    #[test]
    fn page_cache_is_lazy_and_bounded() {
        let mut cache = PageCache::new(2);
        let mut loads = 0usize;

        assert_eq!(
            *cache
                .get_or_try_insert_with(0, || load(&mut loads, 10))
                .unwrap(),
            10
        );
        assert_eq!(
            *cache
                .get_or_try_insert_with(1, || load(&mut loads, 11))
                .unwrap(),
            11
        );
        assert_eq!(
            *cache
                .get_or_try_insert_with(0, || load(&mut loads, 99))
                .unwrap(),
            10
        );
        assert_eq!(
            *cache
                .get_or_try_insert_with(2, || load(&mut loads, 12))
                .unwrap(),
            12
        );

        assert_eq!(loads, 3);
        assert!(cache.items.contains_key(&0));
        assert!(!cache.items.contains_key(&1));
        assert!(cache.items.contains_key(&2));
    }

    fn load(loads: &mut usize, value: i32) -> std::result::Result<i32, ()> {
        *loads += 1;
        Ok(value)
    }

    /// One page whose only mark is a filled black rectangle inside an
    /// optional content group, so "is the layer drawn" is a pixel question
    /// with a yes-or-no answer rather than a judgement about a rendering.
    ///
    /// Object 4 is the group; object 5 is the content stream.
    fn optional_content_document() -> Vec<u8> {
        use crate::testpdf::{dict, pdf, stream};

        pdf(&[
            dict(
                "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                 << /OCGs [4 0 R] /D << >> >> >>",
            ),
            dict("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            dict(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
                 /Resources << /Properties << /MC0 4 0 R >> >> /Contents 5 0 R >>",
            ),
            dict("<< /Type /OCG /Name (Stamp) >>"),
            stream("", b"/OC /MC0 BDC\n0 0 0 rg\n20 20 100 50 re f\nEMC\n"),
        ])
    }

    fn dark_pixels(render: &PageRender) -> usize {
        render
            .raster
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] < 128 && pixel[1] < 128 && pixel[2] < 128)
            .count()
    }

    /// The whole chain the layers pane depends on: the reader finds the
    /// group, the toggle reaches the render worker's options, and the next
    /// render of the page actually stops drawing it.
    ///
    /// Asserted on pixels rather than on the map that was sent, because the
    /// map being right and the renderer ignoring it is exactly the failure
    /// the fork patch exists to prevent.
    #[test]
    fn hiding_a_layer_removes_its_marks_from_the_next_render() {
        let mut doc = Document::open_bytes(optional_content_document()).expect("the fixture opens");
        let layer = doc.layers().expect("the layers read")[0].clone();
        assert!(layer.visible, "the file's default configuration shows it");

        let before = doc.render_page_now(0, 1.0).expect("the page renders");
        assert!(
            dark_pixels(&before) > 1_000,
            "the visible layer paints a filled rectangle, saw {} dark pixels",
            dark_pixels(&before)
        );

        assert!(doc
            .set_layer_visible(layer.id, false)
            .expect("the layer toggles"));
        let hidden = doc.render_page_now(0, 1.0).expect("the page renders again");

        assert_eq!(
            dark_pixels(&hidden),
            0,
            "hiding the layer has to stop it being drawn"
        );
        assert_eq!(hidden.raster.width(), before.raster.width());

        // And back: the override is a value, not a one-way door.
        assert!(doc
            .set_layer_visible(layer.id, true)
            .expect("the layer toggles back"));
        assert_eq!(
            dark_pixels(&doc.render_page_now(0, 1.0).expect("the page renders")),
            dark_pixels(&before)
        );
    }

    #[test]
    fn an_export_snapshot_reopens_with_live_layer_visibility() {
        let mut doc = Document::open_bytes(optional_content_document()).expect("the fixture opens");
        let layer = doc.layers().expect("the layers read")[0].clone();
        assert!(doc
            .set_layer_visible(layer.id, false)
            .expect("the layer toggles"));

        let mut reopened = doc
            .export_snapshot()
            .expect("the snapshot is prepared")
            .open()
            .expect("the snapshot reopens");

        assert!(!reopened.layers().expect("the layers read")[0].visible);
        assert_eq!(
            dark_pixels(
                &reopened
                    .render_page_now(0, 1.0)
                    .expect("the reopened page renders")
            ),
            0,
            "the worker document renders the live hidden state"
        );
    }

    #[test]
    fn an_export_snapshot_omits_unchanged_layer_defaults() {
        let mut doc = Document::open_bytes(optional_content_document()).expect("the fixture opens");
        doc.layers().expect("the layers read");

        let snapshot = doc.export_snapshot().expect("the snapshot is prepared");

        assert!(snapshot.layer_visibility_differences.is_empty());
    }

    #[test]
    fn an_export_snapshot_shares_the_original_bytes() {
        let doc = Document::open_bytes(optional_content_document()).expect("the fixture opens");
        let original = doc.bytes();

        let snapshot = doc.export_snapshot().expect("the snapshot is prepared");

        assert!(Arc::ptr_eq(&original, &snapshot.bytes));
    }

    /// A page carrying both an optional content group and an annotation, so
    /// "the toggle kept the annotations" is a pixel question.
    ///
    /// Object 4 is the group, 5 the page description, 6 the annotation and 7
    /// its appearance stream. The group paints black on the left, the
    /// annotation blue on the right, and neither overlaps the other.
    fn layer_and_annotation_document() -> Vec<u8> {
        use crate::testpdf::{dict, pdf, stream};

        pdf(&[
            dict(
                "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                 << /OCGs [4 0 R] /D << >> >> >>",
            ),
            dict("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            dict(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
                 /Resources << /Properties << /MC0 4 0 R >> >> /Contents 5 0 R \
                 /Annots [6 0 R] >>",
            ),
            dict("<< /Type /OCG /Name (Stamp) >>"),
            stream("", b"/OC /MC0 BDC\n0 0 0 rg\n20 20 60 50 re f\nEMC\n"),
            dict(
                "<< /Type /Annot /Subtype /Square /Rect [120 20 180 80] /F 4 \
                 /AP << /N 7 0 R >> >>",
            ),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 60 60]",
                b"0 0 1 rg\n0 0 60 60 re f\n",
            ),
        ])
    }

    fn blue_pixels(render: &PageRender) -> usize {
        render
            .raster
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[2] > 200 && pixel[0] < 100 && pixel[1] < 100)
            .count()
    }

    /// The P4 review's requirement, in the terms this canvas actually holds:
    /// a layer toggle re-renders the affected pages, and what comes back has
    /// to carry everything the page had that the layer did not own.
    ///
    /// Annotations are drawn into the base raster by the interpreter, under
    /// `RenderOptions::render_annotations`, so "re-rendered and forgot the
    /// overlays" here is a toggle that replaces the render options rather
    /// than changing the one field it means to. Asserted on the annotation's
    /// own pixels, which is the thing a user would lose.
    #[test]
    fn hiding_a_layer_keeps_the_annotations_on_the_page() {
        let mut doc =
            Document::open_bytes(layer_and_annotation_document()).expect("the fixture opens");
        let layer = doc.layers().expect("the layers read")[0].clone();

        let before = doc.render_page_now(0, 1.0).expect("the page renders");
        let annotation_before = blue_pixels(&before);
        assert!(
            dark_pixels(&before) > 1_000 && annotation_before > 1_000,
            "the fixture draws both the layer and the annotation, saw {} dark and {annotation_before} blue",
            dark_pixels(&before)
        );

        assert!(doc
            .set_layer_visible(layer.id, false)
            .expect("the layer toggles"));
        let hidden = doc.render_page_now(0, 1.0).expect("the page renders again");

        assert_eq!(dark_pixels(&hidden), 0, "the layer is gone");
        assert_eq!(
            blue_pixels(&hidden),
            annotation_before,
            "the annotation has to survive the re-render the toggle forced"
        );
    }

    #[test]
    fn toggling_a_layer_to_the_state_it_is_in_changes_nothing() {
        let mut doc = Document::open_bytes(optional_content_document()).expect("the fixture opens");
        let layer = doc.layers().expect("the layers read")[0].clone();

        assert!(!doc
            .set_layer_visible(layer.id, true)
            .expect("a no-op toggle is not an error"));
    }

    /// A locked group is refused rather than quietly ignored: the renderer
    /// would honour the override, so accepting it would show the user a
    /// visibility the document said they may not set.
    #[test]
    fn a_locked_layer_refuses_to_toggle_and_an_unknown_one_fails_loudly() {
        use crate::testpdf::{dict, pdf};

        let mut doc = Document::open_bytes(pdf(&[
            dict(
                "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                 << /OCGs [4 0 R] /D << /Locked [4 0 R] >> >> >>",
            ),
            dict("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            dict("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>"),
            dict("<< /Type /OCG /Name (Locked layer) >>"),
        ]))
        .expect("the fixture opens");
        let layer = doc.layers().expect("the layers read")[0].clone();
        assert!(layer.locked);

        assert!(matches!(
            doc.set_layer_visible(layer.id, false),
            Err(Error::LayerLocked { name }) if name == "Locked layer"
        ));
        assert!(matches!(
            doc.set_layer_visible(ObjRef::new(9_999, 0), false),
            Err(Error::NoSuchLayer { .. })
        ));
        assert!(
            doc.layers().expect("the layers still read")[0].visible,
            "a refused toggle leaves the visibility it refused to change"
        );
    }

    /// The thumbnails pane asks for the rows it is showing and no others, so
    /// a long document costs a screenful of renders rather than a document's
    /// worth. Proven against the worker rather than against a plan to call
    /// it: the pictures that come back are exactly the pages asked for.
    #[test]
    fn only_the_requested_pages_come_back_as_thumbnails() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
        let mut doc = Document::open_path(&path).expect("seed opens");

        doc.request_thumbnail(ThumbnailRequest {
            page: 1,
            zoom: 0.2,
            epoch: 0,
        })
        .expect("the request is queued");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut answered = Vec::new();
        while answered.is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "the thumbnail never arrived"
            );
            while let Some(response) = doc.try_thumbnail_response().expect("the worker is alive") {
                answered.push(response);
            }
        }

        assert_eq!(answered.len(), 1, "one request, one picture");
        let ThumbnailResponse::Ready { request, render } = &answered[0] else {
            panic!("the seed's second page rasterizes");
        };
        assert_eq!(request.page, 1);
        assert_eq!(
            request.epoch, 0,
            "the answer carries the request it came from"
        );
        // The seed's second page is a 180x80 crop box turned 90 degrees, so
        // 0.2 of it is 16x36. Pinned to the rotated, cropped size rather than
        // to the media box: a thumbnail has to look like the page does.
        assert_eq!((render.raster.width(), render.raster.height()), (16, 36));
        assert!(
            doc.try_thumbnail_response()
                .expect("the worker is alive")
                .is_none(),
            "no page the pane did not ask for is rendered"
        );
    }

    #[test]
    fn a_thumbnail_of_a_page_outside_the_document_fails_loudly() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let mut doc = Document::open_path(&path).expect("seed opens");

        assert!(matches!(
            doc.request_thumbnail(ThumbnailRequest {
                page: 7,
                zoom: 0.2,
                epoch: 0,
            }),
            Err(Error::NoSuchPage { page: 7, count: 1 })
        ));
    }

    /// Attachments are extracted by position, so the bound has to be the
    /// list's own length rather than anything the file says.
    #[test]
    fn extracting_an_attachment_that_is_not_listed_fails_loudly() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let mut doc = Document::open_path(&path).expect("seed opens");

        assert!(doc.attachments().expect("the attachments read").is_empty());
        assert!(matches!(
            doc.attachment_bytes(0),
            Err(Error::NoSuchAttachment { index: 0, count: 0 })
        ));
    }

    /// Every pane reads something from a document that has none of it, and
    /// none of them treats "the file says nothing" as an error.
    #[test]
    fn a_plain_document_reads_as_empty_in_every_pane() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
        let mut doc = Document::open_path(&path).expect("seed opens");

        assert!(doc.outline().expect("the outline reads").is_empty());
        assert!(doc.attachments().expect("the attachments read").is_empty());
        assert!(doc.layers().expect("the layers read").is_empty());
        assert!(doc.signatures().expect("the signatures read").is_empty());
    }
}
