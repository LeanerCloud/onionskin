use accesskit::Role;
use gpui::{
    div, prelude::FluentBuilder as _, px, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, StatefulInteractiveElement as _, Styled as _,
};

use super::accessible::{Activation, Element};
use onionskin_plugin_api::Reading;

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

/// The active tool: its name, how it is used, what it is reading off the
/// page, and its settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::shell) struct ToolHelp {
    pub(in crate::shell) name: &'static str,
    pub(in crate::shell) hint: Option<&'static str>,
    pub(in crate::shell) readings: Vec<Reading>,
    pub(in crate::shell) settings: Vec<ToolSetting>,
}

/// One of the active tool's settings, and whether it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct ToolSetting {
    pub(in crate::shell) id: String,
    pub(in crate::shell) label: String,
    pub(in crate::shell) category: String,
    pub(in crate::shell) on: bool,
}

/// What the readings are called, as Acrobat calls its measuring tools'.
const READINGS_LABEL: &str = "Measurement Info";

/// The settings, one group to a category, in the order the tool lists them.
fn by_category(settings: &[ToolSetting]) -> Vec<(&str, Vec<&ToolSetting>)> {
    let mut groups: Vec<(&str, Vec<&ToolSetting>)> = Vec::new();
    for setting in settings {
        match groups
            .iter_mut()
            .find(|(category, _)| *category == setting.category)
        {
            Some((_, members)) => members.push(setting),
            None => groups.push((&setting.category, vec![setting])),
        }
    }
    groups
}

fn setting_id(setting: &ToolSetting) -> SharedString {
    SharedString::from(format!("tool-setting-{}", setting.id))
}

/// The tool's name and hint, readings and settings, to a screen reader.
fn accessible_tool(help: ToolHelp) -> Vec<Element> {
    let tool = Element::new("side-panel-tool", Role::Heading, help.name);
    let mut nodes = vec![match help.hint {
        Some(hint) => tool.with_description(hint),
        None => tool,
    }];
    if !help.readings.is_empty() {
        nodes.push(
            Element::new("side-panel-readings", Role::Group, READINGS_LABEL).with_children(
                help.readings
                    .iter()
                    .enumerate()
                    .map(|(index, reading)| {
                        Element::new(
                            SharedString::from(format!("side-panel-reading-{index}")),
                            Role::Label,
                            format!("{}: {}", reading.label, reading.value),
                        )
                    })
                    .collect(),
            ),
        );
    }
    for (category, members) in by_category(&help.settings) {
        nodes.push(
            Element::new(
                SharedString::from(format!("tool-settings-{category}")),
                Role::Group,
                category,
            )
            .with_children(
                members
                    .into_iter()
                    .map(|setting| {
                        Element::new(setting_id(setting), Role::CheckBox, &setting.label)
                            .with_state(A11yState::toggled(setting.on))
                            .with_activation(Activation::ToolSetting(setting.id.clone()))
                    })
                    .collect(),
            ),
        );
    }
    nodes
}

/// What the side panel tells a screen reader.
pub(super) fn accessible(
    state: SidePanelState,
    help: Option<ToolHelp>,
    content: Option<Element>,
) -> Element {
    let toggle = Element::new("side-panel-toggle", Role::Button, toggle_name(state))
        .with_state(A11yState::toggled(state.is_open()))
        .with_activation(Activation::ToggleSidePanel);
    let panel = Element::new("side-panel", Role::Complementary, PANEL_LABEL);
    if state.is_open() {
        // What the panel is showing instead of the tool's help: the skins,
        // or a chosen comment's properties.
        if let Some(content) = content {
            return panel.child(content).child(toggle);
        }
        let body = match help {
            Some(help) => accessible_tool(help),
            None => vec![Element::new(
                "side-panel-empty",
                Role::Label,
                EMPTY_PANEL_MESSAGE,
            )],
        };
        panel.with_children(body).child(toggle)
    } else {
        panel.child(toggle)
    }
}

