//! Crop Pages (M5): margins, the box they set, and the pages they apply to.
//!
//! Acrobat's Set Page Boxes dialog: which box, four margins measured in from
//! the media box as the page is shown, Remove White Margins, Set To Zero,
//! Change Page Size, and the pages. The margins open on the first chosen
//! page's crop box and the size on its media box, so pressing Crop at once
//! changes nothing.
//!
//! What the form means is plain data ([`CropForm`], [`request`]), tested
//! without a window; the frame runs the crop through `tools-edit`.

mod view;

use gpui::{AppContext as _, Context, Entity};
use onionskin_core::pages::{Margins, PageBox};
use onionskin_core::PageIndex;

use super::accessible::TextField;
use super::{SearchInput, ShellFrame, ThemeTokens};

pub(in crate::shell) use view::{accessible, render};

/// Why a build cannot crop.
#[cfg(not(feature = "tools-edit"))]
pub(in crate::shell) const NO_EDIT_TOOLS: &str = "The Edit PDF plugin is not installed";

/// Why Crop Pages cannot run on a document that refuses edits for
/// `document`, if it cannot: that refusal, or a build without `tools-edit`.
pub(in crate::shell) fn crop_refusal(document: Option<&'static str>) -> Option<&'static str> {
    #[cfg(not(feature = "tools-edit"))]
    {
        let _ = document;
        Some(NO_EDIT_TOOLS)
    }
    #[cfg(feature = "tools-edit")]
    {
        document
    }
}

/// Which pages a crop applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum CropScope {
    /// The pages the dialog was opened on: the grid's selection, or the page
    /// on screen.
    Chosen,
    All,
}

/// What the dialog's controls do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum CropAction {
    SetBox(PageBox),
    SetScope(CropScope),
    /// Toggle Remove White Margins, which fits each page to what it draws.
    RemoveWhiteMargins,
    /// Toggle Change Page Size, which gives the pages a new media box first.
    ChangePageSize,
    SetToZero,
    Submit,
}

/// The dialog's choices, apart from what is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct CropForm {
    pub(in crate::shell) which: PageBox,
    pub(in crate::shell) scope: CropScope,
    pub(in crate::shell) remove_white: bool,
    pub(in crate::shell) resize: bool,
}

impl Default for CropForm {
    fn default() -> Self {
        Self {
            which: PageBox::Crop,
            scope: CropScope::Chosen,
            remove_white: false,
            resize: false,
        }
    }
}

impl CropForm {
    /// Apply a control that changes only the form; Set To Zero and Crop are
    /// the frame's.
    pub(in crate::shell) fn apply(&mut self, action: CropAction) {
        match action {
            CropAction::SetBox(which) => self.which = which,
            CropAction::SetScope(scope) => self.scope = scope,
            CropAction::RemoveWhiteMargins => self.remove_white = !self.remove_white,
            CropAction::ChangePageSize => self.resize = !self.resize,
            CropAction::SetToZero | CropAction::Submit => {}
        }
    }
}

/// A crop the form asks for, checked.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct CropRequest {
    pub(in crate::shell) pages: Vec<PageIndex>,
    pub(in crate::shell) which: PageBox,
    /// `None`: fit each page to what it draws.
    pub(in crate::shell) margins: Option<Margins>,
    /// A new width and height, in points as shown, before the margins.
    pub(in crate::shell) page_size: Option<(f64, f64)>,
}

/// What is typed into the dialog: the four margins (top, bottom, left,
/// right), then the page's width and height.
pub(in crate::shell) type Typed<'a> = [&'a str; 6];

/// The fields, in the order the dialog shows them.
pub(in crate::shell) const TEXT_FIELDS: [TextField; 6] = [
    TextField::CropTop,
    TextField::CropBottom,
    TextField::CropLeft,
    TextField::CropRight,
    TextField::CropWidth,
    TextField::CropHeight,
];

/// Read the form and what is typed into a crop, or say what is wrong.
pub(in crate::shell) fn request(
    form: CropForm,
    chosen: &[PageIndex],
    page_count: usize,
    typed: Typed<'_>,
) -> Result<CropRequest, String> {
    let pages = match form.scope {
        CropScope::Chosen => chosen.to_vec(),
        CropScope::All => (0..page_count).collect(),
    };
    if pages.is_empty() {
        return Err("There are no pages to crop.".to_owned());
    }
    if form.remove_white {
        return Ok(CropRequest {
            pages,
            which: form.which,
            margins: None,
            page_size: None,
        });
    }
    let [top, bottom, left, right] = [0, 1, 2, 3].map(|at| margin(typed[at]));
    let page_size = if form.resize {
        Some((size(typed[4])?, size(typed[5])?))
    } else {
        None
    };
    Ok(CropRequest {
        pages,
        which: form.which,
        margins: Some(Margins {
            top: top?,
            bottom: bottom?,
            left: left?,
            right: right?,
        }),
        page_size,
    })
}

