//! What the frame shows the user outside the document: the modal dialogs, the
//! preferences behind them, and the notice bar.
//!
//! Moved out of `tabs/mod.rs` unchanged. `pub(super)` here reaches
//! `chrome::tabs` and everything under it, which is the scope these items had
//! while they were private items of `chrome::tabs`; the ones that were already
//! `pub(in crate::shell)` keep that spelling, which does not depend on where
//! the item sits.

use gpui::{
    div, App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window,
};

use super::ShellFrame;
use crate::preferences::{PreferenceCategory, Preferences, ThemePreference};
use crate::shell::chrome::global_bar::{main_menu_schema, refresh_native_menus};
use crate::shell::chrome::theme::ShellViewAction;
use crate::shell::dialog::ShellDialog;
use crate::shell::preferences_dialog::PreferenceChange;

impl ShellFrame {
    pub(in crate::shell) fn dialog_focus(&self) -> Option<&gpui::ElementId> {
        self.a11y.focused()
    }

    pub(in crate::shell) fn preferences(&self) -> &Preferences {
        &self.settings.preferences
    }

    pub(super) fn set_theme(&mut self, theme: ThemePreference, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        self.change_preference(PreferenceChange::Theme(theme), cx);
    }

    pub(in crate::shell) fn show_preferences(
        &mut self,
        category: PreferenceCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_dialog(ShellDialog::Preferences(category), window, cx);
    }

    pub(super) fn show_dialog(
        &mut self,
        dialog: ShellDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_dialog(window, cx);
        self.dismiss_menus(cx);
        self.dialog = Some(dialog);
        cx.notify();
    }

