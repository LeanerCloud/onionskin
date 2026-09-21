use accesskit::Role;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    StatefulInteractiveElement as _, Styled as _,
};

use super::accessible::{Activation, Element};
use super::tabs::ShellFrame;
use super::theme::ThemeTokens;
use crate::a11y::State as A11yState;

pub(super) const CLOSED_WIDTH: f32 = 40.0;
pub(super) const OPEN_WIDTH: f32 = 280.0;
const PANEL_LABEL: &str = "Tool details";
const EMPTY_PANEL_MESSAGE: &str = "Choose a tool for details";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum SidePanelState {
    Closed,
    #[default]
    OpenEmpty,
}

impl SidePanelState {
    pub(super) fn is_open(self) -> bool {
        self == Self::OpenEmpty
    }

    pub(super) fn toggle(&mut self) {
        *self = if self.is_open() {
            Self::Closed
        } else {
            Self::OpenEmpty
        };
    }

    pub(super) fn width(self) -> Pixels {
        px(if self.is_open() {
            OPEN_WIDTH
        } else {
            CLOSED_WIDTH
        })
    }
}

/// The chevron the toggle draws. A screen reader reading it says "single
/// right-pointing angle quotation mark", so [`toggle_name`] is what it
/// announces instead.
fn toggle_glyph(state: SidePanelState) -> &'static str {
    if state.is_open() {
        "›"
    } else {
        "‹"
    }
}

fn toggle_name(state: SidePanelState) -> &'static str {
    if state.is_open() {
        "Close Side Panel"
    } else {
        "Open Side Panel"
    }
}

/// The active tool's name and how it is used, when a tool is active.
pub(super) type ToolHelp = Option<(&'static str, Option<&'static str>)>;

/// What the side panel tells a screen reader.
pub(super) fn accessible(
    state: SidePanelState,
    help: ToolHelp,
    inspector: Option<Vec<Element>>,
) -> Element {
    let toggle = Element::new("side-panel-toggle", Role::Button, toggle_name(state))
        .with_state(A11yState::toggled(state.is_open()))
        .with_activation(Activation::ToggleSidePanel);
    let panel = Element::new("side-panel", Role::Complementary, PANEL_LABEL);
    if state.is_open() {
        if let Some(inspector) = inspector {
            return panel
                .child(
                    Element::new("inspector", Role::Group, "Comment Properties")
                        .with_children(inspector),
                )
                .child(toggle);
        }
        let body = match help {
            Some((name, hint)) => {
                let tool = Element::new("side-panel-tool", Role::Heading, name);
                match hint {
                    Some(hint) => tool.with_description(hint),
                    None => tool,
                }
            }
            None => Element::new("side-panel-empty", Role::Label, EMPTY_PANEL_MESSAGE),
        };
        panel.child(body).child(toggle)
    } else {
        panel.child(toggle)
    }
}

