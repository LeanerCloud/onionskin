//! The layers pane: optional content groups, and showing or hiding one.
//!
//! A toggle is a render-options change, so it costs a re-render of every
//! page on screen and of every thumbnail already drawn. Both are dropped
//! here, together, because a cached picture produced with the old
//! visibility is wrong in exactly the way the user was trying to change.
//!
//! A group the document locked is drawn with its control disabled and its
//! reason beside it. The renderer would honour an override on a locked
//! group, so the pane not offering one is the only thing that keeps the
//! file's own statement true.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Point, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::Layer;

use super::super::canvas::CanvasError;
use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::{
    empty_message, error_message, list, menu_row, LayerAction, NavigationPanesState, PaneAction,
    ROW_HEIGHT,
};

/// Parity row 203's menu, counted once for the whole menu. Visibility and the
/// default-state command are the pane's own and live; properties needs a
/// dialog that arrives with M3, and merge and flatten are layer editing,
/// which row 204 puts after 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum LayersCommand {
    Properties,
    ShowAll,
    HideAll,
    ResetVisibility,
    Merge,
    Flatten,
}

impl LayersCommand {
    pub(in crate::shell) const ALL: [Self; 6] = [
        Self::Properties,
        Self::ShowAll,
        Self::HideAll,
        Self::ResetVisibility,
        Self::Merge,
        Self::Flatten,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Properties => "Layer Properties",
            Self::ShowAll => "Show All Layers",
            Self::HideAll => "Hide All Layers",
            Self::ResetVisibility => "Reset To Initial Visibility",
            Self::Merge => "Merge Layers",
            Self::Flatten => "Flatten Layers",
        }
    }

    pub(in crate::shell) fn availability(self) -> MenuAvailability {
        match self {
            Self::ShowAll | Self::HideAll | Self::ResetVisibility => MenuAvailability::Enabled,
            Self::Properties => {
                MenuAvailability::Disabled("Available in M3 with the properties dialog")
            }
            Self::Merge | Self::Flatten => {
                MenuAvailability::Disabled("Available after 1.0 with layer editing")
            }
        }
    }
}

/// Whether this group's control does anything, and what it says when it does
/// not. A query about the document rather than a milestone: `/D /Locked` is
/// the file saying the user may not change it.
pub(super) fn availability(layer: &Layer) -> MenuAvailability {
    if layer.locked {
        MenuAvailability::Disabled("The document locks this layer's visibility")
    } else {
        MenuAvailability::Enabled
    }
}

pub(super) fn run(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    action: LayerAction,
    cx: &mut Context<ShellFrame>,
) {
    match action {
        LayerAction::OpenMenu(at) => {
            state.layers_menu = Some(at);
        }
        LayerAction::SetVisible { layer, visible } => {
            apply_to_canvas(state, canvas, cx, move |canvas| {
                canvas.model.set_layer_visible(layer, visible)
            });
        }
        LayerAction::Run(command) => {
            state.layers_menu = None;
            let layers = state.layers().unwrap_or_default().to_vec();
            match command {
                LayersCommand::ShowAll | LayersCommand::HideAll => {
                    let visible = command == LayersCommand::ShowAll;
                    apply_to_canvas(state, canvas, cx, move |canvas| {
                        let mut changed = false;
                        for layer in layers.iter().filter(|layer| !layer.locked) {
                            changed |= canvas.model.set_layer_visible(layer.id, visible)?;
                        }
                        Ok(changed)
                    });
                }
                LayersCommand::ResetVisibility => {
                    apply_to_canvas(state, canvas, cx, |canvas| {
                        canvas.model.reset_layer_visibility()
                    });
                }
                // Every other entry is disabled, so nothing can raise it.
                LayersCommand::Properties | LayersCommand::Merge | LayersCommand::Flatten => {}
            }
        }
    }
}

/// Run a visibility change, then put the pane and every cached picture back
/// in step with it.
///
/// The re-render the canvas schedules covers the pages on screen. The
/// thumbnails are pictures of the same pages under the same options and are
/// nobody else's to invalidate, so they go here: a toggle that redrew the
/// document and left the thumbnails showing the old layers would be showing
/// two answers to one question.
fn apply_to_canvas(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
    change: impl FnOnce(&mut Canvas) -> Result<bool, CanvasError>,
) {
    let Some(canvas) = canvas else {
        return;
    };
    let outcome = canvas.update(cx, |canvas, cx| match change(canvas) {
        // The same route every other view change takes: it repaints and
        // re-requests the pages the new options invalidated.
        Ok(changed) => {
            canvas.handle_change(Ok(changed), cx);
            Ok(changed)
        }
        Err(error) => Err(error.to_string()),
    });
    match outcome {
        Ok(false) => {}
        Ok(true) => {
            state.invalidate_thumbnails();
            state.reread(canvas, cx);
            state.feedback = None;
        }
        Err(message) => state.feedback = Some(message),
    }
}

