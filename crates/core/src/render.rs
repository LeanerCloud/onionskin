mod placeholder;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use onionskin_render::{
    BaseRaster, ObjectIdentifier, PageRender, PageRenderGeometry, PageSvg, RenderOptions,
};

use crate::{PageGeometry, PageIndex};

pub use placeholder::PagePlaceholder;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderRequest {
    pub page: PageIndex,
    pub zoom: f32,
    pub generation: u64,
}

pub enum RenderResponse {
    Placeholder(PagePlaceholder),
    Raster {
        request: RenderRequest,
        render: PageRender,
    },
    Failed {
        request: RenderRequest,
        error: onionskin_render::RenderError,
    },
}

impl RenderResponse {
    pub fn request(&self) -> RenderRequest {
        match self {
            Self::Placeholder(placeholder) => placeholder.request,
            Self::Raster { request, .. } | Self::Failed { request, .. } => *request,
        }
    }
}

/// One thumbnail asked for: which page, at what size, and under which set
/// of render options.
///
/// `epoch` is the caller's own count of how many times it has changed the
/// options every render goes through. It never reaches the renderer; it
/// comes back on the answer, so the caller can tell a picture of the
/// document it is showing from a picture of the document it was showing. A
/// layer toggle is exactly that change, and without this the raster already
/// in flight when the toggle happened would arrive, be kept, and never be
/// asked for again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbnailRequest {
    pub page: PageIndex,
    pub zoom: f32,
    pub epoch: u64,
}

/// One thumbnail the worker produced, or the reason it could not.
///
/// Separate from [`RenderResponse`] because a thumbnail is not part of the
/// canvas's generation bookkeeping: the interactive queue's staleness is
/// about which pages are on screen, and a thumbnail's is about which options
/// drew it. Both answers carry the request that produced them, so neither
/// makes the receiver remember what it asked for.
pub enum ThumbnailResponse {
    Ready {
        request: ThumbnailRequest,
        render: PageRender,
    },
    Failed {
        request: ThumbnailRequest,
        error: onionskin_render::RenderError,
    },
}

impl ThumbnailResponse {
    pub fn request(&self) -> ThumbnailRequest {
        match self {
            Self::Ready { request, .. } | Self::Failed { request, .. } => *request,
        }
    }

    pub fn page(&self) -> PageIndex {
        self.request().page
    }
}

#[derive(Debug)]
pub enum WorkerError {
    Spawn(std::io::Error),
    Render(onionskin_render::RenderError),
    StaleGeneration { requested: u64, current: u64 },
    InvalidZoom(f32),
    Stopped,
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "starting render worker: {error}"),
            Self::Render(error) => write!(f, "render worker: {error}"),
            Self::StaleGeneration { requested, current } => write!(
                f,
                "render generation {requested} is older than current generation {current}"
            ),
            Self::InvalidZoom(zoom) => {
                write!(f, "render zoom must be positive and finite, got {zoom}")
            }
            Self::Stopped => write!(f, "render worker stopped before answering"),
        }
    }
}

impl std::error::Error for WorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::StaleGeneration { .. } | Self::InvalidZoom(_) | Self::Stopped => None,
        }
    }
}

enum Request {
    Geometry {
        page: usize,
        response: mpsc::SyncSender<Result<PageRenderGeometry, WorkerError>>,
    },
    GeometryAsync {
        page: PageIndex,
    },
    Render(RenderRequest),
    /// One page rasterized now, answered on the caller's own channel. Export
    /// needs the pixels in hand rather than whenever the interactive queue
    /// gets to them, and routing it here keeps it on the one renderer, cache
    /// and options the canvas draws through.
    RenderNow {
        page: PageIndex,
        zoom: f32,
        response: mpsc::SyncSender<Result<PageRender, WorkerError>>,
    },
    /// One page converted to SVG, for the same reason.
    Svg {
        page: PageIndex,
        response: mpsc::SyncSender<Result<PageSvg, WorkerError>>,
    },
    /// One page rasterized small, for the thumbnails pane. Queued behind
    /// every interactive render so a pane full of thumbnails never delays
    /// the page the user is looking at.
    Thumbnail(ThumbnailRequest),
    /// Replace the optional content overrides every later render uses.
    ///
    /// The whole map, not a difference: it is merged onto the file's own
    /// default configuration inside the interpreter, so a partial map would
    /// leave the untouched groups following the file while the pane showed
    /// something else.
    SetLayerVisibility(HashMap<ObjectIdentifier, bool>),
    Shutdown,
}