    pub(in crate::shell) fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(self.a11y.focus_handle());
        self.dialog = None;
        self.export.dialog = None;
        self.organize = Default::default();
        self.stamps = None;
        self.summary = None;
        self.properties = None;
        self.bookmark_title = None;
        self.unsaved = None;
        self.recover = None;
        cx.notify();
    }

    /// Apply a preference, save it, and let whatever it changes see it.
    ///
    /// Saved on every change rather than on closing the dialog: there is no
    /// OK button to press, so a change the user made is a change they meant.
    pub(in crate::shell) fn change_preference(
        &mut self,
        change: PreferenceChange,
        cx: &mut Context<Self>,
    ) {
        let preferences = &mut self.settings.preferences;
        match change {
            PreferenceChange::Theme(theme) => {
                preferences.theme = theme;
                self.apply_shell_view_action(ShellViewAction::SetTheme(theme), cx);
            }
            PreferenceChange::RecentDocuments(count) => {
                preferences.recent_documents = count;
                self.settings.recents.truncate(count);
                if let Some(path) = self.settings.paths.recents.as_deref() {
                    if let Err(error) = self.settings.recents.save(path) {
                        self.notices.push(error.to_string());
                    }
                }
            }
            PreferenceChange::Layout(layout) => preferences.layout = layout,
            PreferenceChange::Zoom(zoom) => preferences.zoom = zoom,
            PreferenceChange::SearchCaseSensitive(on) => preferences.search.case_sensitive = on,
            PreferenceChange::SearchWholeWord(on) => preferences.search.whole_word = on,
            PreferenceChange::SearchMode(mode) => preferences.search.mode = mode,
        }
        if let Some(path) = self.settings.paths.preferences.as_deref() {
            if let Err(error) = self.settings.preferences.save(path) {
                self.notices.push(error.to_string());
            }
        }
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    /// One row per keystroke in force, for the Help menu's local reference.
    pub(in crate::shell) fn shortcut_rows(&self, cx: &App) -> Vec<(String, String)> {
        let schema = main_menu_schema(self.menu_state(cx));
        let registry_titles: Vec<(&str, &str)> = self
            .tabs
            .active()
            .map(|tab| {
                tab.canvas
                    .read(cx)
                    .model
                    .registry()
                    .commands()
                    .iter()
                    .map(|command| (command.id, command.title))
                    .collect()
            })
            .unwrap_or_default();
        crate::shell::dialog::shortcut_rows(&self.settings.bindings, |id| {
            schema
                .iter()
                .flat_map(|section| &section.entries)
                .find(|entry| entry.command.id() == id)
                .map(|entry| entry.label.to_owned())
                .or_else(|| {
                    registry_titles
                        .iter()
                        .find(|(known, _)| *known == id)
                        .map(|(_, title)| (*title).to_owned())
                })
                .unwrap_or_else(|| id.to_owned())
        })
    }

    pub(in crate::shell) fn dismiss_notice(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.notices.len() {
            self.notices.remove(index);
            cx.notify();
        }
    }

    /// The notices waiting to be read, newest last.
    pub(super) fn render_notices(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let mut column = div().flex().flex_col();
        for (index, notice) in self.notices.iter().enumerate() {
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .bg(theme.error_surface)
                    .text_color(theme.error_text)
                    .child(div().flex_1().child(notice.clone()))
                    .child(
                        div()
                            .id(("notice-dismiss", index))
                            .px_2()
                            .cursor_pointer()
                            .rounded_sm()
                            .hover(move |button| button.bg(theme.subtle_hover))
                            .on_click(cx.listener(move |frame, _event, _window, cx| {
                                frame.dismiss_notice(index, cx);
                            }))
                            .child("Dismiss"),
                    ),
            );
        }
        column
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "shell-test-support")]
    use super::super::tests::{bound_window, bound_window_in, keystroke_for};
    #[cfg(feature = "shell-test-support")]
    use super::super::{Activation, MenuCommand, ViewAction};
    #[cfg(feature = "shell-test-support")]
    use crate::preferences::{PreferenceCategory, ThemePreference};
    #[cfg(feature = "shell-test-support")]
    use crate::shell::dialog::ShellDialog;
    #[cfg(feature = "shell-test-support")]
    use gpui::TestAppContext;
    #[cfg(feature = "shell-test-support")]
    use std::path::Path;

    /// The dialog is a chooser, so picking a magnification has to both apply
    /// it and put the dialog away. Driven through `run_activation`, which is
    /// the one route a click and a screen reader share.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn choosing_a_magnification_applies_it_and_closes_the_chooser(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        window
            .update(cx, |frame, window, cx| {
                frame.run_activation(Activation::MainMenu(MenuCommand::ZoomTo), window, cx);
                assert_eq!(frame.dialog, Some(ShellDialog::ZoomTo));
                frame.run_activation(Activation::View(ViewAction::ZoomTo(2.0)), window, cx);
            })
            .unwrap();

        window
            .update(cx, |frame, _window, cx| {
                assert!(frame.dialog.is_none(), "the chooser stayed up");
                let view = frame.active_view_state(cx).expect("the seed is open");
                assert!((view.zoom - 2.0).abs() < 1e-4, "{}", view.zoom);
                assert_eq!(view.zoom_policy, onionskin_core::ZoomPolicy::Fixed);
            })
            .unwrap();
    }

    /// A preference has to reach three places: the state the window paints
    /// from, the file, and the menu that shows which one is in force.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_preference_change_reaches_the_window_and_the_file(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("preferences-frame");
        let file = dir.join(crate::config::PREFERENCES_FILE);
        let _ = std::fs::remove_file(&file);
        let (window, _) =
            bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::in_dir(&dir), cx);

        window
            .update(cx, |frame, _window, cx| {
                frame.change_preference(
                    crate::shell::preferences_dialog::PreferenceChange::Theme(
                        ThemePreference::Light,
                    ),
                    cx,
                );
                assert_eq!(frame.shell_view_state.theme(), ThemePreference::Light);
                assert_eq!(frame.preferences().theme, ThemePreference::Light);
            })
            .unwrap();

        let (saved, errors) = crate::preferences::Preferences::load(Some(&file));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(saved.theme, ThemePreference::Light);
    }

    /// Shortening the list in Preferences shortens it now, not next launch.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn lowering_the_recents_limit_drops_the_extra_entries(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("recents-limit");
        let _ = std::fs::remove_file(dir.join(crate::config::RECENTS_FILE));
        let (window, _) = bound_window_in(&[], crate::config::ConfigPaths::in_dir(&dir), cx);
        let seeds = ["hello.pdf", "two-page.pdf", "minimal.pdf"].map(|name| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../corpus/seeds")
                .join(name)
        });

        window
            .update(cx, |frame, _window, cx| {
                frame.open_documents(&seeds, cx);
                assert_eq!(frame.settings.recents.documents().len(), 3);

                frame.change_preference(
                    crate::shell::preferences_dialog::PreferenceChange::RecentDocuments(1),
                    cx,
                );

                assert_eq!(frame.settings.recents.documents().len(), 1);
            })
            .unwrap();
    }

    /// The Help menu's shortcut reference is the keymap, with the menus'
    /// own names for the commands.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_shortcut_reference_names_the_commands_the_menus_name(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);

        let rows = window
            .update(cx, |frame, _window, cx| frame.shortcut_rows(cx))
            .unwrap();

        assert_eq!(rows.len(), bindings.len());
        assert!(
            rows.iter().any(|(label, keystroke)| label == "Open…"
                && *keystroke == keystroke_for(&bindings, "file.open")),
            "{rows:?}"
        );
        assert!(rows.iter().all(|(label, _)| !label.is_empty()), "{rows:?}");
    }

    /// A dialog had no keyboard way out at all before P12: it closed by
    /// clicking outside it or by clicking its Close button.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn escape_closes_a_dialog(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        window
            .update(cx, |frame, window, cx| {
                frame.show_preferences(PreferenceCategory::General, window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| assert!(frame.dialog.is_none()))
            .unwrap();
    }
}
