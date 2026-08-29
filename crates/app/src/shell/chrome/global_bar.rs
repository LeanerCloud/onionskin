use gpui::{actions, App, Menu, MenuItem, WindowHandle};

use super::ShellFrame;

actions!(onionskin_shell, [CloseTab, CloseOtherTabs, CloseAllTabs]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuSectionId {
    File,
    Edit,
    View,
    Window,
    Help,
}

impl MenuSectionId {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Window => "Window",
            Self::Help => "Help",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuCommand {
    Open,
    CloseTab,
    CloseOtherTabs,
    CloseAllTabs,
    Undo,
    Redo,
    ViewControls,
    NewWindow,
    About,
    KeyboardShortcuts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuAvailability {
    Enabled,
    Disabled(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct MenuState {
    tab_count: usize,
    has_active_tab: bool,
}

impl MenuState {
    pub(in crate::shell) fn new(tab_count: usize) -> Self {
        Self {
            tab_count,
            has_active_tab: tab_count > 0,
        }
    }
}

impl MenuAvailability {
    pub(super) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub(super) fn reason(self) -> Option<&'static str> {
        match self {
            Self::Enabled => None,
            Self::Disabled(reason) => Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MenuEntry {
    pub(super) command: MenuCommand,
    pub(super) label: &'static str,
    pub(super) availability: MenuAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MenuSection {
    pub(super) id: MenuSectionId,
    pub(super) entries: Vec<MenuEntry>,
}

pub(super) fn main_menu_schema(state: MenuState) -> Vec<MenuSection> {
    use MenuAvailability::{Disabled, Enabled};

    let document_command = if state.has_active_tab {
        Enabled
    } else {
        Disabled("No document is open")
    };
    let close_others = if state.tab_count > 1 {
        Enabled
    } else {
        Disabled("No other tabs are open")
    };

    vec![
        MenuSection {
            id: MenuSectionId::File,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::Open,
                    label: "Open…",
                    availability: Disabled("File Open lands in M2 P11"),
                },
                MenuEntry {
                    command: MenuCommand::CloseTab,
                    label: "Close",
                    availability: document_command,
                },
                MenuEntry {
                    command: MenuCommand::CloseOtherTabs,
                    label: "Close Others",
                    availability: close_others,
                },
                MenuEntry {
                    command: MenuCommand::CloseAllTabs,
                    label: "Close All",
                    availability: document_command,
                },
            ],
        },
        MenuSection {
            id: MenuSectionId::Edit,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::Undo,
                    label: "Undo",
                    availability: Disabled("Document editing lands in M3"),
                },
                MenuEntry {
                    command: MenuCommand::Redo,
                    label: "Redo",
                    availability: Disabled("Document editing lands in M3"),
                },
            ],
        },
        MenuSection {
            id: MenuSectionId::View,
            entries: vec![MenuEntry {
                command: MenuCommand::ViewControls,
                label: "View Controls",
                availability: Disabled("View controls land in M2 P7c"),
            }],
        },
        MenuSection {
            id: MenuSectionId::Window,
            entries: vec![MenuEntry {
                command: MenuCommand::NewWindow,
                label: "New Window",
                availability: Disabled("Window management lands in M3"),
            }],
        },
        MenuSection {
            id: MenuSectionId::Help,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::About,
                    label: "About Onionskin",
                    availability: Disabled("Help commands land in M2 P11"),
                },
                MenuEntry {
                    command: MenuCommand::KeyboardShortcuts,
                    label: "Keyboard Shortcuts",
                    availability: Disabled("Help commands land in M2 P11"),
                },
            ],
        },
    ]
}

pub(in crate::shell) fn install_native_menus(
    cx: &mut App,
    window: WindowHandle<ShellFrame>,
    state: MenuState,
) {
    cx.on_action(move |_: &CloseTab, cx| {
        ShellFrame::run_native_command(window, MenuCommand::CloseTab, cx);
    });
    cx.on_action(move |_: &CloseOtherTabs, cx| {
        ShellFrame::run_native_command(window, MenuCommand::CloseOtherTabs, cx);
    });
    cx.on_action(move |_: &CloseAllTabs, cx| {
        ShellFrame::run_native_command(window, MenuCommand::CloseAllTabs, cx);
    });

    refresh_native_menus(cx, state);
}

pub(super) fn refresh_native_menus(cx: &App, state: MenuState) {
    cx.set_menus(native_menus(state));
}

fn native_menus(state: MenuState) -> Vec<Menu> {
    main_menu_schema(state)
        .into_iter()
        .map(|section| Menu {
            name: section.id.label().into(),
            items: section
                .entries
                .into_iter()
                .filter_map(native_menu_item)
                .collect(),
        })
        .collect()
}

fn native_menu_item(entry: MenuEntry) -> Option<MenuItem> {
    if !entry.availability.is_enabled() {
        return None;
    }

    match entry.command {
        MenuCommand::CloseTab => Some(MenuItem::action(entry.label, CloseTab)),
        MenuCommand::CloseOtherTabs => Some(MenuItem::action(entry.label, CloseOtherTabs)),
        MenuCommand::CloseAllTabs => Some(MenuItem::action(entry.label, CloseAllTabs)),
        MenuCommand::Open
        | MenuCommand::Undo
        | MenuCommand::Redo
        | MenuCommand::ViewControls
        | MenuCommand::NewWindow
        | MenuCommand::About
        | MenuCommand::KeyboardShortcuts => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_menu_has_the_five_required_sections_in_order() {
        let ids: Vec<_> = main_menu_schema(MenuState::new(2))
            .into_iter()
            .map(|section| section.id)
            .collect();

        assert_eq!(
            ids,
            [
                MenuSectionId::File,
                MenuSectionId::Edit,
                MenuSectionId::View,
                MenuSectionId::Window,
                MenuSectionId::Help,
            ]
        );
    }

    #[test]
    fn every_deferred_entry_names_its_own_delivery_stage() {
        let disabled: Vec<_> = main_menu_schema(MenuState::new(2))
            .into_iter()
            .flat_map(|section| section.entries)
            .filter_map(|entry| entry.availability.reason())
            .collect();

        assert!(!disabled.is_empty());
        assert!(disabled
            .iter()
            .all(|reason| reason.contains("M2") || reason.contains("M3")));
        assert!(disabled.iter().any(|reason| reason.contains("P7c")));
        assert!(disabled.iter().any(|reason| reason.contains("P11")));
    }

    #[test]
    fn native_menus_are_derived_from_the_same_five_section_schema() {
        let menus = native_menus(MenuState::new(2));

        assert_eq!(menus.len(), 5);
        assert_eq!(menus[0].name.as_ref(), "File");
        assert_eq!(menus[1].name.as_ref(), "Edit");
        assert_eq!(menus[2].name.as_ref(), "View");
        assert_eq!(menus[3].name.as_ref(), "Window");
        assert_eq!(menus[4].name.as_ref(), "Help");
        assert_eq!(menus[0].items.len(), 3);
        assert!(menus[1..].iter().all(|menu| menu.items.is_empty()));
    }

    #[test]
    fn close_others_is_disabled_consistently_for_one_tab() {
        let file = &main_menu_schema(MenuState::new(1))[0];
        let close_others = file
            .entries
            .iter()
            .find(|entry| entry.command == MenuCommand::CloseOtherTabs)
            .unwrap();

        assert_eq!(
            close_others.availability,
            MenuAvailability::Disabled("No other tabs are open")
        );
        assert_eq!(native_menus(MenuState::new(1))[0].items.len(), 2);
    }
}
