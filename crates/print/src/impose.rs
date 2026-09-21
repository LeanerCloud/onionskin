//! Imposition: which page goes on which sheet, where, and how big. Pure
//! arithmetic over page sizes, with no document, renderer or printer, so
//! every rule is asserted by reading the sheets it produces.
//!
//! **Fit fits to the paper**, not to a printer's printable area: the file
//! backend has no hardware margins, and a platform backend that has them
//! passes a smaller paper size rather than this module guessing.

use crate::job::{NUpOrder, Orientation, PrintJob, Sizing};
use crate::sheet::{Placement, Sheet};

/// A page as displayed, `/Rotate` applied: its width and height in points.
pub type PageSize = (f64, f64);

/// The sheets `job` prints from a document whose pages are `pages`.
pub fn impose(job: &PrintJob, pages: &[PageSize]) -> Vec<Sheet> {
    let selected: Vec<usize> = job
        .selection
        .pages(pages.len())
        .into_iter()
        .filter(|page| *page < pages.len())
        .collect();
    let first = selected.first().map_or((1.0, 1.0), |page| pages[*page]);
    let landscape = landscape(job, first);
    let (width, height) = if landscape {
        (job.paper.height, job.paper.width)
    } else {
        (job.paper.width, job.paper.height)
    };
    let (mut columns, mut rows) = job.n_up.grid();
    if landscape {
        std::mem::swap(&mut columns, &mut rows);
    }
    let per_sheet = columns * rows;
    let cell = (width / columns as f64, height / rows as f64);

    let mut sheets: Vec<Sheet> = selected
        .chunks(per_sheet)
        .map(|chunk| {
            let mut sheet = Sheet {
                width,
                height,
                placements: Vec::with_capacity(chunk.len()),
                frames: Vec::new(),
            };
            for (slot, page) in chunk.iter().enumerate() {
                let (column, row) = cell_of(slot, columns, rows, job.n_up.order);
                // Rows count down from the top of the sheet.
                let origin = (column as f64 * cell.0, height - (row as f64 + 1.0) * cell.1);
                sheet.placements.push(Placement {
                    source: *page,
                    transform: place(pages[*page], origin, cell, job.sizing),
                });
                if job.n_up.borders && per_sheet > 1 {
                    let footprint = sheet
                        .placements
                        .last()
                        .expect("pushed")
                        .footprint(pages[*page].0, pages[*page].1);
                    sheet.frames.push(footprint);
                }
            }
            sheet
        })
        .collect();
    if job.duplex && sheets.len() % 2 == 1 {
        sheets.push(Sheet {
            width,
            height,
            placements: Vec::new(),
            frames: Vec::new(),
        });
    }
    sheets
}

/// Whether the sheet is landscape: as asked, or for Auto whichever makes a
/// cell the shape of the first page.
fn landscape(job: &PrintJob, first: PageSize) -> bool {
    match job.orientation {
        Orientation::Portrait => false,
        Orientation::Landscape => true,
        Orientation::Auto => {
            let (columns, rows) = job.n_up.grid();
            let page_wide = first.0 > first.1;
            // A portrait cell is paper-width / columns by paper-height / rows.
            let portrait_cell_wide =
                job.paper.width / columns as f64 > job.paper.height / rows as f64;
            page_wide != portrait_cell_wide
        }
    }
}

/// The column and row of the `slot`-th page on a sheet.
fn cell_of(slot: usize, columns: usize, rows: usize, order: NUpOrder) -> (usize, usize) {
    match order {
        NUpOrder::Horizontal => (slot % columns, slot / columns),
        NUpOrder::HorizontalReversed => (columns - 1 - slot % columns, slot / columns),
        NUpOrder::Vertical => (slot / rows, slot % rows),
        NUpOrder::VerticalReversed => (columns - 1 - slot / rows, slot % rows),
    }
}

