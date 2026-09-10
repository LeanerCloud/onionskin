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
