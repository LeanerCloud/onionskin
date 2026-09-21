//! The comment properties inspector: the side panel's first tool-specific
//! content, as Acrobat's Properties bar is for a chosen comment.
//!
//! Colour and opacity apply the moment they are clicked, each one undoable
//! edit, because that is how Acrobat's colour and opacity pickers behave.
//! Author and subject are typed, so they wait for Save. "Make Current
//! Properties Default" makes this comment's colour and opacity the look of
//! the next comment of its kind.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rgb, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::properties::CommentProperties;
use onionskin_core::{Color, ReadAnnotation};

use super::accessible::{Activation, Element, TextField};
use super::tabs::ShellFrame;
use super::theme::ThemeTokens;
use super::SearchInput;
use crate::a11y::State as A11yState;

/// Acrobat's comment colour swatches, by name so a screen reader can say
/// which one is chosen.
pub(in crate::shell) const PALETTE: [(&str, [u8; 3]); 8] = [
    ("Red", [229, 57, 53]),
    ("Orange", [251, 140, 0]),
    ("Yellow", [255, 209, 51]),
    ("Green", [67, 160, 71]),
    ("Blue", [30, 136, 229]),
    ("Purple", [142, 36, 170]),
    ("Black", [0, 0, 0]),
    ("Grey", [117, 117, 117]),
];

/// The opacities offered, in percent.
pub(in crate::shell) const OPACITIES: [u8; 4] = [100, 75, 50, 25];

/// The ids the inspector's text fields publish.
pub(in crate::shell) const AUTHOR_ID: &str = "inspector-author";
pub(in crate::shell) const SUBJECT_ID: &str = "inspector-subject";

/// What the inspector can be asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum InspectorAction {
    /// Open the side panel on the chosen comment's properties.
    Show,
    /// Colour the chosen comment with swatch `index`.
    Color(usize),
    /// Set the chosen comment's opacity, in percent.
    Opacity(u8),
    /// Write the typed author and subject.
    SaveText,
    /// Make this comment's colour and opacity its kind's default.
    MakeDefault,
}

/// The inspector's own state: its two text fields, and which comment they
/// were filled from, so choosing another comment fills them again and a
/// redraw does not overwrite what is being typed.
pub(in crate::shell) struct InspectorState {
    pub(in crate::shell) author: Entity<SearchInput>,
    pub(in crate::shell) subject: Entity<SearchInput>,
    pub(in crate::shell) filled_from: Option<onionskin_core::ObjRef>,
}

/// A colour as the whole numbers the palette and preferences keep.
pub(in crate::shell) fn to_rgb(color: Color) -> [u8; 3] {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [
        channel(color.red),
        channel(color.green),
        channel(color.blue),
    ]
}

pub(in crate::shell) fn from_rgb([red, green, blue]: [u8; 3]) -> Color {
    let channel = |value: u8| f64::from(value) / 255.0;
    Color::new(channel(red), channel(green), channel(blue))
}

/// The comment's opacity in percent, 100 when the file gives none.
pub(in crate::shell) fn opacity_percent(comment: &ReadAnnotation) -> u8 {
    comment.opacity.map_or(100, |opacity| {
        (opacity.clamp(0.0, 1.0) * 100.0).round() as u8
    })
}

/// The properties the comment has now, which an edit changes one of.
pub(in crate::shell) fn current(comment: &ReadAnnotation) -> CommentProperties {
    CommentProperties {
        color: comment.color,
        opacity: f64::from(opacity_percent(comment)) / 100.0,
        author: comment.author.clone(),
        subject: comment.subject.clone(),
    }
}

/// Which swatch the comment's colour is, if it is one of them.
fn chosen_swatch(comment: &ReadAnnotation) -> Option<usize> {
    let rgb = comment.color.map(to_rgb)?;
    PALETTE.iter().position(|(_, swatch)| *swatch == rgb)
}

fn activation(action: InspectorAction) -> Activation {
    Activation::Inspector(action)
}

/// What the inspector tells a screen reader. `refusal` disables every
/// control that writes, with its reason.
pub(in crate::shell) fn accessible(
    comment: &ReadAnnotation,
    state: &InspectorState,
    refusal: Option<&'static str>,
    cx: &gpui::App,
) -> Vec<Element> {
    let enabled = refusal.is_none();
    let with_reason = |element: Element| match refusal {
        Some(reason) => element.with_description(reason),
        None => element,
    };
    let chosen = chosen_swatch(comment);
    let swatches = PALETTE
        .iter()
        .enumerate()
        .map(|(index, (name, _))| {
            with_reason(
                Element::new(("inspector-color", index), Role::RadioButton, *name)
                    .with_state(A11yState {
                        selected: Some(chosen == Some(index)),
                        disabled: !enabled,
                        ..A11yState::default()
                    })
                    .with_activation(activation(InspectorAction::Color(index))),
            )
        })
        .collect();
    let percent = opacity_percent(comment);
    let opacities = OPACITIES
        .iter()
        .enumerate()
        .map(|(index, value)| {
            with_reason(
                Element::new(
                    ("inspector-opacity", index),
                    Role::RadioButton,
                    format!("{value}%"),
                )
                .with_state(A11yState {
                    selected: Some(percent == *value),
                    disabled: !enabled,
                    ..A11yState::default()
                })
                .with_activation(activation(InspectorAction::Opacity(*value))),
            )
        })
        .collect();
    let button = |id: &'static str, label: &'static str, action| {
        with_reason(
            Element::new(id, Role::Button, label)
                .with_state(A11yState::enabled(enabled))
                .with_activation(activation(action)),
        )
    };
    vec![
        Element::new("inspector-colors", Role::RadioGroup, "Color").with_children(swatches),
        Element::new("inspector-opacities", Role::RadioGroup, "Opacity").with_children(opacities),
        state
            .author
            .read(cx)
            .accessible("Author", TextField::InspectorAuthor),
        state
            .subject
            .read(cx)
            .accessible("Subject", TextField::InspectorSubject),
        button(
            "inspector-save",
            "Save Author and Subject",
            InspectorAction::SaveText,
        ),
        button(
            "inspector-make-default",
            "Make Current Properties Default",
            InspectorAction::MakeDefault,
        ),
    ]
}

