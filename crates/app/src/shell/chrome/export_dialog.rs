use std::fmt;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    actions, div, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_plugin_api::{ExportRequest, PageRange};

use super::accessible::{Activation, Element, Rects, Surface, TextField};
use super::global_bar::ExportTarget;
use super::page_controls::{parse_page_entry, PageEntryError};
use super::{SearchInput, ShellFrame, ThemeTokens};

pub(in crate::shell) const EXPORT_KEY_CONTEXT: &str = "OnionskinExport";
pub(in crate::shell) const DEFAULT_EXPORT_DPI: f32 = 150.0;

actions!(onionskin_export, [SubmitExport]);

pub(in crate::shell) fn install_keybindings(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "enter",
        SubmitExport,
        Some(EXPORT_KEY_CONTEXT),
    )]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Item {
    Target,
    First,
    Last,
    Dpi,
    Error,
    Export,
    Cancel,
}

pub(super) fn items(target: ExportTarget, has_error: bool) -> Vec<Item> {
    let mut items = vec![Item::Target, Item::First, Item::Last];
    if target == ExportTarget::Png {
        items.push(Item::Dpi);
    }
    if has_error {
        items.push(Item::Error);
    }
    items.extend([Item::Export, Item::Cancel]);
    items
}

#[derive(Debug)]
pub(super) enum ValidationError {
    First(PageEntryError),
    Last(PageEntryError),
    Range(onionskin_plugin_api::ExportError),
    Dpi(String),
    AlreadyInProgress,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::First(error) => write!(f, "First page: {error}"),
            Self::Last(error) => write!(f, "Last page: {error}"),
            Self::Range(error) => write!(f, "Page range: {error}"),
            Self::Dpi(value) => write!(
                f,
                "Resolution must be a positive finite number, got {value:?}"
            ),
            Self::AlreadyInProgress => write!(f, "an export is already in progress"),
        }
    }
}

pub(in crate::shell) struct ExportDialogState {
    pub(super) target: ExportTarget,
    pub(super) origin: gpui::EntityId,
    pub(super) page_count: usize,
    pub(super) first: Entity<SearchInput>,
    pub(super) last: Entity<SearchInput>,
    pub(super) dpi: Entity<SearchInput>,
    pub(super) error: Option<ValidationError>,
}

impl ExportDialogState {
    pub(super) fn new(
        target: ExportTarget,
        origin: gpui::EntityId,
        page_count: usize,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let first = cx.new(|cx| {
            let mut input = SearchInput::with_placeholder("export-first", "First", theme, cx);
            input.set_query("1", cx);
            input
        });
        let last = cx.new(|cx| {
            let mut input = SearchInput::with_placeholder("export-last", "Last", theme, cx);
            input.set_query(page_count.to_string(), cx);
            input
        });
        let dpi = cx.new(|cx| {
            let mut input =
                SearchInput::with_placeholder("export-dpi", "Resolution (DPI)", theme, cx);
            input.set_query(DEFAULT_EXPORT_DPI.to_string(), cx);
            input
        });
        Self {
            target,
            origin,
            page_count,
            first,
            last,
            dpi,
            error: None,
        }
    }

    pub(super) fn request(&self, cx: &App) -> Result<ExportRequest, ValidationError> {
        let first = parse_page_entry(self.first.read(cx).query(), self.page_count)
            .map_err(ValidationError::First)?;
        let last = parse_page_entry(self.last.read(cx).query(), self.page_count)
            .map_err(ValidationError::Last)?;
        let pages = PageRange::new(first, last, self.page_count).map_err(ValidationError::Range)?;
        let dpi = if self.target == ExportTarget::Png {
            let value = self.dpi.read(cx).query().trim();
            let dpi = value
                .parse::<f32>()
                .map_err(|_| ValidationError::Dpi(value.to_owned()))?;
            let request = ExportRequest { pages, dpi };
            request
                .zoom()
                .map_err(|_| ValidationError::Dpi(value.to_owned()))?;
            dpi
        } else {
            DEFAULT_EXPORT_DPI
        };
        Ok(ExportRequest { pages, dpi })
    }

    pub(super) fn set_theme(&self, theme: ThemeTokens, cx: &mut Context<ShellFrame>) {
        for input in [&self.first, &self.last, &self.dpi] {
            input.update(cx, |input, cx| input.set_theme(theme, cx));
        }
    }
}

pub(in crate::shell) fn accessible(
    state: &ExportDialogState,
    rects: &Rects,
    cx: &App,
) -> Vec<Element> {
    let mut body: Vec<_> = items(state.target, state.error.is_some())
        .into_iter()
        .map(|item| match item {
            Item::Target => Element::new("export-target", Role::Label, state.target.label()),
            Item::First => state
                .first
                .read(cx)
                .accessible("First page", TextField::ExportFirst),
            Item::Last => state
                .last
                .read(cx)
                .accessible("Last page", TextField::ExportLast),
            Item::Dpi => state
                .dpi
                .read(cx)
                .accessible("Resolution (DPI)", TextField::ExportDpi),
            Item::Error => Element::new(
                "export-error",
                Role::Alert,
                state
                    .error
                    .as_ref()
                    .expect("error item has an error")
                    .to_string(),
            ),
            Item::Export => Element::new("export-submit", Role::Button, "Export")
                .with_activation(Activation::SubmitExport),
            Item::Cancel => Element::new("export-cancel", Role::Button, "Cancel")
                .with_activation(Activation::CloseDialog),
        })
        .collect();
    for (row, bounds) in body.iter_mut().zip(rects.of(Surface::ExportDialog)) {
        row.bounds = Some(bounds);
    }
    body
}

pub(in crate::shell) fn render(
    state: &ExportDialogState,
    rects: Rects,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::ExportDialog, &bounds, window);
        })
        .key_context(EXPORT_KEY_CONTEXT)
        .on_action(cx.listener(|frame, _: &SubmitExport, window, cx| {
            frame.submit_export(window, cx);
        }))
        .flex()
        .flex_col()
        .gap_2();
    for item in items(state.target, state.error.is_some()) {
        body = body.child(match item {
            Item::Target => div()
                .id("export-target")
                .child(state.target.label())
                .into_any_element(),
            Item::First => field("First page", state.first.clone()).into_any_element(),
            Item::Last => field("Last page", state.last.clone()).into_any_element(),
            Item::Dpi => field("Resolution (DPI)", state.dpi.clone()).into_any_element(),
            Item::Error => div()
                .id("export-error")
                .text_color(theme.error_text)
                .child(
                    state
                        .error
                        .as_ref()
                        .expect("error item has an error")
                        .to_string(),
                )
                .into_any_element(),
            Item::Export => button(
                "export-submit",
                "Export",
                theme,
                focused,
                cx,
                Activation::SubmitExport,
            ),
            Item::Cancel => button(
                "export-cancel",
                "Cancel",
                theme,
                focused,
                cx,
                Activation::CloseDialog,
            ),
        });
    }
    body
}

fn field(label: &'static str, input: Entity<SearchInput>) -> gpui::Div {
    div().flex().flex_col().gap_1().child(label).child(input)
}

fn button(
    id: &'static str,
    label: &'static str,
    theme: ThemeTokens,
    focused: Option<&gpui::ElementId>,
    cx: &mut Context<ShellFrame>,
    activation: Activation,
) -> gpui::AnyElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .when(focused == Some(&id.into()), |button| {
            button.bg(theme.selected)
        })
        .cursor_pointer()
        .hover(move |button| button.bg(theme.subtle_hover))
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(activation.clone(), window, cx);
        }))
        .child(label)
        .into_any_element()
}
