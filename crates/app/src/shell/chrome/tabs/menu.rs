//! The main menu: what it offers, what greys an entry out, and what running
//! one does.
//!
//! `run_main_menu_command` is the extension point an M3 package appends its
//! arm to. The match has no wildcard, so an entry added to `MenuCommand`
//! without a decision here is a compile error.
//!
//! Moved out of `tabs/mod.rs` unchanged. `pub(super)` here reaches
//! `chrome::tabs` and everything under it, which is the scope these items had
//! while they were private items of `chrome::tabs`; `run_native_command` was
//! already `pub(super)` in `chrome::tabs` and is spelled
//! `pub(in crate::shell::chrome)` to keep the reach it had.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, WindowHandle,
};
use onionskin_plugin_api::ToolCapability;

use super::{ShellFrame, TabCommand, TabError, GLOBAL_BAR_HEIGHT};
use crate::preferences::{PreferenceCategory, ThemePreference};
use crate::shell::chrome::accessible::{Activation, Surface};
use crate::shell::chrome::global_bar::{
    main_menu_schema, MenuCommand, MenuState, RegistryFacts, NO_DYNAMIC_ZOOM_TOOL,
};
use crate::shell::chrome::tool_search::SearchSelectAll;
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    /// Run a command that arrived from a keystroke or a native menu item.
    ///
    /// A refusal goes on the notice bar as well as to stderr: a keystroke
    /// can reach a command the menus grey out, and the user pressing it is
    /// looking at the window, not at a terminal.
    pub(in crate::shell::chrome) fn run_native_command(
        window_handle: WindowHandle<Self>,
        command: MenuCommand,
        cx: &mut App,
    ) {
        let result = window_handle.update(cx, |frame, window, cx| {
            match frame.run_main_menu_command(command, window, cx) {
                Ok(()) => Ok(()),
                Err(error) => {
                    let message = format!("{}: {error}", command.id());
                    frame.notices.push(message.clone());
                    cx.notify();
                    Err(message)
                }
            }
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) => eprintln!("onionskin: {message}"),
            Err(error) => eprintln!("onionskin: cannot run menu command: {error}"),
        }
    }

    /// The reason the menus would grey `command` out, if they would.
    ///
    /// One rule for both routes: a keystroke reaches exactly the commands a
    /// click on the menu entry reaches. Without this, cmd-1 with no document
    /// open would run a view command against a viewport that is not there.
    pub(super) fn command_unavailable(
        &self,
        command: MenuCommand,
        cx: &App,
    ) -> Option<&'static str> {
        main_menu_schema(self.menu_state(cx))
            .into_iter()
            .flat_map(|section| section.entries)
            .find(|entry| entry.command == command)
            .and_then(|entry| entry.availability.reason())
    }

    pub(super) fn run_main_menu_command(
        &mut self,
        command: MenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), TabError> {
        if let Some(reason) = self.command_unavailable(command, cx) {
            return Err(TabError::Unavailable(reason));
        }
        match command {
            MenuCommand::Open => {
                self.dismiss_menus(cx);
                self.prompt_for_documents(cx);
                Ok(())
            }
            MenuCommand::OpenRecent => {
                self.menus.main_menu_open = false;
                self.menus.recent_menu_open = !self.menus.recent_menu_open;
                cx.notify();
                Ok(())
            }
            MenuCommand::Quit => {
                cx.quit();
                Ok(())
            }
            MenuCommand::Preferences => {
                self.show_preferences(PreferenceCategory::General, window, cx);
                Ok(())
            }
            MenuCommand::About => {
                self.show_dialog(ShellDialog::About, window, cx);
                Ok(())
            }
            MenuCommand::KeyboardShortcuts => {
                self.show_dialog(ShellDialog::KeyboardShortcuts, window, cx);
                Ok(())
            }
            MenuCommand::ZoomTo => {
                self.menus.main_menu_open = false;
                self.show_dialog(ShellDialog::ZoomTo, window, cx);
                Ok(())
            }
            // A drag, not a command: the entry selects the tool, which is
            // what Acrobat's View > Zoom > Dynamic Zoom does too.
            MenuCommand::DynamicZoom => {
                self.dismiss_menus(cx);
                self.activate_tool_with(
                    ToolCapability::DynamicZoom,
                    "Dynamic Zoom",
                    NO_DYNAMIC_ZOOM_TOOL,
                    cx,
                );
                Ok(())
            }
            MenuCommand::Tools => {
                self.dismiss_menus(cx);
                self.toggle_rail_expanded(cx);
                Ok(())
            }
            MenuCommand::SelectAll | MenuCommand::DeselectAll => {
                self.dismiss_menus(cx);
                if command == MenuCommand::SelectAll {
                    let focused_input = self
                        .focused_text_field(window, cx)
                        .is_some_and(|key| self.accessible(window, cx).find(&key).is_some());
                    if focused_input {
                        window.dispatch_action(Box::new(SearchSelectAll), cx);
                        return Ok(());
                    }
                    if self.dialog.is_some() {
                        return Ok(());
                    }
                }
                let id = command
                    .registry_command_id()
                    .expect("both entries name a registry command");
                self.run_registry_command(id, cx);
                Ok(())
            }
            MenuCommand::TakeSnapshot => {
                self.dismiss_menus(cx);
                self.take_a_snapshot(cx);
                Ok(())
            }
            MenuCommand::CloseTab => {
                let active = self.active_index()?;
                self.run_tab_command(TabCommand::Close, active, cx)
            }
            MenuCommand::CloseOtherTabs => {
                let active = self.active_index()?;
                self.run_tab_command(TabCommand::CloseOthers, active, cx)
            }
            MenuCommand::CloseAllTabs => {
                let active = self.active_index()?;
                self.run_tab_command(TabCommand::CloseAll, active, cx)
            }
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
            | MenuCommand::FitVisible
            | MenuCommand::SinglePage
            | MenuCommand::SinglePageContinuous
            | MenuCommand::TwoPage
            | MenuCommand::TwoPageContinuous
            | MenuCommand::ToggleCover => {
                let Some(view) = self.active_view_state(cx) else {
                    return Err(TabError::Unavailable("No document is open"));
                };
                self.menus.main_menu_open = false;
                self.run_view_action(
                    command
                        .view_action(view)
                        .expect("view menu commands map to canvas actions"),
                    cx,
                );
                Ok(())
            }
            MenuCommand::ToggleQuickAction(action) => {
                self.menus.main_menu_open = false;
                self.toggle_quick_action_visibility(action, cx);
                Ok(())
            }
            // Through the preference rather than straight at the view state:
            // the display theme is a setting, so choosing it in the View
            // menu has to survive a restart and has to be what the
            // Preferences dialog shows.
            MenuCommand::ThemeSystem => {
                self.set_theme(ThemePreference::System, cx);
                Ok(())
            }
            MenuCommand::ThemeLight => {
                self.set_theme(ThemePreference::Light, cx);
                Ok(())
            }
            MenuCommand::ThemeDark => {
                self.set_theme(ThemePreference::Dark, cx);
                Ok(())
            }
            command @ (MenuCommand::ToggleNavigationPane
            | MenuCommand::TogglePageControls
            | MenuCommand::ReadMode) => {
                self.menus.main_menu_open = false;
                self.run_shell_view_action(
                    command
                        .shell_view_action()
                        .expect("shell view commands map to shell actions"),
                    cx,
                );
                Ok(())
            }
            MenuCommand::FullScreen => {
                self.menus.main_menu_open = false;
                self.toggle_fullscreen(window, cx);
                Ok(())
            }
            MenuCommand::Export(target) => {
                self.menus.main_menu_open = false;
                self.start_export(target, window, cx);
                Ok(())
            }
            MenuCommand::Find => {
                self.open_find_bar(None, window, cx);
                Ok(())
            }
            // Disabled in the schema for a milestone rather than for a
            // state, so the check at the top of this function returns first.
            // Kept as a loud answer in case an entry is ever enabled before
            // the thing behind it exists.
            MenuCommand::SaveAs
            | MenuCommand::Undo
            | MenuCommand::Redo
            | MenuCommand::LineWeights
            | MenuCommand::NewWindow => Err(TabError::CommandUnavailable),
        }
    }

    pub(super) fn menu_state(&self, cx: &App) -> MenuState {
        MenuState::new(
            self.tabs.tabs().len(),
            self.active_view_state(cx),
            self.shell_view_state,
            self.quick_actions_state.visibility(),
            self.registry_facts(cx),
            self.settings.recents.documents().len(),
        )
    }

    /// What the active document's registry answers about the entries that
    /// ask it: which export codecs, which commands, which capabilities.
    /// Derived from the registry rather than from a hardcoded list, so a
    /// build with a plugin compiled out disables its entries with a reason
    /// instead of offering entries that would fail.
    pub(super) fn registry_facts(&self, cx: &App) -> RegistryFacts {
        match self.tabs.active() {
            Some(tab) => RegistryFacts::of(tab.canvas.read(cx).model.registry()),
            // With no document there is no tab registry to ask, and the
            // entries still have to say whether their plugin is installed.
            None => self.settings.registry,
        }
    }

    pub(super) fn toggle_main_menu(&mut self, cx: &mut Context<Self>) {
        self.menus.main_menu_open = !self.menus.main_menu_open;
        self.context_menus.tab_context_menu = None;
        self.context_menus.canvas_context_menu = None;
        cx.notify();
    }

    /// File > Open Recent, as a flyout rather than a submenu: the entries are
    /// file names, and the menu schema carries static labels.
    pub(super) fn render_recent_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let mut panel = div()
            .id("recent-menu")
            .absolute()
            .top(px(GLOBAL_BAR_HEIGHT))
            .left(px(8.0))
            .w(px(420.0))
            .max_h(px(400.0))
            .overflow_y_scroll()
            .p_2()
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text)
            .occlude();
        for (index, recent) in self.settings.recents.documents().iter().enumerate() {
            let title = recent.title();
            let path = recent.display_path(self.settings.paths.home.as_deref());
            panel = panel.child(
                div()
                    .id(("recent-entry", index))
                    .min_h(px(30.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |row| row.bg(theme.selected))
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        frame.open_recent(index, cx);
                    }))
                    .child(div().flex_none().child(title))
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .text_xs()
                            .text_color(theme.muted_text)
                            .child(path),
                    ),
            );
        }
        panel
    }

    pub(super) fn dismiss_menus(&mut self, cx: &mut Context<Self>) {
        self.menus.main_menu_open = false;
        self.menus.recent_menu_open = false;
        self.context_menus.tab_context_menu = None;
        self.context_menus.canvas_context_menu = None;
        cx.notify();
    }

    pub(super) fn render_main_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let max_height = (window.viewport_size().height - px(GLOBAL_BAR_HEIGHT + 8.0)).max(px(0.0));
        let rects = self.a11y.rects.clone();
        let mut panel = div()
            .on_children_prepainted(move |bounds, window, _cx| {
                rects.record(Surface::MainMenu, &bounds, window);
            })
            .id("main-menu-panel")
            .absolute()
            .top(px(GLOBAL_BAR_HEIGHT))
            .left(px(8.0))
            .w(px(420.0))
            .max_h(max_height)
            .overflow_y_scroll()
            .p_2()
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text);

        let mut menu_entry_index = 0_usize;
        for section in main_menu_schema(self.menu_state(cx)) {
            panel = panel.child(
                div()
                    .mt_2()
                    .px_2()
                    .text_xs()
                    .text_color(theme.secondary_text)
                    .child(section.id.label()),
            );
            for entry in section.entries {
                let row_index = menu_entry_index;
                menu_entry_index += 1;
                let command = entry.command;
                let enabled = entry.availability.is_enabled();
                let reason = entry.availability.reason();
                panel = panel.child(
                    div()
                        .id(("main-menu-entry", row_index))
                        .min_h(px(30.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .rounded_sm()
                        .text_color(if enabled {
                            theme.text
                        } else {
                            theme.disabled_text
                        })
                        .when(enabled, |row| {
                            row.cursor_pointer()
                                .hover(move |row| row.bg(theme.selected))
                        })
                        .on_click(cx.listener(move |frame, _event, window, cx| {
                            if enabled {
                                frame.run_activation(Activation::MainMenu(command), window, cx);
                            }
                        }))
                        .child(div().flex_none().child(if entry.selected {
                            format!("✓ {}", entry.label)
                        } else {
                            entry.label.to_owned()
                        }))
                        .when_some(reason, |row, reason| {
                            row.child(
                                div()
                                    .ml_3()
                                    .flex_1()
                                    .text_right()
                                    .text_xs()
                                    .text_color(theme.muted_text)
                                    .child(reason),
                            )
                        }),
                );
            }
        }
        panel
    }
}

/// Which of the frame's own menus is showing. Two booleans and not one enum
/// because the recents submenu opens inside the main menu rather than instead
/// of it.
///
/// `MenuState` next door is a different thing: the availability snapshot the
/// schema is built from, which is derived rather than stored.
#[derive(Default)]
pub(super) struct MenuOpenState {
    pub(super) main_menu_open: bool,
    pub(super) recent_menu_open: bool,
}