type GeometryResponse = (PageIndex, Result<PageRenderGeometry, WorkerError>);

pub(crate) struct WorkerHandle {
    requests: mpsc::Sender<Request>,
    responses: mpsc::Receiver<RenderResponse>,
    geometry_responses: mpsc::Receiver<GeometryResponse>,
    thumbnail_responses: mpsc::Receiver<ThumbnailResponse>,
    placeholders: VecDeque<PagePlaceholder>,
    generation: Option<u64>,
    latest: BTreeMap<PageIndex, RenderRequest>,
    thread: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    pub(crate) fn spawn(bytes: Arc<Vec<u8>>) -> Result<Self, WorkerError> {
        let (requests, incoming) = mpsc::channel();
        let (outgoing, responses) = mpsc::channel();
        let (geometry_outgoing, geometry_responses) = mpsc::channel();
        let (thumbnail_outgoing, thumbnail_responses) = mpsc::channel();
        let (ready, initialized) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("onionskin-render".into())
            .spawn(move || {
                let document = match onionskin_render::Document::from_shared(bytes) {
                    Ok(document) => {
                        let _ = ready.send(Ok(()));
                        document
                    }
                    Err(error) => {
                        let _ = ready.send(Err(WorkerError::Render(error)));
                        return;
                    }
                };

                let mut pending = PendingRequests::default();
                let mut options = RenderOptions::default();
                document.with_render_session(|renderer| {
                    worker_loop(
                        &document,
                        renderer,
                        &incoming,
                        &Outgoing {
                            renders: &outgoing,
                            geometry: &geometry_outgoing,
                            thumbnails: &thumbnail_outgoing,
                        },
                        &mut pending,
                        &mut options,
                    );
                });
            })
            .map_err(WorkerError::Spawn)?;

        match initialized.recv().map_err(|_| WorkerError::Stopped)? {
            Ok(()) => Ok(Self {
                requests,
                responses,
                geometry_responses,
                thumbnail_responses,
                placeholders: VecDeque::new(),
                generation: None,
                latest: BTreeMap::new(),
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    pub(crate) fn page_geometry(&self, page: usize) -> Result<PageRenderGeometry, WorkerError> {
        let (response, result) = mpsc::sync_channel(1);
        self.requests
            .send(Request::Geometry { page, response })
            .map_err(|_| WorkerError::Stopped)?;
        result.recv().map_err(|_| WorkerError::Stopped)?
    }

    /// Rasterize one page and wait for it, without disturbing the interactive
    /// queue's generation bookkeeping.
    pub(crate) fn render_page_now(
        &self,
        page: PageIndex,
        zoom: f32,
    ) -> Result<PageRender, WorkerError> {
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(WorkerError::InvalidZoom(zoom));
        }
        let (response, result) = mpsc::sync_channel(1);
        self.requests
            .send(Request::RenderNow {
                page,
                zoom,
                response,
            })
            .map_err(|_| WorkerError::Stopped)?;
        result.recv().map_err(|_| WorkerError::Stopped)?
    }

    /// Convert one page to SVG and wait for it.
    pub(crate) fn page_svg(&self, page: PageIndex) -> Result<PageSvg, WorkerError> {
        let (response, result) = mpsc::sync_channel(1);
        self.requests
            .send(Request::Svg { page, response })
            .map_err(|_| WorkerError::Stopped)?;
        result.recv().map_err(|_| WorkerError::Stopped)?
    }

    /// Queue one thumbnail. Returns as soon as the worker has the request;
    /// the picture arrives through [`Self::try_thumbnail_response`].
    pub(crate) fn request_thumbnail(&self, request: ThumbnailRequest) -> Result<(), WorkerError> {
        if !request.zoom.is_finite() || request.zoom <= 0.0 {
            return Err(WorkerError::InvalidZoom(request.zoom));
        }
        self.requests
            .send(Request::Thumbnail(request))
            .map_err(|_| WorkerError::Stopped)
    }

    pub(crate) fn try_thumbnail_response(&self) -> Result<Option<ThumbnailResponse>, WorkerError> {
        match self.thumbnail_responses.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(WorkerError::Stopped),
        }
    }

    /// Replace the optional content overrides. Renders already queued behind
    /// this call use the new map; the ones already in flight do not, which is
    /// why the caller invalidates its cached rasters as well.
    pub(crate) fn set_layer_visibility(
        &self,
        overrides: HashMap<ObjectIdentifier, bool>,
    ) -> Result<(), WorkerError> {
        self.requests
            .send(Request::SetLayerVisibility(overrides))
            .map_err(|_| WorkerError::Stopped)
    }

    pub(crate) fn request_page_geometry(&self, page: PageIndex) -> Result<(), WorkerError> {
        self.requests
            .send(Request::GeometryAsync { page })
            .map_err(|_| WorkerError::Stopped)
    }

    pub(crate) fn try_geometry_response(&self) -> Result<Option<GeometryResponse>, WorkerError> {
        match self.geometry_responses.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(WorkerError::Stopped),
        }
    }

    pub(crate) fn request_render(
        &mut self,
        request: RenderRequest,
        geometry: &PageGeometry,
        source: Option<&BaseRaster>,
    ) -> Result<(), WorkerError> {
        let placeholder = PagePlaceholder::new(request, geometry, source)?;
        self.enqueue_prepared(request, placeholder)
    }

    pub(crate) fn validate_request(&self, request: RenderRequest) -> Result<(), WorkerError> {
        if let Some(current) = self.generation {
            if request.generation < current {
                return Err(WorkerError::StaleGeneration {
                    requested: request.generation,
                    current,
                });
            }
        }
        if !request.zoom.is_finite() || request.zoom <= 0.0 {
            return Err(WorkerError::InvalidZoom(request.zoom));
        }
        Ok(())
    }

    fn enqueue_prepared(
        &mut self,
        request: RenderRequest,
        placeholder: PagePlaceholder,
    ) -> Result<(), WorkerError> {
        self.requests
            .send(Request::Render(request))
            .map_err(|_| WorkerError::Stopped)?;

        self.commit_request(request, placeholder);
        Ok(())
    }

    fn commit_request(&mut self, request: RenderRequest, placeholder: PagePlaceholder) {
        if self.generation != Some(request.generation) {
            self.generation = Some(request.generation);
            self.latest.clear();
            self.placeholders.clear();
        }
        self.latest.insert(request.page, request);
        self.placeholders
            .retain(|queued| queued.request.page != request.page);
        self.placeholders.push_back(placeholder);
    }

    pub(crate) fn try_response(&mut self) -> Result<Option<RenderResponse>, WorkerError> {
        loop {
            while let Some(placeholder) = self.placeholders.pop_front() {
                if self.is_current(placeholder.request) {
                    return Ok(Some(RenderResponse::Placeholder(placeholder)));
                }
            }

            match self.responses.try_recv() {
                Ok(response) if self.is_current(response.request()) => return Ok(Some(response)),
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => return Err(WorkerError::Stopped),
            }
        }
    }

    fn is_current(&self, request: RenderRequest) -> bool {
        self.generation == Some(request.generation)
            && self.latest.get(&request.page) == Some(&request)
    }
}

#[derive(Default)]
struct PendingRequests {
    generation: Option<u64>,
    requests: VecDeque<RenderRequest>,
    /// Thumbnails, oldest first. Kept apart from `requests` so they never
    /// take a turn ahead of a page on screen, and deduplicated by page so a
    /// pane that asks twice for a row costs one render.
    thumbnails: VecDeque<ThumbnailRequest>,
}

/// One unit of work the worker took off its queues.
///
/// The service order lives here rather than in the loop that drains it:
/// "interactive first, a thumbnail only when nothing interactive is waiting"
/// is the rule the pane's laziness rests on, and a rule expressed as control
/// flow inside `worker_loop` is a rule no test can see.
#[derive(Debug, PartialEq)]
enum Work {
    Interactive(RenderRequest),
    Thumbnail(ThumbnailRequest),
}

impl PendingRequests {
    fn is_idle(&self) -> bool {
        self.requests.is_empty() && self.thumbnails.is_empty()
    }

