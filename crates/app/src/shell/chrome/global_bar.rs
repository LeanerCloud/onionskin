use gpui::{actions, App, Menu, MenuItem, WindowHandle};
use onionskin_core::{FitMode, PageLayoutMode};

use super::quick_actions::QuickAction;
use super::theme::{ShellViewAction, ShellViewState, ThemePreference};
use super::ShellFrame;
use crate::shell::canvas::{CanvasViewState, ViewAction};

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
    PreviousView,
    NextView,
    FirstPage,
    PreviousPage,
    NextPage,
    LastPage,
    RotateClockwise,
    ActualSize,
    ZoomOut,
    ZoomIn,
    FitPage,
    FitWidth,
    FitHeight,
    SinglePage,
    SinglePageContinuous,
    TwoPage,
    TwoPageContinuous,
    ToggleCover,
    ToggleNavigationPane,
    ToggleQuickAction(QuickAction),
    TogglePageControls,
    LineWeights,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    ReadMode,
    FullScreen,
    NewWindow,
    About,
    KeyboardShortcuts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuAvailability {
    Enabled,
    Disabled(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) struct MenuState {
    tab_count: usize,
    has_active_tab: bool,
    view: Option<CanvasViewState>,
    shell_view: ShellViewState,
    quick_actions_visible: [bool; QuickAction::ALL.len()],
}

impl MenuState {
    pub(in crate::shell) fn initial(
        tab_count: usize,
        view: Option<CanvasViewState>,
        shell_view: ShellViewState,
    ) -> Self {
        Self::new(tab_count, view, shell_view, [true; QuickAction::ALL.len()])
    }

    pub(in crate::shell) fn new(
        tab_count: usize,
        view: Option<CanvasViewState>,
        shell_view: ShellViewState,
        quick_actions_visible: [bool; QuickAction::ALL.len()],
    ) -> Self {
        Self {
            tab_count,
            has_active_tab: tab_count > 0,
            view,
            shell_view,
            quick_actions_visible,
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
    pub(super) selected: bool,
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
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CloseTab,
                    label: "Close",
                    availability: document_command,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CloseOtherTabs,
                    label: "Close Others",
                    availability: close_others,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CloseAllTabs,
                    label: "Close All",
                    availability: document_command,
                    selected: false,
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
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Redo,
                    label: "Redo",
                    availability: Disabled("Document editing lands in M3"),
                    selected: false,
                },
            ],
        },
        MenuSection {
            id: MenuSectionId::View,
            entries: view_menu_entries(state.view, state.shell_view, state.quick_actions_visible),
        },
        MenuSection {
            id: MenuSectionId::Window,
            entries: vec![MenuEntry {
                command: MenuCommand::NewWindow,
                label: "New Window",
                availability: Disabled("Window management lands in M3"),
                selected: false,
            }],
        },
        MenuSection {
            id: MenuSectionId::Help,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::About,
                    label: "About Onionskin",
                    availability: Disabled("Help commands land in M2 P11"),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::KeyboardShortcuts,
                    label: "Keyboard Shortcuts",
                    availability: Disabled("Help commands land in M2 P11"),
                    selected: false,
                },
            ],
        },
    ]
}

