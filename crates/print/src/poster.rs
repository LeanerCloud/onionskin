//! Poster printing: each page enlarged by the tile scale and split across
//! as many sheets as it takes, neighbouring tiles repeating an overlap so
//! the printed sheets can be trimmed and glued back into one picture.
//!
//! Every tile is one sheet. Its page is placed larger than the sheet and
//! clipped to the tile's area; with cut marks on, the area is inset by a
//! margin that the marks print in. Tiles run left to right, top to bottom,
//! page by page. The only placement in the print model that clips.

use crate::impose::PageSize;
use crate::job::{Handling, Orientation, Poster, PrintJob};
use crate::sheet::{Placement, Sheet};

/// The margin cut marks print in, in points.
pub const CUT_MARGIN: f64 = 18.0;
/// Maximum number of materialized Poster sheets in one operation.
pub const MAX_POSTER_SHEETS: usize = 1024;

/// Why a Poster job cannot be imposed safely.
#[derive(Debug, Clone, PartialEq)]
pub enum PosterError {
    InvalidScale {
        value: f64,
    },
    InvalidOverlap {
        value: f64,
    },
    InvalidPaper {
        width: f64,
        height: f64,
    },
    InvalidPage {
        page: usize,
        width: f64,
        height: f64,
    },
    InvalidGeometry {
        page: usize,
        reason: &'static str,
    },
    TooManySheets,
}

impl std::fmt::Display for PosterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScale { value } => write!(f, "Poster Tile Scale must be finite and from 1 to 9999%, not {value}"),
            Self::InvalidOverlap { value } => write!(f, "Poster Overlap must be finite and from 0 to 144 points, not {value}"),
            Self::InvalidPaper { width, height } => write!(f, "Poster paper dimensions must be finite and positive, not {width} by {height}"),
            Self::InvalidPage { page, width, height } => write!(f, "Poster source page {page} dimensions must be finite and positive, not {width} by {height}"),
            Self::InvalidGeometry { page, reason } => write!(f, "Poster page {page} has invalid geometry: {reason}"),
            Self::TooManySheets => write!(f, "Poster job would exceed the limit of {MAX_POSTER_SHEETS} sheets"),
        }
    }
}

impl std::error::Error for PosterError {}

#[derive(Debug, Clone, Copy)]
struct Geometry {
    width: f64,
    height: f64,
    margin: f64,
    area: (f64, f64),
    step: (f64, f64),
    scaled: (f64, f64),
    columns: usize,
    rows: usize,
}

impl Geometry {
    fn placement(
        self,
        page: usize,
        scale: f64,
        row: usize,
        column: usize,
    ) -> Result<(Placement, [f64; 4]), PosterError> {
        let left = self.margin - column as f64 * self.step.0;
        let top = self.margin + self.area.1 + row as f64 * self.step.1;
        let clip = [
            self.margin,
            self.margin,
            self.margin + self.area.0,
            self.margin + self.area.1,
        ];
        let transform = [scale, 0.0, 0.0, scale, left, top - self.scaled.1];
        if !clip
            .iter()
            .chain(transform.iter())
            .all(|value| value.is_finite())
        {
            return Err(PosterError::InvalidGeometry {
                page,
                reason: "derived placement is not finite",
            });
        }
        Ok((
            Placement {
                source: page,
                transform,
                clip: Some(clip),
            },
            clip,
        ))
    }
}

/// The sheets `job` prints as posters from a document whose pages are
/// `pages`.
pub fn impose_poster(
    job: &PrintJob,
    poster: Poster,
    pages: &[PageSize],
) -> Result<Vec<Sheet>, PosterError> {
    let (count, geometries) = preflight_pages(job, poster, pages)?;
    let mut sheets = Vec::with_capacity(count);
    for (page, geometry) in geometries {
        sheets.extend(tiles(page, poster, geometry)?);
    }
    Ok(sheets)
}

/// The tiles of one page, using the geometry already checked by preflight.
fn tiles(page: usize, poster: Poster, geometry: Geometry) -> Result<Vec<Sheet>, PosterError> {
    let Geometry {
        width,
        height,
        columns,
        rows,
        ..
    } = geometry;
    let scale = poster.scale / 100.0;
    let mut sheets = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            let (placement, clip) = geometry.placement(page, scale, row, column)?;
            sheets.push(Sheet {
                width,
                height,
                placements: vec![placement],
                frames: if poster.cut_marks {
                    vec![clip]
                } else {
                    Vec::new()
                },
            });
        }
    }
    Ok(sheets)
}

