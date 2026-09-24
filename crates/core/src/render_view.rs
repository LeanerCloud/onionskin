//! A second view over one session: View > New Window.
//!
//! Two windows on one document are two viewports over **one** session: one
//! overlay, one undo stack, one file. What each window needs of its own is a
//! render queue, because the worker's queue belongs to one viewport: a newer
//! generation replaces every older request, so two viewports sharing a queue
//! would cancel each other's pages.
//!
//! A [`RenderView`] is that queue: its own worker thread and its own pending
//! geometry, handed the session's current bytes (the preview a save would
//! write, and the layer visibility the session shows) before anything is
//! drawn through it. The session keeps its own primary queue for the first
//! window; every method here mirrors one of the primary's.

use std::collections::BTreeSet;

use super::{Document, PageGeometryResponse};
use crate::render::{
    RenderRequest, RenderResponse, ThumbnailRequest, ThumbnailResponse, WorkerHandle,
};
use crate::{layers, Error, PageGeometry, PageIndex, Result};

/// One extra viewport's render queue over a session.
pub struct RenderView {
    render: WorkerHandle,
    pending_geometry: BTreeSet<PageIndex>,
    /// `(byte generation, edit epoch, layer epoch)` of what the worker holds.
    state: (u64, u64, u64),
    /// Whether the worker draws hairline strokes.
    hairline_strokes: bool,
}

impl std::fmt::Debug for RenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderView")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Document {
    /// A render queue of its own for another viewport over this session.
    pub fn new_render_view(&mut self) -> Result<RenderView> {
        let bytes = self.preview_bytes(crate::AnnotationFilter::DocumentAndMarkups)?;
        let mut view = RenderView {
            render: WorkerHandle::spawn_with_password(
                bytes,
                std::sync::Arc::clone(&self.render_password),
            )?,
            pending_geometry: BTreeSet::new(),
            state: self.view_state(),
            hairline_strokes: false,
        };
        self.send_layers(&mut view)?;
        self.send_hairline_strokes(&mut view)?;
        Ok(view)
    }

    fn view_state(&self) -> (u64, u64, u64) {
        (self.byte_generation, self.edit.epoch(), self.layer_epoch)
    }

    fn send_layers(&self, view: &mut RenderView) -> Result<()> {
        if let Some(layers) = &self.layers {
            view.render
                .set_layer_visibility(layers::overrides(layers))?;
        }
        Ok(())
    }

    fn send_hairline_strokes(&self, view: &mut RenderView) -> Result<()> {
        if view.hairline_strokes != self.hairline_strokes {
            view.render.set_hairline_strokes(self.hairline_strokes)?;
            view.hairline_strokes = self.hairline_strokes;
        }
        Ok(())
    }

    /// Hand the view the session's bytes, layers and stroke width if it holds
    /// older ones.
    fn sync_view(&mut self, view: &mut RenderView) -> Result<()> {
        self.send_hairline_strokes(view)?;
        self.sync_epoch();
        let current = self.view_state();
        if current == view.state {
            return Ok(());
        }
        let bytes = self.preview_bytes(crate::AnnotationFilter::DocumentAndMarkups)?;
        view.render.set_bytes(bytes)?;
        self.send_layers(view)?;
        view.pending_geometry.clear();
        view.state = current;
        Ok(())
    }

    /// [`Document::request_render`], on `view`'s queue.
    pub fn request_render_in(
        &mut self,
        view: &mut RenderView,
        request: RenderRequest,
        source: Option<&onionskin_render::BaseRaster>,
    ) -> Result<()> {
        self.sync_view(view)?;
        view.render.validate_request(request)?;
        let geometry = self.page_geometry(request.page)?.clone();
        view.render.request_render(request, &geometry, source)?;
        Ok(())
    }

    /// [`Document::request_render_with_geometry`], on `view`'s queue.
    pub fn request_render_with_geometry_in(
        &mut self,
        view: &mut RenderView,
        request: RenderRequest,
        geometry: &PageGeometry,
        source: Option<&onionskin_render::BaseRaster>,
    ) -> Result<()> {
        self.sync_view(view)?;
        view.render.validate_request(request)?;
        if request.page >= self.page_count() {
            return Err(Error::NoSuchPage {
                page: request.page,
                count: self.page_count(),
            });
        }
        if geometry.index != request.page {
            return Err(Error::GeometryPageMismatch {
                request: request.page,
                geometry: geometry.index,
            });
        }
        view.render.request_render(request, geometry, source)?;
        Ok(())
    }

    pub fn try_render_response_in(
        &mut self,
        view: &mut RenderView,
    ) -> Result<Option<RenderResponse>> {
        Ok(view.render.try_response()?)
    }

    /// [`Document::request_page_geometry`], answered on `view`'s queue.
    pub fn request_page_geometry_in(
        &mut self,
        view: &mut RenderView,
        index: PageIndex,
    ) -> Result<bool> {
        self.sync_view(view)?;
        if index >= self.page_count() {
            return Err(Error::NoSuchPage {
                page: index,
                count: self.page_count(),
            });
        }
        if !view.pending_geometry.insert(index) {
            return Ok(false);
        }
        if let Err(error) = view.render.request_page_geometry(index) {
            view.pending_geometry.remove(&index);
            return Err(error.into());
        }
        Ok(true)
    }

    pub fn try_page_geometry_response_in(
        &mut self,
        view: &mut RenderView,
    ) -> Result<Option<PageGeometryResponse>> {
        let Some((index, result)) = view.render.try_geometry_response()? else {
            return Ok(None);
        };
        view.pending_geometry.remove(&index);
        let rendered = match result {
            Ok(rendered) => rendered,
            Err(error) => {
                return Ok(Some(PageGeometryResponse::Failed {
                    page: index,
                    error: Error::Worker(error),
                }));
            }
        };
        let page = match self
            .structure()
            .and_then(|structure| Ok(onionskin_content::page(structure, index)?))
        {
            Ok(page) => page,
            Err(error) => {
                return Ok(Some(PageGeometryResponse::Failed { page: index, error }));
            }
        };
        Ok(Some(PageGeometryResponse::Ready(PageGeometry::new(
            &page, rendered,
        ))))
    }

    pub fn request_thumbnail_in(
        &mut self,
        view: &mut RenderView,
        request: ThumbnailRequest,
    ) -> Result<()> {
        self.sync_view(view)?;
        if request.page >= self.page_count() {
            return Err(Error::NoSuchPage {
                page: request.page,
                count: self.page_count(),
            });
        }
        Ok(view.render.request_thumbnail(request)?)
    }

    pub fn try_thumbnail_response_in(
        &mut self,
        view: &mut RenderView,
    ) -> Result<Option<ThumbnailResponse>> {
        Ok(view.render.try_thumbnail_response()?)
    }
}