fn view_menu_entries(
    view: Option<CanvasViewState>,
    shell_view: ShellViewState,
    quick_actions_visible: [bool; QuickAction::ALL.len()],
) -> Vec<MenuEntry> {
    use MenuAvailability::{Disabled, Enabled};

    let availability = view
        .map(|_| Enabled)
        .unwrap_or(Disabled("No document is open"));
    let at_first = view.is_none_or(|view| view.current_page == 0);
    let at_last = view.is_none_or(|view| view.current_page + 1 >= view.page_count);
    let can_previous_view = view.is_some_and(|view| view.can_previous_view);
    let can_next_view = view.is_some_and(|view| view.can_next_view);
    let layout = view.map(|view| view.layout_mode);
    let show_cover = view.is_some_and(|view| view.show_cover);
    let actual_size = view.is_some_and(CanvasViewState::is_actual_size);
    let fit_mode = view.and_then(CanvasViewState::fit_mode);
    let entry = |command, label, availability, selected| MenuEntry {
        command,
        label,
        availability,
        selected,
    };

    let mut entries = vec![
        entry(
            MenuCommand::PreviousView,
            "Previous View",
            if can_previous_view {
                Enabled
            } else {
                Disabled("No previous view")
            },
            false,
        ),
        entry(
            MenuCommand::NextView,
            "Next View",
            if can_next_view {
                Enabled
            } else {
                Disabled("No next view")
            },
            false,
        ),
        entry(
            MenuCommand::FirstPage,
            "First Page",
            if at_first {
                Disabled("Already at the first page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::PreviousPage,
            "Previous Page",
            if at_first {
                Disabled("Already at the first page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::NextPage,
            "Next Page",
            if at_last {
                Disabled("Already at the last page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::LastPage,
            "Last Page",
            if at_last {
                Disabled("Already at the last page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::RotateClockwise,
            "Rotate Clockwise",
            availability,
            false,
        ),
        entry(
            MenuCommand::ActualSize,
            "Actual Size",
            availability,
            actual_size,
        ),
        entry(MenuCommand::ZoomOut, "Zoom Out", availability, false),
        entry(MenuCommand::ZoomIn, "Zoom In", availability, false),
        entry(
            MenuCommand::FitPage,
            "Fit Page",
            availability,
            fit_mode == Some(FitMode::Page),
        ),
        entry(
            MenuCommand::FitWidth,
            "Fit Width",
            availability,
            fit_mode == Some(FitMode::Width),
        ),
        entry(
            MenuCommand::FitHeight,
            "Fit Height",
            availability,
            fit_mode == Some(FitMode::Height),
        ),
        entry(
            MenuCommand::SinglePage,
            "Single Page",
            availability,
            layout == Some(PageLayoutMode::SinglePage),
        ),
        entry(
            MenuCommand::SinglePageContinuous,
            "Single Page Continuous",
            availability,
            layout == Some(PageLayoutMode::SinglePageContinuous),
        ),
        entry(
            MenuCommand::TwoPage,
            "Two Page",
            availability,
            layout == Some(PageLayoutMode::TwoPage),
        ),
        entry(
            MenuCommand::TwoPageContinuous,
            "Two Page Continuous",
            availability,
            layout == Some(PageLayoutMode::TwoPageContinuous),
        ),
        entry(
            MenuCommand::ToggleCover,
            "Show Cover Page",
            availability,
            show_cover,
        ),
    ];
    entries.push(entry(
        MenuCommand::ToggleNavigationPane,
        "Show Navigation Panes",
        Enabled,
        shell_view.navigation_pane_visible(),
    ));
    entries.extend(QuickAction::ALL.into_iter().map(|action| {
        entry(
            MenuCommand::ToggleQuickAction(action),
            action.menu_label(),
            Enabled,
            quick_actions_visible[action.index()],
        )
    }));
    entries.extend([
        entry(
            MenuCommand::TogglePageControls,
            "Show Page Controls",
            Enabled,
            shell_view.page_controls_visible(),
        ),
        entry(
            MenuCommand::LineWeights,
            "Line Weights",
            Disabled("Line Weights land in M3"),
            false,
        ),
        entry(
            MenuCommand::ThemeSystem,
            "System Theme",
            Enabled,
            shell_view.theme() == ThemePreference::System,
        ),
        entry(
            MenuCommand::ThemeLight,
            "Light Theme",
            Enabled,
            shell_view.theme() == ThemePreference::Light,
        ),
        entry(
            MenuCommand::ThemeDark,
            "Dark Theme",
            Enabled,
            shell_view.theme() == ThemePreference::Dark,
        ),
        entry(
            MenuCommand::ReadMode,
            "Read Mode",
            Enabled,
            shell_view.read_mode(),
        ),
        entry(
            MenuCommand::FullScreen,
            "Full Screen",
            Enabled,
            shell_view.fullscreen(),
        ),
    ]);
    entries
}

impl MenuCommand {
    pub(super) fn view_action(self, view: CanvasViewState) -> Option<ViewAction> {
        Some(match self {
            Self::PreviousView => ViewAction::PreviousView,
            Self::NextView => ViewAction::NextView,
            Self::FirstPage => ViewAction::FirstPage,
            Self::PreviousPage => ViewAction::PreviousPage,
            Self::NextPage => ViewAction::NextPage,
            Self::LastPage => ViewAction::LastPage,
            Self::RotateClockwise => ViewAction::RotateClockwise,
            Self::ActualSize => ViewAction::ActualSize,
            Self::ZoomOut => ViewAction::ZoomOut,
            Self::ZoomIn => ViewAction::ZoomIn,
            Self::FitPage => ViewAction::Fit(FitMode::Page),
            Self::FitWidth => ViewAction::Fit(FitMode::Width),
            Self::FitHeight => ViewAction::Fit(FitMode::Height),
            Self::SinglePage => ViewAction::SetLayout(PageLayoutMode::SinglePage),
            Self::SinglePageContinuous => {
                ViewAction::SetLayout(PageLayoutMode::SinglePageContinuous)
            }
            Self::TwoPage => ViewAction::SetLayout(PageLayoutMode::TwoPage),
            Self::TwoPageContinuous => ViewAction::SetLayout(PageLayoutMode::TwoPageContinuous),
            Self::ToggleCover => ViewAction::SetShowCover(!view.show_cover),
            Self::Open
            | Self::CloseTab
            | Self::CloseOtherTabs
            | Self::CloseAllTabs
            | Self::Undo
            | Self::Redo
            | Self::ToggleNavigationPane
            | Self::ToggleQuickAction(_)
            | Self::TogglePageControls
            | Self::LineWeights
            | Self::ThemeSystem
            | Self::ThemeLight
            | Self::ThemeDark
            | Self::ReadMode
            | Self::FullScreen
            | Self::NewWindow
            | Self::About
            | Self::KeyboardShortcuts => return None,
        })
    }

    pub(super) fn shell_view_action(self) -> Option<ShellViewAction> {
        Some(match self {
            Self::ToggleNavigationPane => ShellViewAction::ToggleNavigationPane,
            Self::TogglePageControls => ShellViewAction::TogglePageControls,
            Self::ThemeSystem => ShellViewAction::SetTheme(ThemePreference::System),
            Self::ThemeLight => ShellViewAction::SetTheme(ThemePreference::Light),
            Self::ThemeDark => ShellViewAction::SetTheme(ThemePreference::Dark),
            Self::ReadMode => ShellViewAction::ToggleReadMode,
            Self::Open
            | Self::CloseTab
            | Self::CloseOtherTabs
            | Self::CloseAllTabs
            | Self::Undo
            | Self::Redo
            | Self::PreviousView
            | Self::NextView
            | Self::FirstPage
            | Self::PreviousPage
            | Self::NextPage
            | Self::LastPage
            | Self::RotateClockwise
            | Self::ActualSize
            | Self::ZoomOut
            | Self::ZoomIn
            | Self::FitPage
            | Self::FitWidth
            | Self::FitHeight
            | Self::SinglePage
            | Self::SinglePageContinuous
            | Self::TwoPage
            | Self::TwoPageContinuous
            | Self::ToggleCover
            | Self::ToggleQuickAction(_)
            | Self::LineWeights
            | Self::FullScreen
            | Self::NewWindow
            | Self::About
            | Self::KeyboardShortcuts => return None,
        })
    }
}

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = onionskin_shell, no_json)]
struct RunViewMenu {
    command: MenuCommand,
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
    cx.on_action(move |action: &RunViewMenu, cx| {
        ShellFrame::run_native_command(window, action.command, cx);
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

    let label = if entry.selected {
        format!("✓ {}", entry.label)
    } else {
        entry.label.to_owned()
    };
    match entry.command {
        MenuCommand::CloseTab => Some(MenuItem::action(label, CloseTab)),
        MenuCommand::CloseOtherTabs => Some(MenuItem::action(label, CloseOtherTabs)),
        MenuCommand::CloseAllTabs => Some(MenuItem::action(label, CloseAllTabs)),
        MenuCommand::PreviousView
        | MenuCommand::NextView
        | MenuCommand::FirstPage
        | MenuCommand::PreviousPage
        | MenuCommand::NextPage
        | MenuCommand::LastPage
        | MenuCommand::RotateClockwise
        | MenuCommand::ActualSize
        | MenuCommand::ZoomOut
        | MenuCommand::ZoomIn
        | MenuCommand::FitPage
        | MenuCommand::FitWidth
        | MenuCommand::FitHeight
        | MenuCommand::SinglePage
        | MenuCommand::SinglePageContinuous
        | MenuCommand::TwoPage
        | MenuCommand::TwoPageContinuous
        | MenuCommand::ToggleCover
        | MenuCommand::ToggleNavigationPane
        | MenuCommand::ToggleQuickAction(_)
        | MenuCommand::TogglePageControls
        | MenuCommand::ThemeSystem
        | MenuCommand::ThemeLight
        | MenuCommand::ThemeDark
        | MenuCommand::ReadMode
        | MenuCommand::FullScreen => Some(MenuItem::action(
            label,
            RunViewMenu {
                command: entry.command,
            },
        )),
        MenuCommand::Open
        | MenuCommand::Undo
        | MenuCommand::Redo
        | MenuCommand::LineWeights
        | MenuCommand::NewWindow
        | MenuCommand::About
        | MenuCommand::KeyboardShortcuts => None,
    }
}

#[cfg(test)]
mod tests {
    use gpui::WindowAppearance;
    use onionskin_core::{ViewRotation, ZoomPolicy};

    use super::*;

    fn view() -> CanvasViewState {
        CanvasViewState {
            current_page: 1,
            page_count: 3,
            zoom: 1.25,
            zoom_policy: ZoomPolicy::Fixed,
            layout_mode: PageLayoutMode::TwoPage,
            show_cover: true,
            rotation: ViewRotation::Clockwise90,
            can_previous_view: true,
            can_next_view: true,
        }
    }

    fn menu_state(tab_count: usize, view: Option<CanvasViewState>) -> MenuState {
        MenuState::new(
            tab_count,
            view,
            ShellViewState::new(WindowAppearance::Dark),
            [true; QuickAction::ALL.len()],
        )
    }

    fn view_entries(view: Option<CanvasViewState>) -> Vec<MenuEntry> {
        view_menu_entries(
            view,
            ShellViewState::new(WindowAppearance::Dark),
            [true; QuickAction::ALL.len()],
        )
    }

    #[test]
    fn main_menu_has_the_five_required_sections_in_order() {
        let ids: Vec<_> = main_menu_schema(menu_state(2, None))
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
        let disabled: Vec<_> = main_menu_schema(menu_state(2, None))
            .into_iter()
            .flat_map(|section| section.entries)
            .filter(|entry| {
                matches!(
                    entry.command,
                    MenuCommand::Open
                        | MenuCommand::Undo
                        | MenuCommand::Redo
                        | MenuCommand::LineWeights
                        | MenuCommand::NewWindow
                        | MenuCommand::About
                        | MenuCommand::KeyboardShortcuts
                )
            })
            .filter_map(|entry| entry.availability.reason())
            .collect();

        assert!(!disabled.is_empty());
        assert!(disabled
            .iter()
            .all(|reason| reason.contains("M2") || reason.contains("M3")));
        assert!(disabled.iter().any(|reason| reason.contains("M3")));
        assert!(disabled.iter().any(|reason| reason.contains("P11")));
    }

    #[test]
    fn native_menus_are_derived_from_the_same_five_section_schema() {
        let state = menu_state(2, None);
        let expected_view_items = main_menu_schema(state)[2]
            .entries
            .iter()
            .filter(|entry| entry.availability.is_enabled())
            .count();
        let menus = native_menus(state);

        assert_eq!(menus.len(), 5);
        assert_eq!(menus[0].name.as_ref(), "File");
        assert_eq!(menus[1].name.as_ref(), "Edit");
        assert_eq!(menus[2].name.as_ref(), "View");
        assert_eq!(menus[3].name.as_ref(), "Window");
        assert_eq!(menus[4].name.as_ref(), "Help");
        assert_eq!(menus[0].items.len(), 3);
        assert_eq!(menus[2].items.len(), expected_view_items);
        assert!(menus[1].items.is_empty());
        assert!(menus[3..].iter().all(|menu| menu.items.is_empty()));
    }

    #[test]
    fn close_others_is_disabled_consistently_for_one_tab() {
        let file = &main_menu_schema(menu_state(1, None))[0];
        let close_others = file
            .entries
            .iter()
            .find(|entry| entry.command == MenuCommand::CloseOtherTabs)
            .unwrap();

        assert_eq!(
            close_others.availability,
            MenuAvailability::Disabled("No other tabs are open")
        );
        assert_eq!(native_menus(menu_state(1, None))[0].items.len(), 2);
    }

    #[test]
    fn hamburger_and_native_view_entries_read_the_same_canvas_state() {
        let state = menu_state(1, Some(view()));
        let entries = &main_menu_schema(state)[2].entries;
        let expected_labels: Vec<_> = entries
            .iter()
            .filter(|entry| entry.availability.is_enabled())
            .map(|entry| {
                if entry.selected {
                    format!("✓ {}", entry.label)
                } else {
                    entry.label.to_owned()
                }
            })
            .collect();
        let native_labels: Vec<_> = native_menus(state)[2]
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect();

        assert_eq!(native_labels, expected_labels);
        assert!(native_labels.contains(&"✓ Two Page".to_owned()));
        assert!(native_labels.contains(&"✓ Show Cover Page".to_owned()));
    }

    #[test]
    fn view_menu_commands_map_to_typed_canvas_actions() {
        let view = view();

        assert_eq!(
            MenuCommand::SinglePageContinuous.view_action(view),
            Some(ViewAction::SetLayout(PageLayoutMode::SinglePageContinuous))
        );
        assert_eq!(
            MenuCommand::TwoPageContinuous.view_action(view),
            Some(ViewAction::SetLayout(PageLayoutMode::TwoPageContinuous))
        );
        assert_eq!(
            MenuCommand::ToggleCover.view_action(view),
            Some(ViewAction::SetShowCover(false))
        );
        assert_eq!(MenuCommand::CloseTab.view_action(view), None);
    }

    #[test]
    fn zoom_policy_selects_the_same_actual_or_fit_choice_as_the_controls() {
        let mut actual = view();
        actual.zoom = 1.0;
        let actual_entries = view_entries(Some(actual));
        assert!(
            actual_entries
                .iter()
                .find(|entry| entry.command == MenuCommand::ActualSize)
                .unwrap()
                .selected
        );

        let mut fitted = view();
        fitted.zoom_policy = ZoomPolicy::Fit(FitMode::Width);
        let fitted_entries = view_entries(Some(fitted));
        assert!(
            fitted_entries
                .iter()
                .find(|entry| entry.command == MenuCommand::FitWidth)
                .unwrap()
                .selected
        );
        assert!(
            !fitted_entries
                .iter()
                .find(|entry| entry.command == MenuCommand::ActualSize)
                .unwrap()
                .selected
        );
    }

    #[test]
    fn shell_view_selections_and_native_labels_share_one_schema() {
        let mut shell_view = ShellViewState::new(WindowAppearance::Dark);
        shell_view.apply(ShellViewAction::ToggleNavigationPane);
        shell_view.apply(ShellViewAction::TogglePageControls);
        shell_view.apply(ShellViewAction::SetTheme(ThemePreference::Light));
        shell_view.apply(ShellViewAction::ToggleReadMode);
        shell_view.set_fullscreen(true);
        let mut quick_actions_visible = [true; QuickAction::ALL.len()];
        quick_actions_visible[QuickAction::Comment.index()] = false;
        let state = MenuState::new(1, Some(view()), shell_view, quick_actions_visible);
        let entries = &main_menu_schema(state)[2].entries;
        let native_labels: Vec<_> = native_menus(state)[2]
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect();

        for command in [
            MenuCommand::ToggleNavigationPane,
            MenuCommand::ToggleQuickAction(QuickAction::Comment),
            MenuCommand::TogglePageControls,
        ] {
            assert!(
                !entries
                    .iter()
                    .find(|entry| entry.command == command)
                    .unwrap()
                    .selected
            );
        }
        for command in [
            MenuCommand::ThemeLight,
            MenuCommand::ReadMode,
            MenuCommand::FullScreen,
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry.command == command)
                .unwrap();
            assert!(entry.selected);
            assert!(native_labels.contains(&format!("✓ {}", entry.label)));
        }
    }

    #[test]
    fn line_weights_is_disabled_until_m3_and_has_no_action_route() {
        let entry = view_entries(Some(view()))
            .into_iter()
            .find(|entry| entry.command == MenuCommand::LineWeights)
            .unwrap();

        assert_eq!(
            entry.availability,
            MenuAvailability::Disabled("Line Weights land in M3")
        );
        assert_eq!(entry.command.view_action(view()), None);
        assert_eq!(entry.command.shell_view_action(), None);
        assert!(native_menu_item(entry).is_none());
    }
}
