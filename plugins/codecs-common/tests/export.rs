//! What the three exporters produce, checked against the sources they claim
//! to be faithful to: `content`'s extraction for text, and the canvas render
//! path's own raster for PNG.

use std::time::{Duration, Instant};

use onionskin_codecs_common::{
    CommonCodecsPlugin, JpegCodec, PngCodec, SvgCodec, TextCodec, TiffCodec,
};
use onionskin_core::{BaseRaster, Document, RenderRequest, RenderResponse};
use onionskin_corpus_testing::seed;
use onionskin_plugin_api::{
    CodecPlugin, ExportError, ExportOutputKind, ExportRequest, PageIndex, PageRange, PluginRegistry,
};

enum CollectedExport {
    Single(Vec<u8>),
    PerPage(Vec<(PageIndex, Vec<u8>)>),
}

fn collect_export(
    codec: &dyn CodecPlugin,
    doc: &mut Document,
    request: &ExportRequest,
) -> Result<CollectedExport, ExportError> {
    match codec.output_kind() {
        ExportOutputKind::Single => {
            let mut bytes = Vec::new();
            for (position, page) in request.pages.pages().enumerate() {
                bytes.extend(codec.export_page(doc, request, page, position == 0)?);
            }
            Ok(CollectedExport::Single(bytes))
        }
        ExportOutputKind::PerPage => {
            let mut pages = Vec::new();
            for (position, page) in request.pages.pages().enumerate() {
                pages.push((page, codec.export_page(doc, request, page, position == 0)?));
            }
            Ok(CollectedExport::PerPage(pages))
        }
    }
}

fn open(name: &str) -> Document {
    Document::open_path(&seed(name)).expect("seed opens")
}

fn whole(doc: &Document, dpi: f32) -> ExportRequest {
    ExportRequest {
        pages: PageRange::whole(doc.page_count()).expect("the seed has pages"),
        dpi,
        quality: None,
    }
}

fn decode(bytes: &[u8]) -> image::RgbaImage {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("the export is a PNG")
        .to_rgba8()
}

fn collect_raster(doc: &mut Document) -> BaseRaster {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match doc.try_render_response().expect("worker remains live") {
            Some(RenderResponse::Raster { render, .. }) => return render.raster,
            Some(_) => {}
            None if Instant::now() < deadline => std::thread::yield_now(),
            None => panic!("timed out waiting for the canvas path's raster"),
        }
    }
}

#[test]
fn the_manifest_registers_exactly_the_five_export_formats() {
    let mut registry = PluginRegistry::new();
    registry.install(&CommonCodecsPlugin);

    let installed: Vec<_> = registry
        .codecs()
        .map(|codec| (codec.id(), codec.extension()))
        .collect();
    assert_eq!(
        installed,
        vec![
            ("text", "txt"),
            ("png", "png"),
            ("svg", "svg"),
            ("jpeg", "jpg"),
            ("tiff", "tif")
        ]
    );
}

#[test]
fn codecs_declare_their_destination_layout() {
    assert_eq!(TextCodec.output_kind(), ExportOutputKind::Single);
    assert_eq!(PngCodec.output_kind(), ExportOutputKind::PerPage);
    assert_eq!(SvgCodec.output_kind(), ExportOutputKind::PerPage);
    assert_eq!(
        JpegCodec::default().output_kind(),
        ExportOutputKind::PerPage
    );
    assert_eq!(TiffCodec.output_kind(), ExportOutputKind::PerPage);
}

/// The text codec adds no interpretation of its own: what it writes for one
/// page is what `content` extracted, byte for byte.
#[test]
fn text_export_is_contents_extraction_verbatim() {
    let mut doc = open("hello.pdf");
    let expected = doc.page_text(0).expect("page text extracts").flatten().text;
    let request = whole(&doc, 72.0);

    let CollectedExport::Single(bytes) =
        collect_export(&TextCodec, &mut doc, &request).expect("text exports")
    else {
        panic!("text must be a single output");
    };

    assert_eq!(String::from_utf8(bytes).unwrap(), expected);
}

#[test]
fn a_multi_page_text_export_separates_pages_and_keeps_document_order() {
    let mut doc = open("two-page.pdf");
    let first = doc
        .page_text(0)
        .expect("first page extracts")
        .flatten()
        .text;
    let second = doc
        .page_text(1)
        .expect("second page extracts")
        .flatten()
        .text;
    let request = whole(&doc, 72.0);

    let CollectedExport::Single(bytes) =
        collect_export(&TextCodec, &mut doc, &request).expect("text exports")
    else {
        panic!("text must be a single output");
    };

    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        format!("{first}\n\n{second}")
    );
}