    fn push_thumbnail(&mut self, request: ThumbnailRequest) {
        match self
            .thumbnails
            .iter()
            .position(|queued| queued.page == request.page)
        {
            Some(position) => self.thumbnails[position] = request,
            None => self.thumbnails.push_back(request),
        }
    }

    fn pop_thumbnail(&mut self) -> Option<ThumbnailRequest> {
        self.thumbnails.pop_front()
    }

    /// The next thing to render: every interactive request first, and a
    /// thumbnail only once none is left.
    ///
    /// A pane can queue a screenful of thumbnails in one frame, and the page
    /// the user just scrolled to must not wait behind them.
    fn next(&mut self) -> Option<Work> {
        match self.pop_front() {
            Some(request) => Some(Work::Interactive(request)),
            None => self.pop_thumbnail().map(Work::Thumbnail),
        }
    }

    fn push(&mut self, request: RenderRequest) {
        match self.generation {
            Some(generation) if request.generation < generation => return,
            Some(generation) if request.generation == generation => {}
            _ => {
                self.generation = Some(request.generation);
                self.requests.clear();
            }
        }

        if let Some(position) = self
            .requests
            .iter()
            .position(|queued| queued.page == request.page)
        {
            self.requests[position] = request;
        } else {
            self.requests.push_back(request);
        }
    }

