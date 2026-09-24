mod accessible;
mod advanced_search;
mod auto_scroll;
mod context;
mod create;
mod crop;
mod dialogs;
mod export;
mod export_selection;
mod file;
mod follow_link;
mod forms;
mod frame_state;
mod images;
mod inspector;
mod line_weights;
#[cfg(feature = "tools-edit")]
mod link_editor;
mod manage_tools;
#[cfg(feature = "tools-edit")]
mod marks;
mod menu;
mod organize;
mod outline;
mod page_grid;
mod print;
mod properties;
mod redact;
mod replace;
mod security;
mod send_pages;
mod signature;
mod signature_properties;
mod skins;
#[cfg(feature = "spelling")]
mod spelling;
mod stamps;
mod summary;
mod windows;

pub(in crate::shell) use self::forms::NO_FORMS;
pub(in crate::shell) use self::images::NO_IMAGE_TOOL;

/// What Check Spelling says in a build without the spelling plugin.
pub(in crate::shell) const NO_SPELLING: &str = "No installed plugin checks spelling";
pub(in crate::shell) use self::organize::NO_CORE_COMMANDS;
pub(in crate::shell) use self::redact::{RedactCommand, NO_REDACT};
pub(in crate::shell) use self::security::NO_SECURITY;
pub(in crate::shell) use self::signature::NO_SIGN_TOOL;
pub(in crate::shell) use self::stamps::NO_STAMP_TOOL;

use menu::MenuPanel;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, ClipboardItem, Context, Entity, EntityId, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, PathPromptOptions,
    Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _, Window,
};
use onionskin_core::{Document, ViewSize};
use onionskin_plugin_api::ToolCapability;

pub(in crate::shell) use self::auto_scroll::install_keybindings as install_auto_scroll_keybindings;
pub(in crate::shell) use self::frame_state::ShellFrame;
pub(super) use self::frame_state::TabError;
use super::super::canvas::{CanvasModel, CanvasViewState, ViewAction};
use super::super::context_menu::tool_with;
use super::super::dialog::render_dialog;
use super::super::find_bar::{
    render_find_bar, Dismiss, FindDirection, FindNextMatch, FindOption, FindPreviousMatch,
    FindSummary,
};
use super::super::home::{render_home, HomeView};
// Only the tests and the accessors they call still name the type here, now
// that the frame's construction moved to `frame_state`. Gated on `test`
// alone, not on `shell-test-support`: a plain `#[test]` in this file uses it
// too, so a `--features shell` test build needs it.
#[cfg(test)]
use super::super::panes::NavigationPanesState;
use super::super::panes::{self, PaneAction};
use super::super::Canvas;
use super::super::{record_opened, repair_notice};

use self::export::{export_progress_label, ExportPhaseValue};
use self::frame_state::{activate_tab, close_other_tabs, close_tab, DocumentTab};
use super::accessible::Activation;
use super::global_bar::{
    main_menu_schema, refresh_native_menus, MenuAvailability, MenuCommand, NO_SNAPSHOT_TOOL,
};
use super::page_controls::{
    parse_page_entry, render_page_controls, PageControlsState, PAGE_CONTROLS_HEIGHT,
};
use super::quick_actions::{render_quick_actions, QuickAction, QuickActionEntry};
use super::rail::{apply_rail_selection, rail_width, render_rail, RailEntry};
use super::side_panel::{render_side_panel, SidePanelState};
use super::theme::{ShellViewAction, SurfaceVisibility};
use super::tool_search::{
    document_search_result, search_registry, unavailable_selection, SearchResult,
};

const GLOBAL_BAR_HEIGHT: f32 = 40.0;
/// How far from the window's right edge the Convert panel sits: past the
/// search field, under the Convert button.
const CONVERT_PANEL_RIGHT: f32 = 340.0;
const TAB_BAR_HEIGHT: f32 = 36.0;

/// What the chrome says when a tool will not activate. The canvas status line
/// carries the error itself; the search panel is drawn over it, so a selection
/// made there needs a line of its own.
const TOOL_ACTIVATION_FAILED: &str =
    "This tool did not activate; the canvas status line has the reason";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum TabCommand {
    Close,
    CloseOthers,
    CloseAll,
    RevealPath,
    CopyPath,
}