pub(super) fn render_side_panel(
    state: SidePanelState,
    help: Option<ToolHelp>,
    content: Option<gpui::AnyElement>,
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
        if let Some(content) = content {
            // The skins, or a chosen comment's properties, which are what
            // the panel is for while one is chosen, as Acrobat's properties
            // bar is.
            return panel.child(content);
        }
        panel = panel.child(match help {
            // The tool the canvas is in and what to do with it: every rail
            // icon is a glyph, and without this nothing on screen says what a
            // drag or a click will do.
            Some(help) => render_tool(help, theme, cx),
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

/// The tool's name and hint, then its readings and its settings.
fn render_tool(
    help: ToolHelp,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Stateful<gpui::Div> {
    let mut tool = div()
        .id("side-panel-tool")
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_sm().child(help.name))
        .children(
            help.hint
                .map(|hint| div().text_xs().text_color(theme.muted_text).child(hint)),
        );
    if !help.readings.is_empty() {
        let mut readings = div()
            .flex()
            .flex_col()
            .gap_1()
            .text_xs()
            .child(div().text_color(theme.muted_text).child(READINGS_LABEL));
        for reading in help.readings {
            readings = readings.child(
                div()
                    .flex()
                    .justify_between()
                    .child(div().text_color(theme.muted_text).child(reading.label))
                    .child(reading.value),
            );
        }
        tool = tool.child(readings);
    }
    for (category, members) in by_category(&help.settings) {
        let mut group = div().flex().flex_col().text_xs().child(
            div()
                .text_color(theme.muted_text)
                .child(category.to_owned()),
        );
        for setting in members {
            let id = setting.id.clone();
            group = group.child(
                div()
                    .id(setting_id(setting))
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(setting.on, |row| row.bg(theme.selected))
                    .hover(move |row| row.bg(theme.hover))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(Activation::ToolSetting(id.clone()), window, cx);
                    }))
                    .child(format!(
                        "{} {}",
                        if setting.on { "✓" } else { " " },
                        setting.label
                    )),
            );
        }
        tool = tool.child(group);
    }
    tool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_active_tool_and_its_hint_are_described_in_place_of_the_empty_message() {
        let panel = accessible(
            SidePanelState::OpenEmpty,
            Some(ToolHelp {
                name: "Draw",
                hint: Some("Drag on the page to draw freehand."),
                ..ToolHelp::default()
            }),
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
    fn a_tools_readings_and_settings_are_listed_under_it() {
        let setting = |id: &str, category: &str, on: bool| ToolSetting {
            id: id.to_owned(),
            label: id.to_uppercase(),
            category: category.to_owned(),
            on,
        };
        let panel = accessible(
            SidePanelState::OpenEmpty,
            Some(ToolHelp {
                name: "Distance",
                hint: None,
                readings: vec![Reading::new("Distance", "3.00 in")],
                settings: vec![
                    setting("a", "Scale", true),
                    setting("b", "Snap to", false),
                    setting("c", "Scale", false),
                ],
            }),
            None,
        );
        let readings = panel.find(&"side-panel-readings".into()).expect("read");
        assert_eq!(readings.label, READINGS_LABEL);
        assert_eq!(readings.children[0].label, "Distance: 3.00 in");
        let scale = panel.find(&"tool-settings-Scale".into()).expect("grouped");
        assert_eq!(scale.children.len(), 2, "a category's settings together");
        let on = panel.find(&"tool-setting-a".into()).expect("listed");
        assert_eq!(on.role, Role::CheckBox);
        assert_eq!(on.state.toggled, Some(true));
        assert_eq!(on.activation, Some(Activation::ToolSetting("a".to_owned())));
        let off = panel.find(&"tool-setting-b".into()).expect("listed");
        assert_eq!(off.state.toggled, Some(false));
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
