use std::time::{Duration, Instant};

use onionskin_core::{
    Document, Error, PageGeometryResponse, RenderRequest, RenderResponse, ViewSize, Viewport,
    WorkerError,
};
use onionskin_render::{BaseRaster, InterpreterWarning, Rgba};

#[test]
fn one_generation_renders_every_requested_page_at_the_right_size() {
    let mut document = Document::open_bytes(pages_pdf(21)).expect("document opens");
    for page in 0..21 {
        document
            .request_render(request(page, 2.0, 1), None)
            .expect("request queues");
    }

    let responses = collect(&mut document, 42);
    for page in 0..21 {
        let placeholder_at = responses
            .iter()
            .position(|response| {
                matches!(response, RenderResponse::Placeholder(placeholder) if placeholder.request.page == page)
            })
            .expect("placeholder arrives");
        let raster_at = responses
            .iter()
            .position(|response| {
                matches!(response, RenderResponse::Raster { request, .. } if request.page == page)
            })
            .expect("raster arrives");
        assert!(placeholder_at < raster_at);

        match &responses[placeholder_at] {
            RenderResponse::Placeholder(placeholder) => {
                assert_eq!((placeholder.width, placeholder.height), (144, 288));
            }
            _ => unreachable!(),
        }
        match &responses[raster_at] {
            RenderResponse::Raster { render, .. } => {
                assert_eq!((render.raster.width(), render.raster.height()), (144, 288));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn a_new_generation_hides_all_older_pages() {
    let mut document = Document::open_bytes(pages_pdf(21)).expect("document opens");
    for page in 0..21 {
        document
            .request_render(request(page, 1.0, 1), None)
            .expect("old request queues");
    }
    document
        .request_render(request(0, 3.0, 2), None)
        .expect("new request queues");

    let responses = collect(&mut document, 2);
    assert!(responses
        .iter()
        .all(|response| response.request() == request(0, 3.0, 2)));
    assert!(matches!(responses[0], RenderResponse::Placeholder(_)));
    assert!(matches!(responses[1], RenderResponse::Raster { .. }));
}

#[test]
fn the_latest_zoom_wins_and_an_old_raster_is_display_only() {
    let mut document = Document::open_bytes(pages_pdf(1)).expect("document opens");
    let source = BaseRaster::new(72, 144, 1.0, vec![127; 72 * 144 * 4]);
    document
        .request_render(request(0, 1.0, 7), None)
        .expect("first zoom queues");
    document
        .request_render(request(0, 4.0, 7), Some(&source))
        .expect("new zoom queues");

    let responses = collect(&mut document, 2);
    match &responses[0] {
        RenderResponse::Placeholder(placeholder) => {
            assert_eq!(placeholder.request, request(0, 4.0, 7));
            assert_eq!((placeholder.width, placeholder.height), (288, 576));
            assert_eq!(
                placeholder.background,
                Rgba {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                }
            );
            let cached = placeholder.source().expect("source raster is carried");
            assert_eq!(
                (cached.width(), cached.height(), cached.zoom()),
                (72, 144, 1.0)
            );
            assert_eq!(cached.rgba().as_ptr(), source.rgba().as_ptr());
        }
        _ => panic!("placeholder must be first"),
    }
    assert!(matches!(
        &responses[1],
        RenderResponse::Raster {
            request: response_request,
            render,
        } if *response_request == request(0, 4.0, 7) && render.raster.zoom() == 4.0
    ));
}

#[test]
fn render_warnings_cross_the_worker_boundary() {
    let mut document = Document::open_bytes(warning_pdf()).expect("document opens");
    document
        .request_render(request(0, 1.0, 1), None)
        .expect("request queues");

    let responses = collect(&mut document, 2);
    match &responses[1] {
        RenderResponse::Raster { render, .. } => assert!(render.warnings.iter().any(|warning| {
            matches!(warning, InterpreterWarning::UnresolvedAnnotationAppearance)
        })),
        _ => panic!("raster must follow placeholder"),
    }
}

#[test]
fn invalid_and_stale_requests_queue_nothing() {
    let mut document = Document::open_bytes(pages_pdf(1)).expect("document opens");
    assert!(document.request_render(request(1, 1.0, 1), None).is_err());
    assert!(document
        .request_render(request(0, f32::NAN, 1), None)
        .is_err());
    assert!(document
        .try_render_response()
        .expect("worker is live")
        .is_none());

    document
        .request_render(request(0, 1.0, 2), None)
        .expect("current request queues");
    let error = document
        .request_render(request(999, 1.0, 1), None)
        .expect_err("old generation is refused");
    assert!(matches!(
        error,
        onionskin_core::Error::Worker(WorkerError::StaleGeneration {
            requested: 1,
            current: 2
        })
    ));
    assert!(collect(&mut document, 2)
        .iter()
        .all(|response| response.request().generation == 2));
}

#[test]
fn dropping_a_session_joins_the_worker() {
    let mut document = Document::open_bytes(pages_pdf(1)).expect("document opens");
    document
        .request_render(request(0, 1.0, 1), None)
        .expect("request queues");
    drop(document);
}

#[test]
fn page_geometry_can_be_requested_and_polled_without_waiting() {
    let mut document = Document::open_bytes(pages_pdf(2)).expect("document opens");

    assert!(document
        .request_page_geometry(1)
        .expect("geometry request queues"));
    assert!(!document
        .request_page_geometry(1)
        .expect("duplicate geometry request is suppressed"));
    let geometry = collect_geometry(&mut document);

    let PageGeometryResponse::Ready(geometry) = geometry else {
        panic!("valid page geometry failed");
    };
    assert_eq!(geometry.index, 1);
    assert_eq!(
        document
            .page_geometry(1)
            .expect("async response populated the document cache"),
        &geometry
    );
}

#[test]
fn an_invalid_async_geometry_request_fails_before_it_is_queued() {
    let mut document = Document::open_bytes(pages_pdf(2)).expect("document opens");

    assert!(matches!(
        document.request_page_geometry(2),
        Err(Error::NoSuchPage { page: 2, count: 2 })
    ));
}

#[test]
fn a_viewport_geometry_enqueues_render_after_the_document_cache_evicts_it() {
    let mut document = Document::open_bytes(pages_pdf(130)).expect("document opens");
    let first = document
        .page_geometry(0)
        .expect("first geometry loads")
        .clone();
    for page in 1..130 {
        document.page_geometry(page).expect("geometry loads");
    }

    let mut viewport = Viewport::new(
        130,
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
        12.0,
    )
    .unwrap();
    viewport.measure_page(first.clone()).unwrap();
    let retained = viewport
        .page_geometry(0)
        .expect("viewport retained page zero");

    document
        .request_render_with_geometry(request(0, 1.0, 1), retained, None)
        .expect("explicit geometry queues without a document-cache lookup");
    assert!(matches!(
        document.try_render_response().unwrap(),
        Some(RenderResponse::Placeholder(_))
    ));
}

#[test]
fn explicit_render_geometry_must_belong_to_the_requested_page() {
    let mut document = Document::open_bytes(pages_pdf(2)).expect("document opens");
    let other = document.page_geometry(1).expect("geometry loads").clone();

    assert!(matches!(
        document.request_render_with_geometry(request(0, 1.0, 1), &other, None),
        Err(Error::GeometryPageMismatch {
            request: 0,
            geometry: 1
        })
    ));
    assert!(document.try_render_response().unwrap().is_none());
}

#[test]
fn explicit_render_geometry_cannot_expand_the_target_document() {
    let mut source_document = Document::open_bytes(pages_pdf(2)).expect("source opens");
    let second = source_document
        .page_geometry(1)
        .expect("source geometry loads")
        .clone();
    let mut target_document = Document::open_bytes(pages_pdf(1)).expect("target opens");

    assert!(matches!(
        target_document.request_render_with_geometry(request(1, 1.0, 1), &second, None),
        Err(Error::NoSuchPage { page: 1, count: 1 })
    ));
    assert!(target_document.try_render_response().unwrap().is_none());
}

#[test]
fn a_synchronous_render_matches_the_interactive_one_for_the_same_page_and_zoom() {
    let mut document = Document::open_bytes(pages_pdf(2)).expect("document opens");
    document
        .request_render(request(1, 2.0, 1), None)
        .expect("request queues");
    let interactive = collect(&mut document, 2)
        .into_iter()
        .find_map(|response| match response {
            RenderResponse::Raster { render, .. } => Some(render),
            _ => None,
        })
        .expect("the interactive path produces a raster");

    let exported = document
        .render_page_now(1, 2.0)
        .expect("the same page renders synchronously");

    assert_eq!(
        (exported.raster.width(), exported.raster.height()),
        (interactive.raster.width(), interactive.raster.height())
    );
    assert_eq!(exported.raster.rgba(), interactive.raster.rgba());
}

#[test]
fn a_synchronous_render_leaves_the_interactive_queue_alone() {
    let mut document = Document::open_bytes(pages_pdf(2)).expect("document opens");
    document
        .request_render(request(0, 1.0, 7), None)
        .expect("request queues");

    document
        .render_page_now(1, 1.0)
        .expect("page renders synchronously");

    let responses = collect(&mut document, 2);
    assert!(responses
        .iter()
        .all(|response| response.request() == request(0, 1.0, 7)));
    assert!(matches!(responses[0], RenderResponse::Placeholder(_)));
    assert!(matches!(responses[1], RenderResponse::Raster { .. }));
}

#[test]
fn synchronous_work_refuses_a_page_or_a_zoom_the_document_cannot_serve() {
    let mut document = Document::open_bytes(pages_pdf(1)).expect("document opens");

    assert!(matches!(
        document.render_page_now(4, 1.0),
        Err(Error::NoSuchPage { page: 4, count: 1 })
    ));
    assert!(matches!(
        document.page_svg(4),
        Err(Error::NoSuchPage { page: 4, count: 1 })
    ));
    assert!(matches!(
        document.render_page_now(0, 0.0),
        Err(Error::Worker(WorkerError::InvalidZoom(zoom))) if zoom == 0.0
    ));
    // The worker is still answering, so a refused request never reached it.
    assert!(document.render_page_now(0, 1.0).is_ok());
}

#[test]
fn a_page_converts_to_svg_sized_like_its_raster() {
    let mut document = Document::open_bytes(pages_pdf(1)).expect("document opens");

    let page = document.page_svg(0).expect("page converts");

    assert!(page.svg.starts_with("<svg"));
    assert!(page.svg.contains("viewBox=\"0 0 72 144\""), "{}", page.svg);
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
}

fn request(page: usize, zoom: f32, generation: u64) -> RenderRequest {
    RenderRequest {
        page,
        zoom,
        generation,
    }
}

fn collect(document: &mut Document, expected: usize) -> Vec<RenderResponse> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut responses = Vec::new();
    while responses.len() < expected {
        match document.try_render_response().expect("worker remains live") {
            Some(response) => responses.push(response),
            None if Instant::now() < deadline => std::thread::yield_now(),
            None => panic!(
                "timed out after receiving {} of {expected} responses",
                responses.len()
            ),
        }
    }
    responses
}

fn collect_geometry(document: &mut Document) -> PageGeometryResponse {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match document
            .try_page_geometry_response()
            .expect("worker remains live")
        {
            Some(response) => return response,
            None if Instant::now() < deadline => std::thread::yield_now(),
            None => panic!("timed out waiting for page geometry"),
        }
    }
}

fn pages_pdf(count: usize) -> Vec<u8> {
    let kids = (0..count)
        .map(|index| format!("{} 0 R", index + 3))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>").into_bytes(),
    ];
    objects.extend((0..count).map(|_| {
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 144] /Resources << >> >>".to_vec()
    }));
    pdf(objects)
}

fn warning_pdf() -> Vec<u8> {
    pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 144] /Resources << >> /Annots [4 0 R] >>"
            .to_vec(),
        b"<< /Type /Annot /Subtype /Text /Rect [10 10 20 20] /AP << /N << /Off 5 0 R >> >> >>"
            .to_vec(),
    ])
}

fn pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}
