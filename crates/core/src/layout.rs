//! Where pages sit, and the scroll metric that lets them sit there before
//! they have been parsed.
//!
//! # The estimate-and-refine scroll metric
//!
//! Laying out a scroll of N pages needs N page heights, which contradicts the
//! project's "never parse ahead of need" rule (PLAN.md decision 11). The
//! resolution, and the single most load-bearing idea in this file:
//!
//! - **Page 0 is the sole estimate source.** [`Layout::estimate`] returns page
//!   0's render size and nothing else; every unmeasured page is assumed to be
//!   that size. No query answers anything until page 0 has been measured, so
//!   the estimate never silently changes to a different page's size.
//! - **A row top is a uniform baseline plus a correction.** [`Layout::row_top`]
//!   places row `r` at `gap + r * (baseline + gap)` where `baseline` is the
//!   estimated row height at the query zoom, then adds, for every *measured*
//!   row before `r`, that row's real height minus the baseline. Rows are
//!   therefore exact wherever they have been measured and estimated
//!   everywhere else, in one expression.
//! - **Refinement never moves an earlier row.** The correction for row `r`
//!   sums only rows strictly before `r`, so measuring page 900 cannot move
//!   pages 0..900. That is what keeps the scrollbar from jumping under a
//!   reader: content above the viewport is already measured (it was scrolled
//!   through), and content below it moving is invisible. Only re-measuring
//!   page 0 itself, which changes the baseline, reflows everything, and that
//!   happens once, on open.
//! - **A jump to page 900 lands on page 900** with twelve pages measured,
//!   because [`Layout::scroll_origin_for_page`] uses the same expression the
//!   painter does. The offset is an estimate, not a lie: it is exactly where
//!   page 900 is drawn.
//!
//! [`Layout::row_index`] caches the per-row corrections so the frame path
//! does not walk every measured row; see [`RowIndex`].

use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{PageGeometry, PageIndex};

/// Rows kept laid out on each side of the visible band, so a scroll of less
/// than a row still finds a painted page to show.
const GUARD_ROWS: usize = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewSize {
    pub width: f32,
    pub height: f32,
}

impl ViewSize {
    pub(crate) fn is_valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width > 0.0 && self.height > 0.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewRect {
    pub origin: ViewPoint,
    pub size: ViewSize,
}

impl ViewRect {
    pub(crate) fn right(self) -> f32 {
        self.origin.x + self.size.width
    }

    pub(crate) fn bottom(self) -> f32 {
        self.origin.y + self.size.height
    }

    pub(crate) fn intersects(self, other: Self) -> bool {
        self.origin.x < other.origin.x + other.size.width
            && other.origin.x < self.origin.x + self.size.width
            && self.origin.y < other.origin.y + other.size.height
            && other.origin.y < self.origin.y + self.size.height
    }

