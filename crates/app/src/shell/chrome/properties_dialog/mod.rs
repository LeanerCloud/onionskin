//! File > Properties: the document-level dialog.
//!
//! Five tabs, as Acrobat's unified UI has them. Description and Custom write
//! `/Info` and XMP through `core::metadata`; Initial View writes the
//! catalog's open action, layout and mode. Security and Fonts are read-only:
//! Security because Protect Using Password changes it, Fonts because a
//! document's fonts are what its pages draw with. Read-only is carried in the
//! accessibility state, not only in how the rows look.
//!
//! One Apply for every tab, and one undo step for whatever it changes: the
//! dialog is one gesture however many tabs the user visited.

mod model;
mod render;

use accesskit::Role;
use gpui::{AppContext as _, Context, Entity};
use onionskin_core::metadata::{
    Description, FontEntry, InitialView, PageLayout, PageMode, PropertiesEdit,
};

pub(in crate::shell) use model::{date_label, size_label, FitChoice, PropertiesTab};
pub(in crate::shell) use render::render;

use super::accessible::{Activation, Element, TextField};
use super::{SearchInput, ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PropertiesAction {
    Tab(PropertiesTab),
    Layout(Option<PageLayout>),
    Mode(Option<PageMode>),
    Fit(FitChoice),
    AddCustom,
    RemoveCustom(usize),
    Apply,
}

/// What the dialog read from the document when it opened, and shows without
/// letting it be changed.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct PropertiesFacts {
    /// The Description tab's file rows: location, size, pages, dates, and
    /// the programs that made it.
    pub(in crate::shell) file: Vec<(&'static str, String)>,
    pub(in crate::shell) security: Vec<(&'static str, String)>,
    pub(in crate::shell) fonts: Result<Vec<FontEntry>, String>,
    pub(in crate::shell) page_count: usize,
    /// Why the document may not be edited, which disables Apply.
    pub(in crate::shell) edit_refusal: Option<&'static str>,
}

/// The Security tab's rows, as Acrobat's Document Restrictions Summary
/// lists them: what the document allows as it was opened.
pub(in crate::shell) fn security_rows(
    facts: &onionskin_core::security::SecurityFacts,
    change_refusal: Option<&'static str>,
) -> Vec<(&'static str, String)> {
    let allowed = |allowed: bool| if allowed { "Allowed" } else { "Not Allowed" }.to_owned();
    let p = facts.permitted;
    let printing = match (p.print(), p.print_high()) {
        (false, _) => "Not Allowed",
        (true, false) => "Low Resolution",
        (true, true) => "High Resolution",
    };
    let opened = match facts.access {
        Some(onionskin_core::security::Access::Owner) => "The permissions password",
        Some(onionskin_core::security::Access::User) => "The open password, or none",
        None => "No password",
    };
    vec![
        ("Security Method", facts.method.to_owned()),
        ("Encryption Level", facts.level.unwrap_or("None").to_owned()),
        ("Opened With", opened.to_owned()),
        ("Printing", printing.to_owned()),
        ("Changing the Document", allowed(p.modify())),
        ("Document Assembly", allowed(p.modify() || p.assemble())),
        ("Content Copying", allowed(p.extract())),
        (
            "Content Copying for Accessibility",
            allowed(p.extract() || p.accessibility()),
        ),
        ("Commenting", allowed(p.modify() || p.annotate())),
        (
            "Filling of Form Fields",
            allowed(p.modify() || p.annotate() || p.fill_forms()),
        ),
        (
            "Changing Security Settings",
            match change_refusal {
                Some(_) => "Needs the permissions password",
                None => "Allowed, with File > Protect Using Password",
            }
            .to_owned(),
        ),
    ]
}

/// The dialog's state in the frame.
pub(in crate::shell) struct PropertiesDialogState {
    pub(in crate::shell) tab: PropertiesTab,
    pub(in crate::shell) facts: PropertiesFacts,
    pub(super) title: Entity<SearchInput>,
    pub(super) author: Entity<SearchInput>,
    pub(super) subject: Entity<SearchInput>,
    pub(super) keywords: Entity<SearchInput>,
    pub(super) custom_key: Entity<SearchInput>,
    pub(super) custom_value: Entity<SearchInput>,
    pub(super) open_page: Entity<SearchInput>,
    pub(in crate::shell) custom: Vec<(String, String)>,
    pub(in crate::shell) layout: Option<PageLayout>,
    pub(in crate::shell) mode: Option<PageMode>,
    pub(in crate::shell) fit: FitChoice,
    /// What the document said when the dialog opened: Apply writes only a
    /// half that differs, so a look at the dialog is not an edit.
    original: (PropertiesEdit, InitialView),
    pub(in crate::shell) error: Option<String>,
}

/// What opening the dialog read from the document.
pub(in crate::shell) struct PropertiesSource {
    pub(in crate::shell) tab: PropertiesTab,
    pub(in crate::shell) description: Description,
    pub(in crate::shell) custom: Vec<(String, String)>,
    pub(in crate::shell) view: InitialView,
    pub(in crate::shell) facts: PropertiesFacts,
}

fn field(
    id: &'static str,
    placeholder: &'static str,
    value: Option<&str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> Entity<SearchInput> {
    cx.new(|cx| {
        let mut input = SearchInput::with_placeholder(id, placeholder, theme, cx);
        if let Some(value) = value {
            input.set_query(value, cx);
        }
        input
    })
}

impl PropertiesDialogState {
    pub(super) fn new(
        source: PropertiesSource,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let description = &source.description;
        let page = source.view.page.map(|page| (page + 1).to_string());
        Self {
            tab: source.tab,
            title: field(
                "properties-title",
                "Title",
                description.title.as_deref(),
                theme,
                cx,
            ),
            author: field(
                "properties-author",
                "Author",
                description.author.as_deref(),
                theme,
                cx,
            ),
            subject: field(
                "properties-subject",
                "Subject",
                description.subject.as_deref(),
                theme,
                cx,
            ),
            keywords: field(
                "properties-keywords",
                "Keywords",
                description.keywords.as_deref(),
                theme,
                cx,
            ),
            custom_key: field("properties-custom-key", "Name", None, theme, cx),
            custom_value: field("properties-custom-value", "Value", None, theme, cx),
            open_page: field("properties-open-page", "Page", page.as_deref(), theme, cx),
            custom: source.custom.clone(),
            layout: source.view.layout,
            mode: source.view.mode,
            fit: FitChoice::of(source.view.fit),
            original: (
                PropertiesEdit {
                    description: source.description,
                    custom: source.custom,
                },
                source.view,
            ),
            facts: source.facts,
            error: None,
        }
    }

    /// The Description and Custom tabs as they stand.
    pub(super) fn edit(&self, cx: &gpui::App) -> PropertiesEdit {
        let text = |input: &Entity<SearchInput>| input.read(cx).query().to_owned();
        let fields = [&self.title, &self.author, &self.subject, &self.keywords].map(text);
        PropertiesEdit {
            description: model::description(fields.each_ref().map(String::as_str)),
            custom: self.custom.clone(),
        }
    }

    /// The Initial View tab as it stands, or why the open page is wrong.
    pub(super) fn view(&self, cx: &gpui::App) -> Result<InitialView, String> {
        let page = model::open_page(self.open_page.read(cx).query(), self.facts.page_count)?;
        Ok(model::initial_view(self.layout, self.mode, page, self.fit))
    }

    /// Which halves differ from what the document said.
    pub(super) fn changes(
        &self,
        cx: &gpui::App,
    ) -> Result<(Option<PropertiesEdit>, Option<InitialView>), String> {
        let edit = self.edit(cx);
        let view = self.view(cx)?;
        let (original_edit, original_view) = &self.original;
        Ok((
            (edit != *original_edit).then_some(edit),
            (view != *original_view).then_some(view),
        ))
    }

    /// Take the Custom tab's two fields into the list.
    pub(super) fn add_custom(&mut self, cx: &mut Context<ShellFrame>) -> Result<(), String> {
        let key = self.custom_key.read(cx).query().to_owned();
        let value = self.custom_value.read(cx).query().to_owned();
        model::add_custom(&mut self.custom, &key, &value)?;
        for input in [&self.custom_key, &self.custom_value] {
            input.update(cx, |input, cx| input.set_query("", cx));
        }
        Ok(())
    }

    pub(super) fn text_field(&self, field: TextField) -> Option<&Entity<SearchInput>> {
        let (tab, input) = match field {
            TextField::PropertiesTitle => (PropertiesTab::Description, &self.title),
            TextField::PropertiesAuthor => (PropertiesTab::Description, &self.author),
            TextField::PropertiesSubject => (PropertiesTab::Description, &self.subject),
            TextField::PropertiesKeywords => (PropertiesTab::Description, &self.keywords),
            TextField::PropertiesCustomKey => (PropertiesTab::Custom, &self.custom_key),
            TextField::PropertiesCustomValue => (PropertiesTab::Custom, &self.custom_value),
            TextField::PropertiesOpenPage => (PropertiesTab::InitialView, &self.open_page),
            _ => return None,
        };
        (self.tab == tab).then_some(input)
    }
}

/// The text fields the dialog can hold, for the frame's focus bookkeeping.
pub(in crate::shell) const TEXT_FIELDS: [TextField; 7] = [
    TextField::PropertiesTitle,
    TextField::PropertiesAuthor,
    TextField::PropertiesSubject,
    TextField::PropertiesKeywords,
    TextField::PropertiesCustomKey,
    TextField::PropertiesCustomValue,
    TextField::PropertiesOpenPage,
];

fn tab_id(index: usize) -> gpui::ElementId {
    ("properties-tab", index).into()
}

fn choice_id(group: &'static str, index: usize) -> gpui::ElementId {
    (group, index).into()
}

/// A row the user reads and cannot change.
fn read_only_row(id: gpui::ElementId, label: &str, value: &str) -> Element {
    Element::new(id, Role::TextInput, label)
        .with_value(value)
        .with_state(A11yState::read_only())
}

fn radio_group<T: Copy + PartialEq>(
    id: &'static str,
    label: &'static str,
    choices: impl Iterator<Item = T>,
    in_force: T,
    describe: impl Fn(T) -> (String, PropertiesAction),
) -> Element {
    let rows = choices
        .enumerate()
        .map(|(index, choice)| {
            let (text, action) = describe(choice);
            Element::new(choice_id(id, index), Role::RadioButton, text)
                .with_state(A11yState::selected(choice == in_force))
                .with_activation(Activation::Properties(action))
        })
        .collect();
    Element::new(id, Role::RadioGroup, label).with_children(rows)
}

/// What the dialog tells a screen reader.
pub(in crate::shell) fn accessible(state: &PropertiesDialogState, cx: &gpui::App) -> Vec<Element> {
    let tabs = Element::new("properties-tabs", Role::TabList, "Properties").with_children(
        PropertiesTab::ALL
            .into_iter()
            .enumerate()
            .map(|(index, tab)| {
                Element::new(tab_id(index), Role::Tab, tab.label())
                    .with_state(A11yState::selected(tab == state.tab))
                    .with_activation(Activation::Properties(PropertiesAction::Tab(tab)))
            })
            .collect(),
    );
    let mut body = vec![tabs];
    body.extend(tab_body(state, cx));
    if let Some(error) = &state.error {
        body.push(Element::new("properties-error", Role::Alert, error.clone()));
    }
    let mut apply = Element::new("properties-apply", Role::Button, "Apply")
        .with_activation(Activation::Properties(PropertiesAction::Apply));
    if let Some(reason) = state.facts.edit_refusal {
        apply = apply
            .with_state(A11yState::enabled(false))
            .with_description(reason);
    }
    body.push(apply);
    body
}

fn tab_body(state: &PropertiesDialogState, cx: &gpui::App) -> Vec<Element> {
    let input =
        |entity: &Entity<SearchInput>, label, field| entity.read(cx).accessible(label, field);
    match state.tab {
        PropertiesTab::Description => {
            let mut rows = vec![
                input(&state.title, "Title", TextField::PropertiesTitle),
                input(&state.author, "Author", TextField::PropertiesAuthor),
                input(&state.subject, "Subject", TextField::PropertiesSubject),
                input(&state.keywords, "Keywords", TextField::PropertiesKeywords),
            ];
            rows.extend(
                state
                    .facts
                    .file
                    .iter()
                    .enumerate()
                    .map(|(index, (label, value))| {
                        read_only_row(choice_id("properties-file", index), label, value)
                    }),
            );
            rows
        }
        PropertiesTab::Security => state
            .facts
            .security
            .iter()
            .enumerate()
            .map(|(index, (label, value))| {
                read_only_row(choice_id("properties-security", index), label, value)
            })
            .collect(),
        PropertiesTab::Fonts => vec![fonts_list(&state.facts.fonts)],
        PropertiesTab::InitialView => vec![
            radio_group(
                "properties-layout",
                "Page Layout",
                model::layouts(),
                state.layout,
                |layout| {
                    (
                        model::layout_label(layout).to_owned(),
                        PropertiesAction::Layout(layout),
                    )
                },
            ),
            radio_group(
                "properties-mode",
                "Navigation Tab",
                model::modes(),
                state.mode,
                |mode| {
                    (
                        model::mode_label(mode).to_owned(),
                        PropertiesAction::Mode(mode),
                    )
                },
            ),
            radio_group(
                "properties-fit",
                "Magnification",
                model::fit_choices(state.fit).into_iter(),
                state.fit,
                |fit| (fit.label(), PropertiesAction::Fit(fit)),
            ),
            input(
                &state.open_page,
                "Open to page",
                TextField::PropertiesOpenPage,
            ),
        ],
        PropertiesTab::Custom => {
            let mut rows: Vec<Element> = state
                .custom
                .iter()
                .enumerate()
                .map(|(index, (key, value))| {
                    Element::new(
                        choice_id("properties-custom", index),
                        Role::Button,
                        format!("Remove {key}"),
                    )
                    .with_description(format!("{key}: {value}"))
                    .with_activation(Activation::Properties(PropertiesAction::RemoveCustom(
                        index,
                    )))
                })
                .collect();
            rows.extend([
                input(&state.custom_key, "Name", TextField::PropertiesCustomKey),
                input(
                    &state.custom_value,
                    "Value",
                    TextField::PropertiesCustomValue,
                ),
                Element::new("properties-custom-add", Role::Button, "Add")
                    .with_activation(Activation::Properties(PropertiesAction::AddCustom)),
            ]);
            rows
        }
    }
}

fn fonts_list(fonts: &Result<Vec<FontEntry>, String>) -> Element {
    match fonts {
        Ok(fonts) if fonts.is_empty() => Element::new(
            "properties-fonts-none",
            Role::Label,
            "This document names no fonts.",
        ),
        Ok(fonts) => Element::new("properties-fonts", Role::List, "Fonts").with_children(
            fonts
                .iter()
                .enumerate()
                .map(|(index, font)| {
                    Element::new(
                        choice_id("properties-font", index),
                        Role::ListItem,
                        model::font_label(font),
                    )
                    .with_state(A11yState::read_only())
                })
                .collect(),
        ),
        Err(error) => Element::new("properties-fonts-error", Role::Alert, error.clone()),
    }
}
