//! The Edit Text tool: a click on a line of text asks the shell to open an
//! editor on it, holding what the line says. What is typed there replaces
//! the line, drawn from where it began (see `text`).

use onionskin_core::{Document, PagePoint, TextEditRequest};
use onionskin_plugin_api::marquee::is_drag;
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::text::line_at;

#[derive(Debug, Default)]
pub struct EditTextTool {
    pressed: Option<PagePoint>,
    /// The line last asked for, outlined while the tool is chosen.
    chosen: Option<(usize, [f64; 4])>,
}

impl EditTextTool {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolPlugin for EditTextTool {
    fn id(&self) -> &'static str {
        "edit-text"
    }

    fn name(&self) -> &'static str {
        "Edit Text"
    }

    fn icon(&self) -> &'static str {
        "edit-text"
    }

    fn group(&self) -> &'static str {
        "edit-text"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Click a line of text to edit it. Enter keeps the change, Escape leaves the line as it was.")
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::EditText]
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.pressed = Some(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(pressed) = self.pressed.take() else {
            return;
        };
        if pressed.page != input.at.page || is_drag(pressed, input.at, ctx.viewport) {
            return;
        }
        let page = input.at.page;
        self.chosen = None;
        let Some((line, found)) = line_at(ctx.doc, page, (input.at.x, input.at.y)) else {
            return;
        };
        let bounds = found.bounds();
        self.chosen = Some((page, bounds));
        ctx.doc.request_text_edit(TextEditRequest {
            page,
            line,
            text: found.text,
            bounds,
        });
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.pressed = None;
        self.chosen = None;
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.pressed = None;
        self.chosen = None;
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        let Some((page, [x0, y0, x1, y1])) = self.chosen else {
            return Vec::new();
        };
        vec![Overlay::Polyline {
            points: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .into_iter()
                .map(|(x, y)| PagePoint { page, x, y })
                .collect(),
            closed: true,
        }]
    }
}
