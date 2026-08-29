use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Weak};

use gpui::{point, px, Modifiers as GpuiModifiers, Pixels, Point, RenderImage};
use onionskin_core::{
    Document, FitMode, PageGeometryResponse, PageIndex, PagePlacement, RenderRequest,
    RenderResponse, ViewPoint, ViewRect, ViewRotation, ViewSize, Viewport, ViewportError,
};
use onionskin_plugin_api::{PluginRegistry, PointerInput, ToolCtx};
use onionskin_render::{BaseRaster, Tile, TileStore, TILE_SIZE};
use smallvec::smallvec;

use super::input::{
    pointer_input, validate_pressure, DragKind, DragUpdate, InputError, InputState,
};

const PAGE_GAP: f32 = 12.0;

#[derive(Debug)]
pub enum CanvasError {
    EmptyDocument,
    GenerationExhausted,
    Core(onionskin_core::Error),
    Viewport(ViewportError),
    Input(InputError),
    Render(onionskin_render::RenderError),
    InvalidImageBuffer {
        expected: usize,
        actual: usize,
    },
    InvalidImageCrop {
        width: u32,
        height: u32,
        crop_width: u32,
        crop_height: u32,
    },
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDocument => write!(f, "the PDF has no pages"),
            Self::GenerationExhausted => write!(f, "the render generation counter is exhausted"),
            Self::Core(error) => write!(f, "{error}"),
            Self::Viewport(error) => write!(f, "{error}"),
            Self::Input(error) => write!(f, "{error}"),
            Self::Render(error) => write!(f, "{error}"),
            Self::InvalidImageBuffer { expected, actual } => {
                write!(f, "tile image needs {expected} RGBA bytes, got {actual}")
            }
            Self::InvalidImageCrop {
                width,
                height,
                crop_width,
                crop_height,
            } => write!(
                f,
                "tile crop {crop_width}x{crop_height} is outside {width}x{height}"
            ),
        }
    }
}

impl std::error::Error for CanvasError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::Viewport(error) => Some(error),
            Self::Input(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::EmptyDocument
            | Self::GenerationExhausted
            | Self::InvalidImageBuffer { .. }
            | Self::InvalidImageCrop { .. } => None,
        }
    }
}

impl From<onionskin_core::Error> for CanvasError {
    fn from(error: onionskin_core::Error) -> Self {
        Self::Core(error)
    }
}

impl From<ViewportError> for CanvasError {
    fn from(error: ViewportError) -> Self {
        Self::Viewport(error)
    }
}

impl From<InputError> for CanvasError {
    fn from(error: InputError) -> Self {
        Self::Input(error)
    }
}