/// The transform putting a page of `size` in the cell at `origin`, scaled by
/// `sizing` and centred.
fn place(size: PageSize, origin: (f64, f64), cell: (f64, f64), sizing: Sizing) -> [f64; 6] {
    let fit = (cell.0 / size.0).min(cell.1 / size.1);
    let scale = match sizing {
        Sizing::Fit => fit,
        Sizing::ActualSize => 1.0,
        Sizing::ShrinkOversized => fit.min(1.0),
        Sizing::Custom(percent) => f64::from(percent) / 100.0,
    };
    let (width, height) = (size.0 * scale, size.1 * scale);
    [
        scale,
        0.0,
        0.0,
        scale,
        origin.0 + (cell.0 - width) / 2.0,
        origin.1 + (cell.1 - height) / 2.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{NUp, PageSelection, PaperSize};

    const LETTER_PAGE: PageSize = (612.0, 792.0);

    fn job() -> PrintJob {
        PrintJob {
            paper: PaperSize::LETTER,
            orientation: Orientation::Portrait,
            ..PrintJob::default()
        }
    }

    #[test]
    fn one_page_fits_its_sheet_exactly() {
        let sheets = impose(&job(), &[LETTER_PAGE]);
        assert_eq!(sheets.len(), 1);
        assert_eq!(
            sheets[0].placements[0].transform,
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn four_up_on_a_landscape_sheet_places_pages_left_to_right_then_down() {
        let job = PrintJob {
            orientation: Orientation::Landscape,
            n_up: NUp {
                per_sheet: 4,
                ..NUp::default()
            },
            ..job()
        };
        let sheets = impose(&job, &[LETTER_PAGE; 4]);
        assert_eq!(sheets.len(), 1);
        let sheet = &sheets[0];
        assert!(sheet.is_landscape());
        let centres: Vec<(f64, f64)> = sheet
            .placements
            .iter()
            .map(|placement| {
                let [x0, y0, x1, y1] = placement.footprint(LETTER_PAGE.0, LETTER_PAGE.1);
                ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
            })
            .collect();
        assert!(centres[0].0 < centres[1].0, "first row, left to right");
        assert_eq!(centres[0].1, centres[1].1);
        assert!(centres[2].1 < centres[0].1, "then down");
        assert!(centres[2].0 < centres[3].0);
        assert_eq!(
            sheet
                .placements
                .iter()
                .map(|p| p.source)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn vertical_order_goes_down_the_first_column_first() {
        let job = PrintJob {
            n_up: NUp {
                per_sheet: 4,
                order: NUpOrder::Vertical,
                borders: false,
            },
            ..job()
        };
        let sheet = &impose(&job, &[LETTER_PAGE; 4])[0];
        let footprint = |slot: usize| sheet.placements[slot].footprint(612.0, 792.0);
        assert_eq!(footprint(0)[0], footprint(1)[0], "same column");
        assert!(footprint(1)[1] < footprint(0)[1], "second below the first");
        assert!(footprint(2)[0] > footprint(0)[0], "then the next column");
    }

    #[test]
    fn shrink_oversized_leaves_a_page_that_fits_at_actual_size() {
        let job = PrintJob {
            sizing: Sizing::ShrinkOversized,
            ..job()
        };
        let small = impose(&job, &[(300.0, 400.0)]);
        assert_eq!(
            small[0].placements[0].transform[0], 1.0,
            "fits: actual size"
        );
        let big = impose(&job, &[(1224.0, 1584.0)]);
        assert_eq!(
            big[0].placements[0].transform[0], 0.5,
            "tabloid on letter: halved"
        );
    }

    #[test]
    fn custom_scale_of_fifty_percent_halves_both_axes() {
        let job = PrintJob {
            sizing: Sizing::Custom(50),
            ..job()
        };
        let [a, b, c, d, e, f] = impose(&job, &[LETTER_PAGE])[0].placements[0].transform;
        assert_eq!((a, b, c, d), (0.5, 0.0, 0.0, 0.5));
        assert_eq!((e, f), (153.0, 198.0), "centred");
    }

    #[test]
    fn an_odd_page_count_in_duplex_leaves_the_last_back_blank() {
        let job = PrintJob {
            duplex: true,
            ..job()
        };
        let sheets = impose(&job, &[LETTER_PAGE; 3]);
        assert_eq!(sheets.len(), 4);
        assert!(sheets[3].placements.is_empty());
        assert_eq!(impose(&job, &[LETTER_PAGE; 2]).len(), 2);
    }

    #[test]
    fn auto_orientation_turns_the_sheet_for_a_wide_page_and_for_two_up() {
        let auto = PrintJob {
            orientation: Orientation::Auto,
            ..job()
        };
        assert!(!impose(&auto, &[LETTER_PAGE])[0].is_landscape());
        assert!(impose(&auto, &[(792.0, 612.0)])[0].is_landscape());
        let two_up = PrintJob {
            n_up: NUp {
                per_sheet: 2,
                ..NUp::default()
            },
            ..auto
        };
        assert!(
            impose(&two_up, &[LETTER_PAGE; 2])[0].is_landscape(),
            "two portrait pages side by side"
        );
    }

    #[test]
    fn borders_frame_each_placed_page_only_when_several_share_a_sheet() {
        let bordered = PrintJob {
            n_up: NUp {
                per_sheet: 2,
                borders: true,
                ..NUp::default()
            },
            ..job()
        };
        let sheet = &impose(&bordered, &[LETTER_PAGE; 2])[0];
        assert_eq!(sheet.frames.len(), 2);
        assert_eq!(sheet.frames[0], sheet.placements[0].footprint(612.0, 792.0));
        let single = PrintJob {
            n_up: NUp {
                borders: true,
                ..NUp::default()
            },
            ..job()
        };
        assert!(impose(&single, &[LETTER_PAGE])[0].frames.is_empty());
    }

    #[test]
    fn a_selection_decides_which_pages_reach_the_sheets() {
        let job = PrintJob {
            selection: PageSelection {
                range: Some((1, 2)),
                ..PageSelection::default()
            },
            ..job()
        };
        let sources: Vec<usize> = impose(&job, &[LETTER_PAGE; 5])
            .iter()
            .flat_map(|sheet| sheet.placements.iter().map(|p| p.source))
            .collect();
        assert_eq!(sources, [1, 2]);
    }

    /// Booklet and poster (M4) will place pages that are not a uniform grid;
    /// the sheet model already holds one, and its footprints are what the
    /// transforms say.
    #[test]
    fn a_hand_built_irregular_sheet_is_expressible() {
        let sheet = Sheet {
            width: 792.0,
            height: 612.0,
            placements: vec![
                Placement {
                    source: 3,
                    transform: [0.5, 0.0, 0.0, 0.5, 0.0, 0.0],
                },
                Placement {
                    source: 0,
                    transform: [0.0, 0.5, -0.5, 0.0, 792.0, 0.0],
                },
            ],
            frames: Vec::new(),
        };
        assert_eq!(
            sheet.placements[0].footprint(612.0, 792.0),
            [0.0, 0.0, 306.0, 396.0]
        );
        assert_eq!(
            sheet.placements[1].footprint(612.0, 792.0),
            [396.0, 0.0, 792.0, 306.0],
            "a quarter turn"
        );
    }
}
