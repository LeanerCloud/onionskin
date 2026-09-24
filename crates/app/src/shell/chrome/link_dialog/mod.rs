//! Create Link and Link Properties (M5): where a link goes and how it looks.
//!
//! One dialog for a rectangle the Link tool drew, or selected text, and for
//! an existing link the tool clicked, which it can also delete. What the
//! form means is plain data ([`LinkForm`], [`request`]), tested without a
//! window; the frame writes the link through `tools-edit`.

mod view;

use std::path::PathBuf;

use gpui::{AppContext as _, Context, Entity};
use onionskin_core::links::{Highlight, LineStyle, Link, LinkLook, LinkTarget};
use onionskin_core::{ObjRef, PageIndex, PageRect};

use super::accessible::TextField;
use super::{SearchInput, ShellFrame, ThemeTokens};

pub(in crate::shell) use view::{accessible, render};

/// The colours a visible link's rectangle takes, in turn; Acrobat's blue
/// first.
pub(in crate::shell) const COLORS: [(&str, [f64; 3]); 5] = [
    ("Blue", [0.0, 0.0, 1.0]),
    ("Black", [0.0, 0.0, 0.0]),
    ("Red", [1.0, 0.0, 0.0]),
    ("Green", [0.0, 0.5, 0.0]),
    ("Gray", [0.5, 0.5, 0.5]),
];

/// Where a link goes, as the dialog offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum TargetKind {
    Page,
    Web,
    File,
    /// Keep an action the dialog does not write, as a JavaScript one.
    Keep,
}

impl TargetKind {
    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Page => "Go to a page",
            Self::Web => "Open a web page",
            Self::File => "Open a file",
            Self::Keep => "Keep its action",
        }
    }
}

/// A typed field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::shell) enum LinkField {
    Page,
    Url,
}

impl LinkField {
    pub(in crate::shell) const ALL: [LinkField; 2] = [LinkField::Page, LinkField::Url];

    pub(in crate::shell) fn id(self) -> &'static str {
        match self {
            Self::Page => "link-page",
            Self::Url => "link-url",
        }
    }

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Page => "Page number",
            Self::Url => "Web address",
        }
    }
}

/// What a control that is not typed into does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum LinkAction {
    SetKind(TargetKind),
    Visible,
    NextWidth,
    NextColor,
    NextStyle,
    NextHighlight,
    ChooseFile,
    Submit,
    Delete,
}

/// What the dialog is about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) enum LinkMode {
    Create(PageRect),
    Edit { link: ObjRef, page: PageIndex },
}

/// The dialog's choices, apart from what is typed.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct LinkForm {
    pub(in crate::shell) kind: TargetKind,
    pub(in crate::shell) look: LinkLook,
    pub(in crate::shell) file: Option<PathBuf>,
    /// The action kept by [`TargetKind::Keep`], by name.
    pub(in crate::shell) kept: Option<String>,
}

impl Default for LinkForm {
    fn default() -> Self {
        Self {
            kind: TargetKind::Page,
            look: LinkLook::default(),
            file: None,
            kept: None,
        }
    }
}

impl LinkForm {
    /// The form for `link` as it is.
    pub(in crate::shell) fn of(link: &Link) -> Self {
        let (kind, file, kept) = match &link.target {
            LinkTarget::Page(_) => (TargetKind::Page, None, None),
            LinkTarget::Web(_) => (TargetKind::Web, None, None),
            LinkTarget::File(file) => (TargetKind::File, Some(PathBuf::from(file)), None),
            LinkTarget::Other(action) => (TargetKind::Keep, None, Some(action.clone())),
        };
        Self {
            kind,
            look: link.look,
            file,
            kept,
        }
    }

    /// The kinds offered: Keep only for a link that has an action to keep.
    pub(in crate::shell) fn kinds(&self) -> Vec<TargetKind> {
        let mut kinds = vec![TargetKind::Page, TargetKind::Web, TargetKind::File];
        if self.kept.is_some() {
            kinds.push(TargetKind::Keep);
        }
        kinds
    }

