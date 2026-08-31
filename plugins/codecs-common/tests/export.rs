//! What the three exporters produce, checked against the sources they claim
//! to be faithful to: `content`'s extraction for text, and the canvas render
//! path's own raster for PNG.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use onionskin_codecs_common::{CommonCodecsPlugin, PngCodec, SvgCodec, TextCodec};
use onionskin_core::{BaseRaster, Document, RenderRequest, RenderResponse};
use onionskin_plugin_api::{CodecPlugin, ExportError, ExportRequest, PageRange, PluginRegistry};

fn seed(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the plugin lives under workspace/plugins")
        .join("corpus/seeds")
        .join(name)
}

fn open(name: &str) -> Document {
    Document::open_path(&seed(name)).expect("seed opens")
}

fn whole(doc: &Document, dpi: f32) -> ExportRequest {
    ExportRequest {
        pages: PageRange::whole(doc.page_count()).expect("the seed has pages"),
        dpi,
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
fn the_manifest_registers_exactly_the_three_export_formats() {
    let mut registry = PluginRegistry::new();
    registry.install(&CommonCodecsPlugin);

    let installed: Vec<_> = registry
        .codecs()
        .map(|codec| (codec.id(), codec.extension()))
        .collect();
    assert_eq!(
        installed,
        vec![("text", "txt"), ("png", "png"), ("svg", "svg")]
    );
}

/// The text codec adds no interpretation of its own: what it writes for one
/// page is what `content` extracted, byte for byte.
#[test]
fn text_export_is_contents_extraction_verbatim() {
    let mut doc = open("hello.pdf");
    let expected = doc.page_text(0).expect("page text extracts").flatten().text;
    let request = whole(&doc, 72.0);

    let files = TextCodec.export(&mut doc, &request).expect("text exports");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].page, None, "text is one file for the whole range");
    assert_eq!(String::from_utf8(files[0].bytes.clone()).unwrap(), expected);
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

    let files = TextCodec.export(&mut doc, &request).expect("text exports");

    assert_eq!(
        String::from_utf8(files[0].bytes.clone()).unwrap(),
        format!("{first}\n\n{second}")
    );
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

    let files = PngCodec.export(&mut doc, &request).expect("pages export");

    assert_eq!(files.len(), 2);
    for (index, file) in files.iter().enumerate() {
        assert_eq!(file.page, Some(index), "each PNG names its page");
        assert_eq!(decode(&file.bytes).dimensions(), expected[index]);
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
    };

    let files = PngCodec.export(&mut doc, &request).expect("page exports");

    let decoded = decode(&files[0].bytes);
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

    let files = SvgCodec.export(&mut doc, &request).expect("page exports");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].page, Some(0));
    let svg = String::from_utf8(files[0].bytes.clone()).expect("SVG is UTF-8");
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

    let files = SvgCodec.export(&mut doc, &request).expect("page exports");

    let svg = String::from_utf8(files[0].bytes.clone()).expect("SVG is UTF-8");
    assert!(
        svg.contains(&format!(
            "viewBox=\"0 0 {} {}\"",
            raster.width(),
            raster.height()
        )),
        "{svg:.160}"
    );
}

/// A page that cannot be produced aborts the export naming that page and
/// hands back nothing, so a caller has no half-written set of files to clean
/// up.
#[test]
fn a_page_that_cannot_be_rendered_aborts_the_whole_export_by_page_number() {
    let mut doc = open("two-page.pdf");
    let request = ExportRequest {
        pages: PageRange::whole(doc.page_count()).expect("the seed has pages"),
        // Past what hayro can address on one axis, so every page fails.
        dpi: 72.0 * 5_000.0,
    };

    let failure = PngCodec
        .export(&mut doc, &request)
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
        };
        assert!(
            matches!(
                PngCodec.export(&mut doc, &request),
                Err(ExportError::InvalidDpi(_))
            ),
            "{dpi} was accepted"
        );
    }
}
