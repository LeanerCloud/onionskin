//! The Edit Text tool: a click on a line of text asks the shell to open an
//! editor on it, holding what the line says. What is typed there replaces
//! the line, drawn from where it began (see `text`).
//!
//! The Add Text tool: a click asks for an empty editor there, and what is
//! typed is drawn as a new line starting at the click.

use onionskin_core::{Document, PagePoint, TextEditRequest};
use onionskin_plugin_api::marquee::is_drag;
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::text::{line_at, NEW_TEXT_SIZE};

/// How wide the editor for new text opens, in points.
const NEW_TEXT_WIDTH: f64 = 200.0;

/// A press at `pressed` let go at `at` is a click on one page.
fn clicked(pressed: Option<PagePoint>, at: PagePoint, ctx: &ToolCtx) -> bool {
    pressed.is_some_and(|pressed| pressed.page == at.page && !is_drag(pressed, at, ctx.viewport))
}

fn outline(page: usize, [x0, y0, x1, y1]: [f64; 4]) -> Overlay {
    Overlay::Polyline {
        points: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
            .into_iter()
            .map(|(x, y)| PagePoint { page, x, y })
            .collect(),
        closed: true,
    }
}

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
        if !clicked(self.pressed.take(), input.at, ctx) {
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
            line: Some(line),
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
        self.chosen
            .map(|(page, bounds)| outline(page, bounds))
            .into_iter()
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct AddTextTool {
    pressed: Option<PagePoint>,
}

impl AddTextTool {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolPlugin for AddTextTool {
    fn id(&self) -> &'static str {
        "add-text"
    }

    fn name(&self) -> &'static str {
        "Add Text"
    }

    fn icon(&self) -> &'static str {
        "add-text"
    }

    fn group(&self) -> &'static str {
        "edit-text"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Click where the text starts, then type. Enter adds it, Escape leaves the page as it was.")
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::EditText]
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.pressed = Some(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if !clicked(self.pressed.take(), input.at, ctx) {
            return;
        }
        let PagePoint { page, x, y } = input.at;
        ctx.doc.request_text_edit(TextEditRequest {
            page,
            line: None,
            text: String::new(),
            bounds: [x, y, x + NEW_TEXT_WIDTH, y + NEW_TEXT_SIZE * 1.2],
        });
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.pressed = None;
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        Vec::new()
    }
}