    fn pop_front(&mut self) -> Option<RenderRequest> {
        self.requests.pop_front()
    }

    fn should_publish(&mut self, request: RenderRequest) -> bool {
        match self.generation {
            Some(generation) if generation > request.generation => false,
            Some(generation) if generation == request.generation => {
                let Some(position) = self
                    .requests
                    .iter()
                    .position(|queued| queued.page == request.page)
                else {
                    return true;
                };
                if self.requests[position] == request {
                    self.requests.remove(position);
                    true
                } else {
                    false
                }
            }
            _ => true,
        }
    }
}

/// The three channels the worker answers on, bundled so the loop and its
/// helpers pass one argument rather than three that have to stay in order.
struct Outgoing<'a> {
    renders: &'a mpsc::Sender<RenderResponse>,
    geometry: &'a mpsc::Sender<GeometryResponse>,
    thumbnails: &'a mpsc::Sender<ThumbnailResponse>,
}

fn worker_loop(
    document: &onionskin_render::Document,
    renderer: &mut onionskin_render::RenderSession<'_>,
    incoming: &mpsc::Receiver<Request>,
    outgoing: &Outgoing<'_>,
    pending: &mut PendingRequests,
    options: &mut RenderOptions,
) {
    loop {
        if pending.is_idle() {
            let Ok(request) = incoming.recv() else {
                return;
            };
            if handle_request(document, renderer, options, pending, outgoing, request) {
                return;
            }
        }

        if drain_requests(document, renderer, options, pending, incoming, outgoing) {
            return;
        }
        // Which queue gets served is `PendingRequests`' decision, not this
        // loop's, so it is a decision a test can watch being made.
        match pending.next() {
            None => continue,
            Some(Work::Thumbnail(request)) => {
                if render_one_thumbnail(
                    document, renderer, options, pending, incoming, outgoing, request,
                ) {
                    return;
                }
            }
            Some(Work::Interactive(request)) => {
                let rendered = renderer.render_page(request.page, request.zoom, options);
                if drain_requests(document, renderer, options, pending, incoming, outgoing) {
                    return;
                }
                if !pending.should_publish(request) {
                    continue;
                }

                let response = match rendered {
                    Ok(render) => RenderResponse::Raster { request, render },
                    Err(error) => RenderResponse::Failed { request, error },
                };
                if outgoing.renders.send(response).is_err() {
                    return;
                }
            }
        }
    }
}

/// Rasterize the oldest queued thumbnail, if there is one. Returns true when
/// the loop should end.
///
/// Requests that arrived while it rendered are drained before the answer is
/// sent, so a page that became visible during a thumbnail render is already
/// queued ahead of the next thumbnail.
fn render_one_thumbnail(
    document: &onionskin_render::Document,
    renderer: &mut onionskin_render::RenderSession<'_>,
    options: &mut RenderOptions,
    pending: &mut PendingRequests,
    incoming: &mpsc::Receiver<Request>,
    outgoing: &Outgoing<'_>,
    request: ThumbnailRequest,
) -> bool {
    let rendered = renderer.render_page(request.page, request.zoom, options);
    if drain_requests(document, renderer, options, pending, incoming, outgoing) {
        return true;
    }
    let response = match rendered {
        Ok(render) => ThumbnailResponse::Ready { request, render },
        Err(error) => ThumbnailResponse::Failed { request, error },
    };
    outgoing.thumbnails.send(response).is_err()
}