    pub(crate) fn is_valid(self) -> bool {
        self.origin.x.is_finite()
            && self.origin.y.is_finite()
            && self.size.is_valid()
            && self.right().is_finite()
            && self.bottom().is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRenderRect {
    page: PageIndex,
    origin: ViewPoint,
    size: ViewSize,
}

impl PageRenderRect {
    pub fn new(
        page: PageIndex,
        origin: ViewPoint,
        size: ViewSize,
        page_size: ViewSize,
    ) -> Result<Self, LayoutError> {
        let rect = ViewRect { origin, size };
        if !rect.is_valid() || !page_size.is_valid() {
            return Err(LayoutError::InvalidRect);
        }
        if origin.x < 0.0
            || origin.y < 0.0
            || origin.x + size.width > page_size.width
            || origin.y + size.height > page_size.height
        {
            return Err(LayoutError::RectOutsidePage { page });
        }
        Ok(Self { page, origin, size })
    }

    pub fn page(self) -> PageIndex {
        self.page
    }

    pub fn origin(self) -> ViewPoint {
        self.origin
    }

    pub fn size(self) -> ViewSize {
        self.size
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageLayoutMode {
    SinglePage,
    #[default]
    SinglePageContinuous,
    TwoPage,
    TwoPageContinuous,
}

impl PageLayoutMode {
    fn is_continuous(self) -> bool {
        matches!(self, Self::SinglePageContinuous | Self::TwoPageContinuous)
    }

    fn is_two_page(self) -> bool {
        matches!(self, Self::TwoPage | Self::TwoPageContinuous)
    }
}

/// Ordered by quarter turns clockwise, so the variants can key a map
/// directly instead of being numbered again by the caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViewRotation {
    #[default]
    None,
    Clockwise90,
    HalfTurn,
    Clockwise270,
}

impl ViewRotation {
    pub(crate) fn page_size(self, size: ViewSize) -> ViewSize {
        match self {
            Self::None | Self::HalfTurn => size,
            Self::Clockwise90 | Self::Clockwise270 => ViewSize {
                width: size.height,
                height: size.width,
            },
        }
    }

    pub(crate) fn unrotate_point(self, point: ViewPoint, page: ViewSize) -> ViewPoint {
        match self {
            Self::None => point,
            Self::Clockwise90 => ViewPoint {
                x: point.y,
                y: page.height - point.x,
            },
            Self::HalfTurn => ViewPoint {
                x: page.width - point.x,
                y: page.height - point.y,
            },
            Self::Clockwise270 => ViewPoint {
                x: page.width - point.y,
                y: point.x,
            },
        }
    }

    pub(crate) fn rotate_point(self, point: ViewPoint, page: ViewSize) -> ViewPoint {
        match self {
            Self::None => point,
            Self::Clockwise90 => ViewPoint {
                x: page.height - point.y,
                y: point.x,
            },
            Self::HalfTurn => ViewPoint {
                x: page.width - point.x,
                y: page.height - point.y,
            },
            Self::Clockwise270 => ViewPoint {
                x: point.y,
                y: page.width - point.x,
            },
        }
    }

    pub(crate) fn rotate_rect(self, rect: PageRenderRect, page: ViewSize) -> ViewRect {
        let origin = rect.origin();
        let size = rect.size();
        match self {
            Self::None => ViewRect { origin, size },
            Self::Clockwise90 => ViewRect {
                origin: ViewPoint {
                    x: page.height - origin.y - size.height,
                    y: origin.x,
                },
                size: ViewSize {
                    width: size.height,
                    height: size.width,
                },
            },
            Self::HalfTurn => ViewRect {
                origin: ViewPoint {
                    x: page.width - origin.x - size.width,
                    y: page.height - origin.y - size.height,
                },
                size,
            },
            Self::Clockwise270 => ViewRect {
                origin: ViewPoint {
                    x: origin.y,
                    y: page.width - origin.x - size.width,
                },
                size: ViewSize {
                    width: size.height,
                    height: size.width,
                },
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PagePlacement {
    pub page: PageIndex,
    pub rect: ViewRect,
    pub measured: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageAlignment {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutError {
    InvalidGap(f32),
    InvalidZoom(f32),
    InvalidViewport,
    InvalidRect,
    RectOutsidePage { page: PageIndex },
    InvalidPageSize { page: PageIndex },
    NoSuchPage { page: PageIndex, count: usize },
    MissingEstimate,
    ExtentOverflow,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGap(gap) => {
                write!(f, "page gap must be finite and non-negative, got {gap}")
            }
            Self::InvalidZoom(zoom) => write!(f, "zoom must be positive and finite, got {zoom}"),
            Self::InvalidViewport => write!(f, "viewport must have a positive finite size"),
            Self::InvalidRect => write!(
                f,
                "rectangle must have finite coordinates and positive size"
            ),
            Self::RectOutsidePage { page } => write!(f, "rectangle lies outside page {page}"),
            Self::InvalidPageSize { page } => {
                write!(f, "page {page} has no positive finite render size")
            }
            Self::NoSuchPage { page, count } => {
                write!(f, "page {page} is outside a {count}-page document")
            }
            Self::MissingEstimate => {
                write!(f, "page 0 must be measured before layout can be estimated")
            }
            Self::ExtentOverflow => write!(f, "layout extent is not finite"),
        }
    }
}

impl std::error::Error for LayoutError {}

pub(crate) struct Layout {
    page_count: usize,
    page_gap: f32,
    measured: BTreeMap<PageIndex, PageGeometry>,
    /// Bumped by every measurement, so [`RowIndex`] can tell a stale cache
    /// from a current one without comparing the map.
    revision: u64,
    rows: RefCell<RowIndex>,
}

impl Layout {
    pub(crate) fn new(page_count: usize, page_gap: f32) -> Result<Self, LayoutError> {
        if !page_gap.is_finite() || page_gap < 0.0 {
            return Err(LayoutError::InvalidGap(page_gap));
        }
        Ok(Self {
            page_count,
            page_gap,
            measured: BTreeMap::new(),
            revision: 0,
            rows: RefCell::new(RowIndex::default()),
        })
    }

    pub(crate) fn page_count(&self) -> usize {
        self.page_count
    }

    pub(crate) fn page_gap(&self) -> f32 {
        self.page_gap
    }

    pub(crate) fn measure_page(&mut self, geometry: PageGeometry) -> Result<(), LayoutError> {
        let page = geometry.index;
        self.check_page(page)?;
        let size = render_size(&geometry);
        if !size.is_valid() {
            return Err(LayoutError::InvalidPageSize { page });
        }
        self.measured.insert(page, geometry);
        self.revision += 1;
        Ok(())
    }

    pub(crate) fn geometry(&self, page: PageIndex) -> Option<&PageGeometry> {
        self.measured.get(&page)
    }

    pub(crate) fn page_render_size(&self, page: PageIndex) -> Result<ViewSize, LayoutError> {
        self.check_page(page)?;
        self.page_size(page, ViewRotation::None)
            .map(|(size, _)| size)
    }

    pub(crate) fn placement(
        &self,
        page: PageIndex,
        query: LayoutQuery,
    ) -> Result<Option<PagePlacement>, LayoutError> {
        self.validate_query(query)?;
        // An empty document has no page that survives `check_page`, so from
        // here on `page_count` is known to be non-zero.
        self.check_page(page)?;
        let row = row_for_page(page, query.mode, query.show_cover);
        let active_row = row_for_page(query.current_page, query.mode, query.show_cover);
        if !query.mode.is_continuous() && row != active_row {
            return Ok(None);
        }
        self.placement_in_row(page, row, query).map(Some)
    }

    pub(crate) fn extent(&self, query: LayoutQuery) -> Result<ViewSize, LayoutError> {
        self.validate_query(query)?;
        if self.page_count == 0 {
            return Ok(query.viewport);
        }
        let active_row = row_for_page(query.current_page, query.mode, query.show_cover);
        let rows = if query.mode.is_continuous() {
            row_count(self.page_count, query.mode, query.show_cover)
        } else {
            1
        };
        let content_width = if query.mode.is_continuous() {
            self.max_row_width(query)?
        } else {
            self.row_size(active_row, query)?.width
        };
        // `total_height` already includes the gap above and below the run of
        // rows; the single-row and width cases have to add theirs.
        let width = content_width + self.page_gap * 2.0;
        let height = if query.mode.is_continuous() {
            self.total_height(rows, query)?
        } else {
            self.row_size(active_row, query)?.height + self.page_gap * 2.0
        };
        let extent = ViewSize {
            width: width.max(query.viewport.width),
            height: height.max(query.viewport.height),
        };
        if !extent.is_valid() {
            return Err(LayoutError::ExtentOverflow);
        }
        Ok(extent)
    }

    pub(crate) fn visible_pages(
        &self,
        visible: ViewRect,
        query: LayoutQuery,
    ) -> Result<Vec<PagePlacement>, LayoutError> {
        self.validate_query(query)?;
        if !visible.is_valid() {
            return Err(LayoutError::InvalidRect);
        }
        if self.page_count == 0 {
            return Ok(Vec::new());
        }
        let active_row = row_for_page(query.current_page, query.mode, query.show_cover);
        let total_rows = row_count(self.page_count, query.mode, query.show_cover);
        let (start, end) = if query.mode.is_continuous() {
            let first = self.first_row_reaching(visible.origin.y, total_rows, query)?;
            let last = self.first_row_reaching(visible.bottom(), total_rows, query)?;
            (
                first.saturating_sub(GUARD_ROWS),
                last.saturating_add(GUARD_ROWS + 1).min(total_rows),
            )
        } else {
            (active_row, active_row + 1)
        };

        let mut candidate_rows = Vec::new();
        for row in start..end {
            let mut placements = Vec::new();
            for page in pages_in_row(row, self.page_count, query.mode, query.show_cover) {
                placements.push(self.placement_in_row(page, row, query)?);
            }
            candidate_rows.push((row, placements));
        }
        let mut intersecting = candidate_rows.iter().filter_map(|(row, placements)| {
            placements
                .iter()
                .any(|placement| placement.rect.intersects(visible))
                .then_some(*row)
        });
        let Some(first_intersecting) = intersecting.next() else {
            return Ok(Vec::new());
        };
        let last_intersecting = intersecting.next_back().unwrap_or(first_intersecting);
        // `GUARD_ROWS` on each side: the end is exclusive, hence the extra one.
        let guarded_start = first_intersecting.saturating_sub(GUARD_ROWS);
        let guarded_end = last_intersecting
            .saturating_add(GUARD_ROWS + 1)
            .min(total_rows);
        Ok(candidate_rows
            .into_iter()
            .filter(|(row, _)| (guarded_start..guarded_end).contains(row))
            .flat_map(|(_, placements)| placements)
            .collect())
    }

    /// Where the view has to sit for `page` to be at `alignment`.
    ///
    /// `horizontal` is the caller's current x offset, kept and re-clamped
    /// rather than reset: a vertical navigation must not also pan the reader
    /// sideways, which it did while this returned a hardcoded `x: 0.0` and a
    /// zoomed-in reader lost their horizontal position on every page jump.
    pub(crate) fn scroll_origin_for_page(
        &self,
        page: PageIndex,
        alignment: PageAlignment,
        horizontal: f32,
        query: LayoutQuery,
    ) -> Result<ViewPoint, LayoutError> {
        self.validate_query(query)?;
        self.check_page(page)?;
        let target_query = LayoutQuery {
            current_page: page,
            ..query
        };
        let placement = self
            .placement(page, target_query)?
            .ok_or(LayoutError::NoSuchPage {
                page,
                count: self.page_count,
            })?;
        let extent = self.extent(target_query)?;
        let y = match alignment {
            PageAlignment::Start => placement.rect.origin.y - self.page_gap,
            PageAlignment::Center => {
                placement.rect.origin.y + placement.rect.size.height / 2.0
                    - query.viewport.height / 2.0
            }
            PageAlignment::End => {
                placement.rect.origin.y + placement.rect.size.height + self.page_gap
                    - query.viewport.height
            }
        };
        Ok(ViewPoint {
            x: horizontal.clamp(0.0, (extent.width - query.viewport.width).max(0.0)),
            y: y.clamp(0.0, (extent.height - query.viewport.height).max(0.0)),
        })
    }

    pub(crate) fn row_metrics_for_page(
        &self,
        page: PageIndex,
        query: LayoutQuery,
    ) -> Result<RowMetrics, LayoutError> {
        self.validate_query(query)?;
        self.check_page(page)?;
        let row = row_for_page(page, query.mode, query.show_cover);
        let mut page_width = 0.0;
        let mut page_height: f32 = 0.0;
        let pages = pages_in_row(row, self.page_count, query.mode, query.show_cover);
        for row_page in pages.iter().copied() {
            let (size, _) = self.page_size(row_page, query.rotation)?;
            page_width += size.width;
            page_height = page_height.max(size.height);
        }
        Ok(RowMetrics {
            page_width,
            page_height,
            between_pages: pages.len().saturating_sub(1),
        })
    }

    fn validate_query(&self, query: LayoutQuery) -> Result<(), LayoutError> {
        if !query.viewport.is_valid() {
            return Err(LayoutError::InvalidViewport);
        }
        if !query.zoom.is_finite() || query.zoom <= 0.0 {
            return Err(LayoutError::InvalidZoom(query.zoom));
        }
        if self.page_count > 0 {
            self.check_page(query.current_page)?;
            self.estimate()?;
        }
        Ok(())
    }

    fn check_page(&self, page: PageIndex) -> Result<(), LayoutError> {
        if page >= self.page_count {
            return Err(LayoutError::NoSuchPage {
                page,
                count: self.page_count,
            });
        }
        Ok(())
    }

    /// The size every unmeasured page is assumed to have.
    ///
    /// Page 0 is the sole source, by design: it is the page a lazy open has
    /// already parsed, and pinning the estimate to one page is what makes the
    /// scroll metric stable. An estimate that averaged the measured pages
    /// would change on every measurement and reflow the whole document under
    /// the reader. Absent page 0, every query fails with
    /// [`LayoutError::MissingEstimate`] rather than guessing.
    fn estimate(&self) -> Result<ViewSize, LayoutError> {
        self.measured
            .get(&0)
            .map(render_size)
            .ok_or(LayoutError::MissingEstimate)
    }

    fn page_size(
        &self,
        page: PageIndex,
        rotation: ViewRotation,
    ) -> Result<(ViewSize, bool), LayoutError> {
        let size = match self.measured.get(&page) {
            Some(geometry) => render_size(geometry),
            None => self.estimate()?,
        };
        Ok((rotation.page_size(size), self.measured.contains_key(&page)))
    }

    /// A row's pages at their own scale: summed widths and tallest height,
    /// rotated but neither zoomed nor gapped.
    ///
    /// The unit [`RowIndex`] caches in, because zoom and gap are applied by
    /// the caller and would otherwise make every cached value zoom-specific.
    fn row_page_extent(
        &self,
        row: usize,
        mode: PageLayoutMode,
        show_cover: bool,
        rotation: ViewRotation,
    ) -> Result<ViewSize, LayoutError> {
        let mut width = 0.0;
        let mut height: f32 = 0.0;
        for page in pages_in_row(row, self.page_count, mode, show_cover) {
            let (size, _) = self.page_size(page, rotation)?;
            width += size.width;
            height = height.max(size.height);
        }
        Ok(ViewSize { width, height })
    }

    fn row_size(&self, row: usize, query: LayoutQuery) -> Result<ViewSize, LayoutError> {
        let gaps = gaps_in_row(row, self.page_count, query.mode, query.show_cover);
        let extent = self.row_page_extent(row, query.mode, query.show_cover, query.rotation)?;
        Ok(ViewSize {
            width: extent.width * query.zoom + gaps as f32 * self.page_gap,
            height: extent.height * query.zoom,
        })
    }

    /// Where row `row` starts, in document coordinates.
    ///
    /// The whole estimate-and-refine metric, in one expression: a uniform
    /// baseline of estimated rows, plus the accumulated correction of every
    /// *measured* row before this one. Because the sum stops at `row`,
    /// measuring a later page never moves an earlier one, which is what keeps
    /// the scrollbar still while the reader is reading. See the module docs.
    fn row_top(&self, row: usize, query: LayoutQuery) -> Result<f32, LayoutError> {
        if !query.mode.is_continuous() {
            let row_height = self.row_size(row, query)?.height;
            return Ok(((query.viewport.height - row_height) / 2.0).max(self.page_gap));
        }
        let baseline = query.rotation.page_size(self.estimate()?).height * query.zoom;
        let correction = self.row_index(query)?.correction_before(row);
        let top = self.page_gap + row as f32 * (baseline + self.page_gap) + correction * query.zoom;
        if !top.is_finite() {
            return Err(LayoutError::ExtentOverflow);
        }
        Ok(top)
    }

    fn total_height(&self, rows: usize, query: LayoutQuery) -> Result<f32, LayoutError> {
        if rows == 0 {
            return Ok(query.viewport.height);
        }
        let baseline = query.rotation.page_size(self.estimate()?).height * query.zoom;
        let correction = self.row_index(query)?.total_correction();
        let height = self.page_gap * 2.0
            + rows as f32 * baseline
            + rows.saturating_sub(1) as f32 * self.page_gap
            + correction * query.zoom;
        if !height.is_finite() {
            return Err(LayoutError::ExtentOverflow);
        }
        Ok(height)
    }

    fn max_row_width(&self, query: LayoutQuery) -> Result<f32, LayoutError> {
        let estimate = query.rotation.page_size(self.estimate()?).width;
        let baseline = if query.mode.is_two_page() && self.page_count > 1 {
            estimate * query.zoom * 2.0 + self.page_gap
        } else {
            estimate * query.zoom
        };
        let index = self.row_index(query)?;
        let mut width = baseline;
        for (gaps, widest) in index.widest_by_gaps.iter().enumerate() {
            let Some(widest) = widest else { continue };
            width = width.max(widest * query.zoom + gaps as f32 * self.page_gap);
        }
        Ok(width)
    }

    /// The cached row corrections for this query's layout shape, rebuilt if a
    /// measurement or a shape change has invalidated them.
    fn row_index(&self, query: LayoutQuery) -> Result<Ref<'_, RowIndex>, LayoutError> {
        let shape = RowShape {
            mode: query.mode,
            show_cover: query.show_cover,
            rotation: query.rotation,
            revision: self.revision,
        };
        if self.rows.borrow().shape != Some(shape) {
            let built = self.build_row_index(shape)?;
            *self.rows.borrow_mut() = built;
        }
        Ok(self.rows.borrow())
    }

    fn build_row_index(&self, shape: RowShape) -> Result<RowIndex, LayoutError> {
        let baseline = shape.rotation.page_size(self.estimate()?).height;
        let mut index = RowIndex {
            shape: Some(shape),
            ..RowIndex::default()
        };
        let mut running = 0.0;
        for row in self.measured_rows(shape.mode, shape.show_cover) {
            let extent = self.row_page_extent(row, shape.mode, shape.show_cover, shape.rotation)?;
            running += extent.height - baseline;
            index.rows.push(row);
            index.prefix.push(running);
            let gaps = gaps_in_row(row, self.page_count, shape.mode, shape.show_cover);
            let widest = &mut index.widest_by_gaps[gaps];
            *widest = Some(widest.unwrap_or(extent.width).max(extent.width));
        }
        Ok(index)
    }

    fn measured_rows(&self, mode: PageLayoutMode, show_cover: bool) -> BTreeSet<usize> {
        self.measured
            .keys()
            .map(|&page| row_for_page(page, mode, show_cover))
            .collect()
    }

    fn placement_in_row(
        &self,
        page: PageIndex,
        row: usize,
        query: LayoutQuery,
    ) -> Result<PagePlacement, LayoutError> {
        let extent = self.extent(query)?;
        let row_size = self.row_size(row, query)?;
        let mut x = ((extent.width - row_size.width) / 2.0).max(self.page_gap);
        for row_page in pages_in_row(row, self.page_count, query.mode, query.show_cover) {
            let (size, measured) = self.page_size(row_page, query.rotation)?;
            let scaled = ViewSize {
                width: size.width * query.zoom,
                height: size.height * query.zoom,
            };
            if row_page == page {
                return Ok(PagePlacement {
                    page,
                    rect: ViewRect {
                        origin: ViewPoint {
                            x,
                            y: self.row_top(row, query)?,
                        },
                        size: scaled,
                    },
                    measured,
                });
            }
            x += scaled.width + self.page_gap;
        }
        Err(LayoutError::NoSuchPage {
            page,
            count: self.page_count,
        })
    }

    fn first_row_reaching(
        &self,
        y: f32,
        rows: usize,
        query: LayoutQuery,
    ) -> Result<usize, LayoutError> {
        let mut low = 0;
        let mut high = rows;
        while low < high {
            let mid = low + (high - low) / 2;
            let bottom = self.row_top(mid, query)? + self.row_size(mid, query)?.height;
            if bottom < y {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok(low.min(rows.saturating_sub(1)))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct LayoutQuery {
    pub(crate) mode: PageLayoutMode,
    pub(crate) show_cover: bool,
    pub(crate) rotation: ViewRotation,
    pub(crate) zoom: f32,
    pub(crate) viewport: ViewSize,
    pub(crate) current_page: PageIndex,
}

pub(crate) struct RowMetrics {
    pub(crate) page_width: f32,
    pub(crate) page_height: f32,
    pub(crate) between_pages: usize,
}

/// The layout shape a [`RowIndex`] was built for.
///
/// `revision` makes a measurement invalidate the index without the index
/// having to know what changed.
#[derive(Clone, Copy, PartialEq)]
struct RowShape {
    mode: PageLayoutMode,
    show_cover: bool,
    rotation: ViewRotation,
    revision: u64,
}

/// Row corrections, precomputed so the frame path never walks the measured
/// pages.
///
/// Without this, `row_top` summed every measured row before the one it was
/// asked about, `visible_pages` called it once per candidate page, and
/// `sync_current_page`, `anchor_at` and `page_point_at` each called
/// `visible_pages` again: painting one frame of a fully measured 1000-page
/// document cost millions of operations. Every value here is in unzoomed page
/// units, so one index serves every zoom.
struct RowIndex {
    shape: Option<RowShape>,
    /// The measured rows, ascending.
    rows: Vec<usize>,
    /// `prefix[i]` sums the height corrections of `rows[..i]`, so it always
    /// holds one more entry than `rows`.
    prefix: Vec<f32>,
    /// The widest measured row's summed page widths, keyed by how many gaps
    /// that row carries, because a gap is a constant the zoom does not scale.
    widest_by_gaps: [Option<f32>; 2],
}

impl Default for RowIndex {
    fn default() -> Self {
        Self {
            shape: None,
            rows: Vec::new(),
            // `prefix` is never empty: the sum over no rows is still an entry.
            prefix: vec![0.0],
            widest_by_gaps: [None; 2],
        }
    }
}

impl RowIndex {
    /// The accumulated correction of every measured row before `row`.
    fn correction_before(&self, row: usize) -> f32 {
        self.prefix[self.rows.partition_point(|&measured| measured < row)]
    }

    /// The accumulated correction of every measured row.
    fn total_correction(&self) -> f32 {
        *self.prefix.last().expect("prefix is never empty")
    }
}

pub(crate) fn render_size(geometry: &PageGeometry) -> ViewSize {
    ViewSize {
        width: geometry.render_size.0 as f32,
        height: geometry.render_size.1 as f32,
    }
}

fn row_count(page_count: usize, mode: PageLayoutMode, show_cover: bool) -> usize {
    if !mode.is_two_page() {
        return page_count;
    }
    if show_cover && page_count > 0 {
        let remaining = page_count - 1;
        1 + remaining / 2 + remaining % 2
    } else {
        page_count / 2 + page_count % 2
    }
}

fn row_for_page(page: PageIndex, mode: PageLayoutMode, show_cover: bool) -> usize {
    if !mode.is_two_page() {
        page
    } else if show_cover && page > 0 {
        1 + (page - 1) / 2
    } else {
        page / 2
    }
}

fn pages_in_row(
    row: usize,
    page_count: usize,
    mode: PageLayoutMode,
    show_cover: bool,
) -> Vec<PageIndex> {
    if !mode.is_two_page() {
        return (row < page_count).then_some(row).into_iter().collect();
    }
    let first = if show_cover {
        if row == 0 {
            return (page_count > 0).then_some(0).into_iter().collect();
        }
        1 + (row - 1) * 2
    } else {
        row * 2
    };
    (first..first.saturating_add(2).min(page_count)).collect()
}

/// How many gaps sit between the pages of `row`.
///
/// Kept separate from the page widths because a gap is a screen constant: it
/// is not multiplied by the zoom, so it cannot be folded into the cached row
/// extents in [`RowIndex`].
fn gaps_in_row(row: usize, page_count: usize, mode: PageLayoutMode, show_cover: bool) -> usize {
    pages_in_row(row, page_count, mode, show_cover)
        .len()
        .saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    const VIEWPORT: ViewSize = ViewSize {
        width: 1_100.0,
        height: 861.0,
    };

    fn query(mode: PageLayoutMode) -> LayoutQuery {
        LayoutQuery {
            mode,
            show_cover: false,
            rotation: ViewRotation::None,
            zoom: 1.0,
            viewport: VIEWPORT,
            current_page: 0,
        }
    }

    fn layout(page_count: usize) -> Layout {
        let mut layout = Layout::new(page_count, 12.0).expect("gap is valid");
        if page_count > 0 {
            layout
                .measure_page(geometry(0, 600.0, 800.0))
                .expect("page 0 is valid");
        }
        layout
    }

    fn geometry(index: usize, width: f64, height: f64) -> PageGeometry {
        let mut document = Document::open_bytes(one_page_pdf()).expect("fixture opens");
        let mut geometry = document.page_geometry(0).expect("geometry loads").clone();
        geometry.index = index;
        geometry.render_size = (width, height);
        geometry
    }

    #[test]
    fn single_page_view_places_only_the_current_page() {
        let layout = layout(3);
        let mut q = query(PageLayoutMode::SinglePage);
        q.current_page = 1;
        assert!(layout.placement(0, q).expect("layout succeeds").is_none());
        assert_eq!(
            layout
                .placement(1, q)
                .expect("layout succeeds")
                .unwrap()
                .page,
            1
        );
    }

    #[test]
    fn single_page_continuous_uses_fixed_pixel_gaps_at_every_zoom() {
        let layout = layout(3);
        for zoom in [0.5, 1.0, 3.0] {
            let mut q = query(PageLayoutMode::SinglePageContinuous);
            q.zoom = zoom;
            let first = layout.placement(0, q).unwrap().unwrap();
            let second = layout.placement(1, q).unwrap().unwrap();
            assert!((second.rect.origin.y - first.rect.bottom() - 12.0).abs() < 0.01);
        }
    }

    #[test]
    fn two_page_view_groups_pages_and_leaves_an_odd_last_page() {
        let layout = layout(5);
        let mut q = query(PageLayoutMode::TwoPage);
        q.current_page = 2;
        assert!(layout.placement(2, q).unwrap().is_some());
        assert!(layout.placement(3, q).unwrap().is_some());
        assert!(layout.placement(1, q).unwrap().is_none());
        q.current_page = 4;
        assert!(layout.placement(4, q).unwrap().is_some());
    }

    #[test]
    fn two_page_view_with_a_cover_page_puts_page_one_alone() {
        let layout = layout(5);
        let mut q = query(PageLayoutMode::TwoPageContinuous);
        q.show_cover = true;
        let cover = layout.placement(0, q).unwrap().unwrap();
        let page_two = layout.placement(1, q).unwrap().unwrap();
        let page_three = layout.placement(2, q).unwrap().unwrap();
        assert!(page_two.rect.origin.y > cover.rect.origin.y);
        assert_eq!(page_two.rect.origin.y, page_three.rect.origin.y);
    }

    #[test]
    fn measured_sizes_refine_only_their_rows() {
        let mut layout = layout(4);
        let q = query(PageLayoutMode::SinglePageContinuous);
        let page_one_before = layout.placement(1, q).unwrap().unwrap();
        let page_three_before = layout.placement(3, q).unwrap().unwrap();
        layout.measure_page(geometry(2, 700.0, 1_100.0)).unwrap();
        let page_one_after = layout.placement(1, q).unwrap().unwrap();
        let page_three_after = layout.placement(3, q).unwrap().unwrap();
        assert_eq!(page_one_before.rect, page_one_after.rect);
        assert!(page_three_after.rect.origin.y > page_three_before.rect.origin.y);
    }

    #[test]
    fn quarter_turns_swap_page_size_but_a_half_turn_does_not() {
        let layout = layout(1);
        for (rotation, expected) in [
            (ViewRotation::Clockwise90, (800.0, 600.0)),
            (ViewRotation::HalfTurn, (600.0, 800.0)),
            (ViewRotation::Clockwise270, (800.0, 600.0)),
        ] {
            let mut q = query(PageLayoutMode::SinglePage);
            q.rotation = rotation;
            let page = layout.placement(0, q).unwrap().unwrap();
            assert_eq!((page.rect.size.width, page.rect.size.height), expected);
        }
    }

    #[test]
    fn an_empty_document_needs_no_estimate() {
        let layout = Layout::new(0, 12.0).unwrap();
        assert_eq!(
            layout
                .extent(query(PageLayoutMode::SinglePageContinuous))
                .unwrap(),
            VIEWPORT
        );
        assert!(layout
            .visible_pages(
                ViewRect {
                    origin: ViewPoint::default(),
                    size: VIEWPORT,
                },
                query(PageLayoutMode::SinglePageContinuous),
            )
            .unwrap()
            .is_empty());
    }

    #[test]
    fn page_zero_is_the_only_source_of_the_estimate() {
        let mut layout = Layout::new(2, 12.0).unwrap();
        layout.measure_page(geometry(1, 600.0, 800.0)).unwrap();
        assert_eq!(
            layout.extent(query(PageLayoutMode::SinglePageContinuous)),
            Err(LayoutError::MissingEstimate)
        );
    }

    #[test]
    fn invalid_inputs_fail_loudly() {
        assert!(Layout::new(1, -1.0).is_err());
        assert!(Layout::new(1, f32::NAN).is_err());
        let layout = layout(1);
        let mut q = query(PageLayoutMode::SinglePage);
        q.zoom = 0.0;
        assert!(layout.extent(q).is_err());
        assert!(layout
            .placement(1, query(PageLayoutMode::SinglePage))
            .is_err());
        assert!(PageRenderRect::new(
            0,
            ViewPoint { x: 590.0, y: 0.0 },
            ViewSize {
                width: 20.0,
                height: 20.0
            },
            ViewSize {
                width: 600.0,
                height: 800.0
            },
        )
        .is_err());
        let overflow = ViewRect {
            origin: ViewPoint {
                x: f32::MAX,
                y: f32::MAX,
            },
            size: ViewSize {
                width: f32::MAX,
                height: f32::MAX,
            },
        };
        assert!(layout
            .visible_pages(overflow, query(PageLayoutMode::SinglePageContinuous))
            .is_err());
    }

    #[test]
    fn visible_pages_returns_nothing_outside_the_document() {
        let layout = layout(3);
        let q = query(PageLayoutMode::SinglePageContinuous);
        for visible in [
            ViewRect {
                origin: ViewPoint {
                    x: 10_000.0,
                    y: 0.0,
                },
                size: VIEWPORT,
            },
            ViewRect {
                origin: ViewPoint {
                    x: 0.0,
                    y: 10_000.0,
                },
                size: VIEWPORT,
            },
        ] {
            assert!(layout.visible_pages(visible, q).unwrap().is_empty());
        }
    }

    #[test]
    fn every_page_alignment_uses_and_clamps_its_own_formula() {
        let layout = layout(3);
        let q = query(PageLayoutMode::SinglePageContinuous);
        let start = layout
            .scroll_origin_for_page(1, PageAlignment::Start, 0.0, q)
            .unwrap();
        let center = layout
            .scroll_origin_for_page(1, PageAlignment::Center, 0.0, q)
            .unwrap();
        let end = layout
            .scroll_origin_for_page(1, PageAlignment::End, 0.0, q)
            .unwrap();
        assert!(center.y < start.y);
        assert!(end.y < center.y);
        assert_eq!(
            layout
                .scroll_origin_for_page(0, PageAlignment::End, 0.0, q)
                .unwrap()
                .y,
            0.0
        );
        let extent = layout.extent(q).unwrap();
        assert_eq!(
            layout
                .scroll_origin_for_page(2, PageAlignment::End, 0.0, q)
                .unwrap()
                .y,
            extent.height - VIEWPORT.height
        );
    }

    #[test]
    fn a_jump_to_page_900_lands_there_with_twelve_pages_measured() {
        let mut layout = layout(1_000);
        for page in 1..12 {
            layout
                .measure_page(geometry(page, 580.0 + page as f64, 780.0 + page as f64))
                .unwrap();
        }
        let mut q = query(PageLayoutMode::SinglePageContinuous);
        q.current_page = 899;
        let origin = layout
            .scroll_origin_for_page(899, PageAlignment::Start, 0.0, q)
            .unwrap();
        let visible = ViewRect {
            origin,
            size: VIEWPORT,
        };
        let pages = layout.visible_pages(visible, q).unwrap();
        assert!(pages.iter().any(|page| page.page == 899));
        assert!(
            pages.len() < 8,
            "visible query returned {} pages",
            pages.len()
        );
    }

    fn one_page_pdf() -> Vec<u8> {
        let objects: [&[u8]; 3] = [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources <<>> >>",
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        out
    }
}
