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

use super::{ShellFrame, TabCommand, TabError, CONVERT_PANEL_RIGHT, GLOBAL_BAR_HEIGHT};
use crate::preferences::{PreferenceCategory, ThemePreference};
use crate::shell::chrome::accessible::{Activation, Surface};
use crate::shell::chrome::combine_dialog::CombineEntryPoint;
use crate::shell::chrome::global_bar::{
    convert_section, main_menu_schema, save_as_other_section, MenuAvailability, MenuCommand,
    MenuSection, MenuState, RegistryFacts, NO_DYNAMIC_ZOOM_TOOL,
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
            MenuCommand::AutoScroll => {
                self.toggle_auto_scroll(cx);
                Ok(())
            }
            MenuCommand::ManageTools => {
                self.open_manage_tools(window, cx);
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
            MenuCommand::CombineFiles | MenuCommand::CreateFromFiles => {
                let entry_point = if command == MenuCommand::CombineFiles {
                    CombineEntryPoint::Combine
                } else {
                    CombineEntryPoint::CreateFromFiles
                };
                self.open_combine_dialog(entry_point, window, cx);
                Ok(())
            }
            MenuCommand::CreateFromFile => {
                self.create_from_file(cx);
                Ok(())
            }
            MenuCommand::CreateFromClipboard => {
                self.create_from_clipboard(cx);
                Ok(())
            }
            MenuCommand::Stamps => {
                self.dismiss_menus(cx);
                self.open_stamps_dialog(window, cx);
                Ok(())
            }
            // The registered command writes the comments-only summary beside
            // the document; the entry, live when that command is, opens the
            // dialog that offers both layouts and asks where.
            MenuCommand::SummarizeComments => {
                self.dismiss_menus(cx);
                self.open_summary_dialog(window, cx);
                Ok(())
            }
            MenuCommand::PasteStamp => {
                self.paste_stamp_from_menu(cx);
                Ok(())
            }
            MenuCommand::Properties => {
                self.open_properties_dialog(window, cx);
                Ok(())
            }
            MenuCommand::SaveAsOther => {
                self.toggle_menu_panel(MenuPanel::SaveAsOther, cx);
                Ok(())
            }
            MenuCommand::ExportAllImages => {
                self.export_all_images(cx);
                Ok(())
            }
            // The registered command splits at bookmarks with no questions;
            // the menu entry, live when that command is, opens the dialog that
            // offers every way to split.
            MenuCommand::ReduceFileSize => {
                self.dismiss_menus(cx);
                self.show_dialog(ShellDialog::ReduceFileSize, window, cx);
                Ok(())
            }
            MenuCommand::SplitDocument => {
                self.open_split_dialog(window, cx);
                Ok(())
            }
            MenuCommand::Print => {
                self.open_print_dialog(window, cx);
                Ok(())
            }
            MenuCommand::PageSetup => {
                self.open_page_setup(window, cx);
                Ok(())
            }
            MenuCommand::OrganizePages => {
                self.dismiss_menus(cx);
                self.toggle_organize(cx);
                Ok(())
            }
            MenuCommand::Skins => {
                self.dismiss_menus(cx);
                self.run_skins_action(crate::shell::skins::SkinsAction::Toggle, window, cx);
                Ok(())
            }
            MenuCommand::Page(page) => {
                self.dismiss_menus(cx);
                self.run_registry_command(page.id(), cx);
                Ok(())
            }
            MenuCommand::CloseTab => {
                let active = self.active_index()?;
                self.request_tab_command(TabCommand::Close, active, window, cx)
            }
            MenuCommand::CloseOtherTabs => {
                let active = self.active_index()?;
                self.request_tab_command(TabCommand::CloseOthers, active, window, cx)
            }
            MenuCommand::CloseAllTabs => {
                let active = self.active_index()?;
                self.request_tab_command(TabCommand::CloseAll, active, window, cx)
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
            MenuCommand::Save => {
                self.dismiss_menus(cx);
                self.save_active(cx);
                Ok(())
            }
            MenuCommand::SaveAs => {
                self.dismiss_menus(cx);
                self.save_active_as(cx);
                Ok(())
            }
            MenuCommand::Revert => {
                self.dismiss_menus(cx);
                self.revert_active(cx);
                Ok(())
            }
            MenuCommand::AttachToEmail => {
                self.dismiss_menus(cx);
                self.attach_active_to_email(cx);
                Ok(())
            }
            MenuCommand::CopyFileToClipboard => {
                self.dismiss_menus(cx);
                self.copy_active_file_to_clipboard(cx);
                Ok(())
            }
            MenuCommand::Edit(verb) => {
                self.dismiss_menus(cx);
                self.run_edit_verb(verb, cx);
                Ok(())
            }
            MenuCommand::Undo => {
                self.dismiss_menus(cx);
                self.undo_active(cx);
                Ok(())
            }
            MenuCommand::Redo => {
                self.dismiss_menus(cx);
                self.redo_active(cx);
                Ok(())
            }
            MenuCommand::LineWeights | MenuCommand::NewWindow => Err(TabError::CommandUnavailable),
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
        .with_history(
            self.tabs
                .active()
                .map(|tab| tab.canvas.read(cx).model.history_facts()),
        )
        .with_edit_verbs(self.edit_verb_availability(cx))
    }

    /// What each Edit verb does now: what the active tool answers.
    fn edit_verb_availability(&self, cx: &App) -> [MenuAvailability; 4] {
        onionskin_plugin_api::EditVerb::ALL.map(|verb| match self.tabs.active() {
            None => MenuAvailability::Disabled("No document is open"),
            Some(tab) => match tab.canvas.read(cx).model.edit_verb_availability(verb) {
                Ok(()) => MenuAvailability::Enabled,
                Err(reason) => MenuAvailability::Disabled(reason),
            },
        })
    }

    /// What the active document's registry answers about the entries that
    /// ask it: which export codecs, which commands, which capabilities.
    /// Derived from the registry rather than from a hardcoded list, so a
    /// build with a plugin compiled out disables its entries with a reason
    /// instead of offering entries that would fail.
    pub(super) fn registry_facts(&self, cx: &App) -> RegistryFacts {
        match self.tabs.active() {
            Some(tab) => {
                let model = &tab.canvas.read(cx).model;
                RegistryFacts::of(model.registry())
                    .refusing_edits(model.edit_refusal())
                    .refusing_read_out(model.refusals().read_out)
            }
            // With no document there is no tab registry to ask, and the
            // entries still have to say whether their plugin is installed.
            None => self.settings.registry,
        }
    }

    pub(super) fn toggle_main_menu(&mut self, cx: &mut Context<Self>) {
        self.toggle_menu_panel(MenuPanel::Main, cx);
    }

    /// The global bar's Convert button.
    pub(super) fn toggle_convert_menu(&mut self, cx: &mut Context<Self>) {
        self.toggle_menu_panel(MenuPanel::Convert, cx);
    }

    /// Open `panel`, or close it if it is the one showing.
    fn toggle_menu_panel(&mut self, panel: MenuPanel, cx: &mut Context<Self>) {
        self.menus.main_menu_open = !self.menus.showing(panel);
        self.menus.panel = panel;
        self.menus.recent_menu_open = false;
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

    /// The sections the menu panel shows: the whole main menu, or Convert.
    pub(super) fn menu_panel_sections(&self, cx: &App) -> Vec<MenuSection> {
        let state = self.menu_state(cx);
        match self.menus.panel {
            MenuPanel::Main => main_menu_schema(state),
            MenuPanel::Convert => vec![convert_section(state)],
            MenuPanel::SaveAsOther => vec![save_as_other_section(state)],
        }
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
            .map(|panel| match self.menus.panel {
                MenuPanel::Main | MenuPanel::SaveAsOther => panel.left(px(8.0)),
                // Under the Convert button, which sits beside the search
                // field at the bar's right.
                MenuPanel::Convert => panel.right(px(CONVERT_PANEL_RIGHT)),
            })
            .w(px(420.0))
            .max_h(max_height)
            .overflow_y_scroll()
            .p_2()
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text);

        let mut menu_entry_index = 0_usize;
        for section in self.menu_panel_sections(cx) {
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
    /// Whether the menu panel is showing; `panel` says which.
    pub(super) main_menu_open: bool,
    pub(super) recent_menu_open: bool,
    pub(super) panel: MenuPanel,
}

/// What the menu panel shows: the main menu, or the global bar's Convert
/// entry point. One panel rather than two, so everything that closes the main
/// menu - Escape, a click elsewhere, running an entry - closes Convert too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum MenuPanel {
    #[default]
    Main,
    Convert,
    /// File > Save as Other's formats, in place of the main menu.
    SaveAsOther,
}

impl MenuOpenState {
    /// Whether `panel` is the one showing.
    pub(super) fn showing(&self, panel: MenuPanel) -> bool {
        self.main_menu_open && self.panel == panel
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "shell-test-support")]
    use super::super::tests::{bound_window, bound_window_in, keystroke_for};
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use super::super::ToolCapability;
    #[cfg(feature = "shell-test-support")]
    use super::super::{px, Canvas, Document, MenuCommand, ShellFrame, ShellViewAction, ViewSize};
    #[cfg(feature = "shell-test-support")]
    use crate::preferences::ThemePreference;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::canvas::CanvasModel;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::global_bar::main_menu_schema;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::theme::ShellViewState;
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use crate::shell::context_menu::tool_with;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::dialog::ShellDialog;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::ShellSettings;
    #[cfg(feature = "shell-test-support")]
    use gpui::{AppContext as _, TestAppContext};
    #[cfg(feature = "shell-test-support")]
    use onionskin_plugin_api::PluginRegistry;
    #[cfg(feature = "shell-test-support")]
    use std::path::Path;

    /// The headline binding, dispatched the way the user dispatches it, and
    /// with the keystroke the keymap actually installed.
    ///
    /// Everything else about the find bar was tested by calling its methods,
    /// which is how two separate dead routes shipped: an element listener that
    /// action dispatch never reached, and then a window-wide listener that ran
    /// inside the dispatching window's own update and could not find it.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_find_keystroke_opens_the_find_bar(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);
        window
            .update(cx, |frame, _window, _cx| assert!(!frame.find.is_open()))
            .unwrap();

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "edit.find"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| {
                assert!(
                    frame.find.is_open(),
                    "the find keystroke did not reach the find bar in a real window"
                );
            })
            .unwrap();
    }

    /// The route the close and view commands used to take had the same
    /// latent shape as the find bar's did: a global listener calling back
    /// into the window that is mid-update. Both are pressed here rather than
    /// called, because calling the handler is exactly what missed it twice.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_close_keystroke_closes_the_active_tab(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf", "two-page.pdf"], cx);

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "file.close"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| {
                assert_eq!(
                    frame
                        .tabs
                        .tabs()
                        .iter()
                        .map(|tab| tab.title().to_owned())
                        .collect::<Vec<_>>(),
                    vec!["two-page.pdf".to_owned()],
                    "the close keystroke did not reach the window"
                );
            })
            .unwrap();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_view_keystroke_reaches_the_canvas(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);
        window
            .update(cx, |frame, _window, cx| {
                assert!(!frame.active_view_state(cx).unwrap().is_actual_size());
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "view.actual-size"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                assert!(
                    frame.active_view_state(cx).unwrap().is_actual_size(),
                    "the zoom keystroke did not reach the canvas"
                );
            })
            .unwrap();
    }

    /// Fit Visible is the one zoom command that needs something from the
    /// rendered page, so its route runs further than the others': keystroke,
    /// action listener, deferred window update, canvas, raster. Pressed
    /// rather than called, for the same reason every other route here is.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_fit_visible_keystroke_fits_the_pages_marks(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();
        window
            .update(cx, |frame, _window, cx| {
                frame
                    .tabs
                    .active()
                    .expect("the seed is open")
                    .canvas
                    .update(cx, |canvas, _cx| {
                        // The worker may already have answered the visible
                        // page; only stand in for it when it has not.
                        if !canvas.model.has_rendered_current_page_for_test() {
                            canvas
                                .model
                                .seed_visible_raster_for_test([0, 0, 0, 255])
                                .expect("the visible page takes a raster");
                        }
                    });
                assert!(frame.active_view_state(cx).unwrap().fit_mode().is_some());
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "view.fit-visible"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                let fit = frame.active_view_state(cx).unwrap().fit_mode();
                assert!(
                    matches!(fit, Some(onionskin_core::FitMode::Visible(_))),
                    "the fit visible keystroke did not reach the canvas: {fit:?}"
                );
            })
            .unwrap();
    }

    /// Dynamic Zoom is a drag, so its menu entry selects a tool rather than
    /// changing the view. Pressed on a real window through the binding a
    /// user would give it, and asserted on the tool the canvas ends up with.
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    #[gpui::test]
    fn a_keymap_binding_selects_the_dynamic_zoom_tool(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("dynamic-zoom-keymap");
        std::fs::write(
            dir.join(crate::config::KEYMAP_FILE),
            "{\"view.dynamic-zoom\": \"cmd-shift-y\"}",
        )
        .expect("the test writes its keymap");
        let (window, bindings) =
            bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::in_dir(&dir), cx);
        let dynamic_zoom = window
            .update(cx, |frame, _window, cx| {
                let canvas = frame
                    .tabs
                    .active()
                    .expect("the seed is open")
                    .canvas
                    .read(cx);
                let index = tool_with(canvas.model.registry(), ToolCapability::DynamicZoom)
                    .expect("tools-basic registers a dynamic zoom tool");
                assert_ne!(canvas.model.active_tool(), Some(index));
                index
            })
            .unwrap();

        cx.simulate_keystrokes(
            window.into(),
            &keystroke_for(&bindings, "view.dynamic-zoom"),
        );
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                let canvas = frame
                    .tabs
                    .active()
                    .expect("the seed is open")
                    .canvas
                    .read(cx);
                assert_eq!(
                    canvas.model.active_tool(),
                    Some(dynamic_zoom),
                    "the dynamic zoom keystroke did not select the tool"
                );
                assert!(frame.notices.is_empty(), "{:?}", frame.notices);
            })
            .unwrap();
    }

    /// Zoom To ships unbound because Acrobat's Ctrl+M is Minimize on macOS,
    /// so its keystroke route is the one `keymap.json` gives it. Pressed on a
    /// real window, which proves both halves at once: the file's binding
    /// reaches the command, and the command reaches the dialog.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_keymap_binding_opens_the_zoom_to_dialog(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("zoom-to-keymap");
        std::fs::write(
            dir.join(crate::config::KEYMAP_FILE),
            r#"{"view.zoom-to": "cmd-m"}"#,
        )
        .expect("the test writes its keymap");
        let (window, bindings) =
            bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::in_dir(&dir), cx);
        window
            .update(cx, |frame, _window, _cx| assert!(frame.dialog.is_none()))
            .unwrap();

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "view.zoom-to"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| {
                assert_eq!(
                    frame.dialog,
                    Some(ShellDialog::ZoomTo),
                    "the bound keystroke did not open the magnification chooser"
                );
            })
            .unwrap();
    }

    /// Quit runs inside the same deferred window update every other command
    /// does, and it is the one that tears the application down while it is
    /// there. Pressed rather than called, because that is the route.
    ///
    /// Named for what it asserts: a test context has no process to end, so
    /// this says the update quit ran in did not leave the frame unusable,
    /// and nothing more. Making the Quit arm a no-op leaves it green.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_quit_keystroke_does_not_leave_the_frame_mid_flight(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "file.quit"));
        cx.run_until_parked();

        // The window is still addressable in a test context, which is what
        // says the update that quit did not leave the frame mid-flight.
        window
            .update(cx, |frame, _window, _cx| {
                assert_eq!(frame.tabs.tabs().len(), 1);
            })
            .expect("quitting left the window in a state it can be read in");
    }

    /// A keystroke can reach a command the menus grey out, and telling the
    /// user only on stderr tells them nothing.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_keystroke_the_menus_would_grey_out_says_so_in_the_window(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);
        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "file.close"));
        cx.run_until_parked();

        // No document now, so the zoom command the keystroke reaches is one
        // the menus disable.
        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "view.actual-size"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| {
                let notices = frame.notices.join(" | ");
                assert!(notices.contains("view.actual-size"), "{notices}");
                assert!(notices.contains("No document is open"), "{notices}");
            })
            .unwrap();
    }

    /// Select All is a plugin's command reached by a keystroke: the keymap
    /// binds the id the plugin published, the menu entry names the same id,
    /// and the shell runs whatever the registry holds for it.
    #[cfg(all(feature = "shell-test-support", feature = "commands-core"))]
    #[gpui::test]
    fn the_select_all_keystroke_runs_the_registered_command(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "edit.select-all"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                let selected = frame
                    .tabs
                    .active()
                    .unwrap()
                    .canvas
                    .read(cx)
                    .model
                    .selection_text()
                    .map(str::to_owned);
                assert!(
                    selected.is_some_and(|text| !text.is_empty()),
                    "the Select All keystroke selected nothing"
                );
            })
            .unwrap();

        cx.simulate_keystrokes(
            window.into(),
            &keystroke_for(&bindings, "edit.deselect-all"),
        );
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                assert_eq!(
                    frame
                        .tabs
                        .active()
                        .unwrap()
                        .canvas
                        .read(cx)
                        .model
                        .selection_text(),
                    None
                );
            })
            .unwrap();
    }

    /// The display theme is one setting with two ways in. Choosing it from
    /// the View menu used to change the window and nothing else, so it was
    /// gone on restart and the dialog showed the wrong one.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_view_menus_theme_is_the_preference(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("theme-from-menu");
        let file = dir.join(crate::config::PREFERENCES_FILE);
        let _ = std::fs::remove_file(&file);
        let (window, _) =
            bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::in_dir(&dir), cx);

        window
            .update(cx, |frame, window, cx| {
                frame
                    .run_main_menu_command(MenuCommand::ThemeDark, window, cx)
                    .expect("the theme entry is live");

                assert_eq!(frame.shell_view_state.theme(), ThemePreference::Dark);
                assert_eq!(frame.preferences().theme, ThemePreference::Dark);
            })
            .unwrap();

        let (saved, errors) = crate::preferences::Preferences::load(Some(&file));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            saved.theme,
            ThemePreference::Dark,
            "the View menu's choice did not reach the file"
        );
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn fullscreen_command_updates_the_real_window_and_mirrored_menu_state(cx: &mut TestAppContext) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let document = Document::open_path(&path).unwrap();
        let model = CanvasModel::new(
            document,
            PluginRegistry::new(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
        )
        .unwrap();
        let mut shell_view =
            ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System);
        shell_view.apply(ShellViewAction::ToggleReadMode);
        let theme = shell_view.tokens();
        let (frame, cx) = cx.add_window_view(move |window, cx| {
            let canvas = cx.new(|_| Canvas::new(model, theme));
            ShellFrame::new(
                vec![(path, canvas)],
                shell_view,
                ShellSettings::defaults(),
                window,
                cx,
            )
        });

        cx.update(|window, app| {
            frame.update(app, |frame, cx| {
                frame
                    .run_main_menu_command(MenuCommand::FullScreen, window, cx)
                    .unwrap();
            });
        });
        cx.simulate_resize(gpui::size(px(900.0), px(700.0)));
        cx.run_until_parked();

        let (window_fullscreen, state, read_mode) = cx.update(|window, app| {
            let state = frame.read(app).menu_state(app);
            (
                window.is_fullscreen(),
                state,
                frame.read(app).shell_view_state.read_mode(),
            )
        });
        let full_screen = main_menu_schema(state)[2]
            .entries
            .iter()
            .find(|entry| entry.command == MenuCommand::FullScreen)
            .copied()
            .unwrap();
        assert!(window_fullscreen);
        assert!(full_screen.selected);
        assert!(read_mode);

        cx.update(|window, app| {
            frame.update(app, |frame, cx| {
                frame
                    .run_main_menu_command(MenuCommand::FullScreen, window, cx)
                    .unwrap();
            });
        });
        cx.simulate_resize(gpui::size(px(800.0), px(600.0)));
        cx.run_until_parked();
        cx.update(|window, app| {
            assert!(!window.is_fullscreen());
            assert!(!frame.read(app).shell_view_state.fullscreen());
            assert!(frame.read(app).shell_view_state.read_mode());
        });
    }
}
