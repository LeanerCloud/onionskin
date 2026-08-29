use std::fmt;

use crate::layout::{Layout, LayoutError, LayoutQuery};
use crate::{
    GeometryError, PageAlignment, PageGeometry, PageIndex, PageLayoutMode, PagePlacement,
    PagePoint, PageRenderRect, ViewPoint, ViewRect, ViewRotation, ViewSize,
};

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 32.0;
const ZOOM_STEP: f32 = 1.189_207_1;
const SCROLL_ZOOM_PIXELS_PER_DOUBLING: f32 = 240.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FitMode {
    Page,
    Width,
    Height,
    Visible(PageRenderRect),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomPolicy {
    Fixed,
    Fit(FitMode),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    pub current_page: PageIndex,
    pub offset: ViewPoint,
    pub zoom: f32,
    pub zoom_policy: ZoomPolicy,
    pub mode: PageLayoutMode,
    pub show_cover: bool,
    pub rotation: ViewRotation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewportError {
    Layout(LayoutError),
    Geometry(GeometryError),
    InvalidPoint(ViewPoint),
    AnchorOutsideViewport(ViewPoint),
    InvalidDelta(f32),
    NoPages,
    UnmeasuredPage(PageIndex),
    FitVisiblePageMismatch {
        bounds: PageIndex,
        current: PageIndex,
    },
}

impl fmt::Display for ViewportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => write!(f, "{error}"),
            Self::Geometry(error) => write!(f, "{error}"),
            Self::InvalidPoint(point) => {
                write!(
                    f,
                    "viewport point ({}, {}) must be finite",
                    point.x, point.y
                )
            }
            Self::AnchorOutsideViewport(point) => write!(
                f,
                "zoom anchor ({}, {}) lies outside the viewport",
                point.x, point.y
            ),
            Self::InvalidDelta(delta) => write!(f, "zoom delta must be finite, got {delta}"),
            Self::NoPages => write!(f, "the document has no pages"),
            Self::UnmeasuredPage(page) => write!(f, "page {page} has not been measured"),
            Self::FitVisiblePageMismatch { bounds, current } => write!(
                f,
                "visible bounds belong to page {bounds}, not current page {current}"
            ),
        }
    }
}

impl std::error::Error for ViewportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::Geometry(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LayoutError> for ViewportError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<GeometryError> for ViewportError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
    }
}

pub struct Viewport {
    layout: Layout,
    size: ViewSize,
    offset: ViewPoint,
    zoom: f32,
    zoom_policy: ZoomPolicy,
    mode: PageLayoutMode,
    show_cover: bool,
    rotation: ViewRotation,
    current_page: PageIndex,
}

impl Viewport {
    pub fn new(page_count: usize, size: ViewSize, page_gap: f32) -> Result<Self, ViewportError> {
        if !size.is_valid() {
            return Err(LayoutError::InvalidViewport.into());
        }
        Ok(Self {
            layout: Layout::new(page_count, page_gap)?,
            size,
            offset: ViewPoint::default(),
            zoom: 1.0,
            zoom_policy: ZoomPolicy::Fixed,
            mode: PageLayoutMode::SinglePageContinuous,
            show_cover: false,
            rotation: ViewRotation::None,
            current_page: 0,
        })
    }

    pub fn page_count(&self) -> usize {
        self.layout.page_count()
    }

    pub fn size(&self) -> ViewSize {
        self.size
    }

