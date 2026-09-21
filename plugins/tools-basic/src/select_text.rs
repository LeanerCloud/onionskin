//! Text selection: a drag across glyphs, in document order.

use onionskin_core::textselect::{glyph_order, nearest_glyph, selection_for};
use onionskin_core::{Document, PagePoint, PageQuad};
use onionskin_plugin_api::{EditVerb, Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::marquee::is_drag;

/// Selects the glyphs between the point the drag started on and the point
/// it is over, in the order the page drew them. Reading order is document
/// order, the same order `content` extracts and searches in.
///
/// Selection is per page in M2: `content` extracts one page at a time, so a
/// drag that leaves the page it started on selects to that page's end
/// rather than reaching into the next one.
#[derive(Debug, Default)]
pub struct SelectTextTool {
    /// The fixed end of the selection. It outlives the drag so a later
    /// shift-click can extend from it.
    anchor: Option<PagePoint>,
    dragging: bool,
    quads: Vec<PageQuad>,
}

impl SelectTextTool {
    pub fn new() -> Self {
        Self::default()
    }

    fn extend_to(&mut self, ctx: &mut ToolCtx, at: PagePoint) {
        let Some(anchor) = self.anchor else {
            return;
        };
        if !is_drag(anchor, at, ctx.viewport) {
            self.clear(ctx);
            return;
        }
        let Ok(page) = ctx.doc.page_text(anchor.page) else {
            return;
        };
        let order = glyph_order(page);
        let Some(from) = nearest_glyph(page, &order, anchor) else {
            return;
        };
        let to = if at.page == anchor.page {
            match nearest_glyph(page, &order, at) {
                Some(index) => index,
                None => return,
            }
        } else if at.page > anchor.page {
            order.len() - 1
        } else {
            0
        };
        let selection = selection_for(page, &order, from.min(to)..=from.max(to));
        self.quads = selection.quads.clone();
        ctx.doc.selection_mut().set_text(selection);
    }

    fn clear(&mut self, ctx: &mut ToolCtx) {
        self.quads.clear();
        ctx.doc.selection_mut().clear();
    }
}

impl ToolPlugin for SelectTextTool {
    fn id(&self) -> &'static str {
        "select-text"
    }

    fn name(&self) -> &'static str {
        "Select Text"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag across text to select it. Cmd-C or the right-click menu copies it.")
    }

    fn icon(&self) -> &'static str {
        "select-text"
    }

    fn shortcut(&self) -> Option<&'static str> {
        Some("v")
    }

    fn group(&self) -> &'static str {
        "select"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Select]
    }

    /// Copy is the only verb text selection has: the page's text is not
    /// editable here, so there is nothing to cut, paste or delete.
    fn claims(&self, verb: EditVerb) -> bool {
        verb == EditVerb::Copy
    }

    fn edit(&mut self, ctx: &mut ToolCtx, verb: EditVerb, _pasted: Option<&str>) -> Option<String> {
        if verb != EditVerb::Copy {
            return None;
        }
        ctx.doc
            .selection()
            .text()
            .map(|selection| selection.text.clone())
            .filter(|text| !text.is_empty())
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.dragging = true;
        let extends = input.modifiers.shift
            && self
                .anchor
                .is_some_and(|anchor| anchor.page == input.at.page);
        if extends {
            self.extend_to(ctx, input.at);
            return;
        }
        self.anchor = Some(input.at);
        self.clear(ctx);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.dragging {
            self.extend_to(ctx, input.at);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.dragging {
            self.extend_to(ctx, input.at);
        }
        self.dragging = false;
    }

    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        self.dragging = false;
        self.anchor = None;
        self.clear(ctx);
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        if self.quads.is_empty() {
            Vec::new()
        } else {
            vec![Overlay::Quads(self.quads.clone())]
        }
    }
}
