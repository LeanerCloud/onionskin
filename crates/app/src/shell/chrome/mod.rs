mod global_bar;
mod rail;
mod tabs;
mod tool_search;

pub(in crate::shell) use global_bar::install_native_menus;
pub(in crate::shell) use global_bar::MenuState;
pub(in crate::shell) use tabs::ShellFrame;
pub(in crate::shell) use tool_search::install_keybindings as install_search_keybindings;
