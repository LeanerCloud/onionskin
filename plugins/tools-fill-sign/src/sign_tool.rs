//! Sign: click where the saved signature or initials go.

use onionskin_core::{Annotation, Rect, StampArt, Subtype};
use onionskin_plugin_api::{
    PointerInput, ToolCapability, ToolChoice, ToolCtx, ToolEnvironment, ToolPlugin,
};

use crate::gesture::{Gesture, Press};
use crate::signature::{page_size, SignatureKind, SignatureLibrary};
use crate::{place, GROUP};

#[derive(Debug)]
pub struct SignTool {
    chosen: SignatureKind,
    library: Option<SignatureLibrary>,
    press: Press,
}

impl Default for SignTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SignTool {
    pub fn new() -> Self {
        Self {
            chosen: SignatureKind::Signature,
            library: None,
            press: Press::default(),
        }
    }

    /// The saved page of the chosen kind, as an annotation centred on
    /// `(x, y)` no wider than the kind is placed.
    fn annotation(&self, (x, y): (f64, f64)) -> Option<Annotation> {
        let pdf = self.library.as_ref()?.get(self.chosen)?;
        let (width, height) = page_size(&pdf).ok()?;
        let scale = (self.chosen.max_width() / width).min(1.0);
        let (width, height) = (width * scale, height * scale);
        let mut annotation = Annotation::new(
            Subtype::Stamp,
            Rect::new(
                x - width / 2.0,
                y - height / 2.0,
                x + width / 2.0,
                y + height / 2.0,
            ),
        );
        annotation.stamp_art = Some(StampArt::Page(pdf));
        annotation.icon = Some(self.chosen.label().to_owned());
        annotation.contents = Some(self.chosen.label().to_owned());
        annotation.subject = Some("Fill & Sign".to_owned());
        Some(annotation)
    }
}

impl ToolPlugin for SignTool {
    fn id(&self) -> &'static str {
        "fill-sign.sign"
    }

    fn name(&self) -> &'static str {
        "Sign Yourself"
    }

    fn icon(&self) -> &'static str {
        "fill-sign"
    }

    fn group(&self) -> &'static str {
        GROUP
    }

    fn hint(&self) -> Option<&'static str> {
        Some(
            "Click where your signature goes. Edit > Add Signature makes one, from typed \
             text, a drawing or an image.",
        )
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::AddSignature]
    }

    fn configure(&mut self, environment: &ToolEnvironment) {
        self.library = environment
            .data_dir
            .as_deref()
            .map(SignatureLibrary::in_data_dir);
    }

    fn choices(&self) -> Vec<ToolChoice> {
        let Some(library) = &self.library else {
            return Vec::new();
        };
        SignatureKind::ALL
            .into_iter()
            .filter(|kind| library.get(*kind).is_some())
            .map(|kind| ToolChoice {
                id: kind.id().to_owned(),
                label: kind.label().to_owned(),
                category: "Sign Yourself".to_owned(),
            })
            .collect()
    }

    fn choose(&mut self, id: &str) -> bool {
        match SignatureKind::ALL.into_iter().find(|kind| kind.id() == id) {
            Some(kind) => {
                self.chosen = kind;
                true
            }
            None => false,
        }
    }

    fn chosen(&self) -> Option<String> {
        Some(self.chosen.id().to_owned())
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.press.down(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.press.moved(input.at);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(Gesture::Click(at) | Gesture::Drag(at, _)) = self.press.up(input.at, ctx.viewport)
        else {
            return;
        };
        if let Some(annotation) = self.annotation((at.x, at.y)) {
            place(ctx.doc, at.page, self.chosen.label(), &annotation);
        }
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.press.cancel();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
}