/// The inspector, drawn.
pub(in crate::shell) fn render(
    comment: &ReadAnnotation,
    state: &InspectorState,
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let enabled = refusal.is_none();
    let chosen = chosen_swatch(comment);
    let mut swatches = div().flex().flex_wrap().gap_1();
    for (index, (_, [red, green, blue])) in PALETTE.iter().enumerate() {
        let hex = (u32::from(*red) << 16) | (u32::from(*green) << 8) | u32::from(*blue);
        swatches = swatches.child(
            control(
                ("inspector-color", index),
                enabled,
                InspectorAction::Color(index),
                cx,
            )
            .w(px(24.0))
            .h(px(24.0))
            .rounded_full()
            .bg(rgb(hex))
            .border_2()
            .border_color(if chosen == Some(index) {
                theme.text
            } else {
                theme.surface
            }),
        );
    }
    let percent = opacity_percent(comment);
    let mut opacities = div().flex().gap_1();
    for (index, value) in OPACITIES.iter().enumerate() {
        opacities = opacities.child(
            control(
                ("inspector-opacity", index),
                enabled,
                InspectorAction::Opacity(*value),
                cx,
            )
            .px_2()
            .py_0p5()
            .rounded_sm()
            .text_xs()
            .when(percent == *value, |button| button.bg(theme.selected))
            .child(format!("{value}%")),
        );
    }
    let field = |label: &'static str, input: &Entity<SearchInput>| {
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.secondary_text)
                    .child(label),
            )
            .child(
                div()
                    .p_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(theme.selected)
                    .child(input.clone()),
            )
    };
    let button = |id: &'static str, label: &'static str, action, cx: &mut Context<ShellFrame>| {
        control(id, enabled, action, cx)
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(theme.hover)
            .text_xs()
            .child(label)
    };
    div()
        .id("inspector")
        .p_3()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_sm().child("Comment Properties"))
        .child(section("Color", swatches, theme))
        .child(section("Opacity", opacities, theme))
        .child(field("Author", &state.author))
        .child(field("Subject", &state.subject))
        .child(button(
            "inspector-save",
            "Save Author and Subject",
            InspectorAction::SaveText,
            cx,
        ))
        .child(button(
            "inspector-make-default",
            "Make Current Properties Default",
            InspectorAction::MakeDefault,
            cx,
        ))
        .when_some(refusal, |panel, reason| {
            panel.child(div().text_xs().text_color(theme.muted_text).child(reason))
        })
        .into_any_element()
}

fn section(label: &'static str, body: impl IntoElement, theme: ThemeTokens) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(theme.secondary_text)
                .child(label),
        )
        .child(body)
}

/// A clickable control that runs `action`, or an inert one when the
/// document may not be edited.
fn control(
    id: impl Into<gpui::ElementId>,
    enabled: bool,
    action: InspectorAction,
    cx: &mut Context<ShellFrame>,
) -> gpui::Stateful<gpui::Div> {
    let control = div().id(id);
    if !enabled {
        return control.opacity(0.5);
    }
    control
        .cursor_pointer()
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(activation(action), window, cx);
        }))
}

#[cfg(test)]
mod tests {
    use onionskin_core::{Flags, ObjRef, Rect, Subtype};

    use super::*;

    fn comment(color: Option<Color>, opacity: Option<f64>) -> ReadAnnotation {
        ReadAnnotation {
            objref: ObjRef::new(3, 0),
            page: 0,
            subtype: Some(Subtype::Square),
            raw_subtype: "Square".into(),
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            quads: Vec::new(),
            contents: None,
            author: Some("Zoe".into()),
            modified: None,
            color,
            flags: Flags(4),
            in_reply_to: None,
            has_appearance: true,
            ink: Vec::new(),
            border_width: 1.0,
            subject: None,
            state: None,
            opacity,
        }
    }

    #[test]
    fn a_palette_colour_round_trips_through_the_file_numbers() {
        for (_, swatch) in PALETTE {
            assert_eq!(to_rgb(from_rgb(swatch)), swatch);
        }
        let red = comment(Some(from_rgb(PALETTE[0].1)), None);
        assert_eq!(chosen_swatch(&red), Some(0));
        assert_eq!(
            chosen_swatch(&comment(Some(Color::new(0.1, 0.2, 0.3)), None)),
            None
        );
    }

    #[test]
    fn the_current_properties_are_what_the_comment_carries() {
        let faded = comment(Some(Color::BLACK), Some(0.5));
        assert_eq!(opacity_percent(&faded), 50);
        assert_eq!(opacity_percent(&comment(None, None)), 100);
        let properties = current(&faded);
        assert_eq!(properties.opacity, 0.5);
        assert_eq!(properties.author.as_deref(), Some("Zoe"));
        assert_eq!(properties.color, Some(Color::BLACK));
    }
}