    pub fn offset(&self) -> ViewPoint {
        self.offset
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn zoom_policy(&self) -> ZoomPolicy {
        self.zoom_policy
    }

    pub fn mode(&self) -> PageLayoutMode {
        self.mode
    }

    pub fn show_cover(&self) -> bool {
        self.show_cover
    }

    pub fn rotation(&self) -> ViewRotation {
        self.rotation
    }

    pub fn current_page(&self) -> PageIndex {
        self.current_page
    }

    pub fn measure_page(&mut self, geometry: PageGeometry) -> Result<(), ViewportError> {
        let anchor = self.anchor_for_current_page().ok();
        self.layout.measure_page(geometry)?;
        match self.zoom_policy {
            ZoomPolicy::Fit(mode) => self.apply_fit(mode)?,
            ZoomPolicy::Fixed => {
                if let Some(anchor) = anchor {
                    self.restore_anchor(anchor)?;
                    self.sync_current_page()?;
                } else {
                    self.clamp_offset()?;
                    self.sync_current_page()?;
                }
            }
        }
        Ok(())
    }

    pub fn resize(&mut self, size: ViewSize) -> Result<(), ViewportError> {
        if !size.is_valid() {
            return Err(LayoutError::InvalidViewport.into());
        }
        self.size = size;
        match self.zoom_policy {
            ZoomPolicy::Fit(mode) => self.apply_fit(mode),
            ZoomPolicy::Fixed => self.clamp_offset(),
        }
    }

    pub fn set_mode(&mut self, mode: PageLayoutMode) -> Result<(), ViewportError> {
        let anchor = self.anchor_for_current_page()?;
        self.mode = mode;
        self.reflow_after_change(anchor)
    }

    pub fn set_show_cover(&mut self, show_cover: bool) -> Result<(), ViewportError> {
        let anchor = self.anchor_for_current_page()?;
        self.show_cover = show_cover;
        self.reflow_after_change(anchor)
    }

    pub fn set_rotation(&mut self, rotation: ViewRotation) -> Result<(), ViewportError> {
        let mut anchor = self.anchor_for_current_page()?;
        let page_size = self.layout.page_render_size(anchor.page)?;
        let intrinsic = self.rotation.unrotate_point(anchor.local, page_size);
        anchor.local = rotation.rotate_point(intrinsic, page_size);
        self.rotation = rotation;
        self.reflow_after_change(anchor)
    }

    pub fn fit(&mut self, mode: FitMode) -> Result<(), ViewportError> {
        self.apply_fit(mode)?;
        self.zoom_policy = ZoomPolicy::Fit(mode);
        Ok(())
    }

    pub fn actual_size(&mut self) -> Result<(), ViewportError> {
        self.zoom_to(1.0, viewport_center(self.size))
    }

    pub fn zoom_in(&mut self, anchor: ViewPoint) -> Result<(), ViewportError> {
        self.zoom_at(ZOOM_STEP, anchor)
    }

    pub fn zoom_out(&mut self, anchor: ViewPoint) -> Result<(), ViewportError> {
        self.zoom_at(1.0 / ZOOM_STEP, anchor)
    }

    pub fn zoom_to(&mut self, zoom: f32, anchor: ViewPoint) -> Result<(), ViewportError> {
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(LayoutError::InvalidZoom(zoom).into());
        }
        self.set_zoom(zoom.clamp(MIN_ZOOM, MAX_ZOOM), anchor)
    }

