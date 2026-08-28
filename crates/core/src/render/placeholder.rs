use onionskin_render::{BaseRaster, Rgba};

use super::{RenderRequest, WorkerError};
use crate::PageGeometry;

const PAGE_BACKGROUND: Rgba = Rgba {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

pub struct PagePlaceholder {
    pub request: RenderRequest,
    pub width: u32,
    pub height: u32,
    pub background: Rgba,
    pub(super) source: Option<BaseRaster>,
}

impl PagePlaceholder {
    pub(super) fn new(
        request: RenderRequest,
        geometry: &PageGeometry,
        source: Option<&BaseRaster>,
    ) -> Result<Self, WorkerError> {
        let (width, height) = onionskin_render::raster_size(
            geometry.render_size.0 as f32,
            geometry.render_size.1 as f32,
            request.zoom,
        )
        .map_err(WorkerError::Render)?;
        Ok(Self {
            request,
            width: u32::from(width),
            height: u32::from(height),
            background: PAGE_BACKGROUND,
            source: source.cloned(),
        })
    }

    /// A previous render to scale into this placeholder for display only.
    pub fn source(&self) -> Option<&BaseRaster> {
        self.source.as_ref()
    }
}