impl ShellFrame {
    /// Escape: close the topmost thing that is open.
    ///
    /// In the order a user would expect to peel them off, and propagating
    /// when nothing is open so the keystroke is not swallowed.
    pub(in crate::shell) fn dismiss_overlay(
        &mut self,
        _: &Dismiss,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // In the order the frame stacks them, topmost first, so Escape always
        // closes the thing the user is looking at.
        if self.dialog.is_some() {
            self.close_dialog(window, cx);
            return;
        }
        // The panel is open because the search field has something in it, so
        // emptying the field is what closes it. It never shows at the same
        // time as a menu, so where it sits relative to them does not matter.
        if self.search_panel_visible(cx) {
            self.tool_search
                .search_input
                .update(cx, |input, cx| input.set_query("", cx));
            cx.notify();
            return;
        }
        if self.context_menus.tab_context_menu.is_some()
            || self.context_menus.canvas_context_menu.is_some()
        {
            self.context_menus.tab_context_menu = None;
            self.context_menus.canvas_context_menu = None;
            cx.notify();
            return;
        }
        if self.menus.main_menu_open || self.menus.recent_menu_open {
            self.menus.main_menu_open = false;
            self.menus.recent_menu_open = false;
            cx.notify();
            return;
        }
        if self.find.is_open() {
            self.dismiss_find_bar(cx);
            return;
        }
        if self.stop_auto_scroll(cx) {
            return;
        }
        // The two modes that hide the chrome are the last thing Escape
        // closes, innermost first. Without this a window in Full Screen has
        // no chrome to leave it from, and Read Mode ships unbound because
        // Acrobat's Ctrl+H is Hide on macOS.
        // A shape half built by clicking is the next thing Escape abandons,
        // before the modes that hide the chrome.
        if let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) {
            let pending = canvas.update(cx, |canvas, cx| {
                let had = canvas.model.tool_has_pending_gesture();
                if had {
                    canvas.model.cancel_tool_gesture();
                    canvas.handle_change(Ok(true), cx);
                }
                had
            });
            if pending {
                return;
            }
        }
        if self.shell_view_state.fullscreen() {
            self.toggle_fullscreen(window, cx);
            return;
        }
        if self.shell_view_state.read_mode() {
            self.run_shell_view_action(ShellViewAction::ToggleReadMode, cx);
            return;
        }
        cx.propagate();
    }

    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if activate_tab(&mut self.tabs, &mut self.tool_search.search_feedback, index) {
            self.navigation.document_changed();
            self.page_entry.page_entry_error = None;
            self.observed_view_state = self.active_view_state(cx);
            self.sync_page_entry(cx);
            self.refresh_find(cx);
            refresh_native_menus(cx, self.menu_state(cx));
            cx.notify();
        }
    }

    fn active_index(&self) -> Result<usize, TabError> {
        self.tabs
            .active_index()
            .ok_or(TabError::OutOfRange { index: 0, count: 0 })
    }

    /// Ask for documents, then open them.
    fn prompt_for_documents(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |frame, cx| {
            let chosen = match chosen.await {
                Ok(Ok(Some(paths))) => paths,
                // The user changed their mind, or the task was dropped with
                // the window.
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    frame
                        .update(cx, |frame, cx| {
                            frame
                                .notices
                                .push(format!("no files could be chosen: {error}"));
                            cx.notify();
                        })
                        .ok();
                    return;
                }
            };
            frame
                .update(cx, |frame, cx| frame.open_documents(&chosen, cx))
                .ok();
        })
        .detach();
    }

    /// Open a recent document by its position in the list.
    pub(in crate::shell) fn open_recent(&mut self, index: usize, cx: &mut Context<Self>) {
        self.menus.recent_menu_open = false;
        let Some(path) = self
            .settings
            .recents
            .get(index)
            .map(|recent| recent.path.clone())
        else {
            cx.notify();
            return;
        };
        self.open_documents(&[path], cx);
    }

    /// Home's Starred section.
    pub(in crate::shell) fn open_starred(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(path) = self.settings.recents.starred().get(index).cloned() else {
            return;
        };
        self.open_documents(&[path], cx);
    }

    /// Star a document, or unstar it, and write the list.
    pub(in crate::shell) fn toggle_star(&mut self, path: &Path, cx: &mut Context<Self>) {
        match self.settings.recents.toggle_star(path) {
            Ok(_) => {
                if let Some(file) = self.settings.paths.recents.as_deref() {
                    if let Err(error) = self.settings.recents.save(file) {
                        self.notices.push(error.to_string());
                    }
                }
            }
            Err(error) => self.notices.push(error.to_string()),
        }
        cx.notify();
    }

    /// Open documents into tabs, in the order they were chosen, and record
    /// them as recent.
    ///
    /// A path that will not open is reported on the notice bar and the rest
    /// still open: choosing five files and losing all of them because one is
    /// corrupt would be the wrong trade.
    pub(super) fn open_documents(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let before: Vec<_> = self
            .canvases()
            .into_iter()
            .map(|canvas| canvas.entity_id())
            .collect();
        let mut opened: Vec<PathBuf> = Vec::new();
        for path in paths {
            match self.open_document(path, "", cx) {
                Ok(source) => opened.push(source),
                // Asked for once the frame renders, where there is a window
                // to show the prompt in.
                Err(OpenFailure::NeedsPassword(path)) => {
                    if !self.pending_passwords.contains(&path) {
                        self.pending_passwords.push_back(path);
                    }
                }
                Err(OpenFailure::Failed(failure)) => self.notices.push(failure),
            }
        }
        self.after_opening(&before, &opened, cx);
    }

    /// Open `path` with `password`. `Err(None)` when the password does not
    /// open it, and `Err(Some(why))` when it would not open anyway.
    pub(super) fn open_with_password(
        &mut self,
        path: &Path,
        password: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), Option<String>> {
        let before: Vec<_> = self
            .canvases()
            .into_iter()
            .map(|canvas| canvas.entity_id())
            .collect();
        match self.open_document(path, password, cx) {
            Ok(source) => {
                self.after_opening(&before, &[source], cx);
                Ok(())
            }
            Err(OpenFailure::NeedsPassword(_)) => Err(None),
            Err(OpenFailure::Failed(failure)) => Err(Some(failure)),
        }
    }

    /// What every open does once its documents are in tabs: autosave for
    /// the new ones, the recent list, Home's cards, the menus.
    fn after_opening(
        &mut self,
        before: &[gpui::EntityId],
        opened: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        let new: Vec<_> = self
            .canvases()
            .into_iter()
            .map(|canvas| canvas.entity_id())
            .filter(|id| !before.contains(id))
            .collect();
        self.attach_recovery(&new, cx);
        let notices = record_opened(
            &mut self.settings.recents,
            opened.iter().map(PathBuf::as_path),
            self.settings.preferences.recent_documents,
            self.settings.paths.recents.as_deref(),
        );
        self.notices.extend(notices);
        // Home may be behind this window with the thumbnail view showing;
        // the document just recorded needs its card.
        self.home.refresh(&self.settings.recents);
        self.observed_view_state = self.active_view_state(cx);
        self.sync_page_entry(cx);
        self.refresh_find(cx);
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    fn open_document(
        &mut self,
        path: &Path,
        password: &str,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, OpenFailure> {
        let source = std::path::absolute(path).map_err(|error| {
            OpenFailure::Failed(format!("{} could not be resolved: {error}", path.display()))
        })?;
        if let Some(index) = self.tabs.tabs().iter().position(|tab| tab.source == source) {
            // Already open. Acrobat raises the tab rather than opening the
            // document twice, and two tabs over one file would be two
            // independent view states over one document.
            self.activate(index, cx);
            return Ok(source);
        }
        let document =
            Document::open_path_with_password(&source, password).map_err(|error| match error {
                onionskin_core::Error::NeedsPassword => OpenFailure::NeedsPassword(source.clone()),
                error => OpenFailure::Failed(format!(
                    "{} could not be opened: {error}",
                    source.display()
                )),
            })?;
        let mut model = CanvasModel::new(
            document,
            crate::build_registry(),
            ViewSize {
                width: crate::shell::WINDOW_WIDTH,
                height: crate::shell::WINDOW_HEIGHT,
            },
        )
        .map_err(|error| {
            OpenFailure::Failed(format!("{} could not be opened: {error}", source.display()))
        })?;
        self.settings.configure(&mut model);
        let repaired = repair_notice(&source, &model.provenance());
        if let Err(error) = crate::shell::apply_page_display(&mut model, &self.settings.preferences)
        {
            self.notices.push(format!(
                "{} opened at the default view: {error}",
                source.display()
            ));
        }
        self.notices.extend(repaired);
        let theme = self.shell_view_state.tokens();
        let canvas = cx.new(|_cx| Canvas::new(model, theme));
        cx.observe(&canvas, |frame, _, cx| {
            frame.canvas_view_changed(cx);
        })
        .detach();
        self.tabs.push(DocumentTab::new(source.clone(), canvas));
        self.tool_search.search_feedback = None;
        self.open_initial_pane(cx);
        Ok(source)
    }

    /// Run a command the registry holds against the active document.
    ///
    /// The chrome never carries a command's body: it looks the id up in the
    /// registry the active document was built with, which is the same query
    /// that decided whether the entry was live.
    pub(super) fn run_registry_command(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let failure = canvas.update(cx, |canvas, cx| match canvas.model.run_command(id) {
            Ok(()) => {
                // A command changes the document, so the canvas repaints for
                // the same reasons a tool gesture does.
                canvas.handle_change(Ok(true), cx);
                None
            }
            // Reported on the notice bar rather than on the canvas status
            // line: a command is something the user just asked for, and the
            // status line is where the document's own trouble goes.
            Err(error) => Some(error.to_string()),
        });
        if let Some(message) = failure {
            self.notices.push(message);
        }
        cx.notify();
    }

    /// Activate whichever installed tool carries `capability`.
    ///
    /// The menu entry asked the same question before it went live, so a
    /// miss here means the registry changed underneath it. Reported rather
    /// than returned quietly: the user clicked something that did nothing.
    ///
    /// The rail entry goes along so a tool sharing a rail slot with another
    /// leaves that slot showing the one now active, exactly as a click on
    /// the slot would.
    fn activate_tool_with(
        &mut self,
        capability: ToolCapability,
        label: &str,
        missing: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .tabs
            .active()
            .and_then(|tab| tool_with(tab.canvas.read(cx).model.registry(), capability))
        else {
            self.notices.push(missing.to_owned());
            cx.notify();
            return;
        };
        let entry = self.active_rail_entry(index, cx);
        self.activate_canvas_tool(index, label, entry, cx);
    }

    fn take_a_snapshot(&mut self, cx: &mut Context<Self>) {
        self.activate_tool_with(
            ToolCapability::Snapshot,
            "Take a Snapshot",
            NO_SNAPSHOT_TOOL,
            cx,
        );
    }

    pub(in crate::shell) fn set_home_view(&mut self, view: HomeView, cx: &mut Context<Self>) {
        self.home.set_view(view, &self.settings.recents);
        cx.notify();
    }

    /// Home's Open File button, which is File > Open by another name.
    pub(in crate::shell) fn open_from_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.run_main_menu_command(MenuCommand::Open, window, cx) {
            eprintln!("onionskin: {error}");
        }
    }

    fn run_tab_command(
        &mut self,
        command: TabCommand,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), TabError> {
        self.menus.main_menu_open = false;
        self.context_menus.tab_context_menu = None;
        match command {
            TabCommand::Close => {
                if let Some(tab) = self.tabs.tabs().get(index) {
                    self.cancel_export_for(tab.canvas.entity_id());
                }
                close_tab(&mut self.tabs, &mut self.tool_search.search_feedback, index)?;
                self.navigation.document_changed();
                self.observed_view_state = self.active_view_state(cx);
                self.sync_page_entry(cx);
                self.refresh_find(cx);
                refresh_native_menus(cx, self.menu_state(cx));
                cx.notify();
            }
            TabCommand::CloseOthers => {
                if let Some(kept) = self.tabs.tabs().get(index) {
                    if self
                        .export
                        .export_job
                        .as_ref()
                        .is_some_and(|job| job.origin != kept.canvas.entity_id())
                    {
                        self.cancel_export(cx);
                    }
                }
                close_other_tabs(&mut self.tabs, &mut self.tool_search.search_feedback, index)?;
                self.navigation.document_changed();
                self.observed_view_state = self.active_view_state(cx);
                self.sync_page_entry(cx);
                self.refresh_find(cx);
                refresh_native_menus(cx, self.menu_state(cx));
                cx.notify();
            }
            TabCommand::CloseAll => {
                self.cancel_export(cx);
                self.tabs.close_all();
                self.tool_search.search_feedback = None;
                self.navigation.document_changed();
                self.observed_view_state = None;
                self.refresh_find(cx);
                refresh_native_menus(cx, self.menu_state(cx));
                cx.notify();
            }
            TabCommand::RevealPath => {
                cx.reveal_path(self.tab_source(index)?);
                cx.notify();
            }
            TabCommand::CopyPath => {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    self.tab_source(index)?.to_string_lossy().into_owned(),
                ));
                cx.notify();
            }
        }
        Ok(())
    }

    fn tab_source(&self, index: usize) -> Result<&Path, TabError> {
        self.tabs
            .tabs()
            .get(index)
            .map(|tab| tab.source.as_path())
            .ok_or(TabError::OutOfRange {
                index,
                count: self.tabs.tabs().len(),
            })
    }

    fn active_view_state(&self, cx: &App) -> Option<CanvasViewState> {
        self.tabs
            .active()
            .map(|tab| tab.canvas.read(cx).model.view_state())
    }

    fn apply_theme(&mut self, cx: &mut Context<Self>) {
        let theme = self.shell_view_state.tokens();
        self.tool_search
            .search_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.find_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.page_entry
            .page_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        if let Some(dialog) = &self.export.dialog {
            dialog.set_theme(theme, cx);
        }
        for tab in self.tabs.tabs() {
            tab.canvas
                .update(cx, |canvas, cx| canvas.set_theme(theme, cx));
        }
    }

    fn window_appearance_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .shell_view_state
            .set_system_appearance(window.appearance())
        {
            self.apply_theme(cx);
            cx.notify();
        }
    }

    fn window_bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shell_view_state.set_fullscreen(window.is_fullscreen()) {
            refresh_native_menus(cx, self.menu_state(cx));
            cx.notify();
        }
    }

    fn run_shell_view_action(&mut self, action: ShellViewAction, cx: &mut Context<Self>) {
        if self.apply_shell_view_action(action, cx) {
            refresh_native_menus(cx, self.menu_state(cx));
            cx.notify();
        }
    }

    /// Apply a view-state change and repaint what it changed, without
    /// rebuilding the menus.
    ///
    /// Separate because a preference change ends by rebuilding them anyway,
    /// and the theme is both: going through the whole of
    /// `run_shell_view_action` rebuilt the native menu bar twice per click.
    /// Returns whether anything changed.
    fn apply_shell_view_action(&mut self, action: ShellViewAction, cx: &mut Context<Self>) -> bool {
        let previous_theme = self.shell_view_state.resolved_theme();
        if !self.shell_view_state.apply(action) {
            return false;
        }
        if previous_theme != self.shell_view_state.resolved_theme() {
            self.apply_theme(cx);
        }
        true
    }

    fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.toggle_fullscreen();
        cx.on_next_frame(window, |frame, window, cx| {
            frame.window_bounds_changed(window, cx);
        });
    }

    fn sync_page_entry(&self, cx: &mut Context<Self>) {
        let Some(view) = self.active_view_state(cx) else {
            return;
        };
        self.page_entry.page_input.update(cx, |input, cx| {
            input.set_query((view.current_page + 1).to_string(), cx);
        });
    }

    /// Take whatever thumbnails the worker answered since the last repaint.
    /// Called wherever the canvas notifies, because that is the only signal
    /// the pane gets: the pictures answer on a channel of their own.
    pub(in crate::shell) fn collect_thumbnails(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let navigation = &mut self.navigation;
        if canvas.update(cx, |canvas, _cx| navigation.collect_thumbnails(canvas)) {
            cx.notify();
        }
    }

    fn canvas_view_changed(&mut self, cx: &mut Context<Self>) {
        // The canvas notifies whenever its poll loop applies anything, which
        // is the only signal a thumbnail has landed: it answers on its own
        // channel and changes nothing the view state would show.
        self.collect_thumbnails(cx);
        self.collect_link_request(cx);
        self.collect_redaction_request(cx);
        #[cfg(feature = "tools-form")]
        self.collect_form_notices(cx);
        #[cfg(feature = "tools-form")]
        self.collect_field_request(cx);
        self.follow_document_edits(cx);
        let view = self.active_view_state(cx);
        if self.observed_view_state == view {
            // A running walk reports new hits without moving the view, and the
            // bar's count has to follow them.
            if self.find.is_open() {
                cx.notify();
            }
            return;
        }
        self.observed_view_state = view;
        self.sync_page_entry(cx);
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    /// After an edit on the canvas, which moves no view: read the open pane
    /// again, so a comment just placed is in the Comments list, and redraw
    /// the tab's dirty mark and the Undo entry.
    fn follow_document_edits(&mut self, cx: &mut Context<Self>) {
        // A save or a roll back changes the file's versions without always
        // moving the edit epoch, so the skins look for themselves.
        self.refresh_skins(cx);
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let epoch = Some(canvas.read(cx).model.edit_epoch());
        if self.observed_edit_epoch == epoch {
            return;
        }
        self.observed_edit_epoch = epoch;
        // A page index may now name a different page, so every picture the
        // pane and the grid hold is of a document nobody is showing.
        self.navigation.thumbnails_mut().invalidate_images();
        self.follow_grid(cx);
        self.navigation.reread(&canvas, cx);
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    pub(super) fn run_view_action(&mut self, action: ViewAction, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        canvas.update(cx, |canvas, cx| canvas.run_view_action(action, cx));
        self.page_entry.page_entry_error = None;
        self.observed_view_state = self.active_view_state(cx);
        self.sync_page_entry(cx);
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    pub(super) fn submit_page_entry(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.active_view_state(cx) else {
            return;
        };
        let input = self.page_entry.page_input.read(cx).query().to_owned();
        match parse_page_entry(&input, view.page_count) {
            Ok(page) => self.run_view_action(ViewAction::GoToPage(page), cx),
            Err(error) => {
                self.page_entry.page_entry_error = Some(error);
                cx.notify();
            }
        }
    }

    /// Escape is bound window-wide so it closes the bar from wherever focus
    /// sits, which means a closed bar has to hand the key back.
    pub(in crate::shell) fn find_next_match(
        &mut self,
        _: &FindNextMatch,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_find(FindDirection::Next, cx);
    }

    pub(in crate::shell) fn find_previous_match(
        &mut self,
        _: &FindPreviousMatch,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_find(FindDirection::Previous, cx);
    }

    /// Opens the bar with the find field focused. A query replaces whatever
    /// was typed before; `None` keeps it, so Ctrl+F on an open bar refocuses
    /// the query it is already showing rather than clearing it.
    pub(super) fn open_find_bar(
        &mut self,
        query: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find.open();
        self.menus.main_menu_open = false;
        self.context_menus.tab_context_menu = None;
        if let Some(query) = query {
            self.find_input
                .update(cx, |input, cx| input.set_query(query, cx));
        }
        window.focus(&self.find_input.read(cx).focus_handle(cx));
        self.run_find_query(cx);
        cx.notify();
    }

    /// Closes the bar and drops every walk it started. Every tab, not just the
    /// active one: a walk left running on a tab the user switched away from
    /// would keep polling and keep highlighting.
    pub(in crate::shell) fn dismiss_find_bar(&mut self, cx: &mut Context<Self>) {
        self.find.close();
        for canvas in self.canvases() {
            cancel_find_on(&canvas, cx);
        }
        cx.notify();
    }

    pub(in crate::shell) fn apply_find_option(
        &mut self,
        option: FindOption,
        cx: &mut Context<Self>,
    ) {
        if !self.find.apply(option) {
            return;
        }
        self.run_find_query(cx);
        cx.notify();
    }

    pub(in crate::shell) fn step_find(&mut self, direction: FindDirection, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        canvas.update(cx, |canvas, cx| {
            let result = match direction {
                FindDirection::Next => canvas.model.select_next_match(),
                FindDirection::Previous => canvas.model.select_previous_match(),
            };
            canvas.handle_change(result, cx);
        });
        cx.notify();
    }

    fn find_query_changed(&mut self, cx: &mut Context<Self>) {
        if !self.find.is_open() {
            return;
        }
        self.run_find_query(cx);
        cx.notify();
    }

    /// Results belong to the document the canvas holds, so a change of active
    /// tab runs the query again rather than reading the new tab's empty state
    /// as "no results".
    fn refresh_find(&mut self, cx: &mut Context<Self>) {
        if self.find.is_open() {
            self.run_find_query(cx);
        }
    }

    /// Runs the query on the document on screen, and only on it: the walk the
    /// user left behind on another tab is cancelled rather than left running.
    fn run_find_query(&mut self, cx: &mut Context<Self>) {
        let typed = self.find_input.read(cx).query().to_owned();
        // Blank input searches nothing, but a needle the user typed spaces
        // into is the needle they meant.
        let needle = if typed.trim().is_empty() {
            String::new()
        } else {
            typed
        };
        let options = self.find.options();
        let active = self.tabs.active_index();
        for (index, canvas) in self.canvases().into_iter().enumerate() {
            if Some(index) != active {
                cancel_find_on(&canvas, cx);
                continue;
            }
            canvas.update(cx, |canvas, cx| {
                let result = canvas.model.start_search(&needle, options);
                canvas.handle_change(result, cx);
            });
        }
    }

    /// This window's canvases on the session `file`.
    pub(in crate::shell) fn canvases_on(
        &self,
        file: &crate::shell::canvas::SharedFile,
        cx: &App,
    ) -> Vec<Entity<Canvas>> {
        self.canvases()
            .into_iter()
            .filter(|canvas| std::rc::Rc::ptr_eq(&canvas.read(cx).model.shared_file(), file))
            .collect()
    }

    fn canvases(&self) -> Vec<Entity<Canvas>> {
        self.tabs
            .tabs()
            .iter()
            .map(|tab| tab.canvas.clone())
            .collect()
    }

    /// The global bar's Save, Undo, Redo and Print, each with the menu entry's own
    /// availability, so the bar and the menu cannot disagree about whether
    /// a command runs. The glyph is what is drawn; the menu label is what a
    /// screen reader hears.
    pub(super) fn global_file_buttons(
        &self,
        cx: &App,
    ) -> Vec<(&'static str, &'static str, MenuCommand, MenuAvailability)> {
        let entries: Vec<_> = main_menu_schema(self.menu_state(cx))
            .into_iter()
            .flat_map(|section| section.entries)
            .collect();
        [
            ("global-save", "💾", MenuCommand::Save),
            ("global-save-as", "Save As", MenuCommand::SaveAs),
            ("global-undo", "↶", MenuCommand::Undo),
            ("global-redo", "↷", MenuCommand::Redo),
            ("global-print", "🖨", MenuCommand::Print),
        ]
        .into_iter()
        .filter_map(|(id, glyph, command)| {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .map(|entry| (id, glyph, command, entry.availability))
        })
        .collect()
    }

    /// The active tool's name and how it is used, for the side panel.
    pub(super) fn active_tool_help(&self, cx: &App) -> Option<super::side_panel::ToolHelp> {
        let model = &self.tabs.active()?.canvas.read(cx).model;
        let (name, hint) = model.active_tool_help()?;
        let settings = model
            .active_tool_settings()
            .into_iter()
            .map(|(setting, on)| super::side_panel::ToolSetting {
                id: setting.id,
                label: setting.label,
                category: setting.category,
                on,
            })
            .collect();
        Some(super::side_panel::ToolHelp {
            name,
            hint,
            readings: model.active_tool_readings(),
            settings,
        })
    }

    /// A setting of the active tool chosen from the side panel.
    pub(super) fn choose_tool_setting(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(canvas) = self.active_canvas().cloned() {
            canvas.update(cx, |canvas, cx| {
                if canvas.model.choose_active_tool_setting(id) {
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    fn render_global_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        div()
            .h(px(GLOBAL_BAR_HEIGHT))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .bg(theme.global_bar)
            .text_color(theme.text)
            .child(
                div()
                    .id("main-menu-button")
                    .w(px(32.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .rounded_md()
                    .hover(move |button| button.bg(theme.subtle_hover))
                    .on_click(cx.listener(|frame, _event, _window, cx| {
                        frame.toggle_main_menu(cx);
                    }))
                    .child("☰"),
            )
            .child(div().text_sm().child("Onionskin"))
            .children(self.global_file_buttons(cx).into_iter().map(
                |(id, glyph, command, availability)| {
                    let enabled = availability.is_enabled();
                    let button = div()
                        .id(id)
                        .h(px(28.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .text_color(if enabled {
                            theme.text
                        } else {
                            theme.disabled_text
                        })
                        .child(glyph);
                    if enabled {
                        button
                            .cursor_pointer()
                            .hover(move |button| button.bg(theme.subtle_hover))
                            .on_click(cx.listener(move |frame, _event, window, cx| {
                                frame.run_activation(Activation::MainMenu(command), window, cx);
                            }))
                    } else {
                        button
                    }
                },
            ))
            .child(div().flex_1())
            .child(
                div()
                    .id("convert-button")
                    .h(px(28.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .rounded_md()
                    .when(self.menus.showing(MenuPanel::Convert), |button| {
                        button.bg(theme.selected)
                    })
                    .hover(move |button| button.bg(theme.subtle_hover))
                    .on_click(cx.listener(|frame, _event, _window, cx| {
                        frame.toggle_convert_menu(cx);
                    }))
                    .child("Convert"),
            )
            .child(
                div()
                    .w(px(320.0))
                    .flex_none()
                    .child(self.tool_search.search_input.clone()),
            )
    }

    fn search_results(&self, cx: &App) -> Vec<SearchResult> {
        let query = self.tool_search.search_input.read(cx).query().to_owned();
        let has_document = self.tabs.active().is_some();
        let mut results = self
            .tabs
            .active()
            .map(|tab| search_registry(tab.canvas.read(cx).model.registry(), &query))
            .unwrap_or_default();
        if let Some(document_search) = document_search_result(&query) {
            results.push(document_search);
        }
        results
            .into_iter()
            .map(|result| unavailable_selection(&result, has_document).unwrap_or(result))
            .collect()
    }

    fn choose_search_result(
        &mut self,
        result: SearchResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(unavailable) = unavailable_selection(&result, self.tabs.active().is_some()) {
            self.tool_search.search_feedback = Some(unavailable);
            cx.notify();
            return;
        }
        match result {
            SearchResult::Tool { index, name, .. } => {
                let entry = self.active_rail_entry(index, cx);
                if self.activate_canvas_tool(index, name, entry, cx) {
                    self.tool_search.search_feedback = None;
                    cx.notify();
                }
            }
            SearchResult::DocumentSearch { query } => {
                self.tool_search.search_feedback = None;
                self.open_find_bar(Some(query), window, cx);
            }
            SearchResult::Command { id, .. } => {
                self.tool_search.search_feedback = None;
                self.run_registry_command(id, cx);
            }
            // Everything unavailable returned above.
            SearchResult::Unavailable { .. } => {}
        }
    }

    fn active_rail_entry(&self, index: usize, cx: &App) -> Option<RailEntry> {
        let canvas = self.tabs.active()?.canvas.read(cx);
        self.rail_state
            .entry_for_index(canvas.model.registry(), canvas.model.active_tool(), index)
    }

    fn rail_entries(&self, cx: &App) -> Vec<RailEntry> {
        self.tabs
            .active()
            .map(|tab| {
                let canvas = tab.canvas.read(cx);
                self.rail_state.entries(
                    canvas.model.registry(),
                    canvas.model.active_tool(),
                    &self.settings.preferences.hidden_tools,
                )
            })
            .unwrap_or_default()
    }

    /// Activates a tool, and says so where the user asked for it when it will
    /// not activate.
    ///
    /// Every entry point went through here and dropped the error, so a rail
    /// icon or a quick action that could not take left the chrome silent. The
    /// canvas records the error's own words on its status line; the chrome
    /// gets [`TOOL_ACTIVATION_FAILED`] because the search panel is drawn over
    /// that status line. Returns whether the tool is now active.
    fn activate_canvas_tool(
        &mut self,
        index: usize,
        name: &str,
        entry: Option<RailEntry>,
        cx: &mut Context<Self>,
    ) -> bool {
        let canvas = self
            .tabs
            .active()
            .expect("tool selection requires an active document")
            .canvas
            .clone();
        let result = if let Some(entry) = entry {
            apply_rail_selection(&mut self.rail_state, entry, |index| {
                canvas.update(cx, |canvas, cx| canvas.activate_tool(index, cx))
            })
        } else {
            canvas.update(cx, |canvas, cx| canvas.activate_tool(index, cx))
        };
        if result.is_err() {
            self.tool_search.search_feedback = Some(SearchResult::Unavailable {
                label: name.to_owned(),
                reason: TOOL_ACTIVATION_FAILED,
            });
        }
        // A tool that places a file cannot ask for one itself.
        if result.is_ok() && stamps::chooses_file(canvas.read(cx), index) {
            self.prompt_for_tool_file(index, cx);
        }
        cx.notify();
        result.is_ok()
    }

    pub(super) fn select_rail_entry(&mut self, entry: RailEntry, cx: &mut Context<Self>) {
        self.activate_canvas_tool(entry.registry_index, entry.name, Some(entry), cx);
    }

    pub(super) fn toggle_rail_expanded(&mut self, cx: &mut Context<Self>) {
        self.rail_state.toggle_expanded();
        cx.notify();
    }

    fn quick_action_entries(&self, cx: &App) -> Vec<QuickActionEntry> {
        self.tabs
            .active()
            .map(|tab| {
                let model = &tab.canvas.read(cx).model;
                self.quick_actions_state
                    .entries(model.registry(), model.refusals())
            })
            .unwrap_or_default()
    }

    fn all_quick_action_entries(&self, cx: &App) -> Vec<QuickActionEntry> {
        self.tabs
            .active()
            .map(|tab| {
                let model = &tab.canvas.read(cx).model;
                self.quick_actions_state
                    .all_entries(model.registry(), model.refusals())
            })
            .unwrap_or_default()
    }

    pub(super) fn select_quick_action(&mut self, entry: QuickActionEntry, cx: &mut Context<Self>) {
        let Some(index) = entry.availability.tool_index() else {
            return;
        };
        let rail_entry = self.active_rail_entry(index, cx);
        self.activate_canvas_tool(index, entry.action.label(), rail_entry, cx);
    }

    pub(super) fn toggle_quick_action_customization(&mut self, cx: &mut Context<Self>) {
        self.quick_actions_state.toggle_customizing();
        cx.notify();
    }

    pub(super) fn toggle_quick_action_visibility(
        &mut self,
        action: QuickAction,
        cx: &mut Context<Self>,
    ) {
        self.quick_actions_state.toggle_visibility(action);
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    pub(super) fn drag_quick_actions(
        &mut self,
        id: u64,
        pointer: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewport = window.viewport_size();
        let visibility = self.shell_view_state.visibility();
        let document = document_view_bounds(
            viewport,
            visibility,
            self.rail_state.expanded(),
            self.navigation_width(visibility.navigation_pane),
            self.side_panel_state,
        );
        let bounds = gpui::Bounds {
            origin: Point::default(),
            size: document.size,
        };
        let toolbar_size = self.quick_actions_state.toolbar_size(bounds.size);
        self.quick_actions_state
            .drag_to(id, pointer, bounds, toolbar_size);
        cx.notify();
    }

    /// What the navigation column takes from the document view, which is
    /// nothing at all when View > Show/Hide has it turned off.
    fn navigation_width(&self, visible: bool) -> Pixels {
        if visible {
            self.navigation.width()
        } else {
            px(0.0)
        }
    }

    /// The navigation column, for tests that drive a pane through a real
    /// window and then ask what it holds. Gated on the feature those tests
    /// are gated on, or a `--features shell` test build compiles all three
    /// with nothing calling them.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn navigation(&self) -> &NavigationPanesState {
        &self.navigation
    }

    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn navigation_mut(&mut self) -> &mut NavigationPanesState {
        &mut self.navigation
    }

    pub(in crate::shell) fn active_canvas(&self) -> Option<&Entity<Canvas>> {
        self.tabs.active().map(|tab| &tab.canvas)
    }

    pub(in crate::shell) fn is_active_canvas(&self, origin: EntityId) -> bool {
        self.tabs
            .active()
            .is_some_and(|tab| tab.canvas.entity_id() == origin)
    }

    /// The panes' one way back into the frame. Everything a click in a
    /// navigation pane does goes through here, so this file holds which tab
    /// is active and `shell/panes/` holds what the click means.
    pub(in crate::shell) fn run_pane_action(&mut self, action: PaneAction, cx: &mut Context<Self>) {
        // The thumbnails menu's page entries edit pages with a file prompt
        // or the grid's selection, which are the frame's, not the pane's.
        if let PaneAction::Thumbnail(panes::ThumbnailAction::Run(command)) = action {
            if command.acts_on_pages() {
                self.run_thumbnail_command(command, cx);
                return;
            }
        }
        let canvas = self.tabs.active().map(|tab| tab.canvas.clone());
        let directory = self
            .tabs
            .active()
            .and_then(|tab| tab.source.parent().map(Path::to_path_buf));
        panes::apply(&mut self.navigation, canvas.as_ref(), directory, action, cx);
        self.follow_chosen_comment(cx);
    }

    /// Show what a pane's own asynchronous work could not do, in the pane
    /// that asked for it. Saving an attachment is the only such work today:
    /// it finishes after a file dialog, long after the click that started it.
    pub(in crate::shell) fn report_pane_failure(
        &mut self,
        failure: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.navigation.report(failure);
        cx.notify();
    }

    pub(super) fn toggle_side_panel(&mut self, cx: &mut Context<Self>) {
        self.side_panel_state.toggle();
        cx.notify();
    }

    /// The panel belongs to the global bar's search field, so it goes when
    /// the bar does: a query typed before Read Mode was entered would
    /// otherwise leave results hanging under a bar that is no longer there.
    fn search_panel_visible(&self, cx: &App) -> bool {
        self.shell_view_state.visibility().global_bar
            && !self.menus.main_menu_open
            && self.context_menus.tab_context_menu.is_none()
            && !self
                .tool_search
                .search_input
                .read(cx)
                .query()
                .trim()
                .is_empty()
    }

    fn render_search_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let mut panel = div()
            .absolute()
            .top(px(GLOBAL_BAR_HEIGHT))
            .right(px(12.0))
            .w(px(420.0))
            .p_1()
            .rounded_md()
            .occlude()
            .bg(theme.raised)
            .text_color(theme.text);

        for (index, result) in self.search_results(cx).into_iter().enumerate() {
            let selection = result.clone();
            panel = panel.child(
                div()
                    .id(("global-search-result", index))
                    .min_h(px(38.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |row| row.bg(theme.selected))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(
                            Activation::ChooseSearchResult(selection.clone()),
                            window,
                            cx,
                        );
                    }))
                    .child(result.label())
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_text)
                            .child(result.detail()),
                    ),
            );
        }

        if let Some(feedback) = self.tool_search.search_feedback.as_ref() {
            panel = panel.child(
                div()
                    .mt_1()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(theme.surface)
                    .text_sm()
                    .text_color(theme.feedback_text)
                    .child(feedback.detail()),
            );
        }
        panel
    }
}

/// Why a document did not open.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OpenFailure {
    /// It is encrypted, and the password given, if any, does not open it.
    NeedsPassword(PathBuf),
    /// Anything else, said for the notice bar.
    Failed(String),
}

impl Render for ShellFrame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A recovery found as documents opened is asked about here, the first
        // place after the open with a window to show a dialog in.
        if self.dialog.is_none() && !self.pending_recoveries.is_empty() {
            self.offer_next_recovery(window, cx);
        }
        // Likewise the password an encrypted document asked for as it opened.
        if self.dialog.is_none() && !self.pending_passwords.is_empty() {
            self.ask_next_password(window, cx);
        }
        // Likewise a link the Hand or Link tool asked about.
        self.run_pending_link(window, cx);
        self.run_pending_redaction(window, cx);
        #[cfg(feature = "tools-form")]
        self.run_pending_field(window, cx);
        let theme = self.shell_view_state.tokens();
        let visibility = self.shell_view_state.visibility();
        let document_bounds = document_view_bounds(
            window.viewport_size(),
            visibility,
            self.rail_state.expanded(),
            self.navigation_width(visibility.navigation_pane),
            self.side_panel_state,
        );
        self.quick_actions_state.constrain_to(document_bounds.size);
        let rects = self.a11y.rects.clone();
        let mut tab_bar = div().flex().h(px(TAB_BAR_HEIGHT)).bg(theme.surface);
        for (index, tab) in self.tabs.tabs().iter().enumerate() {
            let active = self.tabs.active_index() == Some(index);
            tab_bar = tab_bar.child(
                div()
                    .id(tab_element_id(&tab.source))
                    .px_3()
                    .h_full()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .bg(if active { theme.selected } else { theme.raised })
                    .text_color(theme.text)
                    .on_click(
                        cx.listener(move |frame, event: &gpui::ClickEvent, window, cx| {
                            if !event.is_right_click() {
                                frame.run_activation(Activation::ActivateTab(index), window, cx);
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |frame, event, _window, cx| {
                            frame.open_tab_context_menu(index, event, cx);
                        }),
                    )
                    .child(tab_label(tab, cx)),
            );
        }

        let rail_entries = self.rail_entries(cx);
        let rail_expanded = self.rail_state.expanded();
        let skins_open = self.skins.is_some();
        let mut body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(visibility.rail, |body| {
                body.child(render_rail(
                    rail_entries,
                    rail_expanded,
                    skins_open,
                    rects.clone(),
                    theme,
                    cx,
                ))
            });
        if visibility.navigation_pane {
            // The column is as tall as the body it sits in, which is the one
            // thing the panes cannot work out for themselves and the one
            // thing the thumbnails pane needs to know how many rows to show.
            let height = (window.viewport_size().height - header_height(visibility)).max(px(0.0));
            let active_canvas = self.tabs.active().map(|tab| tab.canvas.clone());
            body = body.child(panes::render_navigation_panes(
                &mut self.navigation,
                active_canvas.as_ref(),
                height,
                rects.clone(),
                theme,
                cx,
            ));
        }
        // Organize Pages takes the page's place, and draws first because it
        // needs the frame while the tab below is borrowed.
        let grid = self.render_grid(theme, cx);
        if let Some(tab) = self.tabs.active() {
            let canvas = tab.canvas.clone();
            let page_controls_state =
                PageControlsState::from_view(tab.canvas.read(cx).model.view_state());
            let quick_action_entries = self.quick_action_entries(cx);
            let all_quick_action_entries = self.all_quick_action_entries(cx);
            let quick_actions = render_quick_actions(
                quick_action_entries,
                all_quick_action_entries,
                &self.quick_actions_state,
                document_bounds.size,
                rects.clone(),
                theme,
                cx,
            );
            // Summarised only when the bar is on screen to read it.
            let find_summary = self.find.is_open().then(|| {
                FindSummary::new(
                    &tab.canvas.read(cx).model.search(),
                    tab.canvas.read(cx).model.viewport().page_count(),
                )
            });
            let find_state = self.find;
            let find_input = self.find_input.clone();
            let replace_input = self.replace_input.clone();
            let replace_refusal = self.replace_refusal(cx);
            let export_progress = self.export.export_job.as_ref().map(|job| {
                (
                    export_progress_label(job),
                    job.phase.load() == ExportPhaseValue::Running,
                )
            });
            let canvas_view = div()
                .id("document-view")
                .relative()
                .w_full()
                .h(document_bounds.size.height)
                .min_w_0()
                .min_h_0()
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|frame, event, _window, cx| {
                        frame.open_canvas_context_menu(event, cx);
                    }),
                )
                .child(canvas)
                .when(visibility.quick_actions, |view| view.child(quick_actions))
                .when_some(find_summary, |view, summary| {
                    view.child(render_find_bar(
                        find_state,
                        find_input,
                        replace_input,
                        replace_refusal,
                        &summary,
                        rects.clone(),
                        theme,
                        cx,
                    ))
                })
                .when_some(export_progress, |view, (label, can_cancel)| {
                    view.child(
                        div()
                            .id("export-progress")
                            .absolute()
                            .top_0()
                            .right_0()
                            .m_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(theme.raised)
                            .text_color(theme.text)
                            .child(label)
                            .child(
                                div()
                                    .id("cancel-export")
                                    .px_2()
                                    .rounded_sm()
                                    .text_color(if can_cancel {
                                        theme.text
                                    } else {
                                        theme.muted_text
                                    })
                                    .when(can_cancel, |button| {
                                        button
                                            .cursor_pointer()
                                            .hover(move |button| button.bg(theme.subtle_hover))
                                    })
                                    .on_click(cx.listener(|frame, _event, window, cx| {
                                        frame.run_activation(Activation::CancelExport, window, cx);
                                    }))
                                    .child("Cancel Export"),
                            ),
                    )
                });
            let document_column = div()
                .w(document_bounds.size.width)
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .child(match grid {
                    Some(grid) => div()
                        .w_full()
                        .h(document_bounds.size.height)
                        .flex()
                        .child(grid)
                        .into_any_element(),
                    None => canvas_view.into_any_element(),
                })
                .when(visibility.page_controls, |column| {
                    column.child(render_page_controls(
                        page_controls_state,
                        self.page_entry.page_input.clone(),
                        self.page_entry.page_entry_error.as_ref(),
                        document_bounds.size.width,
                        rects.clone(),
                        theme,
                        cx,
                    ))
                });
            body = body.child(document_column);
        } else {
            body = body.child(render_home(
                &self.home,
                &self.settings.recents,
                self.settings.paths.home.as_deref(),
                rects.clone(),
                theme,
                cx,
            ));
        }
        body = body.when(visibility.side_panel, |body| {
            let content = self.render_side_panel_content(theme, cx);
            body.child(render_side_panel(
                self.side_panel_state,
                self.active_tool_help(cx),
                content,
                theme,
                cx,
            ))
        });

        // Read Mode and Full Screen take the top bars away, and what is left
        // is the page. Notices stay in both: a message the user has to see
        // is not chrome.
        let global_bar = visibility
            .global_bar
            .then(|| self.render_global_bar(cx))
            .map(IntoElement::into_any_element);
        let frame = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .when_some(global_bar, |frame, bar| frame.child(bar))
            .child(self.render_notices(cx))
            .when(visibility.tab_bar, |frame| frame.child(tab_bar))
            .child(body);

        let mut root = div()
            .size_full()
            .relative()
            // The chrome has one focus handle, and the ring inside it decides
            // which control the keys apply to. A text field binds its own,
            // more specific, key context and keeps the keys it needs.
            .track_focus(self.a11y.focus_handle())
            .key_context(self.key_context(cx).as_str())
            .on_action(cx.listener(Self::dismiss_overlay))
            .on_action(cx.listener(Self::auto_scroll_faster))
            .on_action(cx.listener(Self::auto_scroll_slower))
            .on_action(cx.listener(Self::auto_scroll_reverse))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_action(cx.listener(Self::focus_next_in_group))
            .on_action(cx.listener(Self::focus_previous_in_group))
            .on_action(cx.listener(Self::activate_focused))
            .child(frame);
        if self.menus.main_menu_open
            || self.menus.recent_menu_open
            || self.context_menus.tab_context_menu.is_some()
            || self.context_menus.canvas_context_menu.is_some()
        {
            root = root.child(
                div()
                    .id("menu-dismiss-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .on_click(cx.listener(|frame, event: &gpui::ClickEvent, _window, cx| {
                        if !event.is_right_click() {
                            frame.dismiss_menus(cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |frame, event, _window, cx| {
                            frame.dismiss_layer_right_click(event, document_bounds, cx);
                        }),
                    ),
            );
        }
        if self.menus.main_menu_open {
            root = root.child(self.render_main_menu(window, cx));
        }
        if self.menus.recent_menu_open {
            root = root.child(self.render_recent_menu(cx));
        }
        if let Some(menu) = self.context_menus.tab_context_menu {
            root = root.child(self.render_tab_context_menu(menu, window, cx));
        }
        if let Some(menu) = self.context_menus.canvas_context_menu {
            root = root.child(self.render_canvas_context_menu(menu, window, cx));
        }
        if self.search_panel_visible(cx) {
            root = root.child(self.render_search_results(cx));
        }
        if let Some(dialog) = self.dialog {
            root = root.child(render_dialog(
                self,
                dialog,
                self.a11y.rects.clone(),
                theme,
                cx,
            ));
        }

        // Published after the surfaces are built, so the description is of
        // this frame, and using the rectangles the previous frame measured,
        // which is the only frame that has any. What a screen reader asked
        // for is not run here: its handler wakes the shell instead, because a
        // window that is not visible draws no frame to run it on.
        let focused_field = self.focused_text_field(window, cx);
        let described = self.accessible(window, cx);
        self.a11y.publish(&described, focused_field, window, cx);
        root
    }
}

/// Drops a tab's walk, if it has one. Silent when it does not: cancelling a
/// search nobody started would repaint every tab on every keystroke.
fn cancel_find_on(canvas: &Entity<Canvas>, cx: &mut Context<ShellFrame>) {
    canvas.update(cx, |canvas, cx| {
        if canvas.model.search().needle().is_empty() {
            return;
        }
        canvas.model.cancel_search();
        canvas.handle_change(Ok(true), cx);
    });
}

fn document_view_bounds(
    viewport: gpui::Size<Pixels>,
    visibility: SurfaceVisibility,
    rail_expanded: bool,
    navigation_width: Pixels,
    side_panel_state: SidePanelState,
) -> gpui::Bounds<Pixels> {
    let header = header_height(visibility);
    let rail = if visibility.rail {
        rail_width(rail_expanded)
    } else {
        px(0.0)
    };
    let side_panel = if visibility.side_panel {
        side_panel_state.width()
    } else {
        px(0.0)
    };
    let page_controls = if visibility.page_controls {
        px(PAGE_CONTROLS_HEIGHT)
    } else {
        px(0.0)
    };
    gpui::Bounds {
        // The navigation column sits between the rail and the document, so
        // it moves the document's left edge as well as narrowing it.
        origin: Point {
            x: rail + navigation_width,
            y: header,
        },
        size: gpui::size(
            (viewport.width - rail - navigation_width - side_panel).max(px(0.0)),
            (viewport.height - header - page_controls).max(px(0.0)),
        ),
    }
}

/// How much chrome sits above the document: whichever of the two top bars is
/// on screen. Read Mode and Full Screen take them away, and the space has to
/// go back to the page rather than stay reserved.
fn header_height(visibility: SurfaceVisibility) -> Pixels {
    let global_bar = if visibility.global_bar {
        GLOBAL_BAR_HEIGHT
    } else {
        0.0
    };
    let tab_bar = if visibility.tab_bar {
        TAB_BAR_HEIGHT
    } else {
        0.0
    };
    px(global_bar + tab_bar)
}

/// A tab's text: its title, marked while the document has unsaved changes.
fn tab_label(tab: &DocumentTab, cx: &App) -> String {
    if ShellFrame::is_dirty(&tab.canvas, cx) {
        format!("● {}", tab.title())
    } else {
        tab.title().to_owned()
    }
}

fn tab_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn tab_element_id(path: &Path) -> Arc<Path> {
    Arc::from(path)
}

/// The encoded image on the clipboard, for any caller that makes something
/// from one: Create PDF From Clipboard here, and a pasted stamp. GPUI owns the
/// pasteboard, so this is the one place in the app that reads an image off
/// it.
pub(in crate::shell) fn clipboard_image(cx: &App) -> Result<Vec<u8>, String> {
    let item = cx
        .read_from_clipboard()
        .ok_or_else(|| "The clipboard is empty".to_owned())?;
    item.entries()
        .iter()
        .find_map(|entry| match entry {
            gpui::ClipboardEntry::Image(image) => Some(image.bytes.clone()),
            _ => None,
        })
        .ok_or_else(|| "The clipboard holds no image".to_owned())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "shell-test-support")]
    mod advanced_search;
    #[cfg(feature = "shell-test-support")]
    mod auto_scroll;
    #[cfg(feature = "shell-test-support")]
    mod comments;
    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    mod convert;
    #[cfg(all(feature = "shell-test-support", feature = "tools-edit"))]
    mod crop;
    #[cfg(feature = "shell-test-support")]
    mod edit_menu;
    #[cfg(feature = "shell-test-support")]
    mod export_selection;
    #[cfg(feature = "shell-test-support")]
    mod export_settings;
    #[cfg(feature = "shell-test-support")]
    mod file;
    #[cfg(all(feature = "shell-test-support", feature = "tools-form"))]
    mod forms;
    #[cfg(all(
        feature = "shell-test-support",
        feature = "tools-edit",
        feature = "codecs-common"
    ))]
    mod images;
    #[cfg(feature = "shell-test-support")]
    mod input_values;
    #[cfg(feature = "shell-test-support")]
    mod line_weights;
    #[cfg(all(feature = "shell-test-support", feature = "tools-edit"))]
    mod links;
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    mod manage_tools;
    #[cfg(all(feature = "shell-test-support", feature = "tools-edit"))]
    mod marks;
    #[cfg(all(feature = "shell-test-support", feature = "tools-measure"))]
    mod measure;
    #[cfg(all(feature = "shell-test-support", feature = "commands-core"))]
    mod native_input;
    #[cfg(feature = "shell-test-support")]
    mod new_window;
    #[cfg(all(feature = "shell-test-support", feature = "commands-core"))]
    mod organize_dialogs;
    #[cfg(feature = "shell-test-support")]
    mod outline;
    #[cfg(feature = "shell-test-support")]
    mod page_grid;
    #[cfg(all(feature = "shell-test-support", feature = "tools-form"))]
    mod prepare_form;
    #[cfg(feature = "shell-test-support")]
    mod print;
    #[cfg(all(
        feature = "shell-test-support",
        unix,
        not(target_os = "macos"),
        not(onionskin_check_windows)
    ))]
    mod print_cups;
    #[cfg(feature = "shell-test-support")]
    mod properties;
    #[cfg(all(feature = "shell-test-support", feature = "redact"))]
    mod redact;
    #[cfg(all(feature = "shell-test-support", feature = "commands-core"))]
    mod reduce;
    #[cfg(feature = "shell-test-support")]
    mod security;
    #[cfg(all(feature = "shell-test-support", feature = "tools-organize"))]
    mod send_pages;
    #[cfg(all(feature = "shell-test-support", feature = "tools-fill-sign"))]
    mod signature;
    mod signatures;
    #[cfg(feature = "shell-test-support")]
    mod skins;
    #[cfg(all(feature = "shell-test-support", feature = "spelling"))]
    mod spelling;
    #[cfg(all(
        feature = "shell-test-support",
        feature = "tools-comment",
        feature = "codecs-common"
    ))]
    mod stamps;
    #[cfg(all(feature = "shell-test-support", feature = "tools-comment"))]
    mod summary;
    #[cfg(all(feature = "shell-test-support", feature = "tools-edit"))]
    mod text_edit;

    // Before the split this module reached its parent through `use super::*`,
    // and a glob import is never reported as unused however many of its names
    // go unused. The names below are explicit now, so each one carries the
    // gate its users carry, or `--features shell` reports it as unused where
    // `--features shell,shell-test-support` does not.
    #[cfg(feature = "shell-test-support")]
    use std::sync::atomic::AtomicUsize;
    use std::sync::{mpsc, Arc, Condvar, Mutex};

    #[cfg(feature = "shell-test-support")]
    use accesskit::Role;
    #[cfg(feature = "shell-test-support")]
    use gpui::{TestAppContext, VisualTestContext};
    use onionskin_core::ViewPoint;
    #[cfg(feature = "shell-test-support")]
    use onionskin_plugin_api::PageRange;
    use onionskin_plugin_api::{
        CodecPlugin, ExportError, ExportOutputKind, ExportRequest, PageIndex, PluginRegistry,
        PointerInput, ToolCtx, ToolPlugin,
    };

    #[cfg(feature = "shell-test-support")]
    use super::context::CanvasContextMenu;
    #[cfg(feature = "shell-test-support")]
    use super::export::{run_export_worker, ExportJob, ExportOutcome, ExportPhase};
    use super::frame_state::TabState;
    use super::*;
    use crate::preferences::ThemePreference;
    use crate::shell::canvas::CanvasModel;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::canvas::PreparedExport;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::global_bar::ExportTarget;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::page_controls::PAGE_ENTRY_ID;
    use crate::shell::chrome::quick_actions::QuickActionsState;
    use crate::shell::chrome::theme::ShellViewState;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::dialog::ShellDialog;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::find_bar::FIND_INPUT_ID;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::ShellSettings;

    /// A window with both top bars on screen, which is what every layout
    /// case below is measured against, with the three surfaces those cases
    /// vary.
    fn chrome(rail: bool, side_panel: bool, page_controls: bool) -> SurfaceVisibility {
        SurfaceVisibility {
            global_bar: true,
            tab_bar: true,
            rail,
            navigation_pane: true,
            quick_actions: true,
            side_panel,
            page_controls,
        }
    }

    pub(super) struct BlockingCodec {
        pub(super) kind: ExportOutputKind,
        pub(super) block_on: PageIndex,
        pub(super) calls: Arc<Mutex<Vec<PageIndex>>>,
        pub(super) started: Mutex<Option<mpsc::Sender<()>>>,
        pub(super) release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl CodecPlugin for BlockingCodec {
        fn id(&self) -> &'static str {
            "blocking"
        }

        fn name(&self) -> &'static str {
            "Blocking"
        }

        fn extension(&self) -> &'static str {
            "test"
        }

        fn output_kind(&self) -> ExportOutputKind {
            self.kind
        }

        fn export_page(
            &self,
            _doc: &mut Document,
            _request: &ExportRequest,
            page: PageIndex,
            _first_in_request: bool,
        ) -> Result<Vec<u8>, ExportError> {
            self.calls.lock().unwrap().push(page);
            if page == self.block_on {
                if let Some(started) = self.started.lock().unwrap().take() {
                    started.send(()).unwrap();
                }
                let (released, ready) = &*self.release;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
            }
            Ok(format!("page-{page}").into_bytes())
        }
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_blocked_export_leaves_tab_actions_responsive_and_cancel_stops_the_next_page(
        cx: &mut TestAppContext,
    ) {
        let window = export_settings::settings_window(cx);
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("blocked.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (started_tx, started_rx) = mpsc::channel();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(3))
            .expect("the fixture opens");
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().expect("the snapshot prepares"),
            codec: Arc::new(BlockingCodec {
                kind: ExportOutputKind::PerPage,
                block_on: 1,
                calls: Arc::clone(&calls),
                started: Mutex::new(Some(started_tx)),
                release: Arc::clone(&release),
            }),
            request: ExportRequest {
                pages: PageRange::whole(3).unwrap(),
                dpi: 72.0,
                quality: None,
            },
            output_kind: ExportOutputKind::PerPage,
            page_count: 3,
        };

        let phase = Arc::new(ExportPhase::new());
        let completed = Arc::new(AtomicUsize::new(0));
        let worker_phase = Arc::clone(&phase);
        let worker_completed = Arc::clone(&completed);
        let worker = std::thread::spawn(move || {
            run_export_worker(prepared, &chosen, &worker_phase, &worker_completed)
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the background codec started");

        window
            .update(cx, |frame, window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                frame.export.export_job = Some(ExportJob {
                    id: 7,
                    origin,
                    phase: Arc::clone(&phase),
                    completed: Arc::clone(&completed),
                    total: 3,
                    last_displayed: 0,
                });
                assert_eq!(frame.poll_export_progress(7), (true, true));
                assert_eq!(
                    frame
                        .accessible(window, cx)
                        .find(&"export-progress".into())
                        .unwrap()
                        .label,
                    "Exporting 1 of 3 pages"
                );
                frame.activate(1, cx);
                assert_eq!(frame.tabs.active_index(), Some(1));
                frame.run_activation(Activation::CancelExport, window, cx);
            })
            .unwrap();
        {
            let (released, ready) = &*release;
            *released.lock().unwrap() = true;
            ready.notify_all();
        }
        assert_eq!(worker.join().unwrap().unwrap(), ExportOutcome::Cancelled);

        assert_eq!(*calls.lock().unwrap(), [0, 1]);
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
        window
            .update(cx, |frame, window, cx| {
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
                let canvas = frame
                    .tabs
                    .tabs()
                    .iter()
                    .find(|tab| {
                        tab.canvas.entity_id() == frame.export.export_job.as_ref().unwrap().origin
                    })
                    .unwrap()
                    .canvas
                    .clone();
                let origin = frame.export.export_job.as_ref().unwrap().origin;
                frame.finish_export(7, origin, &canvas, Ok(ExportOutcome::Cancelled), cx);
                assert!(frame.export.export_job.is_none());
                assert!(frame
                    .accessible(window, cx)
                    .find(&"export-progress".into())
                    .is_none());
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
    }

    struct OriginRecordingTool {
        inputs: Arc<Mutex<Vec<PointerInput>>>,
    }

    impl ToolPlugin for OriginRecordingTool {
        fn id(&self) -> &'static str {
            "origin-recording"
        }

        fn name(&self) -> &'static str {
            "Origin recording"
        }

        fn icon(&self) -> &'static str {
            "origin-recording"
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
            self.inputs.lock().unwrap().push(input);
        }

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    }

    #[test]
    fn rails_and_the_side_panel_adjust_document_bounds_once() {
        let viewport = gpui::size(px(1_100.0), px(860.0));

        let collapsed_closed = document_view_bounds(
            viewport,
            chrome(true, true, true),
            false,
            px(0.0),
            SidePanelState::Closed,
        );
        assert_eq!(
            collapsed_closed.origin,
            Point {
                x: px(88.0),
                y: px(76.0)
            }
        );
        assert_eq!(collapsed_closed.size, gpui::size(px(972.0), px(736.0)));

        let expanded_closed = document_view_bounds(
            viewport,
            chrome(true, true, true),
            true,
            px(0.0),
            SidePanelState::Closed,
        );
        assert_eq!(
            expanded_closed.origin,
            Point {
                x: px(240.0),
                y: px(76.0)
            }
        );
        assert_eq!(expanded_closed.size, gpui::size(px(820.0), px(736.0)));

        let collapsed_open = document_view_bounds(
            viewport,
            chrome(true, true, true),
            false,
            px(0.0),
            SidePanelState::OpenEmpty,
        );
        assert_eq!(collapsed_open.origin, collapsed_closed.origin);
        assert_eq!(collapsed_open.size, gpui::size(px(732.0), px(736.0)));

        let expanded_open = document_view_bounds(
            viewport,
            chrome(true, true, true),
            true,
            px(0.0),
            SidePanelState::OpenEmpty,
        );
        assert_eq!(expanded_open.origin, expanded_closed.origin);
        assert_eq!(expanded_open.size, gpui::size(px(580.0), px(736.0)));
        assert_eq!(
            QuickActionsState::default()
                .toolbar_size(expanded_open.size)
                .width,
            expanded_open.size.width
        );
    }

    /// The column sits between the rail and the document, so it moves the
    /// document's left edge as well as taking width from it. Subtracting it
    /// from the width alone would leave the canvas drawing under the pane and
    /// mapping every pointer position wrongly.
    #[test]
    fn the_navigation_column_moves_the_document_and_narrows_it_by_the_same_amount() {
        let viewport = gpui::size(px(1_100.0), px(860.0));
        let closed = NavigationPanesState::default().width();

        let without = document_view_bounds(
            viewport,
            chrome(true, true, true),
            false,
            px(0.0),
            SidePanelState::Closed,
        );
        let with_strip = document_view_bounds(
            viewport,
            chrome(true, true, true),
            false,
            closed,
            SidePanelState::Closed,
        );

        assert!(closed > px(0.0), "the button strip is always on screen");
        assert_eq!(with_strip.origin.x, without.origin.x + closed);
        assert_eq!(with_strip.size.width, without.size.width - closed);
        assert_eq!(with_strip.size.height, without.size.height);
    }

    #[test]
    fn hidden_document_chrome_returns_its_space_to_the_canvas() {
        let bounds = document_view_bounds(
            gpui::size(px(1_100.0), px(860.0)),
            chrome(false, false, false),
            true,
            px(0.0),
            SidePanelState::OpenEmpty,
        );

        assert_eq!(
            bounds.origin,
            Point {
                x: px(0.0),
                y: px(76.0)
            }
        );
        assert_eq!(bounds.size, gpui::size(px(1_100.0), px(784.0)));
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn every_open_navigation_pane_body_gets_rendered_bounds(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let mut cx = VisualTestContext::from_window(window.into(), cx);

        for pane in panes::NavigationPane::ALL {
            window
                .update(&mut cx, |frame, _window, cx| {
                    frame.run_pane_action(PaneAction::Select(pane), cx);
                })
                .unwrap();
            cx.run_until_parked();
            draw_window(&mut cx);

            window
                .update(&mut cx, |frame, window, cx| {
                    let tree = frame.accessible(window, cx);
                    let body = tree
                        .find(&"navigation-pane-body".into())
                        .and_then(|body| body.bounds)
                        .unwrap_or_else(|| panic!("{pane:?} body has no rendered bounds"));
                    assert!(
                        body.x1 > body.x0 && body.y1 > body.y0,
                        "{pane:?} body has no rendered area: {body:?}"
                    );
                })
                .unwrap();
        }
    }

    #[cfg(feature = "shell-test-support")]
    pub(super) fn draw_window(cx: &mut VisualTestContext) {
        cx.update(|window, app| {
            window.draw(app).clear();
        });
    }

    /// A window with the routes the app installs at startup: the keymap's
    /// keybindings and the one action listener behind them.
    #[cfg(feature = "shell-test-support")]
    pub(super) fn bound_window(
        seeds: &[&str],
        cx: &mut TestAppContext,
    ) -> (gpui::WindowHandle<ShellFrame>, Vec<crate::keymap::Binding>) {
        bound_window_in(seeds, crate::config::ConfigPaths::default(), cx)
    }

    #[cfg(feature = "shell-test-support")]
    pub(super) fn bound_window_in(
        seeds: &[&str],
        paths: crate::config::ConfigPaths,
        cx: &mut TestAppContext,
    ) -> (gpui::WindowHandle<ShellFrame>, Vec<crate::keymap::Binding>) {
        let tabs: Vec<_> = seeds
            .iter()
            .map(|seed| {
                let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../corpus/seeds")
                    .join(seed);
                let document = Document::open_path(&path).expect("the seed opens");
                let model = CanvasModel::new(
                    document,
                    crate::build_registry(),
                    ViewSize {
                        width: 800.0,
                        height: 600.0,
                    },
                )
                .expect("the model builds");
                (path, model)
            })
            .collect();
        bound_window_with_models(tabs, paths, cx)
    }

    #[cfg(feature = "shell-test-support")]
    pub(super) fn bound_window_from_bytes(
        documents: Vec<(&str, Vec<u8>)>,
        cx: &mut TestAppContext,
    ) -> (gpui::WindowHandle<ShellFrame>, Vec<crate::keymap::Binding>) {
        let tabs = documents
            .into_iter()
            .map(|(name, bytes)| {
                let document = Document::open_bytes(bytes).expect("the fixture opens");
                let model = CanvasModel::new(
                    document,
                    crate::build_registry(),
                    ViewSize {
                        width: 800.0,
                        height: 600.0,
                    },
                )
                .expect("the model builds");
                (PathBuf::from(name), model)
            })
            .collect();
        bound_window_with_models(tabs, crate::config::ConfigPaths::default(), cx)
    }

    #[cfg(feature = "shell-test-support")]
    fn bound_window_with_models(
        tabs: Vec<(PathBuf, CanvasModel)>,
        paths: crate::config::ConfigPaths,
        cx: &mut TestAppContext,
    ) -> (gpui::WindowHandle<ShellFrame>, Vec<crate::keymap::Binding>) {
        let settings = ShellSettings::load(paths, &crate::build_registry());
        let bindings = settings.bindings.clone();
        // As `run` builds it: the check mark starts from the saved setting.
        let shell_view = ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System)
            .with_line_weights(settings.preferences.line_weights);
        let theme = shell_view.tokens();
        let window = cx.add_window(move |window, cx| {
            let tabs = tabs
                .into_iter()
                .map(|(path, model)| (path, cx.new(|_| Canvas::new(model, theme))))
                .collect();
            ShellFrame::new(tabs, shell_view, settings, window, cx)
        });
        let state = window
            .update(cx, |frame, _window, cx| frame.menu_state(cx))
            .unwrap();
        let installed = bindings.clone();
        // In the order `shell::run` installs them, and all of them: the
        // search field's own bindings were missing here, so a shell binding
        // that shadowed one of the field's keys looked harmless.
        cx.update(|cx| {
            crate::shell::chrome::install_search_keybindings(cx);
            crate::shell::find_bar::install_keybindings(cx);
            crate::shell::inline_text::install_keybindings(cx);
            #[cfg(feature = "tools-form")]
            crate::shell::field_editor::install_keybindings(cx);
            #[cfg(feature = "tools-edit")]
            crate::shell::line_editor::install_keybindings(cx);
            crate::shell::panes::install_comment_keybindings(cx);
            crate::shell::preferences_dialog::install_keybindings(cx);
            crate::shell::chrome::export_dialog::install_keybindings(cx);
            crate::shell::chrome::accessible::install_keybindings(cx);
            super::auto_scroll::install_keybindings(cx);
            crate::shell::install_command_keybindings(cx, &installed);
            super::super::global_bar::install_native_menus(cx, window, state);
        });
        (window, bindings)
    }

    #[cfg(feature = "shell-test-support")]
    pub(super) fn keystroke_for(bindings: &[crate::keymap::Binding], id: &str) -> String {
        let binding = bindings
            .iter()
            .find(|binding| binding.id == id)
            .unwrap_or_else(|| panic!("{id} is bound"));
        crate::keymap::platform_keystroke(&binding.keystroke, cfg!(target_os = "macos"))
    }

    /// Read Mode hides the top bars, and hiding a control means taking it
    /// out of the accessibility tree: a surface that is off screen but still
    /// described is an invisible tab stop the ring lands on.
    ///
    /// Pressed rather than called, through the binding `keymap.json` gives
    /// it, because Acrobat's Ctrl+H is Hide on macOS and the command ships
    /// unbound.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn read_mode_takes_the_top_bars_out_of_the_tree_and_keeps_the_page_controls(
        cx: &mut TestAppContext,
    ) {
        let dir = crate::config::test_dir("read-mode-keymap");
        std::fs::write(
            dir.join(crate::config::KEYMAP_FILE),
            "{\"view.read-mode\": \"cmd-shift-r\"}",
        )
        .expect("the test writes its keymap");
        let (window, bindings) =
            bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::in_dir(&dir), cx);
        window
            .update(cx, |frame, window, cx| {
                // A query left in the global bar's field, so the panel it
                // opens has to go with the bar rather than hang under it.
                frame
                    .tool_search
                    .search_input
                    .update(cx, |input, cx| input.set_query("zoom", cx));
                let tree = frame.accessible(window, cx);
                assert!(tree.find(&"global-bar".into()).is_some());
                assert!(tree.find(&"tab-bar".into()).is_some());
                assert!(tree.find(&"tool-rail".into()).is_some());
                assert!(tree.find(&"global-search-results".into()).is_some());
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "view.read-mode"));
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                assert!(frame.shell_view_state.read_mode(), "the keystroke missed");
                let tree = frame.accessible(window, cx);
                for gone in [
                    "global-bar",
                    "tab-bar",
                    "tool-rail",
                    "quick-actions",
                    "global-search-results",
                ] {
                    assert!(
                        tree.find(&gone.into()).is_none(),
                        "{gone} is hidden but still described"
                    );
                }
                assert!(
                    tree.find(&"page-controls".into()).is_some(),
                    "read mode left no toolbar to read with"
                );
            })
            .unwrap();

        // Escape is the way out, because there is no chrome left to click.
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                assert!(!frame.shell_view_state.read_mode(), "escape did not exit");
                assert!(frame
                    .accessible(window, cx)
                    .find(&"global-bar".into())
                    .is_some());
            })
            .unwrap();
    }

    /// Leaving a chromeless mode is the LAST thing Escape does, so every
    /// overlay above it still gets the key first.
    ///
    /// The ordering is load-bearing and nothing else pins it: moving the two
    /// mode checks to the top of the dismissal chain leaves every other test
    /// in this file passing while Escape stops closing the dialog, the
    /// search panel, a context menu, a menu and the find bar. Each stage is
    /// armed in turn, and the mode has to survive the key it did not get.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn escape_closes_an_overlay_before_it_leaves_a_chromeless_mode(cx: &mut TestAppContext) {
        type Arm = fn(&mut ShellFrame, &mut Window, &mut Context<ShellFrame>);
        type IsOpen = fn(&ShellFrame, &App) -> bool;
        let stages: [(&str, Arm, IsOpen); 4] = [
            (
                "the Zoom To dialog",
                |frame, window, cx| frame.show_dialog(ShellDialog::ZoomTo, window, cx),
                |frame, _cx| frame.dialog.is_some(),
            ),
            (
                "the canvas context menu",
                |frame, _window, cx| {
                    frame.context_menus.canvas_context_menu = Some(CanvasContextMenu {
                        origin: gpui::point(px(300.0), px(300.0)),
                    });
                    cx.notify();
                },
                |frame, _cx| frame.context_menus.canvas_context_menu.is_some(),
            ),
            (
                "the main menu",
                |frame, _window, cx| frame.toggle_main_menu(cx),
                |frame, _cx| frame.menus.main_menu_open,
            ),
            (
                "the find bar",
                |frame, window, cx| frame.open_find_bar(None, window, cx),
                |frame, _cx| frame.find.is_open(),
            ),
        ];

        // Read Mode first, then Full Screen: each mode's own check sits
        // below every stage, and each has to stay put.
        for (fullscreen, read_mode) in [(false, true), (true, false)] {
            for (name, arm, is_open) in stages {
                let (window, _) = bound_window(&["hello.pdf"], cx);
                window
                    .update(cx, |frame, window, cx| {
                        if fullscreen {
                            frame.shell_view_state.set_fullscreen(true);
                        }
                        if read_mode {
                            frame.apply_shell_view_action(ShellViewAction::ToggleReadMode, cx);
                        }
                        // The fifth stage of the chain, the tool search
                        // panel, cannot be one here: it belongs to the
                        // global bar's field, which these modes hide, so it
                        // does not open however much is typed into it.
                        frame
                            .tool_search
                            .search_input
                            .update(cx, |input, cx| input.set_query("zoom", cx));
                        assert!(
                            !frame.search_panel_visible(cx),
                            "the search panel opened with the global bar hidden"
                        );
                        arm(frame, window, cx);
                        assert!(is_open(frame, cx), "{name} did not arm");
                    })
                    .unwrap();

                cx.simulate_keystrokes(window.into(), "escape");
                cx.run_until_parked();

                window
                    .update(cx, |frame, _window, cx| {
                        assert!(!is_open(frame, cx), "escape did not close {name}");
                        assert_eq!(
                            frame.shell_view_state.fullscreen(),
                            fullscreen,
                            "escape left full screen while {name} was open"
                        );
                        assert_eq!(
                            frame.shell_view_state.read_mode(),
                            read_mode,
                            "escape left read mode while {name} was open"
                        );
                    })
                    .unwrap();
            }
        }
    }

    /// Full Screen is the document and nothing else, and Escape answers it
    /// before Read Mode: a window in both comes back one step at a time.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn full_screen_leaves_no_chrome_described_and_escape_answers_it_first(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        window
            .update(cx, |frame, window, cx| {
                assert!(frame.shell_view_state.set_fullscreen(true));
                assert!(frame.apply_shell_view_action(ShellViewAction::ToggleReadMode, cx));

                let tree = frame.accessible(window, cx);
                for gone in [
                    "global-bar",
                    "tab-bar",
                    "tool-rail",
                    "quick-actions",
                    "page-controls",
                ] {
                    assert!(
                        tree.find(&gone.into()).is_none(),
                        "{gone} is hidden but still described"
                    );
                }
                let visibility = frame.shell_view_state.visibility();
                assert_eq!(header_height(visibility), px(0.0));
                assert_eq!(
                    document_view_bounds(
                        window.viewport_size(),
                        visibility,
                        frame.rail_state.expanded(),
                        frame.navigation_width(visibility.navigation_pane),
                        frame.side_panel_state,
                    )
                    .size,
                    window.viewport_size(),
                    "the hidden chrome kept its space"
                );
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| {
                assert!(
                    frame.shell_view_state.read_mode(),
                    "escape left read mode while full screen was still on"
                );
            })
            .unwrap();
    }

    /// A file that only opens because cos repaired it, built the way
    /// `corpus/make-malformed.sh`'s junk-header variant builds one: bytes
    /// ahead of `%PDF-`, which leaves every cross-reference offset short.
    #[cfg(feature = "shell-test-support")]
    fn junk_header_pdf(name: &str) -> PathBuf {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let mut bytes = b"% this line is not part of the PDF\n".to_vec();
        bytes.extend(std::fs::read(seed).expect("the seed reads"));
        let path = crate::config::test_dir("repair-notice").join(name);
        std::fs::write(&path, bytes).expect("the test writes its file");
        path
    }

    /// Decision 10 opens a repaired file; it does not open one silently.
    /// The notice names the file and what the repair did, from the report
    /// rather than from a fixed sentence.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_repaired_document_says_so_where_the_user_can_read_it(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let broken = junk_header_pdf("junk-header.pdf");

        window
            .update(cx, |frame, _window, cx| {
                assert!(frame.notices.is_empty(), "{:?}", frame.notices);
                frame.open_documents(std::slice::from_ref(&broken), cx);
            })
            .unwrap();

        window
            .update(cx, |frame, _window, cx| {
                assert_eq!(frame.tabs.tabs().len(), 2);
                let notices = frame.notices.join(" | ");
                assert!(notices.contains("junk-header.pdf"), "{notices}");
                assert!(notices.contains("repaired"), "{notices}");
                let summary = frame
                    .tabs
                    .active()
                    .unwrap()
                    .canvas
                    .read(cx)
                    .model
                    .provenance()
                    .report()
                    .expect("the file needed repair")
                    .summary();
                assert!(
                    notices.contains(&summary),
                    "the notice does not carry the repair report: {notices}"
                );
            })
            .unwrap();
    }

    /// Select All is bound window-wide, and the search field binds the same
    /// keystroke in its own key context. The field has to win while it has
    /// focus, or typing in the chrome would select the document instead.
    #[cfg(all(feature = "shell-test-support", feature = "commands-core"))]
    #[gpui::test]
    fn a_field_that_binds_a_keystroke_keeps_it_while_it_has_focus(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);
        cx.update(super::super::tool_search::install_keybindings);
        window
            .update(cx, |frame, window, cx| {
                frame
                    .tool_search
                    .search_input
                    .update(cx, |input, cx| input.set_query("find me".to_owned(), cx));
                window.focus(&frame.tool_search.search_input.read(cx).focus_handle(cx));
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "edit.select-all"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                assert_eq!(
                    frame.tool_search.search_input.read(cx).selected_range(),
                    0.."find me".len(),
                    "the field's own Select All did not run"
                );
                assert_eq!(
                    frame
                        .tabs
                        .active()
                        .unwrap()
                        .canvas
                        .read(cx)
                        .model
                        .selection_text(),
                    None,
                    "the document was selected from inside a text field"
                );
            })
            .unwrap();
    }

    /// Closing the last document leaves the window on Home rather than
    /// taking the window down, which is what Acrobat does and what makes the
    /// no-document state reachable at all.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn closing_the_last_document_returns_to_home(cx: &mut TestAppContext) {
        let (window, bindings) = bound_window(&["hello.pdf"], cx);

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "file.close"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                assert!(frame.tabs.is_empty());
                assert!(frame.active_view_state(cx).is_none());
                // And the menus follow: the document commands say so rather
                // than pointing at a document that is not there. Close is
                // the one to ask, because its reason does not depend on
                // which plugins this build compiled in.
                assert_eq!(
                    frame.command_unavailable(MenuCommand::CloseTab, cx),
                    Some("No document is open")
                );
                assert_eq!(frame.command_unavailable(MenuCommand::Open, cx), None);
            })
            .expect("the window is still open");
    }

    /// Home's own controls: the toggle switches views and the recents rows
    /// are the same list File > Open Recent reads.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn home_lists_the_recents_and_switches_to_thumbnails(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("home-view");
        let _ = std::fs::remove_file(dir.join(crate::config::RECENTS_FILE));
        let (window, _) = bound_window_in(&[], crate::config::ConfigPaths::in_dir(&dir), cx);
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");

        window
            .update(cx, |frame, _window, cx| {
                frame.open_documents(std::slice::from_ref(&seed), cx);
                frame.run_tab_command(TabCommand::CloseAll, 0, cx).unwrap();

                assert!(frame.tabs.is_empty(), "back on Home");
                assert_eq!(frame.settings.recents.documents().len(), 1);
                assert_eq!(frame.home.view(), HomeView::List);

                frame.set_home_view(HomeView::Thumbnail, cx);
                assert_eq!(frame.home.view(), HomeView::Thumbnail);
                assert!(
                    frame
                        .home
                        .thumbnail(&frame.settings.recents.documents()[0].path)
                        .is_some_and(Result::is_ok),
                    "the recent document has no page on its card"
                );
            })
            .unwrap();
    }

    /// Starring from Home writes the star to the recents file, survives the
    /// app starting again, and the Starred row opens the document.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_star_on_home_is_saved_and_opens_its_document(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("home-star");
        let _ = std::fs::remove_file(dir.join(crate::config::RECENTS_FILE));
        let (window, _) = bound_window_in(&[], crate::config::ConfigPaths::in_dir(&dir), cx);
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let seed = std::path::absolute(seed).unwrap();

        window
            .update(cx, |frame, window, cx| {
                frame.open_documents(std::slice::from_ref(&seed), cx);
                frame.run_tab_command(TabCommand::CloseAll, 0, cx).unwrap();
                let tree = frame.accessible(window, cx);
                let star = tree
                    .find(&("home-star", 0usize).into())
                    .expect("the row has a star")
                    .activation
                    .clone()
                    .expect("operable");
                frame.run_activation(star, window, cx);
            })
            .unwrap();
        let (reloaded, errors) =
            crate::recents::Recents::load(Some(&dir.join(crate::config::RECENTS_FILE)));
        assert!(errors.is_empty());
        assert_eq!(
            reloaded.starred(),
            std::slice::from_ref(&seed),
            "written, and read back"
        );
        window
            .update(cx, |frame, window, cx| {
                frame.run_activation(Activation::OpenStarred(0), window, cx);
                assert_eq!(frame.tabs.tabs().len(), 1, "the starred document opened");
            })
            .unwrap();
    }

    /// A document opened while Home is on thumbnails gets a card without
    /// the user toggling the view again.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_document_opened_from_home_gets_a_thumbnail_without_a_second_toggle(
        cx: &mut TestAppContext,
    ) {
        let dir = crate::config::test_dir("home-refresh");
        let _ = std::fs::remove_file(dir.join(crate::config::RECENTS_FILE));
        let (window, _) = bound_window_in(&[], crate::config::ConfigPaths::in_dir(&dir), cx);
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");

        window
            .update(cx, |frame, _window, cx| {
                frame.set_home_view(HomeView::Thumbnail, cx);
                frame.open_documents(std::slice::from_ref(&seed), cx);

                assert!(
                    frame
                        .home
                        .thumbnail(&frame.settings.recents.documents()[0].path)
                        .is_some_and(Result::is_ok),
                    "the document just opened has no card"
                );
            })
            .unwrap();
    }

    /// A clean file gets no notice, so the notice bar means something.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_clean_document_opens_without_a_notice(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");

        window
            .update(cx, |frame, _window, cx| {
                frame.open_documents(&[seed], cx);
                assert_eq!(frame.tabs.tabs().len(), 2);
                assert!(frame.notices.is_empty(), "{:?}", frame.notices);
            })
            .unwrap();
    }

    /// Opening writes the recents list, and Open Recent opens what it
    /// recorded. The file is checked as well as the in-memory list: a
    /// recents list that does not survive the session is not one.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn opening_a_document_records_it_and_open_recent_opens_it_again(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("recents-frame");
        let recents_file = dir.join(crate::config::RECENTS_FILE);
        let _ = std::fs::remove_file(&recents_file);
        let (window, _) = bound_window_in(&[], crate::config::ConfigPaths::in_dir(&dir), cx);
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");

        window
            .update(cx, |frame, _window, cx| {
                frame.open_documents(std::slice::from_ref(&seed), cx);
            })
            .unwrap();

        window
            .update(cx, |frame, _window, _cx| {
                assert_eq!(frame.tabs.tabs().len(), 1);
                assert_eq!(
                    frame
                        .settings
                        .recents
                        .documents()
                        .iter()
                        .map(|recent| recent.title())
                        .collect::<Vec<_>>(),
                    vec!["two-page.pdf".to_owned()]
                );
            })
            .unwrap();
        let (saved, errors) = crate::recents::Recents::load(Some(&recents_file));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(saved.documents().len(), 1, "the list reached disk");
        assert!(saved.documents()[0].path.is_absolute());

        // Open Recent on a document that is already open raises its tab
        // rather than opening it twice.
        window
            .update(cx, |frame, _window, cx| {
                frame.open_recent(0, cx);
                assert_eq!(frame.tabs.tabs().len(), 1);
                assert!(!frame.menus.recent_menu_open);
            })
            .unwrap();
    }

    /// One unopenable path does not cost the user the others.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_document_that_will_not_open_is_reported_and_the_rest_still_open(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&[], cx);
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/gone.pdf");
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");

        window
            .update(cx, |frame, _window, cx| {
                frame.open_documents(&[missing, seed], cx);

                assert_eq!(frame.tabs.tabs().len(), 1);
                assert_eq!(frame.notices.len(), 1, "{:?}", frame.notices);
                assert!(frame.notices[0].contains("gone.pdf"), "{:?}", frame.notices);
            })
            .unwrap();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_document_search_route_opens_the_find_bar_and_escape_closes_it(cx: &mut TestAppContext) {
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
        let shell_view = ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System);
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
                frame.choose_search_result(
                    SearchResult::DocumentSearch {
                        query: "Onionskin".to_owned(),
                    },
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let open = frame.read(app);
            assert!(open.find.is_open());
            assert!(
                open.tool_search.search_feedback.is_none(),
                "the document-text route is live, not a deferred milestone"
            );
            assert_eq!(open.find_input.read(app).query(), "Onionskin");
            // The walk is the canvas's, and it carries the query the global
            // bar typed rather than a copy the bar keeps in step by hand.
            let canvas = open.tabs.active().unwrap().canvas.read(app);
            assert_eq!(canvas.model.search().needle(), "Onionskin");
        });

        cx.update(|window, app| {
            frame.update(app, |frame, cx| {
                frame.dismiss_overlay(&Dismiss, window, cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let closed = frame.read(app);
            assert!(!closed.find.is_open());
            let canvas = closed.tabs.active().unwrap().canvas.read(app);
            assert_eq!(
                canvas.model.search().needle(),
                "",
                "closing the bar drops the walk and its highlights"
            );
            assert!(canvas.model.search().is_empty());
        });
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn document_search_without_an_open_document_renders_its_unavailable_reason(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&[], cx);

        let results = window
            .update(cx, |frame, _window, cx| {
                frame.tool_search.search_input.update(cx, |input, cx| {
                    input.set_query("needle", cx);
                });
                frame.search_results(cx)
            })
            .unwrap();

        assert_eq!(
            results,
            [SearchResult::Unavailable {
                label: "Search document for \"needle\"".to_owned(),
                reason: "No document is open",
            }]
        );
    }

    #[test]
    fn composite_layout_origin_reaches_canvas_pointer_mapping() {
        let bounds = document_view_bounds(
            gpui::size(px(1_100.0), px(860.0)),
            chrome(true, true, true),
            false,
            px(0.0),
            SidePanelState::OpenEmpty,
        );
        let inputs = Arc::new(Mutex::new(Vec::new()));
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(OriginRecordingTool {
            inputs: Arc::clone(&inputs),
        }));
        let document = Document::open_path(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf"),
        )
        .unwrap();
        let model = CanvasModel::new(
            document,
            registry,
            ViewSize {
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            },
        )
        .unwrap();
        let theme =
            ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System).tokens();
        let mut canvas = Canvas::new(model, theme);
        let origin = ViewPoint {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
        };
        canvas.resize_for_bounds(bounds).unwrap();
        let page = canvas.model.viewport().visible_pages().unwrap()[0].rect;
        let local_point = ViewPoint {
            x: page.origin.x + page.size.width / 2.0,
            y: page.origin.y + page.size.height / 2.0,
        };
        let expected = canvas
            .model
            .viewport()
            .page_point_at(local_point)
            .unwrap()
            .unwrap();
        let window_point = Point {
            x: bounds.origin.x + px(local_point.x),
            y: bounds.origin.y + px(local_point.y),
        };

        assert!(canvas
            .model
            .pointer_down(window_point, 1.0, gpui::Modifiers::default())
            .unwrap());
        assert_eq!(
            inputs.lock().unwrap().as_slice(),
            &[PointerInput {
                at: expected,
                pressure: 1.0,
                modifiers: onionskin_core::Modifiers::default(),
                clicks: 1,
            }]
        );
        assert_eq!(canvas.model.canvas_origin(), origin);
        assert_eq!(
            canvas.model.viewport().size(),
            ViewSize {
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            }
        );
    }

    #[test]
    fn duplicate_titles_keep_distinct_path_identity() {
        let first = PathBuf::from("/first/report.pdf");
        let second = PathBuf::from("/second/report.pdf");

        assert_eq!(tab_title(&first), tab_title(&second));
        assert_ne!(tab_element_id(&first), tab_element_id(&second));
    }

    #[test]
    fn activating_another_tab_clears_search_feedback() {
        let mut tabs = TabState::new(vec!["one", "two"]);
        let mut feedback = Some(SearchResult::Unavailable {
            label: "Old tab".to_owned(),
            reason: "Old result",
        });

        assert!(activate_tab(&mut tabs, &mut feedback, 1));
        assert_eq!(tabs.active_index(), Some(1));
        assert_eq!(feedback, None);
    }

    #[test]
    fn closing_the_active_tab_clears_search_feedback() {
        let mut tabs = TabState::new(vec!["one", "two"]);
        let mut feedback = Some(SearchResult::Unavailable {
            label: "Old tab".to_owned(),
            reason: "Old result",
        });

        assert!(!close_tab(&mut tabs, &mut feedback, 0).unwrap());
        assert_eq!(tabs.active(), Some(&"two"));
        assert_eq!(feedback, None);
    }

    #[test]
    fn closing_other_tabs_from_an_inactive_tab_clears_search_feedback() {
        let mut tabs = TabState::new(vec!["one", "two", "three"]);
        let mut feedback = Some(SearchResult::Unavailable {
            label: "Old tab".to_owned(),
            reason: "Old result",
        });

        close_other_tabs(&mut tabs, &mut feedback, 1).unwrap();
        assert_eq!(tabs.tabs(), &["two"]);
        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(feedback, None);
    }

    #[cfg(feature = "shell-test-support")]
    pub(super) fn install_test_export_job(
        frame: &mut ShellFrame,
        origin: EntityId,
    ) -> Arc<ExportPhase> {
        let phase = Arc::new(ExportPhase::new());
        frame.export.export_job = Some(ExportJob {
            id: 7,
            origin,
            phase: Arc::clone(&phase),
            completed: Arc::new(AtomicUsize::new(1)),
            total: 3,
            last_displayed: 1,
        });
        phase
    }

    /// A tool that will not activate used to be silent in the chrome: the rail
    /// and quick-action paths dropped the error with `let _ =`, and the search
    /// panel only cleared its feedback on success, so a click there left the
    /// panel showing whatever it said before. The canvas records the error's
    /// own words underneath, but the panel is drawn over them.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_tool_that_will_not_activate_says_so_in_the_chrome(cx: &mut TestAppContext) {
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
        let shell_view = ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System);
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

        // The registry has no tools, so every index is out of range.
        cx.update(|window, app| {
            frame.update(app, |frame, cx| {
                frame.choose_search_result(
                    SearchResult::Tool {
                        index: 3,
                        id: "onionskin.absent",
                        name: "Absent",
                    },
                    window,
                    cx,
                );
            });
        });
        cx.update(|_window, app| {
            assert_eq!(
                frame.read(app).tool_search.search_feedback,
                Some(SearchResult::Unavailable {
                    label: "Absent".to_owned(),
                    reason: TOOL_ACTIVATION_FAILED,
                }),
                "the search panel said nothing about a tool that did not activate"
            );
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.tool_search.search_feedback = None;
                frame.select_rail_entry(
                    RailEntry {
                        registry_index: 3,
                        id: "onionskin.absent",
                        name: "Absent",
                        icon: "?",
                        shortcut: None,
                        group: "none",
                        active: false,
                    },
                    cx,
                );
            });
        });
        cx.update(|_window, app| {
            assert!(
                frame.read(app).tool_search.search_feedback.is_some(),
                "the rail dropped the activation error"
            );
        });
    }
}
