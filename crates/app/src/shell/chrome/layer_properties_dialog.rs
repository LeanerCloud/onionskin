//! Layer Properties: choose a layer, then its name, its intent and whether
//! it is on when the document opens, as Acrobat's dialog of the same name
//! sets them. Apply writes them to the document as one undoable step.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement as _,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{Layer, LayerIntent, LayerProperties};

use super::accessible::{Activation, Element, TextField};
use super::combine_dialog::button;
use super::{SearchInput, ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum LayerPropertiesAction {
    Choose(usize),
    Intent(LayerIntent),
    DefaultOn(bool),
    Apply,
}

pub(in crate::shell) struct LayerPropertiesState {
    pub(in crate::shell) layers: Vec<Layer>,
    pub(in crate::shell) chosen: usize,
    pub(in crate::shell) name: Entity<SearchInput>,
    pub(in crate::shell) intent: LayerIntent,
    pub(in crate::shell) default_on: bool,
    /// Why Apply is off: the document may not be edited.
    pub(in crate::shell) refusal: Option<&'static str>,
    pub(in crate::shell) error: Option<String>,
}

/// Said when there is nothing to set.
pub(in crate::shell) const NO_LAYERS: &str = "This document has no layers.";

impl LayerPropertiesState {
    /// The dialog on the first of `layers`, read at the file's defaults.
    pub(in crate::shell) fn new(
        layers: Vec<Layer>,
        refusal: Option<&'static str>,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let name =
            cx.new(|cx| SearchInput::with_placeholder("layer-name", "Layer name", theme, cx));
        let mut state = Self {
            layers,
            chosen: 0,
            name,
            intent: LayerIntent::View,
            default_on: true,
            refusal,
            error: None,
        };
        state.choose(0, cx);
        state
    }

    /// Show the layer at `index`'s properties.
    pub(in crate::shell) fn choose(&mut self, index: usize, cx: &mut Context<ShellFrame>) {
        let Some(layer) = self.layers.get(index) else {
            return;
        };
        self.chosen = index;
        self.intent = layer.intent;
        self.default_on = layer.visible;
        self.error = None;
        let name = layer.name.clone();
        self.name.update(cx, |input, cx| input.set_query(name, cx));
    }

    /// The properties as set, or why they cannot be applied.
    pub(in crate::shell) fn properties(&self, cx: &gpui::App) -> Result<LayerProperties, String> {
        let name = self.name.read(cx).query().trim().to_owned();
        if name.is_empty() {
            return Err("A layer needs a name".to_owned());
        }
        Ok(LayerProperties {
            name,
            intent: self.intent,
            default_on: self.default_on,
        })
    }

    pub(in crate::shell) fn chosen_layer(&self) -> Option<&Layer> {
        self.layers.get(self.chosen)
    }
}

/// A layer's name as the chooser shows it.
fn layer_label(layer: &Layer) -> String {
    if layer.name.is_empty() {
        "(unnamed layer)".to_owned()
    } else {
        layer.name.clone()
    }
}

/// Each choice row: its id, label, whether it is in force, and its action.
fn choices(
    state: &LayerPropertiesState,
) -> Vec<(gpui::ElementId, String, bool, LayerPropertiesAction)> {
    let mut rows: Vec<_> = state
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| {
            (
                gpui::ElementId::from(("layer-choice", index)),
                layer_label(layer),
                index == state.chosen,
                LayerPropertiesAction::Choose(index),
            )
        })
        .collect();
    for intent in [LayerIntent::View, LayerIntent::Design] {
        rows.push((
            gpui::ElementId::from(("layer-intent", intent as usize)),
            format!("Intent: {}", intent.label()),
            state.intent == intent,
            LayerPropertiesAction::Intent(intent),
        ));
    }
    for on in [true, false] {
        rows.push((
            gpui::ElementId::from(("layer-default", usize::from(on))),
            format!("Default state: {}", if on { "On" } else { "Off" }),
            state.default_on == on,
            LayerPropertiesAction::DefaultOn(on),
        ));
    }
    rows
}

pub(in crate::shell) fn accessible(state: &LayerPropertiesState, cx: &gpui::App) -> Vec<Element> {
    if state.layers.is_empty() {
        return vec![Element::new(
            "layer-properties-empty",
            Role::Label,
            NO_LAYERS,
        )];
    }
    let mut body: Vec<Element> = choices(state)
        .into_iter()
        .map(|(id, label, selected, action)| {
            Element::new(id, Role::RadioButton, label)
                .with_state(A11yState::selected(selected))
                .with_activation(Activation::LayerProperties(action))
        })
        .collect();
    body.insert(
        state.layers.len(),
        state
            .name
            .read(cx)
            .accessible("Layer name", TextField::LayerName),
    );
    if let Some(error) = state.error.as_deref().or(state.refusal) {
        body.push(Element::new("layer-properties-error", Role::Alert, error));
    }
    let mut apply = Element::new("layer-properties-apply", Role::Button, "Apply");
    if state.refusal.is_none() {
        apply = apply.with_activation(Activation::LayerProperties(LayerPropertiesAction::Apply));
    }
    body.push(apply);
    body
}

pub(in crate::shell) fn render(
    state: &LayerPropertiesState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    if state.layers.is_empty() {
        return div().child(NO_LAYERS).into_any_element();
    }
    let mut body = div().flex().flex_col().gap_1();
    for (position, (id, label, selected, action)) in choices(state).into_iter().enumerate() {
        if position == state.layers.len() {
            body = body.child(div().py_1().child(state.name.clone()));
        }
        body = body.child(
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .when(selected, |row| row.bg(theme.selected))
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::LayerProperties(action), window, cx);
                }))
                .child(label),
        );
    }
    if let Some(error) = state.error.as_deref().or(state.refusal) {
        body = body.child(
            div()
                .id("layer-properties-error")
                .text_color(theme.error_text)
                .child(error.to_owned()),
        );
    }
    body.child(button(
        "layer-properties-apply",
        "Apply",
        state.refusal.is_none(),
        theme,
        focused,
        cx,
        Activation::LayerProperties(LayerPropertiesAction::Apply),
    ))
    .into_any_element()
}
