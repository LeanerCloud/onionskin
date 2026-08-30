use std::collections::BTreeMap;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_plugin_api::{PluginRegistry, ToolPlugin};

use super::tabs::ShellFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RailEntry {
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

pub(super) fn render_rail(
    entries: Vec<RailEntry>,
    expanded: bool,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut rail = div()
        .w(px(if expanded { 240.0 } else { 88.0 }))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .bg(gpui::rgb(0x202124))
        .text_color(gpui::white());

    if entries.is_empty() {
        rail = rail.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(gpui::rgb(0x85878c))
                .child(if expanded {
                    "No tools installed"
                } else {
                    "No tools"
                }),
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
                        gpui::rgb(0x3a3b3f)
                    } else {
                        gpui::rgb(0x292a2d)
                    })
                    .hover(|row| row.bg(gpui::rgb(0x45464b)))
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        frame.select_rail_entry(entry, cx);
                    }))
                    .child(
                        div()
                            .w(px(24.0))
                            .flex_none()
                            .text_xs()
                            .text_color(gpui::rgb(0xaeb0b5))
                            .child(entry.icon),
                    )
                    .when(expanded, |row| {
                        row.child(div().flex_1().text_sm().child(entry.name))
                            .when_some(entry.shortcut, |row, shortcut| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(gpui::rgb(0x85878c))
                                        .child(shortcut),
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
            .bg(gpui::rgb(0x292a2d))
            .hover(|button| button.bg(gpui::rgb(0x3a3b3f)))
            .on_click(cx.listener(|frame, _event, _window, cx| {
                frame.toggle_rail_expanded(cx);
            }))
            .text_xs()
            .child(if expanded { "Show less" } else { "View more" }),
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
}