#[test]
fn a_range_starting_after_page_zero_does_not_gain_a_leading_separator() {
    let mut doc = open("two-page.pdf");
    let expected = doc
        .page_text(1)
        .expect("second page extracts")
        .flatten()
        .text;
    let request = ExportRequest {
        pages: PageRange::new(1, 1, doc.page_count()).expect("the second page is a range"),
        dpi: 72.0,
        quality: None,
    };

    let bytes = TextCodec
        .export_page(&mut doc, &request, 1, true)
        .expect("the first requested page exports");

    assert_eq!(String::from_utf8(bytes).unwrap(), expected);
}

#[test]
fn png_export_is_one_decodable_file_per_page_at_the_requested_resolution() {
    let mut doc = open("two-page.pdf");
    let request = whole(&doc, 144.0);
    let expected: Vec<(u32, u32)> = request
        .pages
        .pages()
        .map(|page| {
            let raster = doc.render_page_now(page, 2.0).expect("page renders").raster;
            (raster.width(), raster.height())
        })
        .collect();

    let CollectedExport::PerPage(files) =
        collect_export(&PngCodec, &mut doc, &request).expect("pages export")
    else {
        panic!("PNG must be per-page output");
    };

    assert_eq!(files.len(), 2);
    assert_eq!(
        files.iter().map(|(page, _)| *page).collect::<Vec<_>>(),
        [0, 1]
    );
    for (index, (_, bytes)) in files.iter().enumerate() {
        assert_eq!(decode(bytes).dimensions(), expected[index]);
    }
}

/// The evidence that PNG export renders through the canvas path rather than
/// beside it: the exported file decodes to the same pixels the interactive
/// queue delivers for the same page at the same zoom.
#[test]
fn a_png_export_decodes_to_the_canvas_paths_own_raster() {
    let mut doc = open("hello.pdf");
    doc.request_render(
        RenderRequest {
            page: 0,
            zoom: 2.0,
            generation: 1,
        },
        None,
    )
    .expect("the canvas path queues a render");
    let on_screen = collect_raster(&mut doc);
    let request = ExportRequest {
        pages: PageRange::whole(1).expect("one page"),
        dpi: 144.0,
        quality: None,
    };

    let bytes = PngCodec
        .export_page(&mut doc, &request, 0, true)
        .expect("page exports");

    let decoded = decode(&bytes);
    assert_eq!(
        decoded.dimensions(),
        (on_screen.width(), on_screen.height())
    );
    assert_eq!(decoded.into_raw(), on_screen.rgba());
}

#[test]
fn svg_export_is_one_parseable_page_per_file_with_its_glyphs() {
    let mut doc = open("hello.pdf");
    let request = whole(&doc, 72.0);

    let bytes = SvgCodec
        .export_page(&mut doc, &request, 0, true)
        .expect("page exports");

    let svg = String::from_utf8(bytes).expect("SVG is UTF-8");
    assert!(svg.starts_with("<svg"), "{svg:.60}");
    assert!(svg.contains("<path"), "no glyph outlines in the export");
    assert_eq!(
        svg.matches("<svg").count(),
        svg.matches("</svg>").count(),
        "the document does not close its root element"
    );
}

/// The SVG covers the same page area the PNG rasterizes, which is what lets
/// one stand in for the other.
#[test]
fn an_exported_svg_covers_the_same_page_box_as_the_raster() {
    let mut doc = open("hello.pdf");
    let raster = doc.render_page_now(0, 1.0).expect("page renders").raster;
    let request = whole(&doc, 72.0);

    let bytes = SvgCodec
        .export_page(&mut doc, &request, 0, true)
        .expect("page exports");

    let svg = String::from_utf8(bytes).expect("SVG is UTF-8");
    assert!(
        svg.contains(&format!(
            "viewBox=\"0 0 {} {}\"",
            raster.width(),
            raster.height()
        )),
        "{svg:.160}"
    );
}

/// A page that cannot be produced names the failing page for the worker.
#[test]
fn a_page_that_cannot_be_rendered_fails_by_page_number() {
    let mut doc = open("two-page.pdf");
    let request = ExportRequest {
        pages: PageRange::whole(doc.page_count()).expect("the seed has pages"),
        // Past what hayro can address on one axis, so every page fails.
        dpi: 72.0 * 5_000.0,
        quality: None,
    };

    let failure = PngCodec
        .export_page(&mut doc, &request, 0, true)
        .expect_err("an unrenderable size is refused");

    assert!(matches!(failure, ExportError::Page { page: 0, .. }));
    assert!(failure.to_string().starts_with("page 1: "), "{failure}");
}

#[test]
fn a_resolution_that_is_not_a_resolution_is_refused_before_any_page_is_read() {
    let mut doc = open("hello.pdf");

    for dpi in [0.0, -300.0, f32::INFINITY] {
        let request = ExportRequest {
            pages: PageRange::whole(1).expect("one page"),
            dpi,
            quality: None,
        };
        assert!(
            matches!(
                PngCodec.export_page(&mut doc, &request, 0, true),
                Err(ExportError::InvalidDpi(_))
            ),
            "{dpi} was accepted"
        );
    }
}