impl From<onionskin_render::RenderError> for CanvasError {
    fn from(error: onionskin_render::RenderError) -> Self {
        Self::Render(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasStatus {
    Warning {
        page: PageIndex,
        message: String,
    },
    Error {
        page: Option<PageIndex>,
        message: String,
    },
}

pub struct PagePaint {
    pub page: PageIndex,
    pub rect: ViewRect,
}

pub struct TilePaint {
    pub page: PageIndex,
    pub rect: ViewRect,
    pub source_zoom: f32,
    pub image: Arc<RenderImage>,
}

#[derive(Default)]
pub struct PaintList {
    pub pages: Vec<PagePaint>,
    pub tiles: Vec<TilePaint>,
}

#[derive(Clone, PartialEq, Eq)]
struct RenderSignature {
    pages: Vec<PageIndex>,
    zoom_bits: u32,
}

#[derive(Clone, Copy)]
enum ToolPointerPhase {
    Down,
    Move,
    Up,
}

pub struct CanvasModel {
    document: Document,
    viewport: Viewport,
    registry: PluginRegistry,
    active_tool: Option<usize>,
    input: InputState,
    tiles: TileStore,
    sources: BTreeMap<PageIndex, BaseRaster>,
    geometry_requests: BTreeSet<PageIndex>,
    requests: BTreeMap<PageIndex, RenderRequest>,
    placeholders: BTreeSet<PageIndex>,
    generation: u64,
    signature: Option<RenderSignature>,
    canvas_origin: ViewPoint,
    image_cache: TileImageCache,
    status: Option<CanvasStatus>,
}

impl CanvasModel {
    pub fn new(
        mut document: Document,
        registry: PluginRegistry,
        size: ViewSize,
    ) -> Result<Self, CanvasError> {
        if document.page_count() == 0 {
            return Err(CanvasError::EmptyDocument);
        }
        let page_count = document.page_count();
        let first = document.page_geometry(0)?.clone();
        let mut viewport = Viewport::new(page_count, size, PAGE_GAP)?;
        viewport.measure_page(first)?;
        viewport.fit(FitMode::Page)?;
        let active_tool = registry.tools().next().map(|_| 0);
        Ok(Self {
            document,
            viewport,
            registry,
            active_tool,
            input: InputState::default(),
            tiles: TileStore::new(),
            sources: BTreeMap::new(),
            geometry_requests: BTreeSet::new(),
            requests: BTreeMap::new(),
            placeholders: BTreeSet::new(),
            generation: 0,
            signature: None,
            canvas_origin: ViewPoint::default(),
            image_cache: TileImageCache::default(),
            status: None,
        })
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    pub fn active_tool(&self) -> Option<usize> {
        self.active_tool
    }

    pub fn canvas_origin(&self) -> ViewPoint {
        self.canvas_origin
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn status(&self) -> Option<&CanvasStatus> {
        self.status.as_ref()
    }

    pub fn has_pending_render(&self) -> bool {
        !self.requests.is_empty()
    }

    pub fn has_pending_work(&self) -> bool {
        !self.geometry_requests.is_empty() || self.has_pending_render()
    }

    pub fn resize(&mut self, origin: ViewPoint, size: ViewSize) -> Result<(), CanvasError> {
        self.canvas_origin = origin;
        if self.viewport.size() != size {
            self.viewport.resize(size)?;
        }
        Ok(())
    }

    pub fn scroll(
        &mut self,
        delta: ViewPoint,
        zooming: bool,
        at: ViewPoint,
    ) -> Result<(), CanvasError> {
        self.viewport.scroll(delta, zooming, at)?;
        Ok(())
    }

    pub fn pinch(&mut self, factor: f32, at: ViewPoint) -> Result<bool, CanvasError> {
        Ok(self.viewport.pinch(factor, at)?)
    }

    pub fn pointer_down(
        &mut self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<bool, CanvasError> {
        validate_pressure(pressure)?;
        if self.active_tool.is_none() {
            self.input.begin_pan(window_point(position));
            return Ok(true);
        }

        let Some(input) = self.map_pointer(position, pressure, modifiers)? else {
            return Ok(false);
        };
        self.input.begin_tool();
        self.dispatch_tool(ToolPointerPhase::Down, input);
        Ok(true)
    }

    pub fn pointer_move(
        &mut self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
        left_button_pressed: bool,
    ) -> Result<bool, CanvasError> {
        let update = self
            .input
            .move_to(window_point(position), left_button_pressed);
        if let Err(error) = validate_pressure(pressure) {
            let cancelled_tool = matches!(update, Some(DragUpdate::CancelTool))
                || matches!(self.input.cancel(), Some(DragUpdate::CancelTool));
            if cancelled_tool {
                self.cancel_active_tool();
            }
            return Err(error.into());
        }
        match update {
            Some(DragUpdate::PanBy(delta)) => {
                self.viewport.pan_by(delta)?;
                Ok(true)
            }
            Some(DragUpdate::ToolMove) => {
                let input = match self.map_pointer(position, pressure, modifiers) {
                    Ok(Some(input)) => input,
                    Ok(None) => {
                        self.input.cancel();
                        self.cancel_active_tool();
                        return Ok(true);
                    }
                    Err(error) => {
                        self.input.cancel();
                        self.cancel_active_tool();
                        return Err(error);
                    }
                };
                self.dispatch_tool(ToolPointerPhase::Move, input);
                Ok(true)
            }
            Some(DragUpdate::CancelTool) => {
                self.cancel_active_tool();
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn pointer_up(
        &mut self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<bool, CanvasError> {
        let drag = self.input.end();
        if let Err(error) = validate_pressure(pressure) {
            if drag == Some(DragKind::Tool) {
                self.cancel_active_tool();
            }
            return Err(error.into());
        }
        match drag {
            Some(DragKind::Pan) => Ok(true),
            Some(DragKind::Tool) => {
                match self.map_pointer(position, pressure, modifiers) {
                    Ok(Some(input)) => self.dispatch_tool(ToolPointerPhase::Up, input),
                    Ok(None) => self.cancel_active_tool(),
                    Err(error) => {
                        self.cancel_active_tool();
                        return Err(error);
                    }
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn cancel_pointer_gesture(&mut self) -> bool {
        match self.input.end() {
            Some(DragKind::Tool) => {
                self.cancel_active_tool();
                true
            }
            Some(DragKind::Pan) => true,
            None => false,
        }
    }

    pub fn update(&mut self) -> Result<(), CanvasError> {
        self.drain_render_responses()?;
        self.drain_geometry_responses()?;
        self.queue_visible_geometry()?;
        self.drain_geometry_responses()?;

        let visible = self.viewport.visible_pages()?;
        self.update_signature(&visible)?;
        self.retain_visible_state(&visible);
        self.pin_visible_tiles(&visible);
        self.schedule_visible_renders(&visible)?;
        self.drain_render_responses()?;
        Ok(())
    }

    pub fn drain_geometry_responses(&mut self) -> Result<usize, CanvasError> {
        let mut drained = 0;
        while let Some(response) = self.document.try_page_geometry_response()? {
            drained += 1;
            self.apply_geometry_response(response)?;
        }
        Ok(drained)
    }

    pub fn drain_render_responses(&mut self) -> Result<usize, CanvasError> {
        self.sync_signature()?;
        let mut drained = 0;
        while let Some(response) = self.document.try_render_response()? {
            drained += usize::from(self.apply_render_response(response));
        }
        Ok(drained)
    }

    pub fn paint_list(&mut self) -> Result<PaintList, CanvasError> {
        let visible = self.viewport.visible_pages()?;
        let mut paint = PaintList {
            pages: visible
                .iter()
                .map(|placement| PagePaint {
                    page: placement.page,
                    rect: placement.rect,
                })
                .collect(),
            tiles: Vec::new(),
        };
        let rotation = self.viewport.rotation();
        let exact_zoom = self.viewport.zoom();
        let viewport_size = self.viewport.size();
        let mut displayed_images = BTreeSet::new();

        for placement in visible.into_iter().filter(|page| page.measured) {
            let Some((source_zoom, raster_width, raster_height)) =
                self.paint_source(placement.page, exact_zoom)?
            else {
                continue;
            };
            let source = RasterPaintSource {
                page: placement.page,
                zoom: source_zoom,
                size: (raster_width, raster_height),
                rect: placement.rect,
                rotation,
            };
            let raw_tiles = collect_tiles(&mut self.tiles, source, viewport_size);
            for raw in raw_tiles {
                let key = TileImageKey::new(
                    placement.page,
                    source_zoom,
                    raw.region.col,
                    raw.region.row,
                    rotation,
                );
                let image = self.image_cache.image_for(
                    key,
                    &raw.tile,
                    raw.region.width,
                    raw.region.height,
                    rotation,
                )?;
                displayed_images.insert(key);
                paint.tiles.push(TilePaint {
                    page: placement.page,
                    rect: raw.rect,
                    source_zoom,
                    image,
                });
            }
        }
        self.image_cache.retain_keys(&displayed_images);
        Ok(paint)
    }

    pub fn record_error(&mut self, error: impl fmt::Display) -> bool {
        let status = CanvasStatus::Error {
            page: None,
            message: error.to_string(),
        };
        if self.status.as_ref() == Some(&status) {
            return false;
        }
        self.status = Some(status);
        true
    }

    fn queue_visible_geometry(&mut self) -> Result<usize, CanvasError> {
        let mut queued = 0;
        for placement in self.viewport.visible_pages()? {
            if !placement.measured {
                if self.document.request_page_geometry(placement.page)? {
                    queued += 1;
                }
                self.geometry_requests.insert(placement.page);
            }
        }
        Ok(queued)
    }

    fn apply_geometry_response(
        &mut self,
        response: PageGeometryResponse,
    ) -> Result<(), CanvasError> {
        self.geometry_requests.remove(&response.page());
        match response {
            PageGeometryResponse::Ready(geometry) => self.viewport.measure_page(geometry)?,
            PageGeometryResponse::Failed { page, error } => {
                self.status = Some(CanvasStatus::Error {
                    page: Some(page),
                    message: error.to_string(),
                });
            }
        }
        Ok(())
    }

    fn update_signature(&mut self, visible: &[PagePlacement]) -> Result<bool, CanvasError> {
        let signature = RenderSignature {
            pages: visible.iter().map(|page| page.page).collect(),
            zoom_bits: self.viewport.zoom().to_bits(),
        };
        if self.signature.as_ref() == Some(&signature) {
            return Ok(false);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(CanvasError::GenerationExhausted)?;
        self.signature = Some(signature);
        self.requests.clear();
        self.placeholders.clear();
        Ok(true)
    }

    fn sync_signature(&mut self) -> Result<bool, CanvasError> {
        let visible = self.viewport.visible_pages()?;
        self.update_signature(&visible)
    }

    fn retain_visible_state(&mut self, visible: &[PagePlacement]) {
        let pages: BTreeSet<_> = visible.iter().map(|page| page.page).collect();
        self.sources.retain(|page, _| pages.contains(page));
    }

    fn pin_visible_tiles(&mut self, visible: &[PagePlacement]) {
        let exact_zoom = self.viewport.zoom();
        let mut keys = Vec::new();
        for placement in visible.iter().filter(|page| page.measured) {
            keys.push((placement.page, exact_zoom));
            if let Some(source_zoom) = self.sources.get(&placement.page).map(BaseRaster::zoom) {
                if source_zoom.to_bits() != exact_zoom.to_bits() {
                    keys.push((placement.page, source_zoom));
                }
            }
        }
        self.tiles.begin_frame(&keys);
    }

    fn schedule_visible_renders(
        &mut self,
        visible: &[PagePlacement],
    ) -> Result<usize, CanvasError> {
        let zoom = self.viewport.zoom();
        let mut queued = 0;
        for placement in visible.iter().filter(|page| page.measured) {
            let request = RenderRequest {
                page: placement.page,
                zoom,
                generation: self.generation,
            };
            if self.tiles.get(placement.page, zoom).is_some()
                || self.requests.get(&placement.page) == Some(&request)
            {
                continue;
            }
            let geometry = self
                .viewport
                .page_geometry(placement.page)
                .expect("a measured placement retains geometry")
                .clone();
            let source = self.sources.get(&placement.page);
            self.document
                .request_render_with_geometry(request, &geometry, source)?;
            self.requests.insert(placement.page, request);
            queued += 1;
        }
        Ok(queued)
    }

    fn apply_render_response(&mut self, response: RenderResponse) -> bool {
        let request = response.request();
        if self.requests.get(&request.page) != Some(&request) {
            return false;
        }

        match response {
            RenderResponse::Placeholder(placeholder) => {
                if let Some(source) = placeholder.source() {
                    self.sources
                        .entry(request.page)
                        .or_insert_with(|| source.clone());
                }
                self.placeholders.insert(request.page);
            }
            RenderResponse::Raster { render, .. } => {
                self.requests.remove(&request.page);
                self.placeholders.remove(&request.page);
                let raster = render.raster;
                self.sources.insert(request.page, raster.clone());
                self.tiles.insert(request.page, raster);
                if !render.warnings.is_empty() {
                    self.status = Some(CanvasStatus::Warning {
                        page: request.page,
                        message: format!("{:?}", render.warnings),
                    });
                }
            }
            RenderResponse::Failed { error, .. } => {
                self.requests.remove(&request.page);
                self.placeholders.remove(&request.page);
                self.status = Some(CanvasStatus::Error {
                    page: Some(request.page),
                    message: format!("page {} at {}x: {error}", request.page, request.zoom),
                });
            }
        }
        true
    }

    fn paint_source(
        &mut self,
        page: PageIndex,
        exact_zoom: f32,
    ) -> Result<Option<(f32, u32, u32)>, CanvasError> {
        if let Some(cache) = self.tiles.get(page, exact_zoom) {
            let source = cache.base().clone();
            let size = (source.width(), source.height());
            self.sources.insert(page, source);
            return Ok(Some((exact_zoom, size.0, size.1)));
        }

        let Some(source) = self.sources.get(&page).cloned() else {
            return Ok(None);
        };
        let source_zoom = source.zoom();
        let size = (source.width(), source.height());
        if self.tiles.get(page, source_zoom).is_none() {
            self.tiles.insert(page, source);
        }
        Ok(Some((source_zoom, size.0, size.1)))
    }

    fn map_pointer(
        &self,
        position: Point<Pixels>,
        pressure: f32,
        modifiers: GpuiModifiers,
    ) -> Result<Option<PointerInput>, CanvasError> {
        Ok(pointer_input(
            &self.viewport,
            position,
            point(px(self.canvas_origin.x), px(self.canvas_origin.y)),
            pressure,
            modifiers,
        )?)
    }

    fn dispatch_tool(&mut self, phase: ToolPointerPhase, input: PointerInput) {
        let index = self
            .active_tool
            .expect("tool dispatch requires an active tool");
        let document = &mut self.document;
        let tool = self
            .registry
            .tool_mut(index)
            .expect("the active tool remains registered");
        let mut context = ToolCtx { doc: document };
        match phase {
            ToolPointerPhase::Down => tool.on_pointer_down(&mut context, input),
            ToolPointerPhase::Move => tool.on_pointer_move(&mut context, input),
            ToolPointerPhase::Up => tool.on_pointer_up(&mut context, input),
        }
    }

    fn cancel_active_tool(&mut self) {
        let Some(index) = self.active_tool else {
            return;
        };
        let document = &mut self.document;
        let tool = self
            .registry
            .tool_mut(index)
            .expect("the active tool remains registered");
        tool.on_cancel(&mut ToolCtx { doc: document });
    }
}

fn window_point(point: Point<Pixels>) -> ViewPoint {
    ViewPoint {
        x: f32::from(point.x),
        y: f32::from(point.y),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TileImageKey {
    page: PageIndex,
    zoom_bits: u32,
    col: u32,
    row: u32,
    rotation: u8,
}

impl TileImageKey {
    fn new(page: PageIndex, zoom: f32, col: u32, row: u32, rotation: ViewRotation) -> Self {
        Self {
            page,
            zoom_bits: zoom.to_bits(),
            col,
            row,
            rotation: rotation_code(rotation),
        }
    }
}

struct TileImageEntry {
    tile: Weak<Tile>,
    image: Arc<RenderImage>,
}

#[derive(Default)]
struct TileImageCache {
    entries: BTreeMap<TileImageKey, TileImageEntry>,
}

impl TileImageCache {
    fn image_for(
        &mut self,
        key: TileImageKey,
        tile: &Arc<Tile>,
        width: u32,
        height: u32,
        rotation: ViewRotation,
    ) -> Result<Arc<RenderImage>, CanvasError> {
        if let Some(entry) = self.entries.get(&key) {
            if let Some(cached) = entry.tile.upgrade() {
                if Arc::ptr_eq(&cached, tile) {
                    return Ok(Arc::clone(&entry.image));
                }
            }
        }

        let (width, height, bgra) =
            tile_bgra(tile.rgba(), TILE_SIZE, TILE_SIZE, width, height, rotation)?;
        let buffer = image::RgbaImage::from_raw(width, height, bgra)
            .expect("tile conversion returns exactly width*height*4 bytes");
        let image = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        self.entries.insert(
            key,
            TileImageEntry {
                tile: Arc::downgrade(tile),
                image: Arc::clone(&image),
            },
        );
        Ok(image)
    }

    fn retain_keys(&mut self, keys: &BTreeSet<TileImageKey>) {
        self.entries.retain(|key, _| keys.contains(key));
    }
}

struct RawTile {
    region: TileRegion,
    rect: ViewRect,
    tile: Arc<Tile>,
}

#[derive(Clone, Copy)]
struct TileRegion {
    col: u32,
    row: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy)]
struct RasterPaintSource {
    page: PageIndex,
    zoom: f32,
    size: (u32, u32),
    rect: ViewRect,
    rotation: ViewRotation,
}

fn collect_tiles(
    store: &mut TileStore,
    source: RasterPaintSource,
    viewport_size: ViewSize,
) -> Vec<RawTile> {
    let (raster_width, raster_height) = source.size;
    let cache = store
        .get(source.page, source.zoom)
        .expect("the selected paint source has a tile cache");
    let mut tiles = Vec::with_capacity((cache.cols() * cache.rows()) as usize);
    for row in 0..cache.rows() {
        for col in 0..cache.cols() {
            let region = TileRegion {
                col,
                row,
                width: TILE_SIZE.min(raster_width - col * TILE_SIZE),
                height: TILE_SIZE.min(raster_height - row * TILE_SIZE),
            };
            let rect = tile_rect(
                source.rect,
                region,
                (raster_width, raster_height),
                source.rotation,
            );
            if !rect_intersects_viewport(rect, viewport_size) {
                continue;
            }
            tiles.push(RawTile {
                region,
                rect,
                tile: cache.tile(col, row),
            });
        }
    }
    tiles
}

fn rect_intersects_viewport(rect: ViewRect, viewport: ViewSize) -> bool {
    rect.origin.x < viewport.width
        && rect.origin.y < viewport.height
        && rect.origin.x + rect.size.width > 0.0
        && rect.origin.y + rect.size.height > 0.0
}

fn tile_bgra(
    rgba: &[u8],
    source_width: u32,
    source_height: u32,
    crop_width: u32,
    crop_height: u32,
    rotation: ViewRotation,
) -> Result<(u32, u32, Vec<u8>), CanvasError> {
    let expected = source_width as usize * source_height as usize * 4;
    if rgba.len() != expected {
        return Err(CanvasError::InvalidImageBuffer {
            expected,
            actual: rgba.len(),
        });
    }
    if crop_width == 0
        || crop_height == 0
        || crop_width > source_width
        || crop_height > source_height
    {
        return Err(CanvasError::InvalidImageCrop {
            width: source_width,
            height: source_height,
            crop_width,
            crop_height,
        });
    }

    let (output_width, output_height) = rotated_size(crop_width, crop_height, rotation);
    let mut output = vec![0; output_width as usize * output_height as usize * 4];
    for y in 0..crop_height {
        for x in 0..crop_width {
            let source = (y as usize * source_width as usize + x as usize) * 4;
            let pixel = unpremultiplied_bgra(&rgba[source..source + 4]);
            let (dx, dy) = rotate_pixel(x, y, crop_width, crop_height, rotation);
            let destination = (dy as usize * output_width as usize + dx as usize) * 4;
            output[destination..destination + 4].copy_from_slice(&pixel);
        }
    }
    Ok((output_width, output_height, output))
}

fn unpremultiplied_bgra(rgba: &[u8]) -> [u8; 4] {
    let alpha = rgba[3];
    if alpha == 0 {
        return [0, 0, 0, 0];
    }
    let straight = |channel: u8| ((u16::from(channel) * 255 / u16::from(alpha)).min(255)) as u8;
    [
        straight(rgba[2]),
        straight(rgba[1]),
        straight(rgba[0]),
        alpha,
    ]
}

fn rotate_pixel(x: u32, y: u32, width: u32, height: u32, rotation: ViewRotation) -> (u32, u32) {
    match rotation {
        ViewRotation::None => (x, y),
        ViewRotation::Clockwise90 => (height - 1 - y, x),
        ViewRotation::HalfTurn => (width - 1 - x, height - 1 - y),
        ViewRotation::Clockwise270 => (y, width - 1 - x),
    }
}

fn rotated_size(width: u32, height: u32, rotation: ViewRotation) -> (u32, u32) {
    match rotation {
        ViewRotation::None | ViewRotation::HalfTurn => (width, height),
        ViewRotation::Clockwise90 | ViewRotation::Clockwise270 => (height, width),
    }
}

fn tile_rect(
    page: ViewRect,
    tile: TileRegion,
    raster: (u32, u32),
    rotation: ViewRotation,
) -> ViewRect {
    let (raster_width, raster_height) = raster;
    let TileRegion {
        col,
        row,
        width,
        height,
    } = tile;
    let x = col * TILE_SIZE;
    let y = row * TILE_SIZE;
    let (x, y, width, height) = match rotation {
        ViewRotation::None => (x, y, width, height),
        ViewRotation::Clockwise90 => (raster_height - y - height, x, height, width),
        ViewRotation::HalfTurn => (
            raster_width - x - width,
            raster_height - y - height,
            width,
            height,
        ),
        ViewRotation::Clockwise270 => (y, raster_width - x - width, height, width),
    };
    let (output_width, output_height) = rotated_size(raster_width, raster_height, rotation);
    let scale_x = page.size.width / output_width as f32;
    let scale_y = page.size.height / output_height as f32;
    ViewRect {
        origin: ViewPoint {
            x: page.origin.x + x as f32 * scale_x,
            y: page.origin.y + y as f32 * scale_y,
        },
        size: ViewSize {
            width: width as f32 * scale_x,
            height: height as f32 * scale_y,
        },
    }
}

fn rotation_code(rotation: ViewRotation) -> u8 {
    match rotation {
        ViewRotation::None => 0,
        ViewRotation::Clockwise90 => 1,
        ViewRotation::HalfTurn => 2,
        ViewRotation::Clockwise270 => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use onionskin_core::{Error as CoreError, PageAlignment, PageLayoutMode};
    use onionskin_plugin_api::{ToolCtx, ToolPlugin};
    use onionskin_render::{InterpreterWarning, PageRender, RenderError};

    use super::*;

    const VIEWPORT: ViewSize = ViewSize {
        width: 800.0,
        height: 600.0,
    };

    fn model() -> CanvasModel {
        model_with_registry(PluginRegistry::new())
    }

    fn model_with_registry(registry: PluginRegistry) -> CanvasModel {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
        CanvasModel::new(
            Document::open_path(&path).expect("seed opens"),
            registry,
            VIEWPORT,
        )
        .expect("canvas starts")
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum RecordedToolEvent {
        Down(PointerInput),
        Move(PointerInput),
        Up(PointerInput),
        Cancel,
    }

    struct RecordingTool {
        events: Arc<Mutex<Vec<RecordedToolEvent>>>,
    }

    impl ToolPlugin for RecordingTool {
        fn id(&self) -> &'static str {
            "recording"
        }

        fn name(&self) -> &'static str {
            "Recording"
        }

        fn icon(&self) -> &'static str {
            "recording"
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
            self.events
                .lock()
                .unwrap()
                .push(RecordedToolEvent::Down(input));
        }

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
            self.events
                .lock()
                .unwrap()
                .push(RecordedToolEvent::Move(input));
        }

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
            self.events
                .lock()
                .unwrap()
                .push(RecordedToolEvent::Up(input));
        }

        fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
            self.events.lock().unwrap().push(RecordedToolEvent::Cancel);
        }
    }

    fn prepare_request(model: &mut CanvasModel) -> RenderRequest {
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.pin_visible_tiles(&visible);
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        *model.requests.get(&0).expect("page zero is requested")
    }

    fn raster(model: &CanvasModel, page: PageIndex, zoom: f32, rgba: [u8; 4]) -> BaseRaster {
        let geometry = model.viewport.page_geometry(page).unwrap();
        let (width, height) = onionskin_render::raster_size(
            geometry.render_size.0 as f32,
            geometry.render_size.1 as f32,
            zoom,
        )
        .unwrap();
        BaseRaster::new(
            u32::from(width),
            u32::from(height),
            zoom,
            rgba.repeat(usize::from(width) * usize::from(height)),
        )
    }

    fn raster_response(request: RenderRequest, raster: BaseRaster) -> RenderResponse {
        RenderResponse::Raster {
            request,
            render: PageRender {
                raster,
                warnings: Vec::new(),
            },
        }
    }

    #[test]
    fn the_default_active_tool_receives_page_space_pointer_input() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(RecordingTool {
            events: Arc::clone(&events),
        }));
        let mut model = model_with_registry(registry);
        let origin = ViewPoint { x: 50.0, y: 40.0 };
        model.resize(origin, VIEWPORT).unwrap();
        assert_eq!(model.active_tool(), Some(0));

        let page = model.viewport().visible_pages().unwrap()[0].rect;
        let local = [
            ViewPoint {
                x: page.origin.x + page.size.width * 0.4,
                y: page.origin.y + page.size.height * 0.4,
            },
            ViewPoint {
                x: page.origin.x + page.size.width * 0.5,
                y: page.origin.y + page.size.height * 0.5,
            },
            ViewPoint {
                x: page.origin.x + page.size.width * 0.6,
                y: page.origin.y + page.size.height * 0.6,
            },
        ];
        let positions = local.map(|at| point(px(at.x + origin.x), px(at.y + origin.y)));
        let modifiers = [
            GpuiModifiers {
                shift: true,
                ..Default::default()
            },
            GpuiModifiers {
                alt: true,
                ..Default::default()
            },
            GpuiModifiers {
                platform: true,
                ..Default::default()
            },
        ];
        let pressures = [0.25, 0.5, 0.75];
        let expected: [PointerInput; 3] = std::array::from_fn(|index| {
            pointer_input(
                model.viewport(),
                positions[index],
                point(px(origin.x), px(origin.y)),
                pressures[index],
                modifiers[index],
            )
            .unwrap()
            .unwrap()
        });

        assert!(model
            .pointer_down(positions[0], pressures[0], modifiers[0])
            .unwrap());
        assert!(model
            .pointer_move(positions[1], pressures[1], modifiers[1], true)
            .unwrap());
        assert!(model
            .pointer_up(positions[2], pressures[2], modifiers[2])
            .unwrap());

        assert_eq!(
            *events.lock().unwrap(),
            [
                RecordedToolEvent::Down(expected[0]),
                RecordedToolEvent::Move(expected[1]),
                RecordedToolEvent::Up(expected[2]),
            ]
        );
    }

    #[test]
    fn leaving_the_window_cancels_an_active_tool_gesture() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(RecordingTool {
            events: Arc::clone(&events),
        }));
        let mut model = model_with_registry(registry);
        let page = model.viewport().visible_pages().unwrap()[0].rect;
        let position = point(
            px(page.origin.x + page.size.width / 2.0),
            px(page.origin.y + page.size.height / 2.0),
        );

        assert!(model
            .pointer_down(position, 1.0, GpuiModifiers::default())
            .unwrap());
        assert!(model.cancel_pointer_gesture());
        assert!(!model.cancel_pointer_gesture());

        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [RecordedToolEvent::Down(_), RecordedToolEvent::Cancel]
        ));
    }

    #[test]
    fn an_empty_registry_drag_pans_through_the_viewport() {
        let mut model = model();
        assert_eq!(model.active_tool(), None);
        let before = model.viewport().offset();

        assert!(model
            .pointer_down(point(px(400.0), px(300.0)), 1.0, GpuiModifiers::default(),)
            .unwrap());
        assert!(model
            .pointer_move(
                point(px(400.0), px(400.0)),
                1.0,
                GpuiModifiers::default(),
                true,
            )
            .unwrap());
        assert!(model
            .pointer_up(point(px(400.0), px(400.0)), 1.0, GpuiModifiers::default(),)
            .unwrap());

        let panned = model.viewport().offset();
        assert_ne!(panned, before);
        model
            .resize(ViewPoint { x: 25.0, y: 15.0 }, VIEWPORT)
            .unwrap();
        assert_eq!(model.viewport().offset(), panned);
        assert!(!model
            .pointer_move(
                point(px(500.0), px(350.0)),
                1.0,
                GpuiModifiers::default(),
                true,
            )
            .unwrap());
    }

    #[test]
    fn estimated_geometry_is_queued_without_a_synchronous_measurement() {
        let mut model = model();
        model.viewport.go_to_page(1, PageAlignment::Start).unwrap();

        assert!(model.viewport.page_geometry(1).is_none());
        assert_eq!(model.queue_visible_geometry().unwrap(), 1);
        assert!(model.viewport.page_geometry(1).is_none());
        assert_eq!(model.queue_visible_geometry().unwrap(), 0);
        assert!(model.has_pending_work());
        assert!(model.geometry_requests.contains(&1));
    }

    #[test]
    fn geometry_failure_is_visible() {
        let mut model = model();
        model.geometry_requests.insert(1);
        model
            .apply_geometry_response(PageGeometryResponse::Failed {
                page: 1,
                error: CoreError::NoSuchPage { page: 1, count: 1 },
            })
            .unwrap();

        assert!(!model.geometry_requests.contains(&1));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { page: Some(1), message })
                if message.contains("outside a 1-page document")
        ));
    }

    #[test]
    fn a_resident_exact_raster_is_not_requested_twice() {
        let mut model = model();
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, [255, 255, 255, 255]);
        assert!(model.apply_render_response(raster_response(request, rendered)));

        let visible = model.viewport.visible_pages().unwrap();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 0);
        assert!(!model.has_pending_render());
    }

    #[test]
    fn a_stale_generation_cannot_replace_the_current_page() {
        let mut model = model();
        let stale = prepare_request(&mut model);
        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        assert!(model.update_signature(&visible).unwrap());
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);

        let old_raster = raster(&model, stale.page, stale.zoom, [255, 0, 0, 255]);
        assert!(!model.apply_render_response(raster_response(stale, old_raster)));
        assert!(model.sources.is_empty());
    }

