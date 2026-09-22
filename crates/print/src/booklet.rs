//! Booklet printing: two pages to a side of a landscape sheet, in the order
//! that, printed on both sides, folded in half and stapled on the fold,
//! reads front to back.
//!
//! For `n` pages padded with blanks to `4k`, sheet `i` carries on its front
//! pages `4k - 1 - 2i` (outside) and `2i` (inside), and on its back `2i + 1`
//! and `4k - 2 - 2i`. That is the saddle-stitch order Acrobat's Booklet
//! prints. A right binding mirrors each side, for right-to-left documents.
//!
//! The sides come out front, back, front, back, which a printer set to flip
//! on the short edge prints as sheets. "Front side only" and "back side
//! only" print one kind, for a printer that cannot print both sides.

use crate::impose::{place, PageSize};
use crate::job::{Binding, Booklet, BookletSides, PrintJob};
use crate::sheet::{Placement, Sheet};

/// The sides `job` prints as a booklet from a document whose pages are
/// `pages`.
pub fn impose_booklet(job: &PrintJob, booklet: Booklet, pages: &[PageSize]) -> Vec<Sheet> {
    let selected: Vec<usize> = job
        .selection
        .pages(pages.len())
        .into_iter()
        .filter(|page| *page < pages.len())
        .collect();
    if selected.is_empty() {
        return Vec::new();
    }
    // A booklet side is always landscape: two portrait halves.
    let (width, height) = (
        job.paper.width.max(job.paper.height),
        job.paper.width.min(job.paper.height),
    );
    let half = (width / 2.0, height);
    let padded = selected.len().div_ceil(4) * 4;
    let slot = |index: usize| selected.get(index).copied();

    let mut sides = Vec::new();
    for sheet in 0..padded / 4 {
        let front = (padded - 1 - 2 * sheet, 2 * sheet);
        let back = (2 * sheet + 1, padded - 2 - 2 * sheet);
        let wanted = match booklet.sides {
            BookletSides::BothSides => [Some(front), Some(back)],
            BookletSides::FrontSideOnly => [Some(front), None],
            BookletSides::BackSideOnly => [None, Some(back)],
        };
        for (left, right) in wanted.into_iter().flatten() {
            let (left, right) = match booklet.binding {
                Binding::Left => (left, right),
                Binding::Right => (right, left),
            };
            let mut side = Sheet {
                width,
                height,
                placements: Vec::new(),
                frames: Vec::new(),
            };
            for (index, origin) in [(left, (0.0, 0.0)), (right, (half.0, 0.0))] {
                if let Some(page) = slot(index) {
                    side.placements.push(Placement {
                        source: page,
                        transform: place(pages[page], origin, half, job.sizing),
                        clip: None,
                    });
                }
            }
            sides.push(side);
        }
    }
    sides
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{Handling, PaperSize};

    const PAGE: PageSize = (612.0, 792.0);

    fn job(booklet: Booklet) -> PrintJob {
        PrintJob {
            paper: PaperSize::LETTER,
            handling: Handling::Booklet(booklet),
            ..PrintJob::default()
        }
    }

    /// `(left, right)` page numbers from one, `None` for a blank half.
    fn sides(sheets: &[Sheet]) -> Vec<(Option<usize>, Option<usize>)> {
        sheets
            .iter()
            .map(|side| {
                let at = |left: bool| {
                    side.placements
                        .iter()
                        .find(|placement| (placement.transform[4] < side.width / 2.0) == left)
                        .map(|placement| placement.source + 1)
                };
                (at(true), at(false))
            })
            .collect()
    }

    #[test]
    fn eight_pages_fold_into_two_sheets_in_saddle_stitch_order() {
        let sheets = impose_booklet(&job(Booklet::default()), Booklet::default(), &[PAGE; 8]);
        assert_eq!(
            sides(&sheets),
            [
                (Some(8), Some(1)),
                (Some(2), Some(7)),
                (Some(6), Some(3)),
                (Some(4), Some(5)),
            ]
        );
        assert!(sheets.iter().all(Sheet::is_landscape));
        assert_eq!((sheets[0].width, sheets[0].height), (792.0, 612.0));
    }

    /// Five pages pad to eight: the three blanks fall at the back.
    #[test]
    fn a_page_count_short_of_four_is_padded_with_blanks_at_the_end() {
        let sheets = impose_booklet(&job(Booklet::default()), Booklet::default(), &[PAGE; 5]);
        assert_eq!(
            sides(&sheets),
            [
                (None, Some(1)),
                (Some(2), None),
                (None, Some(3)),
                (Some(4), Some(5)),
            ]
        );
    }

    #[test]
    fn one_side_only_and_right_binding() {
        let front = Booklet {
            sides: BookletSides::FrontSideOnly,
            ..Booklet::default()
        };
        assert_eq!(
            sides(&impose_booklet(&job(front), front, &[PAGE; 8])),
            [(Some(8), Some(1)), (Some(6), Some(3))]
        );
        let back = Booklet {
            sides: BookletSides::BackSideOnly,
            binding: Binding::Right,
        };
        assert_eq!(
            sides(&impose_booklet(&job(back), back, &[PAGE; 4])),
            [(Some(3), Some(2))],
            "the back of the one sheet, mirrored"
        );
        assert!(impose_booklet(&job(back), back, &[]).is_empty());
    }

    /// Each half is a portrait page's space, and a page fits in it.
    #[test]
    fn each_page_fits_its_half_of_the_side() {
        let sheets = impose_booklet(&job(Booklet::default()), Booklet::default(), &[PAGE; 4]);
        for side in &sheets {
            for placement in &side.placements {
                let [x0, y0, x1, y1] = placement.footprint(PAGE.0, PAGE.1);
                assert!(x1 - x0 <= side.width / 2.0 + 1e-6);
                assert!(y0 >= -1e-6 && y1 <= side.height + 1e-6);
            }
        }
    }
}