pub(super) fn render(
    items: Result<&[Layer], &String>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let items = match items {
        Ok(items) => items,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    if items.is_empty() {
        return empty_message("This document has no layers.", theme).into_any_element();
    }

    let mut body = list("layer-rows").on_mouse_down(
        MouseButton::Right,
        cx.listener(|frame, event: &gpui::MouseDownEvent, _window, cx| {
            frame.run_pane_action(PaneAction::Layer(LayerAction::OpenMenu(event.position)), cx);
        }),
    );
    for (index, layer) in items.iter().enumerate() {
        let availability = availability(layer);
        let enabled = availability.is_enabled();
        let id = layer.id;
        let visible = layer.visible;
        let mut row = div()
            .id(("layer-row", index))
            .min_h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .text_sm()
            .text_color(if enabled {
                theme.text
            } else {
                theme.disabled_text
            })
            .child(if visible { "☑" } else { "☐" })
            .child(if layer.name.is_empty() {
                "(unnamed layer)".to_owned()
            } else {
                layer.name.clone()
            });
        if enabled {
            row = row
                .cursor_pointer()
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    frame.run_pane_action(
                        PaneAction::Layer(LayerAction::SetVisible {
                            layer: id,
                            visible: !visible,
                        }),
                        cx,
                    );
                }));
        } else {
            row = row.when_some(availability.reason(), |row, reason| {
                row.child(div().text_xs().text_color(theme.muted_text).child(reason))
            });
        }
        body = body.child(row);
    }
    body.into_any_element()
}

pub(super) fn render_menu(
    at: Point<Pixels>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut menu = div()
        .id("layers-context-menu")
        .absolute()
        .top(at.y)
        .left(px(4.0))
        .w(px(228.0))
        .p_1()
        .rounded_md()
        .occlude()
        .bg(theme.raised)
        .text_color(theme.text);
    for (index, command) in LayersCommand::ALL.into_iter().enumerate() {
        menu = menu.child(menu_row(
            "layers-menu-entry",
            index,
            command.label(),
            command.availability(),
            theme,
            cx,
            move |frame, cx| {
                frame.run_pane_action(PaneAction::Layer(LayerAction::Run(command)), cx);
            },
        ));
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_core::ObjRef;

    fn layer(name: &str, visible: bool, locked: bool) -> Layer {
        Layer {
            id: ObjRef::new(1, 0),
            name: name.to_owned(),
            visible,
            locked,
        }
    }

    /// A locked group is disabled with the file's own reason. The renderer
    /// would honour an override on it, so nothing but this stops one.
    #[test]
    fn a_locked_layer_is_disabled_with_a_reason_and_an_unlocked_one_is_not() {
        let locked = availability(&layer("Watermark", true, true));

        assert!(!locked.is_enabled());
        assert_eq!(
            locked.reason(),
            Some("The document locks this layer's visibility")
        );
        assert!(availability(&layer("Background", true, false)).is_enabled());
    }

    /// Parity row 203 counts the menu once and names properties, visibility
    /// and default-state commands. Every entry is present; the visibility
    /// ones are live because they need nothing but the renderer, and the
    /// editing ones say which milestone they wait on.
    #[test]
    fn the_menu_ships_whole_with_the_visibility_commands_live() {
        assert_eq!(LayersCommand::ALL.len(), 6);

        for command in [
            LayersCommand::ShowAll,
            LayersCommand::HideAll,
            LayersCommand::ResetVisibility,
        ] {
            assert!(
                command.availability().is_enabled(),
                "{} needs only the renderer",
                command.label()
            );
        }

        for (command, milestone) in [
            (LayersCommand::Properties, "M3"),
            (LayersCommand::Merge, "1.0"),
            (LayersCommand::Flatten, "1.0"),
        ] {
            let availability = command.availability();
            assert!(!availability.is_enabled(), "{}", command.label());
            let reason = availability.reason().expect("a disabled entry says why");
            assert!(
                reason.contains(milestone),
                "{} should name {milestone}, said {reason:?}",
                command.label()
            );
        }
    }

    #[test]
    fn every_entry_has_a_label_and_every_disabled_one_has_a_reason() {
        for command in LayersCommand::ALL {
            assert!(!command.label().is_empty());
            let availability = command.availability();
            assert_eq!(availability.is_enabled(), availability.reason().is_none());
        }
    }
}