    pub fn zoom_at(&mut self, factor: f32, anchor: ViewPoint) -> Result<(), ViewportError> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(LayoutError::InvalidZoom(factor).into());
        }
        self.set_zoom((self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM), anchor)
    }

    pub fn pan_by(&mut self, delta: ViewPoint) -> Result<(), ViewportError> {
        validate_point(delta)?;
        self.offset.x -= delta.x;
        self.offset.y -= delta.y;
        self.clamp_offset()?;
        self.sync_current_page()
    }

    pub fn scroll(
        &mut self,
        delta: ViewPoint,
        zooming: bool,
        at: ViewPoint,
    ) -> Result<(), ViewportError> {
        validate_point(delta)?;
        if zooming {
            self.dynamic_zoom(delta.y, at)
        } else {
            self.pan_by(delta)
        }
    }

    pub fn pinch(&mut self, factor: f32, at: ViewPoint) -> Result<bool, ViewportError> {
        validate_anchor(at, self.size)?;
        if !(factor.is_finite() && factor > 0.0) {
            return Ok(false);
        }
        self.zoom_at(factor, at)?;
        Ok(true)
    }

    pub fn dynamic_zoom(&mut self, delta_y: f32, at: ViewPoint) -> Result<(), ViewportError> {
        validate_anchor(at, self.size)?;
        if !delta_y.is_finite() {
            return Err(ViewportError::InvalidDelta(delta_y));
        }
        if delta_y == 0.0 {
            return Ok(());
        }
        self.zoom_at(2.0_f32.powf(delta_y / SCROLL_ZOOM_PIXELS_PER_DOUBLING), at)
    }

    pub fn go_to_page(
        &mut self,
        page: PageIndex,
        alignment: PageAlignment,
    ) -> Result<(), ViewportError> {
        let query = LayoutQuery {
            current_page: page,
            ..self.query()
        };
        let offset = self.layout.scroll_origin_for_page(page, alignment, query)?;
        let zoom_policy = if matches!(
            self.zoom_policy,
            ZoomPolicy::Fit(FitMode::Visible(bounds)) if bounds.page() != page
        ) {
            ZoomPolicy::Fixed
        } else {
            self.zoom_policy
        };
        self.current_page = page;
        self.offset = offset;
        self.zoom_policy = zoom_policy;
        if let ZoomPolicy::Fit(mode) = self.zoom_policy {
            self.apply_fit(mode)?;
        }
        Ok(())
    }

    pub fn first_page(&mut self) -> Result<(), ViewportError> {
        self.require_pages()?;
        self.go_to_page(0, PageAlignment::Start)
    }

    pub fn previous_page(&mut self) -> Result<(), ViewportError> {
        self.require_pages()?;
        self.go_to_page(self.current_page.saturating_sub(1), PageAlignment::Start)
    }

    pub fn next_page(&mut self) -> Result<(), ViewportError> {
        self.require_pages()?;
        self.go_to_page(
            (self.current_page + 1).min(self.page_count() - 1),
            PageAlignment::Start,
        )
    }

    pub fn last_page(&mut self) -> Result<(), ViewportError> {
        self.require_pages()?;
        self.go_to_page(self.page_count() - 1, PageAlignment::Start)
    }

    pub fn visible_pages(&self) -> Result<Vec<PagePlacement>, ViewportError> {
        let visible = ViewRect {
            origin: self.offset,
            size: self.size,
        };
        Ok(self
            .layout
            .visible_pages(visible, self.query())?
            .into_iter()
            .map(|mut placement| {
                placement.rect.origin.x -= self.offset.x;
                placement.rect.origin.y -= self.offset.y;
                placement
            })
            .collect())
    }

    pub fn page_point_at(&self, point: ViewPoint) -> Result<Option<PagePoint>, ViewportError> {
        validate_point(point)?;
        let Some(placement) = self
            .visible_pages()?
            .into_iter()
            .find(|placement| contains(placement.rect, point))
        else {
            return Ok(None);
        };
        let geometry = self
            .layout
            .geometry(placement.page)
            .ok_or(ViewportError::UnmeasuredPage(placement.page))?;
        let rotated = ViewPoint {
            x: (point.x - placement.rect.origin.x) / self.zoom,
            y: (point.y - placement.rect.origin.y) / self.zoom,
        };
        let page_size = ViewSize {
            width: geometry.render_size.0 as f32,
            height: geometry.render_size.1 as f32,
        };
        let unrotated = self.rotation.unrotate_point(rotated, page_size);
        Ok(Some(geometry.device_to_user(
            f64::from(unrotated.x * self.zoom),
            f64::from(unrotated.y * self.zoom),
            self.zoom,
        )?))
    }

    pub fn snapshot(&self) -> ViewState {
        ViewState {
            current_page: self.current_page,
            offset: self.offset,
            zoom: self.zoom,
            zoom_policy: self.zoom_policy,
            mode: self.mode,
            show_cover: self.show_cover,
            rotation: self.rotation,
        }
    }

    pub fn restore(&mut self, state: ViewState) -> Result<(), ViewportError> {
        if !state.zoom.is_finite() || !(MIN_ZOOM..=MAX_ZOOM).contains(&state.zoom) {
            return Err(LayoutError::InvalidZoom(state.zoom).into());
        }
        validate_point(state.offset)?;
        if self.page_count() == 0 {
            if state.current_page != 0 {
                return Err(LayoutError::NoSuchPage {
                    page: state.current_page,
                    count: 0,
                }
                .into());
            }
        } else if state.current_page >= self.page_count() {
            return Err(LayoutError::NoSuchPage {
                page: state.current_page,
                count: self.page_count(),
            }
            .into());
        }
        if let ZoomPolicy::Fit(FitMode::Visible(bounds)) = state.zoom_policy {
            if bounds.page() != state.current_page {
                return Err(ViewportError::FitVisiblePageMismatch {
                    bounds: bounds.page(),
                    current: state.current_page,
                });
            }
            if self.page_count() == 0 {
                return Err(ViewportError::NoPages);
            }
            let geometry = self
                .layout
                .geometry(state.current_page)
                .ok_or(ViewportError::UnmeasuredPage(state.current_page))?;
            PageRenderRect::new(
                bounds.page(),
                bounds.origin(),
                bounds.size(),
                ViewSize {
                    width: geometry.render_size.0 as f32,
                    height: geometry.render_size.1 as f32,
                },
            )?;
        }
        self.current_page = state.current_page;
        self.offset = state.offset;
        self.zoom = state.zoom;
        self.zoom_policy = state.zoom_policy;
        self.mode = state.mode;
        self.show_cover = state.show_cover;
        self.rotation = state.rotation;
        Ok(())
    }

    fn query(&self) -> LayoutQuery {
        LayoutQuery {
            mode: self.mode,
            show_cover: self.show_cover,
            rotation: self.rotation,
            zoom: self.zoom,
            viewport: self.size,
            current_page: self.current_page,
        }
    }

    fn set_zoom(&mut self, zoom: f32, anchor: ViewPoint) -> Result<(), ViewportError> {
        validate_anchor(anchor, self.size)?;
        let anchor = self.anchor_at(anchor)?;
        self.zoom = zoom;
        self.zoom_policy = ZoomPolicy::Fixed;
        self.restore_anchor(anchor)?;
        self.sync_current_page()
    }

    fn apply_fit(&mut self, mode: FitMode) -> Result<(), ViewportError> {
        self.require_pages()?;
        let gap = self.layout.page_gap();
        let available = ViewSize {
            width: self.size.width - gap * 2.0,
            height: self.size.height - gap * 2.0,
        };
        if !available.is_valid() {
            return Err(LayoutError::InvalidViewport.into());
        }
        let (zoom, visible_target) = match mode {
            FitMode::Page | FitMode::Width | FitMode::Height => {
                let metrics = self
                    .layout
                    .row_metrics_for_page(self.current_page, self.query())?;
                let fixed_gap = metrics.between_pages as f32 * gap;
                let width = (available.width - fixed_gap) / metrics.page_width;
                let height = available.height / metrics.page_height;
                let zoom = match mode {
                    FitMode::Page => width.min(height),
                    FitMode::Width => width,
                    FitMode::Height => height,
                    FitMode::Visible(_) => unreachable!(),
                };
                (zoom, None)
            }
            FitMode::Visible(bounds) => {
                if bounds.page() != self.current_page {
                    return Err(ViewportError::FitVisiblePageMismatch {
                        bounds: bounds.page(),
                        current: self.current_page,
                    });
                }
                let geometry = self
                    .layout
                    .geometry(self.current_page)
                    .ok_or(ViewportError::UnmeasuredPage(self.current_page))?;
                let page_size = ViewSize {
                    width: geometry.render_size.0 as f32,
                    height: geometry.render_size.1 as f32,
                };
                PageRenderRect::new(bounds.page(), bounds.origin(), bounds.size(), page_size)?;
                let visible = self.rotation.rotate_rect(bounds, page_size);
                (
                    (available.width / visible.size.width)
                        .min(available.height / visible.size.height),
                    Some(visible),
                )
            }
        };
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(LayoutError::InvalidZoom(zoom).into());
        }
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.zoom_policy = ZoomPolicy::Fit(mode);
        if let Some(visible) = visible_target {
            let placement = self
                .layout
                .placement(self.current_page, self.query())?
                .ok_or(ViewportError::NoPages)?;
            self.offset = ViewPoint {
                x: placement.rect.origin.x
                    + (visible.origin.x + visible.size.width / 2.0) * self.zoom
                    - self.size.width / 2.0,
                y: placement.rect.origin.y
                    + (visible.origin.y + visible.size.height / 2.0) * self.zoom
                    - self.size.height / 2.0,
            };
            Ok(())
        } else {
            self.offset = self.layout.scroll_origin_for_page(
                self.current_page,
                PageAlignment::Center,
                self.query(),
            )?;
            self.clamp_offset()
        }
    }

    fn reflow_after_change(&mut self, anchor: Anchor) -> Result<(), ViewportError> {
        match self.zoom_policy {
            ZoomPolicy::Fit(mode) => self.apply_fit(mode),
            ZoomPolicy::Fixed => {
                self.restore_anchor(anchor)?;
                self.sync_current_page()
            }
        }
    }

    fn anchor_at(&self, screen: ViewPoint) -> Result<Anchor, ViewportError> {
        validate_point(screen)?;
        if let Some(placement) = self
            .visible_pages()?
            .into_iter()
            .find(|placement| contains(placement.rect, screen))
        {
            return Ok(Anchor {
                page: placement.page,
                local: ViewPoint {
                    x: (screen.x - placement.rect.origin.x) / self.zoom,
                    y: (screen.y - placement.rect.origin.y) / self.zoom,
                },
                screen,
            });
        }
        self.anchor_for_current_page()
    }

    fn anchor_for_current_page(&self) -> Result<Anchor, ViewportError> {
        self.require_pages()?;
        let placement = self
            .layout
            .placement(self.current_page, self.query())?
            .ok_or(ViewportError::NoPages)?;
        let screen_rect = ViewRect {
            origin: ViewPoint {
                x: placement.rect.origin.x - self.offset.x,
                y: placement.rect.origin.y - self.offset.y,
            },
            size: placement.rect.size,
        };
        let screen = ViewPoint {
            x: (screen_rect.origin.x.max(0.0) + screen_rect.right().min(self.size.width)) / 2.0,
            y: (screen_rect.origin.y.max(0.0) + screen_rect.bottom().min(self.size.height)) / 2.0,
        };
        let screen = if screen.x.is_finite()
            && screen.y.is_finite()
            && contains(
                ViewRect {
                    origin: ViewPoint::default(),
                    size: self.size,
                },
                screen,
            ) {
            screen
        } else {
            viewport_center(self.size)
        };
        Ok(Anchor {
            page: self.current_page,
            local: ViewPoint {
                x: ((screen.x - screen_rect.origin.x) / self.zoom)
                    .clamp(0.0, screen_rect.size.width / self.zoom),
                y: ((screen.y - screen_rect.origin.y) / self.zoom)
                    .clamp(0.0, screen_rect.size.height / self.zoom),
            },
            screen,
        })
    }

    fn restore_anchor(&mut self, anchor: Anchor) -> Result<(), ViewportError> {
        let query = LayoutQuery {
            current_page: anchor.page,
            ..self.query()
        };
        let placement = self
            .layout
            .placement(anchor.page, query)?
            .ok_or(ViewportError::NoPages)?;
        self.offset = ViewPoint {
            x: placement.rect.origin.x + anchor.local.x * self.zoom - anchor.screen.x,
            y: placement.rect.origin.y + anchor.local.y * self.zoom - anchor.screen.y,
        };
        Ok(())
    }

    fn clamp_offset(&mut self) -> Result<(), ViewportError> {
        let extent = self.layout.extent(self.query())?;
        self.offset.x = self
            .offset
            .x
            .clamp(0.0, (extent.width - self.size.width).max(0.0));
        self.offset.y = self
            .offset
            .y
            .clamp(0.0, (extent.height - self.size.height).max(0.0));
        Ok(())
    }

    fn sync_current_page(&mut self) -> Result<(), ViewportError> {
        if self.page_count() == 0 {
            return Ok(());
        }
        let center = viewport_center(self.size);
        if let Some(page) = self.visible_pages()?.into_iter().min_by(|a, b| {
            distance_squared(rect_center(a.rect), center)
                .total_cmp(&distance_squared(rect_center(b.rect), center))
        }) {
            self.current_page = page.page;
        }
        Ok(())
    }

    fn require_pages(&self) -> Result<(), ViewportError> {
        if self.page_count() == 0 {
            Err(ViewportError::NoPages)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
struct Anchor {
    page: PageIndex,
    local: ViewPoint,
    screen: ViewPoint,
}

fn validate_point(point: ViewPoint) -> Result<(), ViewportError> {
    if point.x.is_finite() && point.y.is_finite() {
        Ok(())
    } else {
        Err(ViewportError::InvalidPoint(point))
    }
}

fn validate_anchor(point: ViewPoint, viewport: ViewSize) -> Result<(), ViewportError> {
    validate_point(point)?;
    if point.x < 0.0 || point.y < 0.0 || point.x > viewport.width || point.y > viewport.height {
        Err(ViewportError::AnchorOutsideViewport(point))
    } else {
        Ok(())
    }
}

fn viewport_center(size: ViewSize) -> ViewPoint {
    ViewPoint {
        x: size.width / 2.0,
        y: size.height / 2.0,
    }
}

fn contains(rect: ViewRect, point: ViewPoint) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.right()
        && point.y >= rect.origin.y
        && point.y <= rect.bottom()
}

fn rect_center(rect: ViewRect) -> ViewPoint {
    ViewPoint {
        x: rect.origin.x + rect.size.width / 2.0,
        y: rect.origin.y + rect.size.height / 2.0,
    }
}

fn distance_squared(a: ViewPoint, b: ViewPoint) -> f32 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}
