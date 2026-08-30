//! What the window is showing: pan offset, zoom, layout mode, view rotation.
//!
//! [`Viewport`] is the only place that turns a scroll, a pinch or a page jump
//! into an offset and a zoom. It owns a [`Layout`] and asks it where pages sit;
//! it never places pages itself, and nothing above it (the GPUI canvas, the MCP
//! verbs) places them either. See [`crate::layout`] for the estimate-and-refine
//! scroll metric every position here is expressed in.
//!
//! Two invariants run through the file. Zoom only moves through
//! [`Viewport::set_zoom`] or [`Viewport::apply_fit`], so it is clamped to
//! [`Viewport::zoom_limits`] exactly once. Anything that reflows the layout
//! (a measurement, a mode change, a rotation) captures an [`Anchor`] first and
//! restores it after, so the page point in the middle of the view stays there;
//! re-centring belongs only to the calls that explicitly ask for it.

use std::fmt;

use crate::history::ViewState;
use crate::layout;
use crate::layout::{Layout, LayoutError, LayoutQuery};
use crate::{
    GeometryError, PageAlignment, PageGeometry, PageIndex, PageLayoutMode, PagePlacement,
    PagePoint, PageQuad, PageRenderRect, ViewPoint, ViewRect, ViewRotation, ViewSize,
};
use onionskin_render::MAX_RASTER_AXIS;

