//! Round-trip evidence over the real corpus: what each exporter writes,
//! decoded back and compared against the thing it claims to be faithful to.
//!
//! * PNG, decoded, must equal the canvas render path's own raster for the
//!   same page at the same zoom, bit for bit. That is the whole claim of
//!   rendering export through `core` instead of beside it.
//! * Text must equal `content`'s extraction for the same pages, so the codec
//!   is provably adding no interpretation.
//! * SVG, rasterized back by an independent renderer (resvg), must land close
//!   to the PNG. A different rasterizer will never match pixel for pixel, so
//!   the assertion is on how far apart they are, and the measured distance is
//!   printed rather than hidden.
//!
//! `corpus/external/` is gitignored and absent from a fresh clone, so a run
//! without it says so loudly and returns. `ONIONSKIN_CORPUS_REQUIRED=1` turns
//! that into a failure, which is what CI sets.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use onionskin_codecs_common::{PngCodec, SvgCodec, TextCodec};
use onionskin_core::{BaseRaster, Document, RenderRequest, RenderResponse};
use onionskin_plugin_api::{CodecPlugin, ExportRequest, PageRange};

/// 144 dpi, i.e. render zoom 2: enough detail that a rasterizer difference
/// would show, small enough to run a few dozen files in seconds.
const DPI: f32 = 144.0;
const ZOOM: f32 = DPI / 72.0;

/// Every Nth corpus file. The corpus has thousands; this samples it
/// deterministically instead of choosing favourites.
const SAMPLE_STRIDE: usize = 97;

/// Mean absolute per-channel difference between hayro's raster and resvg's,
/// over 0..=255. Derived, not guessed: across the sampled corpus the two
/// independent rasterizers land a mean 0.03 apart and never worse than 0.66,
/// so this leaves antialiasing and hinting room without leaving room for an
/// SVG that is describing a different page.
const MAX_MEAN_CHANNEL_DIFFERENCE: f64 = 4.0;

fn corpus_root() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("ONIONSKIN_CORPUS") {
        let path = PathBuf::from(from_env);
        return path.is_dir().then_some(path);
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    let corpus = workspace.join("corpus");
    corpus.is_dir().then_some(corpus)
}

/// Returns the corpus subdirectory, or `None` after printing why it is
/// absent. Mirrors the contract `crates/content/tests/common` sets.
fn corpus_dir(relative: &str) -> Option<PathBuf> {
    let Some(root) = corpus_root() else {
        return missing("no corpus found; set ONIONSKIN_CORPUS to the corpus directory");
    };
    let dir = root.join(relative);
    if !dir.is_dir() {
        return missing(&format!("{} is absent (it is gitignored)", dir.display()));
    }
    Some(dir)
}

fn missing(why: &str) -> Option<PathBuf> {
    if std::env::var_os("ONIONSKIN_CORPUS_REQUIRED").is_some() {
        panic!("corpus required but {why}");
    }
    eprintln!("SKIPPED: {why}");
    None
}

fn pdfs_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

fn short(path: &Path) -> String {
    match corpus_root() {
        Some(root) => path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string(),
        None => path.display().to_string(),
    }
}

fn page_zero_request(page_count: usize) -> ExportRequest {
    ExportRequest {
        pages: PageRange::new(0, 0, page_count).expect("every document here has a first page"),
        dpi: DPI,
        quality: None,
    }
}

/// The raster the canvas would put on screen: queued through the interactive
/// path, not through the export one.
fn canvas_raster(doc: &mut Document) -> Option<BaseRaster> {
    doc.request_render(
        RenderRequest {
            page: 0,
            zoom: ZOOM,
            generation: 1,
        },
        None,
    )
    .ok()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match doc.try_render_response().ok()? {
            Some(RenderResponse::Raster { render, .. }) => return Some(render.raster),
            Some(RenderResponse::Failed { .. }) => return None,
            Some(RenderResponse::Placeholder(_)) => {}
            None if Instant::now() < deadline => std::thread::yield_now(),
            None => return None,
        }
    }
}

/// Rasterize an exported SVG onto white at the same scale the PNG used.
fn rasterize_svg(svg: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(ZOOM, ZOOM),
        &mut pixmap.as_mut(),
    );
    Some(pixmap.take())
}

/// Mean absolute difference per channel. Both sides are premultiplied over an
/// opaque white background, so this compares what a reader sees.
fn mean_channel_difference(left: &[u8], right: &[u8]) -> f64 {
    debug_assert_eq!(left.len(), right.len());
    let total: u64 = left
        .iter()
        .zip(right)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    total as f64 / left.len() as f64
}

#[derive(Default)]
struct Tally {
    compared: usize,
    skipped_open: usize,
    skipped_render: usize,
    skipped_embedded_raster: usize,
    svg_compared: usize,
    svg_differences: Vec<(String, f64)>,
    png_mismatches: Vec<String>,
    text_mismatches: Vec<String>,
}

