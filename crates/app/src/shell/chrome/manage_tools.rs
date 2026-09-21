//! Manage Tools: which tools the rail shows.
//!
//! Acrobat's "Customize the tool rail" lets the user take tools they never use
//! off the rail. Here each rail tool is a checkbox, and a cleared one is kept
//! in the preferences file as `hidden_tools`. Hiding is only about the rail:
//! the tool still runs from its menu entry, its shortcut and Tool Search, and
//! the rail keeps showing a hidden tool while it is the selected one.

use std::collections::BTreeSet;

use accesskit::Role;
use gpui::{
    div, px, Context, InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _,
};
use onionskin_plugin_api::PluginRegistry;

use super::accessible::{Activation, Element};
use super::tabs::ShellFrame;
use super::theme::ThemeTokens;
use crate::a11y::State as A11yState;

/// One rail tool as the dialog lists it. Taken from the registry when the
/// dialog opens, so drawing it does not build a registry every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct ManagedTool {
    pub(in crate::shell) id: &'static str,
    pub(in crate::shell) name: &'static str,
}

/// Every tool that has a rail button, in rail order.
pub(in crate::shell) fn managed_tools(registry: &PluginRegistry) -> Vec<ManagedTool> {
    registry
        .tools()
        .filter(|tool| tool.in_rail())
        .map(|tool| ManagedTool {
            id: tool.id(),
            name: tool.name(),
        })
        .collect()
}

/// Show a hidden tool again, or hide a shown one. Whether it is now shown.
pub(in crate::shell) fn toggle(hidden: &mut BTreeSet<String>, id: &str) -> bool {
    if hidden.remove(id) {
        return true;
    }
    hidden.insert(id.to_owned());
    false
}

/// A check mark for a shown tool, a ring for a hidden one, as the quick
/// actions' customization list draws them.
fn glyph(shown: bool) -> &'static str {
    if shown {
        "✓"
    } else {
        "○"
    }
}

/// What the dialog tells a screen reader: one checkbox per tool, checked
/// when the rail shows it.
pub(in crate::shell) fn accessible(
    tools: &[ManagedTool],
    hidden: &BTreeSet<String>,
) -> Vec<Element> {
    tools
        .iter()
        .enumerate()
        .map(|(index, tool)| {
            Element::new(("manage-tools-row", index), Role::CheckBox, tool.name)
                .with_state(A11yState::toggled(!hidden.contains(tool.id)))
                .with_activation(Activation::ToggleToolShown(tool.id))
        })
        .collect()
}

pub(in crate::shell) fn render(
    tools: &[ManagedTool],
    hidden: &BTreeSet<String>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list =
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().pb_2().text_color(theme.secondary_text).child(
                "Tools you clear leave the rail. Menus, shortcuts and Tool Search still run them.",
            ));
    for (index, tool) in tools.iter().enumerate() {
        let id = tool.id;
        list = list.child(
            div()
                .id(("manage-tools-row", index))
                .h(px(32.0))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |row| row.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::ToggleToolShown(id), window, cx);
                }))
                .child(glyph(!hidden.contains(id)))
                .child(tool.name),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &'static str, name: &'static str) -> ManagedTool {
        ManagedTool { id, name }
    }

    #[test]
    fn toggling_hides_a_shown_tool_and_shows_a_hidden_one() {
        let mut hidden = BTreeSet::new();
        assert!(!toggle(&mut hidden, "tool.ink"));
        assert!(hidden.contains("tool.ink"));
        assert!(toggle(&mut hidden, "tool.ink"));
        assert!(hidden.is_empty());
    }

    /// Each row is a checkbox that is checked when the rail shows the tool,
    /// and its activation toggles that same tool.
    #[test]
    fn every_row_is_a_checkbox_checked_when_the_rail_shows_it() {
        let tools = [tool("a", "Alpha"), tool("b", "Beta")];
        let hidden = BTreeSet::from(["b".to_owned()]);

        let rows = accessible(&tools, &hidden);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, Role::CheckBox);
        assert_eq!(rows[0].state.toggled, Some(true));
        assert_eq!(rows[1].state.toggled, Some(false));
        assert_eq!(rows[1].label, "Beta");
        assert_eq!(rows[1].activation, Some(Activation::ToggleToolShown("b")));
        assert_eq!(glyph(true), "✓");
        assert_eq!(glyph(false), "○");
    }

    /// The dialog lists exactly the tools with a rail button, in rail order.
    #[test]
    fn the_dialog_lists_the_rail_tools_of_the_registry() {
        let registry = crate::build_registry();
        let listed = managed_tools(&registry);
        let rail: Vec<_> = registry
            .tools()
            .filter(|tool| tool.in_rail())
            .map(|tool| tool.id())
            .collect();
        assert_eq!(listed.iter().map(|tool| tool.id).collect::<Vec<_>>(), rail);
    }
}