fn drain_requests(
    document: &onionskin_render::Document,
    renderer: &mut onionskin_render::RenderSession<'_>,
    options: &mut RenderOptions,
    pending: &mut PendingRequests,
    incoming: &mpsc::Receiver<Request>,
    outgoing: &Outgoing<'_>,
) -> bool {
    loop {
        match incoming.try_recv() {
            Ok(request) => {
                if handle_request(document, renderer, options, pending, outgoing, request) {
                    return true;
                }
            }
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => return true,
        }
    }
}

fn handle_request(
    document: &onionskin_render::Document,
    renderer: &mut onionskin_render::RenderSession<'_>,
    options: &mut RenderOptions,
    pending: &mut PendingRequests,
    outgoing: &Outgoing<'_>,
    request: Request,
) -> bool {
    match request {
        Request::Geometry { page, response } => {
            let result = document.page_geometry(page).map_err(WorkerError::Render);
            let _ = response.send(result);
            false
        }
        Request::GeometryAsync { page } => {
            let result = document.page_geometry(page).map_err(WorkerError::Render);
            let _ = outgoing.geometry.send((page, result));
            false
        }
        Request::Render(request) => {
            pending.push(request);
            false
        }
        Request::RenderNow {
            page,
            zoom,
            response,
        } => {
            let result = renderer
                .render_page(page, zoom, options)
                .map_err(WorkerError::Render);
            let _ = response.send(result);
            false
        }
        Request::Svg { page, response } => {
            let result = document
                .render_page_svg(page, options)
                .map_err(WorkerError::Render);
            let _ = response.send(result);
            false
        }
        Request::Thumbnail(request) => {
            pending.push_thumbnail(request);
            false
        }
        Request::SetLayerVisibility(overrides) => {
            options.layer_visibility = overrides;
            false
        }
        Request::Shutdown => true,
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn request(page: usize, zoom: f32, generation: u64) -> RenderRequest {
        RenderRequest {
            page,
            zoom,
            generation,
        }
    }

    fn placeholder(request: RenderRequest) -> PagePlaceholder {
        PagePlaceholder {
            request,
            width: 1,
            height: 1,
            background: onionskin_render::Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            source: None,
        }
    }

    fn idle_handle() -> (
        WorkerHandle,
        mpsc::Sender<RenderResponse>,
        mpsc::Sender<GeometryResponse>,
    ) {
        let (requests, _incoming) = mpsc::channel();
        let (outgoing, responses) = mpsc::channel();
        let (geometry_outgoing, geometry_responses) = mpsc::channel();
        let (_thumbnail_outgoing, thumbnail_responses) = mpsc::channel();
        (
            WorkerHandle {
                requests,
                responses,
                geometry_responses,
                thumbnail_responses,
                placeholders: VecDeque::new(),
                generation: None,
                latest: BTreeMap::new(),
                thread: None,
            },
            outgoing,
            geometry_outgoing,
        )
    }

    #[test]
    fn a_new_generation_replaces_every_older_page() {
        let mut pending = PendingRequests::default();
        pending.push(request(1, 1.0, 3));
        pending.push(request(2, 1.0, 3));
        pending.push(request(9, 2.0, 4));

        assert_eq!(pending.pop_front(), Some(request(9, 2.0, 4)));
        assert_eq!(pending.pop_front(), None);
    }

    #[test]
    fn the_latest_zoom_for_a_page_wins_within_one_generation() {
        let mut pending = PendingRequests::default();
        pending.push(request(5, 1.0, 8));
        pending.push(request(6, 1.0, 8));
        pending.push(request(5, 4.0, 8));

        assert_eq!(pending.pop_front(), Some(request(5, 4.0, 8)));
        assert_eq!(pending.pop_front(), Some(request(6, 1.0, 8)));
    }

    #[test]
    fn queued_work_marks_an_in_flight_request_as_superseded() {
        let mut pending = PendingRequests::default();
        let old = request(5, 1.0, 2);
        pending.push(old);
        assert_eq!(pending.pop_front(), Some(old));

        pending.push(request(5, 4.0, 2));
        assert!(!pending.should_publish(old));
        pending.push(request(1, 1.0, 3));
        assert!(!pending.should_publish(old));
    }

    #[test]
    fn a_duplicate_request_reuses_the_in_flight_result() {
        let mut pending = PendingRequests::default();
        let current = request(5, 2.0, 4);
        pending.push(current);
        assert_eq!(pending.pop_front(), Some(current));
        pending.push(current);

        assert!(pending.should_publish(current));
        assert_eq!(pending.pop_front(), None);
    }

    fn thumbnail(page: usize, zoom: f32, epoch: u64) -> ThumbnailRequest {
        ThumbnailRequest { page, zoom, epoch }
    }

    /// The rule the thumbnails pane's laziness rests on: the page the user
    /// is looking at is rendered before any thumbnail, however many the pane
    /// has queued.
    ///
    /// Asserted on what comes out of `next`, which is where the decision is
    /// made. Asserting only that both queues answer would pass a worker that
    /// served the pane first and left the page blank behind a screenful of
    /// thumbnails.
    #[test]
    fn every_interactive_render_is_served_before_any_thumbnail() {
        let mut pending = PendingRequests::default();
        pending.push_thumbnail(thumbnail(5, 0.2, 0));
        pending.push_thumbnail(thumbnail(6, 0.2, 0));
        assert!(!pending.is_idle(), "a queued thumbnail is work to do");

        pending.push(request(1, 1.0, 7));
        pending.push(request(2, 1.0, 7));

        assert_eq!(pending.next(), Some(Work::Interactive(request(1, 1.0, 7))));
        assert_eq!(pending.next(), Some(Work::Interactive(request(2, 1.0, 7))));
        assert_eq!(pending.next(), Some(Work::Thumbnail(thumbnail(5, 0.2, 0))));
        assert_eq!(pending.next(), Some(Work::Thumbnail(thumbnail(6, 0.2, 0))));
        assert_eq!(pending.next(), None);
        assert!(pending.is_idle());
    }

    /// An interactive request that arrives while thumbnails are queued goes
    /// first, which is the case that matters: the pane fills its queue on
    /// one frame and the user scrolls on the next.
    #[test]
    fn a_render_queued_after_a_thumbnail_still_overtakes_it() {
        let mut pending = PendingRequests::default();
        pending.push_thumbnail(thumbnail(5, 0.2, 0));

        pending.push(request(1, 1.0, 1));

        assert_eq!(pending.next(), Some(Work::Interactive(request(1, 1.0, 1))));
        assert_eq!(pending.next(), Some(Work::Thumbnail(thumbnail(5, 0.2, 0))));
    }

    /// A pane that asks twice for a row costs one render, and the request it
    /// asked with last is the one served: scrolling back over a row already
    /// queued must not queue it again, and a size change or a layer toggle
    /// must not leave the superseded request in front of its replacement.
    #[test]
    fn asking_twice_for_a_row_queues_it_once_at_the_latest_request() {
        let mut pending = PendingRequests::default();

        pending.push_thumbnail(thumbnail(5, 0.2, 0));
        pending.push_thumbnail(thumbnail(6, 0.2, 0));
        pending.push_thumbnail(thumbnail(5, 0.36, 1));

        assert_eq!(pending.pop_thumbnail(), Some(thumbnail(5, 0.36, 1)));
        assert_eq!(pending.pop_thumbnail(), Some(thumbnail(6, 0.2, 0)));
        assert_eq!(pending.pop_thumbnail(), None);
    }

    /// A new generation replaces the interactive queue, which is the pages on
    /// screen changing. Thumbnails are pictures of pages, not of a view, so
    /// scrolling the document invalidates none of them.
    #[test]
    fn advancing_the_generation_leaves_the_thumbnail_queue_alone() {
        let mut pending = PendingRequests::default();
        pending.push_thumbnail(thumbnail(5, 0.2, 0));
        pending.push(request(1, 1.0, 1));

        pending.push(request(2, 1.0, 2));

        assert_eq!(pending.next(), Some(Work::Interactive(request(2, 1.0, 2))));
        assert_eq!(pending.next(), Some(Work::Thumbnail(thumbnail(5, 0.2, 0))));
    }

    #[test]
    fn a_generation_advance_prunes_placeholders_across_pages() {
        let (mut handle, _outgoing, _geometry_outgoing) = idle_handle();
        let old = request(7, 1.0, 1);
        let current = request(0, 2.0, 2);
        handle.commit_request(old, placeholder(old));
        handle.commit_request(current, placeholder(current));

        let response = handle.try_response().unwrap().unwrap();
        assert_eq!(response.request(), current);
        assert!(handle.try_response().unwrap().is_none());
    }

    #[test]
    fn polling_discards_a_raster_that_lost_the_outbound_race() {
        let (mut handle, outgoing, _geometry_outgoing) = idle_handle();
        let old = request(7, 1.0, 1);
        let current = request(0, 2.0, 2);
        outgoing
            .send(RenderResponse::Raster {
                request: old,
                render: PageRender {
                    raster: BaseRaster::new(1, 1, 1.0, vec![255; 4]),
                    warnings: Vec::new(),
                },
            })
            .unwrap();
        handle.commit_request(current, placeholder(current));

        assert_eq!(handle.try_response().unwrap().unwrap().request(), current);
        assert!(handle.try_response().unwrap().is_none());
    }

    #[test]
    fn a_disconnected_request_channel_commits_no_placeholder() {
        let (requests, incoming) = mpsc::channel();
        drop(incoming);
        let (_outgoing, responses) = mpsc::channel();
        let (_geometry_outgoing, geometry_responses) = mpsc::channel();
        let (_thumbnail_outgoing, thumbnail_responses) = mpsc::channel();
        let mut handle = WorkerHandle {
            requests,
            responses,
            geometry_responses,
            thumbnail_responses,
            placeholders: VecDeque::new(),
            generation: None,
            latest: BTreeMap::new(),
            thread: None,
        };
        let current = request(0, 1.0, 1);

        assert!(matches!(
            handle.enqueue_prepared(current, placeholder(current)),
            Err(WorkerError::Stopped)
        ));
        assert!(handle.placeholders.is_empty());
        assert!(handle.latest.is_empty());
        assert_eq!(handle.generation, None);
    }

    #[test]
    fn an_async_geometry_request_returns_before_the_worker_answers() {
        let (requests, incoming) = mpsc::channel();
        let (_outgoing, responses) = mpsc::channel();
        let (_geometry_outgoing, geometry_responses) = mpsc::channel();
        let (_thumbnail_outgoing, thumbnail_responses) = mpsc::channel();
        let handle = WorkerHandle {
            requests,
            responses,
            geometry_responses,
            thumbnail_responses,
            placeholders: VecDeque::new(),
            generation: None,
            latest: BTreeMap::new(),
            thread: None,
        };

        handle.request_page_geometry(7).unwrap();

        assert!(matches!(
            incoming.try_recv(),
            Ok(Request::GeometryAsync { page: 7 })
        ));
        assert!(handle.try_geometry_response().unwrap().is_none());
    }

    #[test]
    fn geometry_queued_while_the_worker_is_busy_is_polled_without_blocking() {
        let (requests, incoming) = mpsc::channel();
        let (_outgoing, responses) = mpsc::channel();
        let (geometry_outgoing, geometry_responses) = mpsc::channel();
        let (_thumbnail_outgoing, thumbnail_responses) = mpsc::channel();
        let (started, worker_started) = mpsc::sync_channel(1);
        let (release, worker_release) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            assert!(matches!(incoming.recv(), Ok(Request::Render(_))));
            started.send(()).unwrap();
            worker_release.recv().unwrap();
            let Ok(Request::GeometryAsync { page }) = incoming.recv() else {
                panic!("geometry request follows the in-flight render");
            };
            geometry_outgoing
                .send((page, Err(WorkerError::InvalidZoom(-1.0))))
                .unwrap();
        });
        let handle = WorkerHandle {
            requests,
            responses,
            geometry_responses,
            thumbnail_responses,
            placeholders: VecDeque::new(),
            generation: None,
            latest: BTreeMap::new(),
            thread: None,
        };
        handle
            .requests
            .send(Request::Render(request(0, 1.0, 1)))
            .unwrap();
        worker_started.recv().unwrap();

        handle.request_page_geometry(4).unwrap();
        assert!(handle.try_geometry_response().unwrap().is_none());

        release.send(()).unwrap();
        worker.join().unwrap();
        let (page, result) = handle.try_geometry_response().unwrap().unwrap();
        assert_eq!(page, 4);
        assert!(matches!(result, Err(WorkerError::InvalidZoom(-1.0))));
    }

    #[test]
    fn a_failed_render_response_retains_its_page_request() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/minimal.pdf");
        let bytes = Arc::new(std::fs::read(path).expect("seed is readable"));
        let handle = WorkerHandle::spawn(bytes).expect("worker starts");
        let oversized = request(0, 1_000.0, 9);

        handle.requests.send(Request::Render(oversized)).unwrap();

        let response = handle
            .responses
            .recv_timeout(Duration::from_secs(5))
            .expect("failed render response arrives");
        assert!(matches!(
            response,
            RenderResponse::Failed { request, .. } if request == oversized
        ));
    }
}