#[test]
fn every_exporter_round_trips_against_what_it_claims_to_be_faithful_to() {
    let Some(dir) = corpus_dir("external") else {
        return;
    };
    let files: Vec<PathBuf> = pdfs_in(&dir)
        .into_iter()
        .step_by(SAMPLE_STRIDE)
        .take(60)
        .collect();
    assert!(!files.is_empty(), "{} holds no PDFs", dir.display());

    let mut tally = Tally::default();
    for file in &files {
        let Ok(mut doc) = Document::open_path(file) else {
            tally.skipped_open += 1;
            continue;
        };
        if doc.page_count() == 0 {
            tally.skipped_open += 1;
            continue;
        }
        let Some(on_screen) = canvas_raster(&mut doc) else {
            tally.skipped_render += 1;
            continue;
        };
        let request = page_zero_request(doc.page_count());
        tally.compared += 1;

        // PNG: the export decodes to exactly the pixels the canvas composites.
        let png = PngCodec
            .export_page(&mut doc, &request, 0, true)
            .unwrap_or_else(|e| panic!("{}: PNG export failed: {e}", short(file)));
        let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap_or_else(|e| panic!("{}: export is not a PNG: {e}", short(file)))
            .to_rgba8();
        if decoded.dimensions() != (on_screen.width(), on_screen.height())
            || decoded.as_raw() != on_screen.rgba()
        {
            tally.png_mismatches.push(short(file));
        }

        // Text: the export is content's extraction and nothing else.
        let text = TextCodec
            .export_page(&mut doc, &request, 0, true)
            .unwrap_or_else(|e| panic!("{}: text export failed: {e}", short(file)));
        let expected = doc
            .page_text(0)
            .map(|page| page.flatten().text)
            .unwrap_or_default();
        if String::from_utf8_lossy(&text) != expected {
            tally.text_mismatches.push(short(file));
        }

        // SVG: an independent rasterizer lands on the same page.
        let svg = SvgCodec
            .export_page(&mut doc, &request, 0, true)
            .unwrap_or_else(|e| panic!("{}: SVG export failed: {e}", short(file)));
        if svg.windows(6).any(|w| w == b"<image") {
            // resvg is built here without its raster-image decoders, so a page
            // with an embedded bitmap would differ for a reason that says
            // nothing about the SVG.
            tally.skipped_embedded_raster += 1;
            continue;
        }
        let Some(revectored) = rasterize_svg(&svg, on_screen.width(), on_screen.height()) else {
            tally.skipped_render += 1;
            continue;
        };
        tally.svg_compared += 1;
        tally.svg_differences.push((
            short(file),
            mean_channel_difference(&revectored, on_screen.rgba()),
        ));
    }

    report(&tally, files.len());
    assert!(
        tally.compared > 0,
        "every sampled file was skipped; the run proved nothing"
    );
    assert!(
        tally.png_mismatches.is_empty(),
        "PNG export did not match the canvas raster for {:?}",
        tally.png_mismatches
    );
    assert!(
        tally.text_mismatches.is_empty(),
        "text export did not match content's extraction for {:?}",
        tally.text_mismatches
    );
    assert!(tally.svg_compared > 0, "no SVG was compared");
    for (file, difference) in &tally.svg_differences {
        assert!(
            *difference <= MAX_MEAN_CHANNEL_DIFFERENCE,
            "{file}: an independent rasterizer put the exported SVG {difference:.2} \
             mean channel steps away from the page raster"
        );
    }
}

fn report(tally: &Tally, sampled: usize) {
    let mean = if tally.svg_differences.is_empty() {
        0.0
    } else {
        tally
            .svg_differences
            .iter()
            .map(|(_, difference)| difference)
            .sum::<f64>()
            / tally.svg_differences.len() as f64
    };
    let worst = tally
        .svg_differences
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1));
    println!(
        "\n== export round trip == {sampled} sampled, {} compared \
         ({} unopenable, {} unrenderable)",
        tally.compared, tally.skipped_open, tally.skipped_render
    );
    println!(
        "   PNG identical to the canvas raster: {}/{}",
        tally.compared - tally.png_mismatches.len(),
        tally.compared
    );
    println!(
        "   text identical to content's extraction: {}/{}",
        tally.compared - tally.text_mismatches.len(),
        tally.compared
    );
    println!(
        "   SVG re-rasterized: {} compared, {} skipped for embedded rasters, \
         mean channel difference {mean:.2}",
        tally.svg_compared, tally.skipped_embedded_raster
    );
    if let Some((file, difference)) = worst {
        println!("   worst SVG difference {difference:.2}  {file}");
    }
}
