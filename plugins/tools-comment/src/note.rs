//! Sticky note: a `/Text` annotation placed by a click.
//!
//! **A click, not a drag.** A sticky note has no extent the user chooses: it
//! is an icon of a fixed size wherever they put it, and Acrobat draws it the
//! same size at every zoom because a reader renders `/Text` at its own icon
//! size whatever the `/Rect` says. Treating it as a drag would give the user a
//! rubber band that changes nothing.

use onionskin_core::{add_annotation, Annotation, Color, PagePoint, Rect, Subtype};
use onionskin_plugin_api::{PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::place::{now, page_object};

/// The icon's box, in page units. 20pt is what Acrobat writes, and a reader
/// draws its own icon into it rather than scaling one.
const ICON: f64 = 20.0;

/// Past this, in page units, the pointer moved between press and release and
/// the user was doing something else - dragging the page, or changing their
/// mind. A click that wanders a point or two is still a click.
const SLIP: f64 = 4.0;

pub struct NoteTool {
    color: Color,
    pressed: Option<PagePoint>,
}

impl Default for NoteTool {
    fn default() -> Self {
        NoteTool::new()
    }
}

impl NoteTool {
    pub fn new() -> Self {
        NoteTool {
            color: Color::new(1.0, 0.82, 0.2),
            pressed: None,
        }
    }

    /// The note's rect: the click is its upper-left corner, which is where a
    /// reader hangs the icon from and what makes the icon appear under the
    /// pointer rather than above and to the right of it.
    fn rect(at: PagePoint) -> Rect {
        Rect::new(at.x, at.y - ICON, at.x + ICON, at.y)
    }
}

impl ToolPlugin for NoteTool {
    fn id(&self) -> &'static str {
        "sticky-note"
    }

    fn name(&self) -> &'static str {
        "Sticky Note"
    }

    fn icon(&self) -> &'static str {
        "sticky-note"
    }

    fn shortcut(&self) -> Option<&'static str> {
        Some("s")
    }

    fn group(&self) -> &'static str {
        "note"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Comment]
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.pressed = Some(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(pressed) = self.pressed.take() else {
            return;
        };
        if pressed.page != input.at.page
            || (input.at.x - pressed.x).hypot(input.at.y - pressed.y) > SLIP
        {
            return;
        }

        let Some(page) = page_object(ctx.doc, pressed.page) else {
            return;
        };
        let mut annotation = Annotation::new(Subtype::Text, NoteTool::rect(pressed));
        annotation.icon = Some("Note".into());
        annotation.color = Some(self.color);
        let _ = ctx.doc.edit_annotations("Sticky Note", |tx, structure| {
            add_annotation(tx, structure, page, &annotation, now()).map(|_| ())
        });
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.pressed = None;
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
}
