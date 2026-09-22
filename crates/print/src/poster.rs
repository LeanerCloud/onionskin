//! Poster printing: each page enlarged by the tile scale and split across
//! as many sheets as it takes, neighbouring tiles repeating an overlap so
//! the printed sheets can be trimmed and glued back into one picture.
//!
//! Every tile is one sheet. Its page is placed larger than the sheet and
//! clipped to the tile's area; with cut marks on, the area is inset by a
//! margin that the marks print in. Tiles run left to right, top to bottom,
//! page by page. The only placement in the print model that clips.

use crate::impose::PageSize;
use crate::job::{Orientation, Poster, PrintJob};
use crate::sheet::{Placement, Sheet};

/// The margin cut marks print in, in points.
pub const CUT_MARGIN: f64 = 18.0;

/// The sheets `job` prints as posters from a document whose pages are
/// `pages`.
pub fn impose_poster(job: &PrintJob, poster: Poster, pages: &[PageSize]) -> Vec<Sheet> {
    let scale = f64::from(poster.scale.max(1)) / 100.0;
    job.selection
        .pages(pages.len())
        .into_iter()
        .filter(|page| *page < pages.len())
        .flat_map(|page| tiles(job, poster, scale, page, pages[page]))
        .collect()
}

/// The tiles of one page.
fn tiles(job: &PrintJob, poster: Poster, scale: f64, page: usize, size: PageSize) -> Vec<Sheet> {
    let (width, height) = paper(job, size);
    let margin = if poster.cut_marks { CUT_MARGIN } else { 0.0 };
    let area = (width - 2.0 * margin, height - 2.0 * margin);
    let overlap = poster.overlap.clamp(0.0, area.0.min(area.1) / 2.0);
    let step = (area.0 - overlap, area.1 - overlap);
    let scaled = (size.0 * scale, size.1 * scale);
    let columns = count(scaled.0, area.0, step.0);
    let rows = count(scaled.1, area.1, step.1);

    let mut sheets = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            // The page's top-left corner, relative to the tile area's.
            let left = margin - column as f64 * step.0;
            let top = margin + area.1 + row as f64 * step.1;
            let clip = [margin, margin, margin + area.0, margin + area.1];
            sheets.push(Sheet {
                width,
                height,
                placements: vec![Placement {
                    source: page,
                    transform: [scale, 0.0, 0.0, scale, left, top - scaled.1],
                    clip: Some(clip),
                }],
                frames: if poster.cut_marks {
                    vec![clip]
                } else {
                    Vec::new()
                },
            });
        }
    }
    sheets
}

/// How many tiles of `area`, each `step` after the last, cover `length`.
fn count(length: f64, area: f64, step: f64) -> usize {
    if length <= area || step <= 0.0 {
        return 1;
    }
    1 + ((length - area) / step).ceil() as usize
}

/// The sheet: the job's paper, turned for Auto to the page's shape.
fn paper(job: &PrintJob, size: PageSize) -> (f64, f64) {
    let (short, long) = (
        job.paper.width.min(job.paper.height),
        job.paper.width.max(job.paper.height),
    );
    let landscape = match job.orientation {
        Orientation::Portrait => false,
        Orientation::Landscape => true,
        Orientation::Auto => size.0 > size.1,
    };
    if landscape {
        (long, short)
    } else {
        (short, long)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{Handling, PaperSize};

    const LETTER_PAGE: PageSize = (612.0, 792.0);

    fn job(poster: Poster) -> PrintJob {
        PrintJob {
            paper: PaperSize::LETTER,
            handling: Handling::Poster(poster),
            ..PrintJob::default()
        }
    }

    fn poster(scale: u16, overlap: f64, cut_marks: bool) -> Poster {
        Poster {
            scale,
            overlap,
            cut_marks,
        }
    }

    #[test]
    fn at_full_size_with_no_margin_a_page_is_one_tile() {
        let options = poster(100, 0.0, false);
        let sheets = impose_poster(&job(options), options, &[LETTER_PAGE]);
        assert_eq!(sheets.len(), 1);
        assert_eq!(
            sheets[0].placements[0].transform,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        );
        assert!(sheets[0].frames.is_empty());
    }

    /// Doubled, a Letter page is 1224 by 1584 points. With no overlap and no
    /// margin that is exactly two tiles by two, and together the tiles show
    /// every part of the page once.
    #[test]
    fn doubling_a_page_makes_four_tiles_that_cover_it_exactly() {
        let options = poster(200, 0.0, false);
        let sheets = impose_poster(&job(options), options, &[LETTER_PAGE]);
        assert_eq!(sheets.len(), 4);
        // Which part of the page (in page points) each tile shows.
        let shown: Vec<[f64; 4]> = sheets
            .iter()
            .map(|sheet| {
                let placement = sheet.placements[0];
                let [a, _, _, d, e, f] = placement.transform;
                let [x0, y0, x1, y1] = placement.clip.expect("a tile clips");
                [(x0 - e) / a, (y0 - f) / d, (x1 - e) / a, (y1 - f) / d]
            })
            .collect();
        assert_eq!(
            shown,
            [
                [0.0, 396.0, 306.0, 792.0],
                [306.0, 396.0, 612.0, 792.0],
                [0.0, 0.0, 306.0, 396.0],
                [306.0, 0.0, 612.0, 396.0],
            ],
            "top row first, left to right"
        );
    }

    /// Overlap and cut marks: the tile area shrinks by the margin, each tile
    /// starts `area - overlap` after the last, and the marks frame the area.
    #[test]
    fn overlap_and_cut_marks_add_tiles_and_frame_each_one() {
        let options = poster(200, 36.0, true);
        let sheets = impose_poster(&job(options), options, &[LETTER_PAGE]);
        // Area 576 by 756, step 540 by 720: 1224 needs 3 columns, 1584 needs
        // 3 rows.
        assert_eq!(sheets.len(), 9);
        let area = [
            CUT_MARGIN,
            CUT_MARGIN,
            612.0 - CUT_MARGIN,
            792.0 - CUT_MARGIN,
        ];
        assert!(sheets.iter().all(|sheet| sheet.frames == [area]));
        let second = sheets[1].placements[0].transform;
        let first = sheets[0].placements[0].transform;
        assert!(((first[4] - second[4]) - 540.0).abs() < 1e-9);
    }

    #[test]
    fn a_landscape_page_prints_on_landscape_tiles_and_pages_follow_one_another() {
        let options = poster(100, 0.0, false);
        let sheets = impose_poster(&job(options), options, &[(792.0, 612.0), LETTER_PAGE]);
        assert_eq!(sheets.len(), 2);
        assert!(sheets[0].is_landscape());
        assert!(!sheets[1].is_landscape());
        assert_eq!(sheets[1].placements[0].source, 1);
        assert_eq!(count(10.0, 5.0, 0.0), 1, "a degenerate step is one tile");
    }
}