/// How many tiles of `area`, each `step` after the last, cover `length`.
fn count(length: f64, area: f64, step: f64, page: usize) -> Result<usize, PosterError> {
    if !length.is_finite()
        || !area.is_finite()
        || !step.is_finite()
        || length <= 0.0
        || area <= 0.0
        || step <= 0.0
    {
        return Err(PosterError::InvalidGeometry {
            page,
            reason: "non-finite or non-positive tile dimensions",
        });
    }
    if length <= area {
        return Ok(1);
    }
    let additional = ((length - area) / step).ceil();
    if !additional.is_finite() || additional > (MAX_POSTER_SHEETS - 1) as f64 {
        return Err(PosterError::TooManySheets);
    }
    Ok(1 + additional as usize)
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

fn geometry(
    job: &PrintJob,
    poster: Poster,
    page: usize,
    size: PageSize,
) -> Result<Geometry, PosterError> {
    if !size.0.is_finite() || !size.1.is_finite() || size.0 <= 0.0 || size.1 <= 0.0 {
        return Err(PosterError::InvalidPage {
            page,
            width: size.0,
            height: size.1,
        });
    }
    if !job.paper.width.is_finite()
        || !job.paper.height.is_finite()
        || job.paper.width <= 0.0
        || job.paper.height <= 0.0
    {
        return Err(PosterError::InvalidPaper {
            width: job.paper.width,
            height: job.paper.height,
        });
    }
    let (width, height) = paper(job, size);
    let margin = if poster.cut_marks { CUT_MARGIN } else { 0.0 };
    let area = (width - 2.0 * margin, height - 2.0 * margin);
    if !area.0.is_finite() || !area.1.is_finite() || area.0 <= 0.0 || area.1 <= 0.0 {
        return Err(PosterError::InvalidGeometry {
            page,
            reason: "paper has no printable tile area",
        });
    }
    if poster.overlap >= area.0.min(area.1) {
        return Err(PosterError::InvalidGeometry {
            page,
            reason: "overlap leaves no positive tile step",
        });
    }
    let step = (area.0 - poster.overlap, area.1 - poster.overlap);
    if !step.0.is_finite() || !step.1.is_finite() || step.0 <= 0.0 || step.1 <= 0.0 {
        return Err(PosterError::InvalidGeometry {
            page,
            reason: "tile step is not finite and positive",
        });
    }
    let scale = poster.scale / 100.0;
    let scaled = (size.0 * scale, size.1 * scale);
    if !scaled.0.is_finite() || !scaled.1.is_finite() || scaled.0 <= 0.0 || scaled.1 <= 0.0 {
        return Err(PosterError::InvalidGeometry {
            page,
            reason: "scaled page dimensions are not finite and positive",
        });
    }
    let columns = count(scaled.0, area.0, step.0, page)?;
    let rows = count(scaled.1, area.1, step.1, page)?;
    let page_count = columns
        .checked_mul(rows)
        .ok_or(PosterError::TooManySheets)?;
    if page_count > MAX_POSTER_SHEETS {
        return Err(PosterError::TooManySheets);
    }
    let geometry = Geometry {
        width,
        height,
        margin,
        area,
        step,
        scaled,
        columns,
        rows,
    };
    for row in 0..rows {
        for column in 0..columns {
            geometry.placement(page, scale, row, column)?;
        }
    }
    Ok(geometry)
}

fn preflight_pages(
    job: &PrintJob,
    poster: Poster,
    pages: &[PageSize],
) -> Result<(usize, Vec<(usize, Geometry)>), PosterError> {
    if !poster.scale.is_finite() || !(1.0..=9999.0).contains(&poster.scale) {
        return Err(PosterError::InvalidScale {
            value: poster.scale,
        });
    }
    if !poster.overlap.is_finite() || !(0.0..=144.0).contains(&poster.overlap) {
        return Err(PosterError::InvalidOverlap {
            value: poster.overlap,
        });
    }
    let mut total = 0usize;
    let mut geometries = Vec::new();
    for page in job
        .selection
        .pages(pages.len())
        .into_iter()
        .filter(|page| *page < pages.len())
    {
        let geometry = geometry(job, poster, page, pages[page])?;
        let page_count = geometry
            .columns
            .checked_mul(geometry.rows)
            .ok_or(PosterError::TooManySheets)?;
        total = total
            .checked_add(page_count)
            .ok_or(PosterError::TooManySheets)?;
        if total > MAX_POSTER_SHEETS {
            return Err(PosterError::TooManySheets);
        }
        geometries.push((page, geometry));
    }
    Ok((total, geometries))
}

/// Count a Poster job without allocating its sheets.
pub fn sheet_count(
    job: &PrintJob,
    poster: Poster,
    pages: &[PageSize],
) -> Result<usize, PosterError> {
    let (count, _) = preflight_pages(job, poster, pages)?;
    Ok(count)
}

/// Validate one or two documents before either output is dispatched.
pub fn preflight(
    job: &PrintJob,
    main: &[PageSize],
    appendix: Option<&[PageSize]>,
) -> Result<(), PosterError> {
    let Handling::Poster(poster) = job.handling else {
        return Ok(());
    };
    let main_count = sheet_count(job, poster, main)?;
    let appendix_count = appendix.map_or(Ok(0), |pages| {
        sheet_count(&crate::appendix::appendix_job(job), poster, pages)
    })?;
    let total = main_count
        .checked_add(appendix_count)
        .ok_or(PosterError::TooManySheets)?;
    if total > MAX_POSTER_SHEETS {
        return Err(PosterError::TooManySheets);
    }
    Ok(())
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

    fn poster(scale: f64, overlap: f64, cut_marks: bool) -> Poster {
        Poster {
            scale,
            overlap,
            cut_marks,
        }
    }

    #[test]
    fn at_full_size_with_no_margin_a_page_is_one_tile() {
        let options = poster(100.0, 0.0, false);
        let sheets = impose_poster(&job(options), options, &[LETTER_PAGE]).expect("valid poster");
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
        let options = poster(200.0, 0.0, false);
        let sheets = impose_poster(&job(options), options, &[LETTER_PAGE]).expect("valid poster");
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
        let options = poster(200.0, 36.0, true);
        let sheets = impose_poster(&job(options), options, &[LETTER_PAGE]).expect("valid poster");
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
        let options = poster(100.0, 0.0, false);
        let sheets = impose_poster(&job(options), options, &[(792.0, 612.0), LETTER_PAGE])
            .expect("valid poster");
        assert_eq!(sheets.len(), 2);
        assert!(sheets[0].is_landscape());
        assert!(!sheets[1].is_landscape());
        assert_eq!(sheets[1].placements[0].source, 1);
        assert!(
            count(10.0, 5.0, 0.0, 0).is_err(),
            "a degenerate step is rejected"
        );
    }

    #[test]
    fn poster_controls_accept_fractional_scale_and_reject_invalid_values() {
        let options = poster(125.5, 9.0, false);
        let sheets = impose_poster(&job(options), options, &[LETTER_PAGE]).expect("valid poster");
        assert_eq!(sheets.len(), 4);
        let expected = [
            [1.255, 0.0, 0.0, 1.255, 0.0, -201.96],
            [1.255, 0.0, 0.0, 1.255, -603.0, -201.96],
            [1.255, 0.0, 0.0, 1.255, 0.0, 581.04],
        ];
        for (actual, expected) in sheets
            .iter()
            .take(3)
            .map(|sheet| sheet.placements[0].transform)
            .zip(expected)
        {
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| (actual - expected).abs() < 1e-10),
                "transform {actual:?} differs from {expected:?}"
            );
        }
        assert_eq!(sheets[0].placements[0].clip, Some([0.0, 0.0, 612.0, 792.0]));
        assert!(matches!(
            impose_poster(
                &job(poster(0.0, 0.0, false)),
                poster(0.0, 0.0, false),
                &[LETTER_PAGE]
            ),
            Err(PosterError::InvalidScale { .. })
        ));
        assert!(matches!(
            impose_poster(
                &job(poster(100.0, 145.0, false)),
                poster(100.0, 145.0, false),
                &[LETTER_PAGE]
            ),
            Err(PosterError::InvalidOverlap { .. })
        ));
    }

    #[test]
    fn poster_controls_reject_late_invalid_geometry_before_materializing_tiles() {
        let options = poster(200.0, 0.0, false);
        let error = impose_poster(&job(options), options, &[LETTER_PAGE, (0.0, 1.0)])
            .expect_err("invalid later page must reject the operation");
        assert!(matches!(error, PosterError::InvalidPage { page: 1, .. }));
    }

    #[test]
    fn poster_controls_allow_exact_cap_and_reject_one_more_selected_page() {
        let options = poster(100.0, 0.0, false);
        let exact = vec![LETTER_PAGE; MAX_POSTER_SHEETS];
        assert_eq!(
            sheet_count(&job(options), options, &exact).expect("exact cap"),
            MAX_POSTER_SHEETS
        );
        let over = vec![LETTER_PAGE; MAX_POSTER_SHEETS + 1];
        assert!(matches!(
            sheet_count(&job(options), options, &over),
            Err(PosterError::TooManySheets)
        ));
    }

    #[test]
    fn poster_controls_reject_nonfinite_derived_coordinates() {
        let options = poster(100.0, 0.0, false);
        let job = PrintJob {
            paper: PaperSize {
                name: "Huge",
                width: 1.3e308,
                height: 1.3e308,
            },
            handling: Handling::Poster(options),
            ..PrintJob::default()
        };
        let error = sheet_count(&job, options, &[(1.0e308, 1.7e308)])
            .expect_err("overflowing placement arithmetic must reject");
        assert!(matches!(error, PosterError::InvalidGeometry { .. }));
    }

    #[test]
    fn poster_controls_reject_huge_finite_counts_before_integer_conversion() {
        let options = poster(100.0, 0.0, false);
        let job = PrintJob {
            paper: PaperSize {
                name: "Tiny",
                width: 1.0,
                height: 1.0,
            },
            handling: Handling::Poster(options),
            ..PrintJob::default()
        };
        let error = sheet_count(&job, options, &[(1.0e308, 1.0)])
            .expect_err("huge finite count must reject before casting");
        assert!(matches!(error, PosterError::TooManySheets));
    }

    #[test]
    fn poster_controls_cover_numeric_endpoints_and_geometry_transitions() {
        let one = poster(1.0, 0.0, false);
        assert_eq!(
            sheet_count(&job(one), one, &[LETTER_PAGE]).expect("scale one"),
            1
        );
        let max = poster(9999.0, 144.0, false);
        assert_eq!(
            sheet_count(&job(max), max, &[(0.01, 0.01)]).expect("scale and overlap endpoints"),
            1
        );
        let marked_job = job(poster(100.0, 0.0, true));
        assert!(matches!(
            sheet_count(
                &PrintJob {
                    paper: PaperSize {
                        name: "36pt",
                        width: 36.0,
                        height: 36.0
                    },
                    ..marked_job
                },
                Poster {
                    cut_marks: true,
                    ..poster(100.0, 0.0, true)
                },
                &[(10.0, 10.0)]
            ),
            Err(PosterError::InvalidGeometry { .. })
        ));
        assert_eq!(count(5.0, 5.0, 1.0, 0).expect("one tile"), 1);
        assert_eq!(count(6.0, 5.0, 1.0, 0).expect("two tiles"), 2);
        assert_eq!(count(6.000_001, 5.0, 1.0, 0).expect("three tiles"), 3);
    }

    #[test]
    fn poster_controls_reject_underflow_and_overflow_and_preserve_duplicate_reverse_selection() {
        let underflow = poster(1.0, 0.0, false);
        assert!(matches!(
            sheet_count(&job(underflow), underflow, &[(f64::from_bits(1), 1.0)]),
            Err(PosterError::InvalidGeometry { .. })
        ));
        let overflow = poster(9999.0, 0.0, false);
        assert!(matches!(
            sheet_count(&job(overflow), overflow, &[(f64::MAX, 1.0)]),
            Err(PosterError::InvalidGeometry { .. })
        ));
        let options = poster(100.0, 0.0, false);
        let job = PrintJob {
            selection: crate::job::PageSelection {
                ranges: vec![(0, 1), (1, 1)],
                reverse: true,
                ..Default::default()
            },
            ..job(options)
        };
        let sheets = impose_poster(&job, options, &[LETTER_PAGE, (792.0, 612.0)])
            .expect("duplicate selection");
        assert_eq!(
            sheets
                .iter()
                .map(|sheet| sheet.placements[0].source)
                .collect::<Vec<_>>(),
            [1, 1, 0]
        );
    }

    #[test]
    fn poster_controls_reject_all_nonfinite_and_out_of_range_inputs() {
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 10000.0] {
            assert!(matches!(
                sheet_count(
                    &job(poster(value, 0.0, false)),
                    poster(value, 0.0, false),
                    &[LETTER_PAGE]
                ),
                Err(PosterError::InvalidScale { .. })
            ));
        }
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 145.0] {
            assert!(matches!(
                sheet_count(
                    &job(poster(100.0, value, false)),
                    poster(100.0, value, false),
                    &[LETTER_PAGE]
                ),
                Err(PosterError::InvalidOverlap { .. })
            ));
        }
        for (width, height) in [
            (0.0, 792.0),
            (-1.0, 792.0),
            (f64::NAN, 792.0),
            (f64::INFINITY, 792.0),
            (f64::NEG_INFINITY, 792.0),
        ] {
            assert!(matches!(
                sheet_count(
                    &job(poster(100.0, 0.0, false)),
                    poster(100.0, 0.0, false),
                    &[(width, height)]
                ),
                Err(PosterError::InvalidPage { .. })
            ));
        }
        for (width, height) in [
            (0.0, 792.0),
            (-1.0, 792.0),
            (f64::NAN, 792.0),
            (f64::INFINITY, 792.0),
            (f64::NEG_INFINITY, 792.0),
        ] {
            let mut invalid_paper = job(poster(100.0, 0.0, false));
            invalid_paper.paper = PaperSize {
                name: "invalid",
                width,
                height,
            };
            assert!(matches!(
                sheet_count(&invalid_paper, poster(100.0, 0.0, false), &[LETTER_PAGE]),
                Err(PosterError::InvalidPaper { .. })
            ));
        }
    }

    #[test]
    fn poster_controls_reject_overlap_at_area_and_allow_just_below_it() {
        let mut small = job(poster(100.0, 0.0, false));
        small.paper = PaperSize {
            name: "small",
            width: 100.0,
            height: 100.0,
        };
        assert!(matches!(
            geometry(&small, poster(100.0, 100.0, false), 0, (10.0, 10.0)),
            Err(PosterError::InvalidGeometry { .. })
        ));
        assert!(geometry(&small, poster(100.0, 99.999, false), 0, (10.0, 10.0)).is_ok());

        let mut marked = job(poster(100.0, 0.0, true));
        marked.paper = PaperSize {
            name: "35pt",
            width: 35.0,
            height: 35.0,
        };
        assert!(matches!(
            sheet_count(&marked, poster(100.0, 0.0, true), &[(10.0, 10.0)]),
            Err(PosterError::InvalidGeometry { .. })
        ));
        let mut unmarked = marked;
        unmarked.paper.width = 35.0;
        unmarked.paper.height = 35.0;
        assert!(sheet_count(&unmarked, poster(100.0, 0.0, false), &[(10.0, 10.0)]).is_ok());
    }

    #[test]
    fn poster_controls_apply_the_cap_to_main_and_appendix_together() {
        let options = poster(100.0, 0.0, false);
        let job = job(options);
        assert!(preflight(
            &job,
            &[LETTER_PAGE; MAX_POSTER_SHEETS / 2],
            Some(&[LETTER_PAGE; MAX_POSTER_SHEETS / 2]),
        )
        .is_ok());
        let error = preflight(
            &job,
            &[LETTER_PAGE; MAX_POSTER_SHEETS / 2],
            Some(&[LETTER_PAGE; MAX_POSTER_SHEETS / 2 + 1]),
        )
        .expect_err("combined operation must reject one sheet over the cap");
        assert!(matches!(error, PosterError::TooManySheets));
    }
}