/// The zoom range the product offers, matching Acrobat's 5% to 3200%.
///
/// This is a UI range, not a renderable one. The renderer sizes pixmaps with
/// `u16` ([`MAX_RASTER_AXIS`]) and refuses anything under one pixel, so the
/// largest page a PDF can declare (14400 pt) runs out of raster at about 4.5x,
/// far inside [`MAX_ZOOM`]. [`Viewport::zoom_limits`] narrows this range to
/// what the current page can be rasterized at.
pub const MIN_ZOOM: f32 = 0.05;
/// See [`MIN_ZOOM`].
pub const MAX_ZOOM: f32 = 32.0;
/// One zoom-button press: the fourth root of two, so four presses double.
const ZOOM_STEP: f32 = 1.189_207_1;
/// Trackpad pixels of a modified scroll that double the zoom.
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

    pub fn page_geometry(&self, page: PageIndex) -> Option<&PageGeometry> {
        self.layout.geometry(page)
    }

    /// Refine the layout with a page's real geometry.
    ///
    /// A measurement arrives while the user is reading, so it must not move
    /// what they are looking at: the anchor captured before the refinement is
    /// restored afterwards under both zoom policies. Under a fit policy the
    /// zoom is re-derived as well, because the refinement can change the
    /// current page's size, but the re-centring stays with the explicit
    /// [`Viewport::fit`] call that asked for it.
    pub fn measure_page(&mut self, geometry: PageGeometry) -> Result<(), ViewportError> {
        let page = geometry.index;
        let anchor = match self.anchor_for_current_page() {
            Ok(anchor) => Some(anchor),
            // Before the first measurement the layout has no estimate, so
            // there is no anchor to preserve. Every other failure is real.
            Err(ViewportError::Layout(LayoutError::MissingEstimate) | ViewportError::NoPages) => {
                None
            }
            Err(error) => return Err(error),
        };
        self.layout.measure_page(geometry)?;
        if page != 0 && self.layout.geometry(0).is_none() {
            return Ok(());
        }
        let Some(anchor) = anchor else {
            // Nothing to preserve: this measurement is what makes the layout
            // usable, so give the policy its opening position.
            return match self.zoom_policy {
                ZoomPolicy::Fit(mode) => self.apply_fit(mode),
                ZoomPolicy::Fixed => {
                    self.clamp_offset()?;
                    self.sync_current_page()
                }
            };
        };
        if let ZoomPolicy::Fit(mode) = self.zoom_policy {
            self.zoom = self.fit_zoom(mode)?.0;
        }
        self.restore_anchor(anchor)?;
        self.sync_current_page()
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

    /// Fit `mode`, and hold it: a later resize or measurement re-derives the
    /// zoom rather than leaving a stale one.
    pub fn fit(&mut self, mode: FitMode) -> Result<(), ViewportError> {
        self.apply_fit(mode)
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
        self.set_zoom(zoom, anchor)
    }

    pub fn zoom_at(&mut self, factor: f32, anchor: ViewPoint) -> Result<(), ViewportError> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(LayoutError::InvalidZoom(factor).into());
        }
        let requested = self.zoom * factor;
        if !requested.is_finite() || requested <= 0.0 {
            return Err(LayoutError::InvalidZoom(requested).into());
        }
        self.set_zoom(requested, anchor)
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

    /// A pinch gesture: the named entry point for [`Self::zoom_at`].
    ///
    /// A factor that is not a positive finite number is an error here for the
    /// same reason it is there. This used to answer `Ok(false)` instead, so
    /// the same nonsense factor was a refusal through one door and a silent
    /// no-op through the other. Deciding that a particular OS gesture is noise
    /// worth dropping before it gets this far is the shell's job.
    pub fn pinch(&mut self, factor: f32, at: ViewPoint) -> Result<(), ViewportError> {
        self.zoom_at(factor, at)
    }

    pub fn dynamic_zoom(&mut self, delta_y: f32, at: ViewPoint) -> Result<(), ViewportError> {
        validate_anchor(at, self.size)?;
        if !delta_y.is_finite() {
            return Err(ViewportError::InvalidDelta(delta_y));
        }
        if delta_y == 0.0 {
            return Ok(());
        }
        let exponent = delta_y / SCROLL_ZOOM_PIXELS_PER_DOUBLING;
        let (floor, ceiling) = self.zoom_limits()?;
        let minimum = (floor / self.zoom).log2();
        let maximum = (ceiling / self.zoom).log2();
        self.zoom_at(2.0_f32.powf(exponent.clamp(minimum, maximum)), at)
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
        let offset = self
            .layout
            .scroll_origin_for_page(page, alignment, self.offset.x, query)?;
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
        // Unrotate in device pixels, the unit `device_to_user` wants, rather
        // than dividing into page units only to multiply straight back out.
        let rotated = ViewPoint {
            x: point.x - placement.rect.origin.x,
            y: point.y - placement.rect.origin.y,
        };
        let page_size = layout::render_size(geometry);
        let device_size = ViewSize {
            width: page_size.width * self.zoom,
            height: page_size.height * self.zoom,
        };
        let unrotated = self.rotation.unrotate_point(rotated, device_size);
        Ok(Some(geometry.device_to_user(
            f64::from(unrotated.x),
            f64::from(unrotated.y),
            self.zoom,
        )?))
    }

    /// Where a page point sits in the viewport, the forward direction of
    /// [`Viewport::page_point_at`]. `None` when the page is not laid out in
    /// the current mode. Tools that move the view work in page space and
    /// need this to say what a gesture means in viewport pixels.
    pub fn view_point_for(&self, point: PagePoint) -> Result<Option<ViewPoint>, ViewportError> {
        let geometry = self
            .layout
            .geometry(point.page)
            .ok_or(ViewportError::UnmeasuredPage(point.page))?;
        let Some(placement) = self.layout.placement(point.page, self.query())? else {
            return Ok(None);
        };
        let (x, y) = geometry.user_to_device_point(point, self.zoom)?;
        let unrotated = ViewPoint {
            x: (x / f64::from(self.zoom)) as f32,
            y: (y / f64::from(self.zoom)) as f32,
        };
        let page_size = ViewSize {
            width: geometry.render_size.0 as f32,
            height: geometry.render_size.1 as f32,
        };
        let rotated = self.rotation.rotate_point(unrotated, page_size);
        Ok(Some(ViewPoint {
            x: placement.rect.origin.x + rotated.x * self.zoom - self.offset.x,
            y: placement.rect.origin.y + rotated.y * self.zoom - self.offset.y,
        }))
    }

    /// Where page-space quads land in the viewport right now, one rectangle per
    /// quad, in the same order.
    ///
    /// This is the direction [`Viewport::page_point_at`] does not go, and what
    /// turns a search hit into a highlight. The rectangle bounds the quad: text
    /// drawn on a slant, or a page shown rotated, still highlights as an
    /// upright box, which is what a viewer draws.
    ///
    /// Empty when the page is not currently laid out or has not been measured
    /// yet - a highlight has nowhere to land until the page has geometry, and
    /// the caller asks again when it arrives. A quad belonging to another page
    /// is an error rather than an empty answer.
    ///
    /// Unlike [`Viewport::view_point_for`], which starts from the layout, this
    /// starts from the placements the viewport is showing, so the offset is
    /// already in them.
    pub fn page_quad_rects(
        &self,
        page: PageIndex,
        quads: &[PageQuad],
    ) -> Result<Vec<ViewRect>, ViewportError> {
        let Some(placement) = self
            .visible_pages()?
            .into_iter()
            .find(|placement| placement.page == page)
        else {
            return Ok(Vec::new());
        };
        let Some(geometry) = self.layout.geometry(page) else {
            return Ok(Vec::new());
        };
        let page_size = ViewSize {
            width: geometry.render_size.0 as f32,
            height: geometry.render_size.1 as f32,
        };
        quads
            .iter()
            .map(|quad| {
                let device = geometry.user_to_device(*quad, 1.0)?;
                let corners = device.corners.map(|(x, y)| {
                    let rotated = self.rotation.rotate_point(
                        ViewPoint {
                            x: x as f32,
                            y: y as f32,
                        },
                        page_size,
                    );
                    ViewPoint {
                        x: placement.rect.origin.x + rotated.x * self.zoom,
                        y: placement.rect.origin.y + rotated.y * self.zoom,
                    }
                });
                Ok(bounding_rect(corners))
            })
            .collect()
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

    /// Put the view back exactly where a [`Self::snapshot`] left it.
    ///
    /// The offset is restored verbatim and deliberately not re-clamped to the
    /// current extent. A snapshot's offset is frequently outside the scroll
    /// clamps by design (an anchored zoom leaves a negative x when the page is
    /// narrower than the viewport), and clamping it would make "previous view"
    /// land somewhere the user never was. The caller invariant that makes this
    /// safe: `state` came from `snapshot` on a viewport over *this* document.
    /// Everything cross-document that can be checked is checked here (page
    /// count, zoom range, and that a fit-visible rect still fits its page);
    /// what cannot be checked is that the pages have the same sizes.
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
                layout::render_size(geometry),
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
        let (minimum, maximum) = self.zoom_limits()?;
        let anchor = self.anchor_at(anchor)?;
        self.zoom = zoom.clamp(minimum, maximum);
        self.zoom_policy = ZoomPolicy::Fixed;
        self.restore_anchor(anchor)?;
        self.sync_current_page()
    }

    /// The zoom range the current page can actually be rasterized in.
    ///
    /// Product decision: a gesture past the raster ceiling is capped, not
    /// refused. Refusing it stops a 14400 pt page zooming with an error the
    /// user cannot act on; letting it through fails the frame with
    /// [`onionskin_render::RenderError::UnrenderableSize`] instead.
    fn zoom_limits(&self) -> Result<(f32, f32), ViewportError> {
        match self.layout.page_render_size(self.current_page) {
            Ok(size) => Ok(zoom_limits_for(size)),
            // No page is measured yet, so there is no raster to be bounded by.
            Err(LayoutError::MissingEstimate | LayoutError::NoSuchPage { .. }) => {
                Ok((MIN_ZOOM, MAX_ZOOM))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// The zoom `mode` asks for, and the page-space rect it wants centred.
    ///
    /// Split out of [`Self::apply_fit`] so a measurement can re-derive the
    /// zoom without also re-centring the view the user has scrolled.
    fn fit_zoom(&self, mode: FitMode) -> Result<(f32, Option<ViewRect>), ViewportError> {
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
                let page_size = layout::render_size(geometry);
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
        let (minimum, maximum) = self.zoom_limits()?;
        Ok((zoom.clamp(minimum, maximum), visible_target))
    }

    fn apply_fit(&mut self, mode: FitMode) -> Result<(), ViewportError> {
        let (zoom, visible_target) = self.fit_zoom(mode)?;
        self.zoom = zoom;
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
                self.offset.x,
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

fn bounding_rect(corners: [ViewPoint; 4]) -> ViewRect {
    let (mut left, mut top) = (f32::INFINITY, f32::INFINITY);
    let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for corner in corners {
        left = left.min(corner.x);
        right = right.max(corner.x);
        top = top.min(corner.y);
        bottom = bottom.max(corner.y);
    }
    ViewRect {
        origin: ViewPoint { x: left, y: top },
        size: ViewSize {
            width: right - left,
            height: bottom - top,
        },
    }
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

/// Narrow [`MIN_ZOOM`]..=[`MAX_ZOOM`] to what a page of `size` points can be
/// rasterized at: at least one pixel on its short axis, at most
/// [`MAX_RASTER_AXIS`] on its long one.
fn zoom_limits_for(size: ViewSize) -> (f32, f32) {
    let longest = size.width.max(size.height);
    let shortest = size.width.min(size.height);
    // `raster_size` floors an `f32` product, so step one ulp up rather than
    // land on the pixel boundary the floor would round away.
    let minimum = MIN_ZOOM.max((1.0 / shortest).next_up());
    let maximum = MAX_ZOOM.min(MAX_RASTER_AXIS as f32 / longest);
    (minimum, minimum.max(maximum))
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
