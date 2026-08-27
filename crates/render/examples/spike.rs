//! M1(b) evidence: composite each given PDF's first page with a highlight and
//! an ink stroke over it, write the result as a PNG, and time the pipeline
//! against decision 11's 200 ms time-to-first-page budget.
//!
//! ```text
//! cargo run --release -p onionskin-render --example spike -- <out-dir> <pdf>...
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use onionskin_render::{
    BaseRaster, DeviceRect, Document, InterpreterWarning, Overlay, RenderError, Rgba, TileCache,
};

/// Acrobat's highlighter yellow, at the alpha a multiply blend needs to keep
/// text legible.
const HIGHLIGHT: Rgba = Rgba {
    r: 255,
    g: 222,
    b: 23,
    a: 210,
};
const INK: Rgba = Rgba {
    r: 214,
    g: 33,
    b: 33,
    a: 255,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let out_dir = PathBuf::from(args.next().ok_or("usage: spike <out-dir> <pdf>...")?);
    let inputs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if inputs.is_empty() {
        return Err("usage: spike <out-dir> <pdf>...".into());
    }
    std::fs::create_dir_all(&out_dir)?;

    for input in &inputs {
        match run(input, &out_dir) {
            Ok(()) => {}
            Err(e) => println!("{}: FAILED: {e}", name(input)),
        }
    }
    Ok(())
}

fn run(input: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let read_start = Instant::now();
    let bytes = std::fs::read(input)?;
    let read = read_start.elapsed();
    let size_mb = bytes.len() as f64 / 1_048_576.0;

    let open_start = Instant::now();
    let doc = Document::open(bytes)?;
    let open = open_start.elapsed();
    let pages = doc.page_count();

    let (page_1x, render_1x) = time(|| doc.render_page(0, 1.0))?;
    let (page_2x, render_2x) = time(|| doc.render_page(0, 2.0))?;

    let at_1x = compose(page_1x.raster, out_dir.join(format!("{}.png", name(input))))?;
    let at_2x = compose(
        page_2x.raster,
        out_dir.join(format!("{}@2x.png", name(input))),
    )?;

    println!(
        "{name}\n  \
         {pages} pages, {size_mb:.2} MiB, page 1 = {w}x{h} px at 1x\n  \
         read {read}  open {open}  render 1x {r1}  render 2x {r2}\n  \
         time-to-first-page (read + open + render 1x) {ttfp}\n  \
         1x: {t1} tiles composite {warm1}, one damaged tile {tile1}\n  \
         2x: {t2} tiles composite {warm2}, one damaged tile {tile2}\n  \
         interpreter warnings: {warnings}\n  \
         {png1}\n  {png2}",
        name = name(input),
        w = at_1x.width,
        h = at_1x.height,
        read = ms(read),
        open = ms(open),
        r1 = ms(render_1x),
        r2 = ms(render_2x),
        ttfp = ms(read + open + render_1x),
        t1 = at_1x.tiles,
        warm1 = ms(at_1x.warm),
        tile1 = ms(at_1x.one_tile),
        t2 = at_2x.tiles,
        warm2 = ms(at_2x.warm),
        tile2 = ms(at_2x.one_tile),
        warnings = summarize(&page_1x.warnings, &page_2x.warnings),
        png1 = at_1x.png.display(),
        png2 = at_2x.png.display(),
    );
    Ok(())
}

struct Composite {
    width: u32,
    height: u32,
    tiles: u32,
    warm: Duration,
    one_tile: Duration,
    png: PathBuf,
}

fn compose(base: BaseRaster, png: PathBuf) -> Result<Composite, Box<dyn std::error::Error>> {
    let (width, height, zoom) = (base.width(), base.height(), base.zoom());
    let mut cache = TileCache::new(base);
    // Overlays are in page points, so the same coordinates serve every zoom.
    for overlay in overlays(width as f32 / zoom, height as f32 / zoom) {
        cache.add_overlay(overlay);
    }

    let warm_start = Instant::now();
    let page = cache.page_image();
    let warm = warm_start.elapsed();

    // What an in-progress ink stroke actually costs: damage one tile, take it.
    cache.damage(DeviceRect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    });
    let one_tile_start = Instant::now();
    let _ = cache.tile(0, 0);
    let one_tile = one_tile_start.elapsed();

    std::fs::write(&png, page.encode_png()?)?;
    Ok(Composite {
        width,
        height,
        tiles: cache.cols() * cache.rows(),
        warm,
        one_tile,
        png,
    })
}

/// A highlight over a band of body text and an ink squiggle below it, in page
/// points, placed as fractions of the page so one set of coordinates suits
/// every document.
fn overlays(w: f32, h: f32) -> Vec<Overlay> {
    let (x0, x1) = (0.12 * w, 0.72 * w);
    let (y0, y1) = (0.16 * h, 0.20 * h);

    let ink = (0..=48)
        .map(|i| {
            let t = i as f32 / 48.0;
            let x = 0.15 * w + t * 0.7 * w;
            let y = 0.45 * h + (t * std::f32::consts::TAU * 2.0).sin() * 0.04 * h;
            (x, y)
        })
        .collect();

    vec![
        Overlay::Highlight {
            corners: [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
            color: HIGHLIGHT,
        },
        Overlay::Ink {
            points: ink,
            color: INK,
            width: 0.004 * w.max(h),
        },
    ]
}

fn summarize(one_x: &[InterpreterWarning], two_x: &[InterpreterWarning]) -> String {
    let mut fonts = 0;
    let mut images = 0;
    let mut appearances = 0;
    for warning in one_x.iter().chain(two_x) {
        match warning {
            InterpreterWarning::UnsupportedFont => fonts += 1,
            InterpreterWarning::ImageDecodeFailure => images += 1,
            InterpreterWarning::UnresolvedAnnotationAppearance => appearances += 1,
        }
    }
    if fonts == 0 && images == 0 && appearances == 0 {
        return "none".into();
    }
    format!(
        "{fonts} unsupported fonts, {images} image decode failures, \
         {appearances} unresolved annotation appearances (1x and 2x combined)"
    )
}

fn time<T>(f: impl FnOnce() -> Result<T, RenderError>) -> Result<(T, Duration), RenderError> {
    let start = Instant::now();
    let value = f()?;
    Ok((value, start.elapsed()))
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

fn name(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
