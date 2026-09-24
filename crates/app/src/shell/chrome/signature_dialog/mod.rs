//! Add Signature and Add Initials (M5 Fill & Sign): make one from typed
//! text, a drawing or an image, and keep it in the signature library.
//!
//! What the form means is plain data ([`SignatureForm`], [`request`]),
//! tested without a window; the frame reads an image, saves the page and
//! arms the Sign tool.

mod view;

use std::path::PathBuf;

use gpui::{AppContext as _, Context, Entity};
use onionskin_tools_fill_sign::signature::{self, SignatureKind};

use super::accessible::TextField;
use super::{SearchInput, ShellFrame, ThemeTokens};

pub(in crate::shell) use view::{accessible, render, PadEvent};

/// The drawing pad's size, in pixels.
pub(in crate::shell) const PAD_WIDTH: f32 = 360.0;
pub(in crate::shell) const PAD_HEIGHT: f32 = 120.0;

/// How the signature is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Method {
    Type,
    Draw,
    Image,
}

impl Method {
    pub(in crate::shell) const ALL: [Method; 3] = [Method::Type, Method::Draw, Method::Image];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Type => "Type",
            Self::Draw => "Draw",
            Self::Image => "Image",
        }
    }
}

/// The one typed field: the name a typed signature spells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::shell) enum SignatureField {
    Name,
}

impl SignatureField {
    pub(in crate::shell) fn id(self) -> &'static str {
        "signature-name"
    }

    pub(in crate::shell) fn label(self) -> &'static str {
        "Name"
    }
}

/// What a control that is not typed into does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SignatureAction {
    SetMethod(Method),
    ClearPad,
    ChooseImage,
    Save,
    ClearSaved,
}

/// What Save makes the signature from.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) enum Made {
    /// The page, made already.
    Page(Vec<u8>),
    /// An image or PDF file, which the frame reads.
    File(PathBuf),
}

/// The dialog's choices, apart from what is typed.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct SignatureForm {
    pub(in crate::shell) kind: SignatureKind,
    pub(in crate::shell) method: Method,
    /// The strokes drawn on the pad, in pad pixels, `y` down.
    pub(in crate::shell) strokes: Vec<Vec<(f64, f64)>>,
    /// Whether the pointer is down on the pad, drawing the last stroke.
    pub(in crate::shell) drawing: bool,
    pub(in crate::shell) image: Option<PathBuf>,
    /// Whether one of this kind is saved already, which Clear Saved forgets.
    pub(in crate::shell) saved: bool,
}

impl SignatureForm {
    pub(in crate::shell) fn new(kind: SignatureKind, saved: bool) -> Self {
        Self {
            kind,
            method: Method::Type,
            strokes: Vec::new(),
            drawing: false,
            image: None,
            saved,
        }
    }

    pub(in crate::shell) fn apply(&mut self, action: SignatureAction) {
        match action {
            SignatureAction::SetMethod(method) => self.method = method,
            SignatureAction::ClearPad => {
                self.strokes.clear();
                self.drawing = false;
            }
            SignatureAction::ChooseImage | SignatureAction::Save | SignatureAction::ClearSaved => {}
        }
    }

    /// The pointer went down at `at`, in pad pixels: a new stroke, if it is
    /// on the pad.
    pub(in crate::shell) fn pad_down(&mut self, at: (f64, f64)) {
        if on_pad(at) {
            self.strokes.push(vec![at]);
            self.drawing = true;
        }
    }

    /// The pointer moved to `at` while drawing: the stroke follows it, held
    /// to the pad's edge.
    pub(in crate::shell) fn pad_move(&mut self, at: (f64, f64)) {
        if !self.drawing {
            return;
        }
        if let Some(stroke) = self.strokes.last_mut() {
            stroke.push(clamp_to_pad(at));
        }
    }

    pub(in crate::shell) fn pad_up(&mut self) {
        self.drawing = false;
    }

    /// The fields shown.
    pub(in crate::shell) fn fields(&self) -> Vec<SignatureField> {
        match self.method {
            Method::Type => vec![SignatureField::Name],
            Method::Draw | Method::Image => Vec::new(),
        }
    }
}

fn on_pad((x, y): (f64, f64)) -> bool {
    (0.0..=f64::from(PAD_WIDTH)).contains(&x) && (0.0..=f64::from(PAD_HEIGHT)).contains(&y)
}

fn clamp_to_pad((x, y): (f64, f64)) -> (f64, f64) {
    (
        x.clamp(0.0, f64::from(PAD_WIDTH)),
        y.clamp(0.0, f64::from(PAD_HEIGHT)),
    )
}

/// What Save makes the signature from, or what is missing.
pub(in crate::shell) fn request(form: &SignatureForm, name: &str) -> Result<Made, String> {
    match form.method {
        Method::Type => signature::typed(name)
            .map(Made::Page)
            .map_err(|_| "Type the name to sign with.".to_owned()),
        Method::Draw => signature::drawn(&form.strokes)
            .map(Made::Page)
            .map_err(|_| "Draw on the pad first.".to_owned()),
        Method::Image => form
            .image
            .clone()
            .map(Made::File)
            .ok_or_else(|| "Choose the image to sign with.".to_owned()),
    }
}

/// The dialog, open.
pub(in crate::shell) struct SignatureDialogState {
    pub(in crate::shell) form: SignatureForm,
    pub(in crate::shell) name: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
    /// Where the pad was last painted, in window pixels, so a pointer event
    /// can be put in pad pixels.
    pub(in crate::shell) pad_origin:
        std::rc::Rc<std::cell::Cell<Option<gpui::Point<gpui::Pixels>>>>,
}

impl SignatureDialogState {
    pub(in crate::shell) fn new(
        form: SignatureForm,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let field = SignatureField::Name;
        Self {
            form,
            name: cx.new(|cx| SearchInput::with_placeholder(field.id(), field.label(), theme, cx)),
            error: None,
            pad_origin: Default::default(),
        }
    }

    pub(in crate::shell) fn text_field(
        &self,
        field: SignatureField,
    ) -> Option<&Entity<SearchInput>> {
        self.form.fields().contains(&field).then_some(&self.name)
    }

    pub(in crate::shell) fn request(&self, cx: &gpui::App) -> Result<Made, String> {
        request(&self.form, self.name.read(cx).query())
    }

    /// A window point in pad pixels, once the pad has been painted.
    pub(in crate::shell) fn on_pad(&self, at: gpui::Point<gpui::Pixels>) -> Option<(f64, f64)> {
        let origin = self.pad_origin.get()?;
        Some((
            f64::from(f32::from(at.x - origin.x)),
            f64::from(f32::from(at.y - origin.y)),
        ))
    }
}

/// The dialog's text fields, for the focus ring.
pub(in crate::shell) fn text_fields() -> impl Iterator<Item = TextField> {
    std::iter::once(TextField::Signature(SignatureField::Name))
}

/// The kind the menu entry makes.
pub(in crate::shell) fn kind(initials: bool) -> SignatureKind {
    if initials {
        SignatureKind::Initials
    } else {
        SignatureKind::Signature
    }
}

#[cfg(test)]
mod tests;