    #[test]
    fn polling_refreshes_the_signature_before_accepting_a_response() {
        let mut model = model();
        let stale = prepare_request(&mut model);
        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();

        assert_eq!(model.drain_render_responses().unwrap(), 0);
        assert!(model.sources.is_empty());
        assert!(!model.requests.contains_key(&stale.page));
        assert_ne!(model.generation, stale.generation);
    }

    #[test]
    fn a_placeholder_keeps_polling_armed_until_a_terminal_response() {
        let mut model = model();
        let request = prepare_request(&mut model);
        let placeholder = model
            .document
            .try_render_response()
            .unwrap()
            .expect("placeholder is immediate");
        assert!(matches!(placeholder, RenderResponse::Placeholder(_)));
        assert!(model.apply_render_response(placeholder));
        assert!(model.has_pending_render());
        assert!(model.placeholders.contains(&request.page));

        assert!(model.apply_render_response(RenderResponse::Failed {
            request,
            error: RenderError::UnrenderableSize {
                width: 100_000.0,
                height: 100_000.0,
            },
        }));
        assert!(!model.has_pending_render());
        assert!(!model.placeholders.contains(&request.page));
    }

    #[test]
    fn a_previous_raster_scales_while_the_new_zoom_is_pending() {
        let mut model = model();
        let first = prepare_request(&mut model);
        let previous = raster(&model, first.page, first.zoom, [220, 220, 220, 255]);
        assert!(model.apply_render_response(raster_response(first, previous)));

        model
            .viewport
            .zoom_to(2.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.pin_visible_tiles(&visible);
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let placeholder = model
            .document
            .try_render_response()
            .unwrap()
            .expect("new placeholder is immediate");
        assert!(matches!(
            &placeholder,
            RenderResponse::Placeholder(value)
                if value.source().is_some_and(|source| source.zoom() == first.zoom)
        ));
        assert!(model.apply_render_response(placeholder));

        let paint = model.paint_list().unwrap();
        assert!(!paint.tiles.is_empty());
        assert!(paint
            .tiles
            .iter()
            .all(|tile| tile.source_zoom == first.zoom));
    }

    #[test]
    fn a_revisited_resident_raster_becomes_the_next_placeholder_source() {
        let mut model = model();
        let anchor = ViewPoint { x: 400.0, y: 300.0 };
        model.viewport.zoom_to(1.0, anchor).unwrap();
        let at_one = prepare_request(&mut model);
        let one = raster(&model, at_one.page, at_one.zoom, [100, 100, 100, 255]);
        assert!(model.apply_render_response(raster_response(at_one, one)));

        model.viewport.zoom_to(2.0, anchor).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.pin_visible_tiles(&visible);
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let at_two = *model.requests.get(&0).expect("2x raster is requested");
        let two = raster(&model, at_two.page, at_two.zoom, [200, 200, 200, 255]);
        assert!(model.apply_render_response(raster_response(at_two, two)));

        model.viewport.zoom_to(1.0, anchor).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.pin_visible_tiles(&visible);
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 0);
        assert!(!model.paint_list().unwrap().tiles.is_empty());
        assert_eq!(model.sources.get(&0).map(BaseRaster::zoom), Some(1.0));

        model.viewport.zoom_to(3.0, anchor).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        model.pin_visible_tiles(&visible);
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let placeholder = model
            .document
            .try_render_response()
            .unwrap()
            .expect("3x placeholder is immediate");
        assert!(matches!(
            placeholder,
            RenderResponse::Placeholder(value)
                if value.source().is_some_and(|source| source.zoom() == 1.0)
        ));
    }

