//! The Windows backend's pictures of the sheets, on any platform: what the
//! file backend wrote, rendered one sheet at a time as GDI will draw it.

use std::sync::Arc;

use onionskin_corpus_testing::seed;
use onionskin_print::backend::windows::raster::{SheetImage, Sheets};
use onionskin_print::{impose, FileBackend, NUp, PrintBackend, PrintJob};

/// The file backend's output for `two-page.pdf` printed two-up, and the
/// size in points of the one sheet imposition made of it.
fn two_up_sheets() -> (Vec<u8>, (f64, f64)) {
    let bytes = std::fs::read(seed("two-page.pdf")).expect("seed");
    let mut backend = FileBackend::new(Arc::new(bytes)).expect("opens");
    let job = PrintJob {
        n_up: NUp {
            per_sheet: 2,
            ..NUp::default()
        },
        ..PrintJob::default()
    };
    let sheets = impose(&job, &backend.page_sizes().expect("sizes")).expect("imposes");
    assert_eq!(sheets.len(), 1, "two pages two-up are one sheet");
    backend.print(&job, &sheets).expect("prints");
    let size = (sheets[0].width, sheets[0].height);
    (backend.output().expect("written").to_vec(), size)
}

/// Whether any pixel in the rectangle is darker than paper.
fn inked(image: &SheetImage, columns: std::ops::Range<u32>, rows: std::ops::Range<u32>) -> bool {
    rows.into_iter().any(|y| {
        columns.clone().any(|x| {
            let at = ((y * image.width + x) * 4) as usize;
            image.bgra[at..at + 3].iter().any(|&channel| channel < 128)
        })
    })
}

#[test]
fn each_sheet_is_rendered_at_the_asked_resolution_with_both_pages_on_it() {
    let (pdf, (width, height)) = two_up_sheets();
    let mut sheets = Sheets::new(pdf, 72).expect("the sheets open");
    assert_eq!(sheets.sheet_count(), 1);

    let sheet = sheets.next().expect("a sheet").expect("renders");
    // At 72 dpi a point is a pixel.
    assert_eq!(
        (sheet.width, sheet.height),
        (width.round() as u32, height.round() as u32)
    );
    assert_eq!(sheet.points, (width, height));
    assert_eq!(sheet.bgra.len(), (sheet.width * sheet.height * 4) as usize);
    assert!(
        sheet.bgra.as_chunks::<4>().0.iter().all(|px| px[3] == 255),
        "opaque"
    );
    // Two cells split the sheet along its long side; each has a page's ink.
    let (w, h) = (sheet.width, sheet.height);
    let halves = if h > w {
        [(0..w, 0..h / 2), (0..w, h / 2..h)]
    } else {
        [(0..w / 2, 0..h), (w / 2..w, 0..h)]
    };
    for (columns, rows) in halves {
        assert!(
            inked(&sheet, columns.clone(), rows.clone()),
            "no page in {columns:?} x {rows:?}"
        );
    }
    assert!(sheets.next().is_none());
}

#[test]
fn the_resolution_scales_the_picture_not_the_sheet() {
    let (pdf, (width, height)) = two_up_sheets();
    let sheet = Sheets::new(pdf, 144)
        .expect("opens")
        .next()
        .expect("a sheet")
        .expect("renders");
    assert_eq!(
        (sheet.width, sheet.height),
        ((width * 2.0).round() as u32, (height * 2.0).round() as u32)
    );
    assert_eq!(sheet.points, (width, height), "the paper is the same size");
}

#[test]
fn bytes_that_are_not_a_pdf_are_refused() {
    assert!(Sheets::new(b"not a pdf".to_vec(), 72).is_err());
}