pub(super) fn render_side_panel(
    state: SidePanelState,
    help: ToolHelp,
    inspector: Option<gpui::AnyElement>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let toggle = div()
        .id("side-panel-toggle")
        .w(px(32.0))
        .h(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |button| button.bg(theme.hover))
        .on_click(cx.listener(|frame, _event, window, cx| {
            frame.run_activation(Activation::ToggleSidePanel, window, cx);
        }))
        .child(toggle_glyph(state));
    let mut panel = div()
        .id("side-panel")
        .w(state.width())
        .h_full()
        .flex_none()
        .bg(theme.surface)
        .text_color(theme.text);

    if state.is_open() {
        panel = panel.child(
            div()
                .h(px(48.0))
                .flex()
                .items_center()
                .justify_between()
                .px_2()
                .child(PANEL_LABEL)
                .child(toggle),
        );
        if let Some(inspector) = inspector {
            // A chosen comment's properties are what the panel is for while
            // one is chosen, as Acrobat's properties bar is.
            return panel.child(inspector);
        }
        panel = panel.child(match help {
            // The tool the canvas is in and what to do with it: every rail
            // icon is a glyph, and without this nothing on screen says what a
            // drag or a click will do.
            Some((name, hint)) => div()
                .id("side-panel-tool")
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_sm().child(name))
                .children(
                    hint.map(|hint| div().text_xs().text_color(theme.muted_text).child(hint)),
                ),
            None => div()
                .id("side-panel-empty")
                .p_3()
                .text_sm()
                .text_color(theme.muted_text)
                .child(EMPTY_PANEL_MESSAGE),
        });
    } else {
        panel = panel
            .flex()
            .items_start()
            .justify_center()
            .pt_2()
            .child(toggle);
    }

    panel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_active_tool_and_its_hint_are_described_in_place_of_the_empty_message() {
        let panel = accessible(
            SidePanelState::OpenEmpty,
            Some(("Draw", Some("Drag on the page to draw freehand."))),
            None,
        );
        let tool = panel.find(&"side-panel-tool".into()).expect("described");
        assert_eq!(tool.label, "Draw");
        assert_eq!(
            tool.description.as_deref(),
            Some("Drag on the page to draw freehand.")
        );
        assert!(panel.find(&"side-panel-empty".into()).is_none());
    }

    #[test]
    fn the_empty_host_opens_by_default_and_toggles_without_content() {
        let mut state = SidePanelState::default();

        assert_eq!(state, SidePanelState::OpenEmpty);
        assert_eq!(state.width(), px(OPEN_WIDTH));

        state.toggle();
        assert_eq!(state, SidePanelState::Closed);
        assert_eq!(state.width(), px(CLOSED_WIDTH));

        state.toggle();
        assert_eq!(state, SidePanelState::OpenEmpty);
    }

    /// The toggle is a chevron on screen. A screen reader reading the chevron
    /// says "angle quotation mark", so the name it announces has to be words.
    #[test]
    fn the_toggle_is_announced_by_words_rather_than_by_the_chevron_it_draws() {
        for state in [SidePanelState::OpenEmpty, SidePanelState::Closed] {
            let toggle = accessible(state, None, None)
                .find(&"side-panel-toggle".into())
                .expect("the panel describes its toggle")
                .clone();

            assert!(toggle.label.chars().all(|c| c.is_alphabetic() || c == ' '));
            assert!(!toggle.label.contains(toggle_glyph(state)));
        }

        assert_eq!(
            accessible(SidePanelState::OpenEmpty, None, None)
                .find(&"side-panel-toggle".into())
                .unwrap()
                .label,
            "Close Side Panel"
        );
        assert_eq!(
            accessible(SidePanelState::Closed, None, None)
                .find(&"side-panel-toggle".into())
                .unwrap()
                .label,
            "Open Side Panel"
        );
    }

    #[test]
    fn the_toggle_carries_whether_the_panel_is_open_as_state_and_the_action_its_click_runs() {
        let open = accessible(SidePanelState::OpenEmpty, None, None);
        let closed = accessible(SidePanelState::Closed, None, None);

        let open = open.find(&"side-panel-toggle".into()).unwrap();
        let closed = closed.find(&"side-panel-toggle".into()).unwrap();
        assert_eq!(open.state.toggled, Some(true));
        assert_eq!(closed.state.toggled, Some(false));
        assert_eq!(open.activation, Some(Activation::ToggleSidePanel));
        assert_eq!(closed.activation, Some(Activation::ToggleSidePanel));
    }

    #[test]
    fn the_panel_describes_the_empty_state_and_its_control() {
        let described = accessible(SidePanelState::OpenEmpty, None, None);

        assert_eq!(described.role, Role::Complementary);
        assert_eq!(described.label, PANEL_LABEL);
        assert_eq!(described.children.len(), 2);
        assert_eq!(described.children[0].role, Role::Label);
        assert_eq!(described.children[0].label, EMPTY_PANEL_MESSAGE);
        assert_eq!(described.children[1].role, Role::Button);

        let closed = accessible(SidePanelState::Closed, None, None);
        assert_eq!(closed.children.len(), 1);
        assert_eq!(closed.children[0].role, Role::Button);
    }
}
