//! The Link tool: drag a rectangle to make a link there, or click a link to
//! change it. Where a link goes is the shell's dialog to ask, so the tool
//! raises a request and the shell opens it.
//!
//! While the tool is chosen the page's links are outlined, as Acrobat shows
//! them, so an invisible link can be found to be changed.

use onionskin_core::{Document, LinkRequest, PageRect};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use onionskin_plugin_api::marquee::Marquee;

#[derive(Debug, Default)]
pub struct LinkTool {
    marquee: Marquee,
    /// The document's links as of the tool's last look, to outline.
    outlined: Vec<PageRect>,
}

impl LinkTool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look at the document's links again.
    fn refresh(&mut self, doc: &mut Document) {
        self.outlined = doc
            .links()
            .map(|links| {
                links
                    .iter()
                    .map(|link| PageRect {
                        page: link.page,
                        x0: link.rect[0],
                        y0: link.rect[1],
                        x1: link.rect[2],
                        y1: link.rect[3],
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
}

impl ToolPlugin for LinkTool {
    fn id(&self) -> &'static str {
        "link"
    }

    fn name(&self) -> &'static str {
        "Link"
    }

    fn icon(&self) -> &'static str {
        "link"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag a rectangle to make a link there, or click a link to change or delete it.")
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Link]
    }

    fn on_activate(&mut self, ctx: &mut ToolCtx) {
        self.refresh(ctx.doc);
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.begin(input.at);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
        match self.marquee.finish() {
            Some(rect) => ctx.doc.request_link(LinkRequest::Create(rect)),
            None => {
                let at = input.at;
                let hit = ctx.doc.links().ok().and_then(|links| {
                    onionskin_core::links::link_at(&links, at.page, (at.x, at.y))
                        .map(|link| link.objref)
                });
                if let Some(link) = hit {
                    ctx.doc.request_link(LinkRequest::Edit(link));
                }
            }
        }
        self.refresh(ctx.doc);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.marquee.cancel();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        let mut shown: Vec<Overlay> = self.outlined.iter().copied().map(Overlay::Rect).collect();
        shown.extend(self.marquee.rect().map(Overlay::AntsRect));
        shown
    }
}
