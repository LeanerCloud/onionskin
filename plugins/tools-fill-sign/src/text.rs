//! Add Text: click where the text goes, and type.

use onionskin_core::{Annotation, Intent, Rect, TextStyle};
use onionskin_plugin_api::{PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::gesture::{Gesture, Press};
use crate::{place, GROUP};

/// The box a click gets: wide enough for a name or a date at the default
/// size, and grown by the text field as it is typed into.
const BOX: (f64, f64) = (160.0, 18.0);

#[derive(Debug, Default)]
pub struct FillTextTool {
    press: Press,
}

impl FillTextTool {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolPlugin for FillTextTool {
    fn id(&self) -> &'static str {
        "fill-sign.text"
    }

    fn name(&self) -> &'static str {
        "Add Text"
    }

    fn icon(&self) -> &'static str {
        "fill-text"
    }

    fn group(&self) -> &'static str {
        GROUP
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Click where the text goes, then type it.")
    }

    fn takes_text(&self) -> bool {
        true
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::FillTextFields]
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.press.down(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.press.moved(input.at);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(gesture) = self.press.up(input.at, ctx.viewport) else {
            return;
        };
        let at = match gesture {
            Gesture::Click(at) | Gesture::Drag(at, _) => at,
        };
        // The text's baseline sits on the click, as a typed line would.
        let rect = Rect::new(at.x, at.y - 4.0, at.x + BOX.0, at.y - 4.0 + BOX.1);
        let mut annotation =
            Annotation::free_text(rect, TextStyle::default(), Some(Intent::FreeTextTypewriter));
        annotation.border_width = 0.0;
        annotation.subject = Some("Fill & Sign Text".to_owned());
        place(ctx.doc, at.page, "Add Text", &annotation);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.press.cancel();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
}
