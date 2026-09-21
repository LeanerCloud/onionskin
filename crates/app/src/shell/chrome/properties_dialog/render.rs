//! The dialog's pixels. Built from the same model the accessible description
//! is, so a row on screen and the node describing it name the same thing.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::model::{self, PropertiesTab};
use super::{choice_id, tab_id, PropertiesAction, PropertiesDialogState};
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::combine_dialog::button;
use crate::shell::chrome::{ShellFrame, ThemeTokens};

/// How tall a tab's content may grow before it scrolls: what fits in the
/// dialog frame's 520 with its title, the tabs and Apply.
const CONTENT_HEIGHT: f32 = 330.0;

pub(in crate::shell) fn render(
    state: &PropertiesDialogState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut tabs = div().flex().gap_1().pb_2();
    for (index, tab) in PropertiesTab::ALL.into_iter().enumerate() {
        tabs = tabs.child(choice(
            tab_id(index),
            tab.label().to_owned(),
            tab == state.tab,
            PropertiesAction::Tab(tab),
            theme,
            cx,
        ));
    }
    // The tab's own content scrolls, between the tabs and Apply, so Apply
    // stays on screen on every tab instead of scrolling away under the
    // longest one (found by hand on Initial View).
    let mut body = div().flex().flex_col().gap_2();
    body = match state.tab {
        PropertiesTab::Description => description(state, theme, body),
        PropertiesTab::Security => state
            .facts
            .security
            .iter()
            .fold(body, |body, (label, value)| {
                body.child(fact(label, value, theme))
            }),
        PropertiesTab::Fonts => fonts(state, theme, body),
        PropertiesTab::InitialView => initial_view(state, theme, cx, body),
        PropertiesTab::Custom => custom(state, theme, focused, cx, body),
    };
    let mut body = div().flex().flex_col().gap_2().child(tabs).child(
        div()
            .id("properties-content")
            .max_h(px(CONTENT_HEIGHT))
            .overflow_y_scroll()
            .child(body),
    );
    if let Some(error) = &state.error {
        body = body.child(
            div()
                .id("properties-error")
                .text_color(theme.error_text)
                .child(error.clone()),
        );
    }
    body.child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(button(
                "properties-apply",
                "Apply",
                state.facts.edit_refusal.is_none(),
                theme,
                focused,
                cx,
                Activation::Properties(PropertiesAction::Apply),
            ))
            .when_some(state.facts.edit_refusal, |row, reason| {
                row.child(div().text_xs().text_color(theme.muted_text).child(reason))
            }),
    )
}

fn labelled(label: &'static str, theme: ThemeTokens, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(120.0))
                .flex_none()
                .text_sm()
                .text_color(theme.secondary_text)
                .child(label),
        )
        .child(div().flex_1().child(control))
}

fn fact(label: &'static str, value: &str, theme: ThemeTokens) -> gpui::Div {
    labelled(
        label,
        theme,
        div().text_color(theme.muted_text).child(value.to_owned()),
    )
}

fn description(state: &PropertiesDialogState, theme: ThemeTokens, body: gpui::Div) -> gpui::Div {
    let body = body
        .child(labelled("Title", theme, state.title.clone()))
        .child(labelled("Author", theme, state.author.clone()))
        .child(labelled("Subject", theme, state.subject.clone()))
        .child(labelled("Keywords", theme, state.keywords.clone()));
    state.facts.file.iter().fold(body, |body, (label, value)| {
        body.child(fact(label, value, theme))
    })
}

fn fonts(state: &PropertiesDialogState, theme: ThemeTokens, body: gpui::Div) -> gpui::Div {
    match &state.facts.fonts {
        Ok(fonts) if fonts.is_empty() => body.child("This document names no fonts."),
        Ok(fonts) => fonts.iter().fold(body, |body, font| {
            body.child(div().child(model::font_label(font)))
        }),
        Err(error) => body.child(div().text_color(theme.error_text).child(error.clone())),
    }
}

fn choice(
    id: gpui::ElementId,
    label: String,
    selected: bool,
    action: PropertiesAction,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .cursor_pointer()
        .when(selected, |row| row.bg(theme.selected))
        .hover(move |row| row.bg(theme.subtle_hover))
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(Activation::Properties(action), window, cx);
        }))
        .child(label)
}

fn choices<T: Copy + PartialEq>(
    group: &'static str,
    label: &'static str,
    items: impl Iterator<Item = T>,
    in_force: T,
    describe: impl Fn(T) -> (String, PropertiesAction),
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut row = div().flex().flex_wrap().gap_1();
    for (index, item) in items.enumerate() {
        let (text, action) = describe(item);
        row = row.child(choice(
            choice_id(group, index),
            text,
            item == in_force,
            action,
            theme,
            cx,
        ));
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(theme.secondary_text)
                .child(label),
        )
        .child(row)
}

fn initial_view(
    state: &PropertiesDialogState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
    body: gpui::Div,
) -> gpui::Div {
    body.child(choices(
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
        theme,
        cx,
    ))
    .child(choices(
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
        theme,
        cx,
    ))
    .child(choices(
        "properties-fit",
        "Magnification",
        model::fit_choices(state.fit).into_iter(),
        state.fit,
        |fit| (fit.label(), PropertiesAction::Fit(fit)),
        theme,
        cx,
    ))
    .child(labelled("Open to page", theme, state.open_page.clone()))
}

fn custom(
    state: &PropertiesDialogState,
    theme: ThemeTokens,
    focused: Option<&gpui::ElementId>,
    cx: &mut Context<ShellFrame>,
    body: gpui::Div,
) -> gpui::Div {
    let mut body = body;
    for (index, (key, value)) in state.custom.iter().enumerate() {
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(format!("{key}: {value}"))
                .child(choice(
                    choice_id("properties-custom", index),
                    "Remove".to_owned(),
                    false,
                    PropertiesAction::RemoveCustom(index),
                    theme,
                    cx,
                )),
        );
    }
    body.child(labelled("Name", theme, state.custom_key.clone()))
        .child(labelled("Value", theme, state.custom_value.clone()))
        .child(button(
            "properties-custom-add",
            "Add",
            true,
            theme,
            focused,
            cx,
            Activation::Properties(PropertiesAction::AddCustom),
        ))
}