    pub(in crate::shell) fn apply(&mut self, action: LinkAction) {
        let look = &mut self.look;
        match action {
            LinkAction::SetKind(kind) => self.kind = kind,
            LinkAction::Visible => look.visible = !look.visible,
            LinkAction::NextWidth => {
                look.width = if look.width >= 3.0 {
                    1.0
                } else {
                    look.width.floor() + 1.0
                }
            }
            LinkAction::NextColor => {
                let at = COLORS.iter().position(|(_, color)| *color == look.color);
                look.color = COLORS[at.map_or(0, |at| (at + 1) % COLORS.len())].1;
            }
            LinkAction::NextStyle => look.style = next(&LineStyle::ALL, look.style),
            LinkAction::NextHighlight => look.highlight = next(&Highlight::ALL, look.highlight),
            LinkAction::ChooseFile | LinkAction::Submit | LinkAction::Delete => {}
        }
    }

    /// The fields shown.
    pub(in crate::shell) fn fields(&self) -> Vec<LinkField> {
        match self.kind {
            TargetKind::Page => vec![LinkField::Page],
            TargetKind::Web => vec![LinkField::Url],
            TargetKind::File | TargetKind::Keep => Vec::new(),
        }
    }
}

fn next<T: Copy + PartialEq>(all: &[T], current: T) -> T {
    let at = all.iter().position(|each| *each == current).unwrap_or(0);
    all[(at + 1) % all.len()]
}

/// The colour's name, or "Custom" for one not in the list.
pub(in crate::shell) fn color_name(color: [f64; 3]) -> &'static str {
    COLORS
        .iter()
        .find(|(_, named)| *named == color)
        .map_or("Custom", |(name, _)| name)
}

/// Read the form, the typed page number and web address, into a target and
/// a look, or say what is wrong.
pub(in crate::shell) fn request(
    form: &LinkForm,
    page: &str,
    url: &str,
    page_count: usize,
) -> Result<(LinkTarget, LinkLook), String> {
    let target = match form.kind {
        TargetKind::Page => {
            let typed = page.trim();
            let number = typed
                .parse::<usize>()
                .ok()
                .filter(|number| (1..=page_count).contains(number))
                .ok_or_else(|| format!("{typed:?} is not a page from 1 to {page_count}"))?;
            LinkTarget::Page(number - 1)
        }
        TargetKind::Web => {
            let typed = url.trim();
            if typed.is_empty() || typed.contains(char::is_whitespace) {
                return Err(format!("{typed:?} is not a web address"));
            }
            let has_scheme = typed.contains("://") || typed.starts_with("mailto:");
            LinkTarget::Web(if has_scheme {
                typed.to_owned()
            } else {
                format!("http://{typed}")
            })
        }
        TargetKind::File => LinkTarget::File(
            form.file
                .as_ref()
                .ok_or("Choose the file to open.")?
                .to_string_lossy()
                .into_owned(),
        ),
        TargetKind::Keep => LinkTarget::Other(form.kept.clone().unwrap_or_default()),
    };
    Ok((target, form.look))
}

/// The dialog, open.
pub(in crate::shell) struct LinkDialogState {
    pub(in crate::shell) mode: LinkMode,
    pub(in crate::shell) form: LinkForm,
    pub(in crate::shell) page_count: usize,
    pub(in crate::shell) page: Entity<SearchInput>,
    pub(in crate::shell) url: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
}

impl LinkDialogState {
    /// The dialog for `mode`, its fields holding `page` (one-based) and
    /// `url`.
    pub(in crate::shell) fn new(
        mode: LinkMode,
        form: LinkForm,
        page_count: usize,
        (page, url): (String, String),
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let mut field = |which: LinkField, text: String| {
            cx.new(|cx| {
                let mut input = SearchInput::with_placeholder(which.id(), which.label(), theme, cx);
                input.set_query(text, cx);
                input
            })
        };
        Self {
            mode,
            form,
            page_count,
            page: field(LinkField::Page, page),
            url: field(LinkField::Url, url),
            error: None,
        }
    }

    pub(in crate::shell) fn text_field(&self, field: LinkField) -> Option<&Entity<SearchInput>> {
        if !self.form.fields().contains(&field) {
            return None;
        }
        Some(match field {
            LinkField::Page => &self.page,
            LinkField::Url => &self.url,
        })
    }

    pub(in crate::shell) fn request(
        &self,
        cx: &gpui::App,
    ) -> Result<(LinkTarget, LinkLook), String> {
        request(
            &self.form,
            self.page.read(cx).query(),
            self.url.read(cx).query(),
            self.page_count,
        )
    }
}

/// The dialog's text fields, for the focus ring.
pub(in crate::shell) fn text_fields() -> impl Iterator<Item = TextField> {
    LinkField::ALL.into_iter().map(TextField::Link)
}

#[cfg(test)]
mod tests;
