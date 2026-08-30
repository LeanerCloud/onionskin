mod placeholder;

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use onionskin_render::{BaseRaster, PageRender, PageRenderGeometry, PageSvg, RenderOptions};

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
    Shutdown,
}

type GeometryResponse = (PageIndex, Result<PageRenderGeometry, WorkerError>);

pub(crate) struct WorkerHandle {
    requests: mpsc::Sender<Request>,
    responses: mpsc::Receiver<RenderResponse>,
    geometry_responses: mpsc::Receiver<GeometryResponse>,
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
                let options = RenderOptions::default();
                document.with_render_session(|renderer| {
                    worker_loop(
                        &document,
                        renderer,
                        &incoming,
                        &outgoing,
                        &geometry_outgoing,
                        &mut pending,
                        &options,
                    );
                });
            })
            .map_err(WorkerError::Spawn)?;

        match initialized.recv().map_err(|_| WorkerError::Stopped)? {
            Ok(()) => Ok(Self {
                requests,
                responses,
                geometry_responses,
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
}

impl PendingRequests {
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

fn worker_loop(
    document: &onionskin_render::Document,
    renderer: &mut onionskin_render::RenderSession<'_>,
    incoming: &mpsc::Receiver<Request>,
    outgoing: &mpsc::Sender<RenderResponse>,
    geometry_outgoing: &mpsc::Sender<GeometryResponse>,
    pending: &mut PendingRequests,
    options: &RenderOptions,
) {
    loop {
        if pending.requests.is_empty() {
            let Ok(request) = incoming.recv() else {
                return;
            };
            if handle_request(
                document,
                renderer,
                options,
                pending,
                geometry_outgoing,
                request,
            ) {
                return;
            }
        }

        if drain_requests(
            document,
            renderer,
            options,
            pending,
            incoming,
            geometry_outgoing,
        ) {
            return;
        }
        let Some(request) = pending.pop_front() else {
            continue;
        };
        let rendered = renderer.render_page(request.page, request.zoom, options);
        if drain_requests(
            document,
            renderer,
            options,
            pending,
            incoming,
            geometry_outgoing,
        ) {
            return;
        }
        if !pending.should_publish(request) {
            continue;
        }

        let response = match rendered {
            Ok(render) => RenderResponse::Raster { request, render },
            Err(error) => RenderResponse::Failed { request, error },
        };
        if outgoing.send(response).is_err() {
            return;
        }
    }
}

fn drain_requests(
    document: &onionskin_render::Document,
    renderer: &mut onionskin_render::RenderSession<'_>,
    options: &RenderOptions,
    pending: &mut PendingRequests,
    incoming: &mpsc::Receiver<Request>,
    geometry_outgoing: &mpsc::Sender<GeometryResponse>,
) -> bool {
    loop {
        match incoming.try_recv() {
            Ok(request) => {
                if handle_request(
                    document,
                    renderer,
                    options,
                    pending,
                    geometry_outgoing,
                    request,
                ) {
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
    options: &RenderOptions,
    pending: &mut PendingRequests,
    geometry_outgoing: &mpsc::Sender<GeometryResponse>,
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
            let _ = geometry_outgoing.send((page, result));
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
        (
            WorkerHandle {
                requests,
                responses,
                geometry_responses,
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
        let mut handle = WorkerHandle {
            requests,
            responses,
            geometry_responses,
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
        let handle = WorkerHandle {
            requests,
            responses,
            geometry_responses,
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
