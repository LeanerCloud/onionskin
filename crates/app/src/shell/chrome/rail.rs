use std::collections::BTreeMap;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_plugin_api::{PluginRegistry, ToolPlugin};

use super::accessible::{Activation, Element, Rects, Surface};
use super::tabs::ShellFrame;
use super::theme::ThemeTokens;
use crate::a11y::State as A11yState;

const COLLAPSED_WIDTH: f32 = 88.0;
const EXPANDED_WIDTH: f32 = 240.0;

pub(super) fn rail_width(expanded: bool) -> gpui::Pixels {
    px(if expanded {
        EXPANDED_WIDTH
    } else {
        COLLAPSED_WIDTH
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct RailEntry {
    pub(super) registry_index: usize,
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    pub(super) icon: &'static str,
    pub(super) shortcut: Option<&'static str>,
    pub(super) group: &'static str,
    pub(super) active: bool,
}

impl RailEntry {
    fn from_tool(registry_index: usize, tool: &dyn ToolPlugin, active_tool: Option<usize>) -> Self {
        Self {
            registry_index,
            id: tool.id(),
            name: tool.name(),
            icon: tool.icon(),
            shortcut: tool.shortcut(),
            group: tool.group(),
            active: active_tool == Some(registry_index),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct RailState {
    expanded: bool,
    remembered_by_group: BTreeMap<&'static str, &'static str>,
}

impl RailState {
    pub(super) fn expanded(&self) -> bool {
        self.expanded
    }

    pub(super) fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }

    pub(super) fn entries(
        &self,
        registry: &PluginRegistry,
        active_tool: Option<usize>,
    ) -> Vec<RailEntry> {
        let entries = registry
            .tools()
            .enumerate()
            .filter(|(_, tool)| tool.in_rail())
            .map(|(registry_index, tool)| RailEntry::from_tool(registry_index, tool, active_tool))
            .collect::<Vec<_>>();
        if self.expanded {
            return entries;
        }

        let mut groups: Vec<(&'static str, Vec<RailEntry>)> = Vec::new();
        for entry in entries {
            if let Some((_, members)) = groups.iter_mut().find(|(group, _)| *group == entry.group) {
                members.push(entry);
            } else {
                groups.push((entry.group, vec![entry]));
            }
        }

        groups
            .into_iter()
            .map(|(group, members)| {
                members
                    .iter()
                    .find(|entry| entry.active)
                    .or_else(|| {
                        self.remembered_by_group.get(group).and_then(|remembered| {
                            members.iter().find(|entry| entry.id == *remembered)
                        })
                    })
                    .or_else(|| members.first())
                    .copied()
                    .expect("rail groups contain at least one tool")
            })
            .collect()
    }

    pub(super) fn entry_for_index(
        &self,
        registry: &PluginRegistry,
        active_tool: Option<usize>,
        index: usize,
    ) -> Option<RailEntry> {
        let tool = registry.tools().nth(index)?;
        tool.in_rail()
            .then(|| RailEntry::from_tool(index, tool, active_tool))
    }

    pub(super) fn remember(&mut self, entry: RailEntry) {
        self.remembered_by_group.insert(entry.group, entry.id);
    }
}

pub(super) fn apply_rail_selection<E>(
    state: &mut RailState,
    entry: RailEntry,
    activate: impl FnOnce(usize) -> Result<bool, E>,
) -> Result<bool, E> {
    let changed = activate(entry.registry_index)?;
    state.remember(entry);
    Ok(changed)
}

fn empty_label(expanded: bool) -> &'static str {
    if expanded {
        "No tools installed"
    } else {
        "No tools"
    }
}

fn view_more_label(expanded: bool) -> &'static str {
    if expanded {
        "Show less"
    } else {
        "View more"
    }
}

fn icon_glyph(icon: &str) -> &'static str {
    match icon {
        "hand" => "✋",
        "select-text" => "T",
        "select-region" => "□",
        "zoom" => "⌕",
        "dynamic-zoom" => "⇕",
        "snapshot" => "▣",
        _ => "?",
    }
}

/// What the tool rail tells a screen reader.
///
/// One node per child the column renders, in the same order, so the
/// rectangles the column reports after prepaint land on the right nodes.
pub(super) fn accessible(entries: &[RailEntry], expanded: bool) -> Element {
    let mut rail = Element::new("tool-rail", Role::Toolbar, "Tools");

    if entries.is_empty() {
        rail = rail.child(Element::new(
            "tool-rail-empty",
            Role::Label,
            empty_label(expanded),
        ));
    } else {
        for entry in entries {
            let mut element = Element::new(
                ("tool-rail-entry", entry.registry_index),
                Role::Button,
                entry.name,
            )
            .with_state(A11yState::selected(entry.active))
            .with_activation(Activation::Rail(*entry));
            if let Some(shortcut) = entry.shortcut {
                element = element.with_description(format!("Shortcut {shortcut}"));
            }
            rail = rail.child(element);
        }
    }

    rail.child(
        Element::new(
            "tool-rail-view-more",
            Role::Button,
            view_more_label(expanded),
        )
        .with_state(A11yState::toggled(expanded))
        .with_activation(Activation::ToggleRailExpanded),
    )
}

pub(super) fn render_rail(
    entries: Vec<RailEntry>,
    expanded: bool,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut rail = div()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::Rail, &bounds, window);
        })
        .w(rail_width(expanded))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .bg(theme.surface)
        .text_color(theme.text);

    if entries.is_empty() {
        rail = rail.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(theme.muted_text)
                .child(empty_label(expanded)),
        );
    } else {
        for entry in entries {
            rail = rail.child(
                div()
                    .id(("tool-rail-entry", entry.registry_index))
                    .min_h(px(36.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(if entry.active {
                        theme.selected
                    } else {
                        theme.raised
                    })
                    .hover(move |row| row.bg(theme.hover))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(Activation::Rail(entry), window, cx);
                    }))
                    .child(
                        div()
                            .w(px(24.0))
                            .flex_none()
                            .text_xs()
                            .text_color(theme.secondary_text)
                            .child(icon_glyph(entry.icon)),
                    )
                    .when(expanded, |row| {
                        row.child(div().flex_1().text_sm().child(entry.name))
                            .when_some(entry.shortcut, |row, shortcut| {
                                row.child(
                                    div().text_xs().text_color(theme.muted_text).child(shortcut),
                                )
                            })
                    }),
            );
        }
    }

    rail.child(
        div()
            .id("tool-rail-view-more")
            .min_h(px(34.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .bg(theme.raised)
            .hover(move |button| button.bg(theme.selected))
            .on_click(cx.listener(|frame, _event, window, cx| {
                frame.run_activation(Activation::ToggleRailExpanded, window, cx);
            }))
            .text_xs()
            .child(view_more_label(expanded)),
    )
}

#[cfg(test)]
mod tests {
    use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};

    use super::*;

    struct FakeTool {
        id: &'static str,
        name: &'static str,
        group: &'static str,
        in_rail: bool,
        shortcut: Option<&'static str>,
    }

    impl ToolPlugin for FakeTool {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn icon(&self) -> &'static str {
            self.id
        }

        fn group(&self) -> &'static str {
            self.group
        }

        fn shortcut(&self) -> Option<&'static str> {
            self.shortcut
        }

        fn in_rail(&self) -> bool {
            self.in_rail
        }

        fn on_pointer_down(&mut self, _: &mut ToolCtx, _: PointerInput) {}
        fn on_pointer_move(&mut self, _: &mut ToolCtx, _: PointerInput) {}
        fn on_pointer_up(&mut self, _: &mut ToolCtx, _: PointerInput) {}
    }

    fn tool(
        id: &'static str,
        name: &'static str,
        group: &'static str,
        in_rail: bool,
        shortcut: Option<&'static str>,
    ) -> Box<dyn ToolPlugin> {
        Box::new(FakeTool {
            id,
            name,
            group,
            in_rail,
            shortcut,
        })
    }

    fn registry() -> PluginRegistry {
        let mut registry = PluginRegistry::new();
        registry.register_tool(tool("select", "Select", "cursor", true, Some("v")));
        registry.register_tool(tool("marquee", "Marquee", "cursor", true, Some("m")));
        registry.register_tool(tool("hidden", "Hidden", "cursor", false, None));
        registry.register_tool(tool("comment", "Comment", "annotate", true, Some("c")));
        registry
    }

    #[test]
    fn collapsed_entries_group_tools_in_registry_order() {
        let registry = registry();
        let entries = RailState::default().entries(&registry, None);

        assert_eq!(
            entries
                .iter()
                .map(|entry| {
                    (
                        entry.registry_index,
                        entry.id,
                        entry.name,
                        entry.icon,
                        entry.group,
                        entry.shortcut,
                    )
                })
                .collect::<Vec<_>>(),
            [
                (0, "select", "Select", "select", "cursor", Some("v")),
                (3, "comment", "Comment", "comment", "annotate", Some("c")),
            ]
        );
    }

    #[test]
    fn view_more_exposes_the_complete_flat_in_rail_list() {
        let registry = registry();
        let mut state = RailState::default();
        state.toggle_expanded();

        assert!(state.expanded());
        assert_eq!(
            state
                .entries(&registry, None)
                .iter()
                .map(|entry| {
                    (
                        entry.registry_index,
                        entry.id,
                        entry.name,
                        entry.icon,
                        entry.group,
                        entry.shortcut,
                    )
                })
                .collect::<Vec<_>>(),
            [
                (0, "select", "Select", "select", "cursor", Some("v")),
                (1, "marquee", "Marquee", "marquee", "cursor", Some("m")),
                (3, "comment", "Comment", "comment", "annotate", Some("c")),
            ]
        );
    }

    #[test]
    fn expanded_selection_updates_the_collapsed_group_member() {
        let registry = registry();
        let mut state = RailState::default();
        state.toggle_expanded();
        let marquee = state.entries(&registry, None)[1];

        state.remember(marquee);
        state.toggle_expanded();

        assert_eq!(state.entries(&registry, None)[0].id, "marquee");
    }

    #[test]
    fn active_tool_wins_without_replacing_window_memory() {
        let registry = registry();
        let mut state = RailState::default();
        state.toggle_expanded();
        state.remember(state.entries(&registry, None)[1]);
        state.toggle_expanded();

        let active_entries = state.entries(&registry, Some(0));
        assert_eq!(active_entries[0].id, "select");
        assert!(active_entries[0].active);

        let remembered_entries = state.entries(&registry, None);
        assert_eq!(remembered_entries[0].id, "marquee");
    }

    #[test]
    fn a_hidden_active_tool_is_neither_shown_nor_marked_active() {
        let registry = registry();
        let mut state = RailState::default();
        state.toggle_expanded();
        state.remember(state.entries(&registry, None)[1]);
        state.toggle_expanded();

        let entries = state.entries(&registry, Some(2));
        assert_eq!(entries[0].id, "marquee");
        assert!(entries.iter().all(|entry| entry.id != "hidden"));
        assert!(entries.iter().all(|entry| !entry.active));
    }

    #[test]
    fn a_registry_without_the_remembered_tool_falls_back_to_its_first_member() {
        let first_registry = registry();
        let mut state = RailState::default();
        state.toggle_expanded();
        state.remember(state.entries(&first_registry, None)[1]);
        state.toggle_expanded();

        let mut other_registry = PluginRegistry::new();
        other_registry.register_tool(tool("hand", "Hand", "cursor", true, Some("h")));
        other_registry.register_tool(tool("zoom", "Zoom", "cursor", true, Some("z")));

        assert_eq!(state.entries(&other_registry, None)[0].id, "hand");
    }

    #[test]
    fn empty_registries_are_usable_in_both_view_more_states() {
        let registry = PluginRegistry::new();
        let mut state = RailState::default();

        assert!(state.entries(&registry, None).is_empty());
        state.toggle_expanded();
        assert!(state.expanded());
        assert!(state.entries(&registry, None).is_empty());
    }

    #[test]
    fn rail_icon_tokens_are_drawn_as_compact_marks() {
        assert_eq!(icon_glyph("hand"), "✋");
        assert_eq!(icon_glyph("select-text"), "T");
        assert_eq!(icon_glyph("select-region"), "□");
        assert_eq!(icon_glyph("zoom"), "⌕");
        assert_eq!(icon_glyph("dynamic-zoom"), "⇕");
        assert_eq!(icon_glyph("snapshot"), "▣");
        assert_eq!(icon_glyph("plugin-id-token"), "?");
    }

    #[test]
    fn real_rail_contents_equal_the_in_rail_registry_tools() {
        let registry = crate::build_registry();
        let expected = registry
            .tools()
            .enumerate()
            .filter(|(_, tool)| tool.in_rail())
            .map(|(index, tool)| (index, tool.id()))
            .collect::<Vec<_>>();
        let mut state = RailState::default();
        state.toggle_expanded();
        let actual = state
            .entries(&registry, None)
            .iter()
            .map(|entry| (entry.registry_index, entry.id))
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn selection_activates_the_real_canvas_model_and_updates_group_memory() {
        use std::path::PathBuf;

        use onionskin_core::{Document, ViewSize};

        use crate::shell::canvas::CanvasModel;

        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
        let mut model = CanvasModel::new(
            Document::open_path(&path).unwrap(),
            registry(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
        )
        .unwrap();
        let mut state = RailState::default();
        state.toggle_expanded();
        let marquee = state.entries(model.registry(), model.active_tool())[1];

        assert!(
            apply_rail_selection(&mut state, marquee, |index| { model.activate_tool(index) })
                .unwrap()
        );
        assert_eq!(model.active_tool(), Some(1));

        model.activate_tool(3).unwrap();
        state.toggle_expanded();
        let collapsed = state.entries(model.registry(), model.active_tool());
        assert_eq!(collapsed[0].id, "marquee");
        assert_eq!(collapsed[1].id, "comment");
        assert!(collapsed[1].active);
    }

    /// The icon is a glyph or an id-like token drawn as text. A rail entry
    /// announced by its icon would be read out as that token, so the name a
    /// screen reader gets is the tool's name.
    #[test]
    fn a_rail_entry_is_announced_by_the_tool_name_and_never_by_its_icon_token() {
        let entries = vec![
            RailEntry {
                registry_index: 0,
                id: "select",
                name: "Select",
                icon: "sel-ico",
                shortcut: Some("v"),
                group: "cursor",
                active: false,
            },
            RailEntry {
                registry_index: 3,
                id: "comment",
                name: "Comment",
                icon: "cmt-ico",
                shortcut: None,
                group: "annotate",
                active: true,
            },
        ];

        let described = accessible(&entries, true);

        for entry in &entries {
            let node = described
                .find(&("tool-rail-entry", entry.registry_index).into())
                .expect(entry.id);
            assert_eq!(node.label, entry.name);
            assert_ne!(node.label, entry.icon);
            assert!(!node.label.contains(entry.icon));
        }
    }

    #[test]
    fn the_active_rail_entry_is_the_selected_one_and_carries_the_action_its_click_runs() {
        let entries = vec![
            RailEntry {
                registry_index: 0,
                id: "select",
                name: "Select",
                icon: "sel-ico",
                shortcut: Some("v"),
                group: "cursor",
                active: false,
            },
            RailEntry {
                registry_index: 1,
                id: "marquee",
                name: "Marquee",
                icon: "mar-ico",
                shortcut: Some("m"),
                group: "cursor",
                active: true,
            },
        ];

        let described = accessible(&entries, true);

        let inactive = described.find(&("tool-rail-entry", 0usize).into()).unwrap();
        let active = described.find(&("tool-rail-entry", 1usize).into()).unwrap();
        assert_eq!(inactive.state.selected, Some(false));
        assert_eq!(active.state.selected, Some(true));
        assert_eq!(inactive.activation, Some(Activation::Rail(entries[0])));
        assert_eq!(active.activation, Some(Activation::Rail(entries[1])));
    }

    /// The shortcut is drawn beside the name only when the rail is expanded.
    /// A screen reader hears it either way, because it is the only place the
    /// keyboard route is stated.
    #[test]
    fn a_rail_entry_announces_its_keyboard_shortcut_and_omits_the_description_without_one() {
        let with_shortcut = RailEntry {
            registry_index: 0,
            id: "select",
            name: "Select",
            icon: "sel-ico",
            shortcut: Some("v"),
            group: "cursor",
            active: false,
        };
        let without_shortcut = RailEntry {
            registry_index: 1,
            id: "plain",
            name: "Plain",
            icon: "pln-ico",
            shortcut: None,
            group: "cursor",
            active: false,
        };

        for expanded in [true, false] {
            let described = accessible(&[with_shortcut, without_shortcut], expanded);
            assert_eq!(
                described
                    .find(&("tool-rail-entry", 0usize).into())
                    .unwrap()
                    .description
                    .as_deref(),
                Some("Shortcut v")
            );
            assert_eq!(
                described
                    .find(&("tool-rail-entry", 1usize).into())
                    .unwrap()
                    .description,
                None
            );
        }
    }

    #[test]
    fn the_view_more_toggle_announces_what_it_draws_and_carries_expansion_as_state() {
        let expanded = accessible(&[], true);
        let collapsed = accessible(&[], false);

        let expanded = expanded.find(&"tool-rail-view-more".into()).unwrap();
        let collapsed = collapsed.find(&"tool-rail-view-more".into()).unwrap();
        assert_eq!(expanded.label, "Show less");
        assert_eq!(collapsed.label, "View more");
        assert_eq!(expanded.state.toggled, Some(true));
        assert_eq!(collapsed.state.toggled, Some(false));
        assert_eq!(expanded.activation, Some(Activation::ToggleRailExpanded));
    }

    /// The empty rail still draws a child, so it still describes one, or the
    /// toggle would inherit the message's rectangle.
    #[test]
    fn an_empty_rail_describes_its_message_before_the_toggle() {
        let described = accessible(&[], false);

        assert_eq!(described.children.len(), 2);
        assert_eq!(described.children[0].role, Role::Label);
        assert_eq!(described.children[0].label, "No tools");
        assert_eq!(described.children[0].activation, None);
        assert_eq!(
            accessible(&[], true).children[0].label,
            "No tools installed"
        );
    }

    /// Both halves of "one list drives both": the column renders one child
    /// per entry and then the toggle, and the description has to match that
    /// count and order for the prepainted rectangles to line up.
    #[test]
    fn the_description_has_one_node_per_rendered_column_child_in_column_order() {
        let entries = (0..3)
            .map(|registry_index| RailEntry {
                registry_index,
                id: "tool",
                name: "Tool",
                icon: "ico",
                shortcut: None,
                group: "cursor",
                active: false,
            })
            .collect::<Vec<_>>();

        let described = accessible(&entries, true);

        assert_eq!(described.children.len(), entries.len() + 1);
        let expected: Vec<gpui::ElementId> = vec![
            ("tool-rail-entry", 0usize).into(),
            ("tool-rail-entry", 1usize).into(),
            ("tool-rail-entry", 2usize).into(),
            "tool-rail-view-more".into(),
        ];
        assert_eq!(
            described
                .children
                .iter()
                .map(|child| child.key.clone())
                .collect::<Vec<_>>(),
            expected
        );
    }
}
