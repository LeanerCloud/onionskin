//! Checkmark, Cross and Dot: a small mark where the page is clicked, for a
//! box on a paper form.

use onionskin_core::{Annotation, BaseFont, Color, Rect, StampArt, Subtype};
use onionskin_plugin_api::{PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::gesture::{Gesture, Press};
use crate::{place, GROUP};

/// How big a mark is, in points.
pub const SIZE: f64 = 12.0;

/// Which mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    Check,
    Cross,
    Dot,
}

impl Symbol {
    pub const ALL: [Symbol; 3] = [Symbol::Check, Symbol::Cross, Symbol::Dot];

    fn id(self) -> &'static str {
        match self {
            Symbol::Check => "fill-sign.check",
            Symbol::Cross => "fill-sign.cross",
            Symbol::Dot => "fill-sign.dot",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Symbol::Check => "Checkmark",
            Symbol::Cross => "Cross",
            Symbol::Dot => "Dot",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Symbol::Check => "fill-check",
            Symbol::Cross => "fill-cross",
            Symbol::Dot => "fill-dot",
        }
    }

    /// The mark, drawn in a `SIZE` square with its origin at the lower left.
    pub fn drawing(self) -> &'static str {
        match self {
            Symbol::Check => "0 G 1.5 w 1 J 1 j 1.5 6.5 m 4.5 2.5 l 10.5 10 l S",
            Symbol::Cross => "0 G 1.5 w 1 J 2 2 m 10 10 l S 2 10 m 10 2 l S",
            // A circle of radius 3.5 from four Bezier arcs.
            Symbol::Dot => {
                "0 g 9.5 6 m 9.5 7.93 7.93 9.5 6 9.5 c 4.07 9.5 2.5 7.93 2.5 6 c \
                 2.5 4.07 4.07 2.5 6 2.5 c 7.93 2.5 9.5 4.07 9.5 6 c f"
            }
        }
    }

    /// The annotation that puts this mark centred on `(x, y)`.
    pub fn annotation(self, (x, y): (f64, f64)) -> Annotation {
        let half = SIZE / 2.0;
        let mut annotation = Annotation::new(
            Subtype::Stamp,
            Rect::new(x - half, y - half, x + half, y + half),
        );
        annotation.stamp_art = Some(StampArt::Drawing {
            size: (SIZE, SIZE),
            content: self.drawing().to_owned(),
            fonts: Vec::<BaseFont>::new(),
        });
        annotation.icon = Some(self.name().to_owned());
        annotation.contents = Some(self.name().to_owned());
        annotation.subject = Some("Fill & Sign".to_owned());
        annotation.color = Some(Color::BLACK);
        annotation
    }
}

#[derive(Debug)]
pub struct SymbolTool {
    symbol: Symbol,
    press: Press,
}

impl SymbolTool {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            press: Press::default(),
        }
    }
}

impl ToolPlugin for SymbolTool {
    fn id(&self) -> &'static str {
        self.symbol.id()
    }

    fn name(&self) -> &'static str {
        self.symbol.name()
    }

    fn icon(&self) -> &'static str {
        self.symbol.icon()
    }

    fn group(&self) -> &'static str {
        GROUP
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Click where the mark goes.")
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
        // A mark goes where the press was: a slip while clicking a small box
        // must not move it out of the box.
        if let Some(Gesture::Click(at) | Gesture::Drag(at, _)) =
            self.press.up(input.at, ctx.viewport)
        {
            place(
                ctx.doc,
                at.page,
                self.symbol.name(),
                &self.symbol.annotation((at.x, at.y)),
            );
        }
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.press.cancel();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
}