    #[test]
    fn render_failures_and_interpreter_warnings_are_visible() {
        let mut model = model();
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, [255, 255, 255, 255]);
        assert!(model.apply_render_response(RenderResponse::Raster {
            request,
            render: PageRender {
                raster: rendered,
                warnings: vec![InterpreterWarning::ImageDecodeFailure],
            },
        }));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Warning { page: 0, message })
                if message.contains("ImageDecodeFailure")
        ));

        let visible = model.viewport.visible_pages().unwrap();
        model.tiles.clear();
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 1);
        let request = *model.requests.get(&0).unwrap();
        assert!(model.apply_render_response(RenderResponse::Failed {
            request,
            error: RenderError::UnrenderableSize {
                width: 100_000.0,
                height: 100_000.0,
            },
        }));
        assert!(matches!(
            model.status(),
            Some(CanvasStatus::Error { page: Some(0), message })
                if message.contains("100000")
        ));
    }

    #[test]
    fn generation_exhaustion_fails_instead_of_wrapping() {
        let mut model = model();
        model.generation = u64::MAX;
        model.signature = Some(RenderSignature {
            pages: Vec::new(),
            zoom_bits: 0,
        });
        let visible = model.viewport.visible_pages().unwrap();

        assert!(matches!(
            model.update_signature(&visible),
            Err(CanvasError::GenerationExhausted)
        ));
        assert_eq!(model.generation, u64::MAX);
    }

    #[test]
    fn visible_rasters_are_pinned_even_when_they_exceed_the_budget() {
        let mut model = model();
        let second = model.document.page_geometry(1).unwrap().clone();
        model.viewport.measure_page(second).unwrap();
        model.viewport.set_mode(PageLayoutMode::TwoPage).unwrap();
        model.viewport.fit(FitMode::Page).unwrap();
        let visible = model.viewport.visible_pages().unwrap();
        assert_eq!(visible.len(), 2);
        model.tiles = TileStore::with_budget(1);
        for page in 0..2 {
            model
                .sources
                .insert(page, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));
        }
        model.pin_visible_tiles(&visible);
        for page in 0..2 {
            model
                .tiles
                .insert(page, BaseRaster::new(1, 1, 1.0, vec![255, 255, 255, 255]));
        }

        assert_eq!(model.tiles.len(), 2);
        assert!(model.tiles.over_budget() > 0);
    }

    #[test]
    fn pinning_a_placeholder_also_protects_a_resident_exact_raster() {
        let mut model = model();
        let visible = model.viewport.visible_pages().unwrap();
        model.update_signature(&visible).unwrap();
        let exact_zoom = model.viewport.zoom();
        let source_zoom = 1.0_f32;
        assert_ne!(exact_zoom.to_bits(), source_zoom.to_bits());
        model.sources.insert(
            0,
            BaseRaster::new(1, 1, source_zoom, vec![200, 200, 200, 255]),
        );
        model.tiles = TileStore::with_budget(1);
        model.pin_visible_tiles(&visible);
        model.tiles.insert(
            0,
            BaseRaster::new(1, 1, exact_zoom, vec![255, 255, 255, 255]),
        );
        model.tiles.insert(
            0,
            BaseRaster::new(1, 1, source_zoom, vec![200, 200, 200, 255]),
        );

        model.pin_visible_tiles(&visible);
        assert_eq!(model.tiles.len(), 2);
        assert_eq!(model.schedule_visible_renders(&visible).unwrap(), 0);
        assert!(!model.has_pending_render());
    }

    #[test]
    fn transparent_and_partial_pixels_become_straight_bgra() {
        let (_, _, output) = tile_bgra(
            &[10, 20, 30, 0, 64, 32, 16, 128],
            2,
            1,
            2,
            1,
            ViewRotation::None,
        )
        .unwrap();

        assert_eq!(output, [0, 0, 0, 0, 31, 63, 127, 128]);
    }

    #[test]
    fn an_edge_tile_is_cropped_before_conversion() {
        let mut source = Vec::new();
        for value in 1..=9 {
            source.extend_from_slice(&[value, 0, 0, 255]);
        }
        let (width, height, output) = tile_bgra(&source, 3, 3, 2, 2, ViewRotation::None).unwrap();

        assert_eq!((width, height), (2, 2));
        assert_eq!(red_values(&output), vec![1, 2, 4, 5]);
    }

    #[test]
    fn cropped_pixels_rotate_in_all_four_orientations() {
        let mut source = Vec::new();
        for value in 1..=6 {
            source.extend_from_slice(&[value, 0, 0, 255]);
        }
        let cases = [
            (ViewRotation::None, (2, 3), vec![1, 2, 3, 4, 5, 6]),
            (ViewRotation::Clockwise90, (3, 2), vec![5, 3, 1, 6, 4, 2]),
            (ViewRotation::HalfTurn, (2, 3), vec![6, 5, 4, 3, 2, 1]),
            (ViewRotation::Clockwise270, (3, 2), vec![2, 4, 6, 1, 3, 5]),
        ];
        for (rotation, size, expected) in cases {
            let (width, height, output) = tile_bgra(&source, 2, 3, 2, 3, rotation).unwrap();
            assert_eq!((width, height), size, "{rotation:?}");
            assert_eq!(red_values(&output), expected, "{rotation:?}");
        }
    }

    #[test]
    fn replacing_a_tile_replaces_its_gpui_image() {
        let mut store = TileStore::new();
        let key = TileImageKey::new(0, 1.0, 0, 0, ViewRotation::None);
        let mut images = TileImageCache::default();

        let first_tile = store
            .insert(0, BaseRaster::new(1, 1, 1.0, vec![255, 0, 0, 255]))
            .tile(0, 0);
        let first = images
            .image_for(key, &first_tile, 1, 1, ViewRotation::None)
            .unwrap();
        let second_tile = store
            .insert(0, BaseRaster::new(1, 1, 1.0, vec![0, 0, 255, 255]))
            .tile(0, 0);
        let second = images
            .image_for(key, &second_tile, 1, 1, ViewRotation::None)
            .unwrap();

        assert_ne!(first.id, second.id);
        assert_eq!(first.as_bytes(0).unwrap(), [0, 0, 255, 255]);
        assert_eq!(second.as_bytes(0).unwrap(), [255, 0, 0, 255]);
    }

    #[test]
    fn paint_composites_and_caches_only_tiles_inside_the_viewport() {
        let mut model = model();
        model
            .viewport
            .zoom_to(10.0, ViewPoint { x: 400.0, y: 300.0 })
            .unwrap();
        let request = prepare_request(&mut model);
        let rendered = raster(&model, request.page, request.zoom, [255, 255, 255, 255]);
        assert!(model.apply_render_response(raster_response(request, rendered)));

        let paint = model.paint_list().unwrap();
        let cache = model.tiles.get(request.page, request.zoom).unwrap();
        let page_tiles = cache.cols() * cache.rows();
        assert!(!paint.tiles.is_empty());
        assert!((paint.tiles.len() as u32) < page_tiles);
        assert_eq!(cache.composites(), paint.tiles.len() as u64);
        assert_eq!(model.image_cache.entries.len(), paint.tiles.len());
    }

    #[test]
    fn tile_destinations_rotate_with_their_pixels() {
        let page = ViewRect {
            origin: ViewPoint { x: 10.0, y: 20.0 },
            size: ViewSize {
                width: 500.0,
                height: 300.0,
            },
        };
        let region = TileRegion {
            col: 1,
            row: 0,
            width: 44,
            height: 100,
        };
        let none = tile_rect(page, region, (300, 500), ViewRotation::None);
        assert_rect_close(
            none,
            ViewRect {
                origin: ViewPoint {
                    x: 436.66666,
                    y: 20.0,
                },
                size: ViewSize {
                    width: 73.333336,
                    height: 60.0,
                },
            },
        );

        let clockwise = tile_rect(page, region, (300, 500), ViewRotation::Clockwise90);
        assert_rect_close(
            clockwise,
            ViewRect {
                origin: ViewPoint { x: 410.0, y: 276.0 },
                size: ViewSize {
                    width: 100.0,
                    height: 44.0,
                },
            },
        );

        let half_turn = tile_rect(page, region, (300, 500), ViewRotation::HalfTurn);
        assert_rect_close(
            half_turn,
            ViewRect {
                origin: ViewPoint { x: 10.0, y: 260.0 },
                size: ViewSize {
                    width: 73.333336,
                    height: 60.0,
                },
            },
        );

        let counterclockwise = tile_rect(page, region, (300, 500), ViewRotation::Clockwise270);
        assert_rect_close(
            counterclockwise,
            ViewRect {
                origin: ViewPoint { x: 10.0, y: 20.0 },
                size: ViewSize {
                    width: 100.0,
                    height: 44.0,
                },
            },
        );
    }

    fn assert_rect_close(actual: ViewRect, expected: ViewRect) {
        for (actual, expected) in [
            (actual.origin.x, expected.origin.x),
            (actual.origin.y, expected.origin.y),
            (actual.size.width, expected.size.width),
            (actual.size.height, expected.size.height),
        ] {
            assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
        }
    }

    fn red_values(bgra: &[u8]) -> Vec<u8> {
        bgra.as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[2])
            .collect()
    }
}