/// One side of a page, in points: at least a point.
fn size(typed: &str) -> Result<f64, String> {
    let typed = typed.trim();
    typed
        .parse::<f64>()
        .ok()
        .filter(|points| points.is_finite() && *points >= onionskin_core::pages::MIN_BOX_SIZE)
        .ok_or_else(|| format!("{typed:?} is not a page size in points"))
}

/// One margin, in points: a number no less than zero.
fn margin(typed: &str) -> Result<f64, String> {
    let typed = typed.trim();
    typed
        .parse::<f64>()
        .ok()
        .filter(|points| points.is_finite() && *points >= 0.0)
        .ok_or_else(|| format!("{typed:?} is not a margin in points"))
}

/// Whether the form shows `field`: the margins unless white margins are
/// removed, and the size while the page size is changed as well.
pub(in crate::shell) fn shows(form: CropForm, field: TextField) -> bool {
    match field {
        TextField::CropWidth | TextField::CropHeight => form.resize && !form.remove_white,
        _ => !form.remove_white,
    }
}

/// A margin as the field shows it: no more than two decimals, and none
/// that are zero.
pub(in crate::shell) fn shown(points: f64) -> String {
    let text = format!("{points:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The dialog, open.
pub(in crate::shell) struct CropDialogState {
    pub(in crate::shell) form: CropForm,
    pub(in crate::shell) chosen: Vec<PageIndex>,
    pub(in crate::shell) page_count: usize,
    pub(in crate::shell) top: Entity<SearchInput>,
    pub(in crate::shell) bottom: Entity<SearchInput>,
    pub(in crate::shell) left: Entity<SearchInput>,
    pub(in crate::shell) right: Entity<SearchInput>,
    pub(in crate::shell) width: Entity<SearchInput>,
    pub(in crate::shell) height: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
}

impl CropDialogState {
    /// The dialog on `chosen` of `page_count` pages, its margins those of
    /// the first chosen page's crop box and its size that page's, as shown.
    pub(in crate::shell) fn new(
        chosen: Vec<PageIndex>,
        page_count: usize,
        margins: Margins,
        size: (f64, f64),
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let mut field = |id: &'static str, points: f64| {
            cx.new(|cx| {
                let mut input = SearchInput::with_placeholder(id, "0", theme, cx);
                input.set_query(shown(points), cx);
                input
            })
        };
        Self {
            form: CropForm::default(),
            top: field("crop-top", margins.top),
            bottom: field("crop-bottom", margins.bottom),
            left: field("crop-left", margins.left),
            right: field("crop-right", margins.right),
            width: field("crop-width", size.0),
            height: field("crop-height", size.1),
            chosen,
            page_count,
            error: None,
        }
    }

    pub(in crate::shell) fn text_field(&self, field: TextField) -> Option<&Entity<SearchInput>> {
        if !shows(self.form, field) {
            return None;
        }
        match field {
            TextField::CropTop => Some(&self.top),
            TextField::CropBottom => Some(&self.bottom),
            TextField::CropLeft => Some(&self.left),
            TextField::CropRight => Some(&self.right),
            TextField::CropWidth => Some(&self.width),
            TextField::CropHeight => Some(&self.height),
            _ => None,
        }
    }

    fn fields(&self) -> [&Entity<SearchInput>; 6] {
        [
            &self.top,
            &self.bottom,
            &self.left,
            &self.right,
            &self.width,
            &self.height,
        ]
    }

    /// The crop the dialog asks for, as it stands.
    pub(in crate::shell) fn request(&self, cx: &gpui::App) -> Result<CropRequest, String> {
        let typed = self.fields().map(|field| field.read(cx).query().to_owned());
        request(
            self.form,
            &self.chosen,
            self.page_count,
            std::array::from_fn(|at| typed[at].as_str()),
        )
    }

    /// Set To Zero: every margin 0.
    pub(in crate::shell) fn zero(&self, cx: &mut Context<ShellFrame>) {
        for field in &self.fields()[..4] {
            field.update(cx, |input, cx| input.set_query("0", cx));
        }
    }

    /// What the Pages choice says for the chosen pages.
    pub(in crate::shell) fn chosen_label(&self) -> String {
        match self.chosen.as_slice() {
            [page] => format!("Page {}", page + 1),
            pages => format!("The {} chosen pages", pages.len()),
        }
    }
}

#[cfg(test)]
mod tests;
