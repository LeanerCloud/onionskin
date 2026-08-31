mod global_bar;
mod page_controls;
mod quick_actions;
mod rail;
mod side_panel;
mod tabs;
mod theme;
mod tool_search;

pub(in crate::shell) use global_bar::install_native_menus;
pub(in crate::shell) use global_bar::{ExportCodecs, MenuState};
pub(in crate::shell) use tabs::ShellFrame;
pub(in crate::shell) use theme::{ShellViewState, ThemeTokens};
pub(in crate::shell) use tool_search::install_keybindings as install_search_keybindings;
