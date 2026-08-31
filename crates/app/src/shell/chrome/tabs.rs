use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, ClipboardItem, Context, Entity, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _,
    PathPromptOptions, Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _, Window,
    WindowHandle,
};
use onionskin_core::{Document, ViewSize};
use onionskin_plugin_api::{ExportedFile, PageIndex, ToolCapability};

use super::super::canvas::{CanvasError, CanvasModel, CanvasViewState, ViewAction};
use super::super::context_menu::{
    canvas_context_entries, tool_with, CanvasContextCommand, CanvasContextEntry,
};
use super::super::dialog::{render_dialog, ShellDialog};
use super::super::find_bar::{
    render_find_bar, CloseFindBar, FindBarState, FindDirection, FindNextMatch, FindOption,
    FindPreviousMatch, FindSummary,
};
use super::super::home::{render_home, HomeState, HomeView};
use super::super::panes::{self, NavigationPanesState, PaneAction};
use super::super::preferences_dialog::PreferenceChange;
use super::super::Canvas;
use super::super::{record_opened, repair_notice, ShellSettings};
use super::global_bar::{
    main_menu_schema, refresh_native_menus, ExportTarget, MenuAvailability, MenuCommand, MenuState,
    RegistryFacts,
};
use super::page_controls::{
    parse_page_entry, render_page_controls, PageControlsState, PageEntryError, PAGE_CONTROLS_HEIGHT,
};
use super::quick_actions::{
    render_quick_actions, QuickAction, QuickActionEntry, QuickActionsState,
};
use super::rail::{apply_rail_selection, rail_width, render_rail, RailEntry, RailState};
use super::side_panel::{render_side_panel, SidePanelState};
use super::theme::{ShellViewAction, ShellViewState};
use super::tool_search::{
    document_search_result, search_registry, unavailable_selection, SearchInput, SearchResult,
};
use crate::preferences::{PreferenceCategory, Preferences, ThemePreference};

const GLOBAL_BAR_HEIGHT: f32 = 40.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
const CONTEXT_MENU_ROW_HEIGHT: f32 = 30.0;
const CONTEXT_MENU_PADDING: f32 = 4.0;
const CANVAS_CONTEXT_MENU_WIDTH: f32 = 300.0;
const TAB_CONTEXT_MENU_WIDTH: f32 = 230.0;

/// What the chrome says when a tool will not activate. The canvas status line
/// carries the error itself; the search panel is drawn over it, so a selection
/// made there needs a line of its own.
const TOOL_ACTIVATION_FAILED: &str =
    "This tool did not activate; the canvas status line has the reason";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabError {
    OutOfRange {
        index: usize,
        count: usize,
    },
    CommandUnavailable,
    /// The menus grey this command out right now, and a keystroke reaches
    /// the same commands the menus do.
    Unavailable(&'static str),
}

impl fmt::Display for TabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { index, count } => {
                write!(f, "tab {index} is outside a {count}-tab window")
            }
            Self::CommandUnavailable => write!(f, "menu command is not available yet"),
            Self::Unavailable(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for TabError {}

struct DocumentTab {
    source: PathBuf,
    title: String,
    canvas: Entity<Canvas>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabCommand {
    Close,
    CloseOthers,
    CloseAll,
    RevealPath,
    CopyPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TabContextEntry {
    pub(super) command: TabCommand,
    pub(super) label: &'static str,
    pub(super) tab_index: usize,
    pub(super) availability: MenuAvailability,
}

#[derive(Debug, Clone, Copy)]
struct TabContextMenu {
    tab_index: usize,
    origin: Point<Pixels>,
}

/// Where the canvas context menu was opened, in frame coordinates.
#[derive(Debug, Clone, Copy)]
struct CanvasContextMenu {
    origin: Point<Pixels>,
}

impl DocumentTab {
    fn new(source: PathBuf, canvas: Entity<Canvas>) -> Self {
        let title = tab_title(&source);
        Self {
            source,
            title,
            canvas,
        }
    }

    fn title(&self) -> &str {
        &self.title
    }
}

struct TabState<T> {
    tabs: Vec<T>,
    active: Option<usize>,
}

impl<T> TabState<T> {
    pub fn new(tabs: Vec<T>) -> Self {
        let active = (!tabs.is_empty()).then_some(0);
        Self { tabs, active }
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn tabs(&self) -> &[T] {
        &self.tabs
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&T> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    pub fn activate(&mut self, index: usize) -> Result<bool, TabError> {
        if index >= self.tabs.len() {
            return Err(TabError::OutOfRange {
                index,
                count: self.tabs.len(),
            });
        }
        if self.active == Some(index) {
            return Ok(false);
        }
        self.active = Some(index);
        Ok(true)
    }

    /// Add a tab and make it the active one, the way opening a document
    /// does. Returns its index.
    pub fn push(&mut self, tab: T) -> usize {
        self.tabs.push(tab);
        let index = self.tabs.len() - 1;
        self.active = Some(index);
        index
    }

    pub fn close(&mut self, index: usize) -> Result<T, TabError> {
        if index >= self.tabs.len() {
            return Err(TabError::OutOfRange {
                index,
                count: self.tabs.len(),
            });
        }
        let closed = self.tabs.remove(index);
        self.active = match (self.active, self.tabs.is_empty()) {
            (_, true) => None,
            (Some(active), false) if active > index => Some(active - 1),
            (Some(active), false) if active == index => Some(index.min(self.tabs.len() - 1)),
            (active, false) => active,
        };
        Ok(closed)
    }

    pub fn close_others(&mut self, index: usize) -> Result<Vec<T>, TabError> {
        if index >= self.tabs.len() {
            return Err(TabError::OutOfRange {
                index,
                count: self.tabs.len(),
            });
        }
        let kept = self.tabs.remove(index);
        let closed = std::mem::replace(&mut self.tabs, vec![kept]);
        self.active = Some(0);
        Ok(closed)
    }

    pub fn close_all(&mut self) -> Vec<T> {
        self.active = None;
        std::mem::take(&mut self.tabs)
    }
}

pub(in crate::shell) struct ShellFrame {
    tabs: TabState<DocumentTab>,
    main_menu_open: bool,
    tab_context_menu: Option<TabContextMenu>,
    canvas_context_menu: Option<CanvasContextMenu>,
    search_input: Entity<SearchInput>,
    search_feedback: Option<SearchResult>,
    find: FindBarState,
    find_input: Entity<SearchInput>,
    page_input: Entity<SearchInput>,
    page_entry_error: Option<PageEntryError>,
    observed_view_state: Option<CanvasViewState>,
    shell_view_state: ShellViewState,
    rail_state: RailState,
    quick_actions_state: QuickActionsState,
    side_panel_state: SidePanelState,
    navigation: NavigationPanesState,
    settings: ShellSettings,
    /// What the app has to tell the user: a file it repaired to open, a
    /// config file it could not read, a document that would not open. Shown
    /// in the window and dismissed there, not only printed to stderr.
    notices: Vec<String>,
    dialog: Option<ShellDialog>,
    recent_menu_open: bool,
    home: HomeState,
}

fn activate_tab<T>(
    tabs: &mut TabState<T>,
    search_feedback: &mut Option<SearchResult>,
    index: usize,
) -> bool {
    let activated = tabs.activate(index).unwrap_or(false);
    if activated {
        *search_feedback = None;
    }
    activated
}

fn close_tab<T>(
    tabs: &mut TabState<T>,
    search_feedback: &mut Option<SearchResult>,
    index: usize,
) -> Result<bool, TabError> {
    tabs.close(index)?;
    *search_feedback = None;
    Ok(tabs.is_empty())
}

fn close_other_tabs<T>(
    tabs: &mut TabState<T>,
    search_feedback: &mut Option<SearchResult>,
    index: usize,
) -> Result<(), TabError> {
    tabs.close_others(index)?;
    *search_feedback = None;
    Ok(())
}

impl ShellFrame {
    pub(in crate::shell) fn new(
        tabs: Vec<(PathBuf, Entity<Canvas>)>,
        mut shell_view_state: ShellViewState,
        mut settings: ShellSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        shell_view_state.set_fullscreen(window.is_fullscreen());
        let theme = shell_view_state.tokens();
        let search_input = cx.new(|cx| SearchInput::new(theme, cx));
        let find_input =
            cx.new(|cx| SearchInput::with_placeholder("find-input", "Find in document", theme, cx));
        let page_input =
            cx.new(|cx| SearchInput::with_placeholder("page-entry-input", "Page", theme, cx));
        cx.observe(&search_input, |frame, _, cx| {
            frame.search_feedback = None;
            cx.notify();
        })
        .detach();
        cx.observe(&find_input, |frame, _, cx| {
            frame.find_query_changed(cx);
        })
        .detach();
        cx.observe(&page_input, |frame, _, cx| {
            frame.page_entry_error = None;
            cx.notify();
        })
        .detach();
        cx.observe_window_appearance(window, |frame, window, cx| {
            frame.window_appearance_changed(window, cx);
        })
        .detach();
        cx.observe_window_bounds(window, |frame, window, cx| {
            frame.window_bounds_changed(window, cx);
        })
        .detach();
        let document_tabs: Vec<_> = tabs
            .into_iter()
            .map(|(source, canvas)| DocumentTab::new(source, canvas))
            .collect();
        for tab in &document_tabs {
            cx.observe(&tab.canvas, |frame, _, cx| {
                frame.canvas_view_changed(cx);
            })
            .detach();
        }
        let tabs = TabState::new(document_tabs);
        let observed_view_state = tabs
            .active()
            .map(|tab| tab.canvas.read(cx).model.view_state());
        let notices = std::mem::take(&mut settings.notices);
        let frame = Self {
            tabs,
            main_menu_open: false,
            tab_context_menu: None,
            canvas_context_menu: None,
            search_input,
            search_feedback: None,
            find: FindBarState::with_options(settings.preferences.search),
            find_input,
            page_input,
            page_entry_error: None,
            observed_view_state,
            shell_view_state,
            rail_state: RailState::default(),
            quick_actions_state: QuickActionsState::default(),
            side_panel_state: SidePanelState::default(),
            navigation: NavigationPanesState::default(),
            settings,
            notices,
            dialog: None,
            recent_menu_open: false,
            home: HomeState::default(),
        };
        frame.sync_page_entry(cx);
        frame
    }

    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if activate_tab(&mut self.tabs, &mut self.search_feedback, index) {
            self.navigation.document_changed();
            self.page_entry_error = None;
            self.observed_view_state = self.active_view_state(cx);
            self.sync_page_entry(cx);
            self.refresh_find(cx);
            refresh_native_menus(cx, self.menu_state(cx));
            cx.notify();
        }
    }

    pub(super) fn run_native_command(
        window_handle: WindowHandle<Self>,
        command: MenuCommand,
        cx: &mut App,
    ) {
        let result = window_handle.update(cx, |frame, window, cx| {
            frame.run_main_menu_command(command, window, cx)
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("onionskin: {error}"),
            Err(error) => eprintln!("onionskin: cannot run menu command: {error}"),
        }
    }

    /// The reason the menus would grey `command` out, if they would.
    ///
    /// One rule for both routes: a keystroke reaches exactly the commands a
    /// click on the menu entry reaches. Without this, cmd-1 with no document
    /// open would run a view command against a viewport that is not there.
    fn command_unavailable(&self, command: MenuCommand, cx: &App) -> Option<&'static str> {
        main_menu_schema(self.menu_state(cx))
            .into_iter()
            .flat_map(|section| section.entries)
            .find(|entry| entry.command == command)
            .and_then(|entry| entry.availability.reason())
    }

    fn run_main_menu_command(
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
                self.main_menu_open = false;
                self.recent_menu_open = !self.recent_menu_open;
                cx.notify();
                Ok(())
            }
            MenuCommand::Quit => {
                cx.quit();
                Ok(())
            }
            MenuCommand::Preferences => {
                self.show_preferences(PreferenceCategory::General, cx);
                Ok(())
            }
            MenuCommand::About => {
                self.show_dialog(ShellDialog::About, cx);
                Ok(())
            }
            MenuCommand::KeyboardShortcuts => {
                self.show_dialog(ShellDialog::KeyboardShortcuts, cx);
                Ok(())
            }
            MenuCommand::Tools => {
                self.dismiss_menus(cx);
                self.toggle_rail_expanded(cx);
                Ok(())
            }
            MenuCommand::SelectAll | MenuCommand::DeselectAll => {
                self.dismiss_menus(cx);
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
            | MenuCommand::SinglePage
            | MenuCommand::SinglePageContinuous
            | MenuCommand::TwoPage
            | MenuCommand::TwoPageContinuous
            | MenuCommand::ToggleCover => {
                let Some(view) = self.active_view_state(cx) else {
                    return Err(TabError::Unavailable("No document is open"));
                };
                self.main_menu_open = false;
                self.run_view_action(
                    command
                        .view_action(view)
                        .expect("view menu commands map to canvas actions"),
                    cx,
                );
                Ok(())
            }
            MenuCommand::ToggleQuickAction(action) => {
                self.main_menu_open = false;
                self.toggle_quick_action_visibility(action, cx);
                Ok(())
            }
            MenuCommand::ThemeSystem | MenuCommand::ThemeLight | MenuCommand::ThemeDark => {
                self.dismiss_menus(cx);
                let theme = match command {
                    MenuCommand::ThemeLight => ThemePreference::Light,
                    MenuCommand::ThemeDark => ThemePreference::Dark,
                    _ => ThemePreference::System,
                };
                // Through the preference rather than straight at the view
                // state: the display theme is a setting, so choosing it in
                // the View menu has to survive a restart and has to be what
                // the Preferences dialog shows.
                self.change_preference(PreferenceChange::Theme(theme), cx);
                Ok(())
            }
            command @ (MenuCommand::ToggleNavigationPane
            | MenuCommand::TogglePageControls
            | MenuCommand::ReadMode) => {
                self.main_menu_open = false;
                self.run_shell_view_action(
                    command
                        .shell_view_action()
                        .expect("shell view commands map to shell actions"),
                    cx,
                );
                Ok(())
            }
            MenuCommand::FullScreen => {
                self.main_menu_open = false;
                self.toggle_fullscreen(window, cx);
                Ok(())
            }
            MenuCommand::Export(target) => {
                self.main_menu_open = false;
                self.start_export(target, cx);
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
        self.recent_menu_open = false;
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

    /// Open documents into tabs, in the order they were chosen, and record
    /// them as recent.
    ///
    /// A path that will not open is reported on the notice bar and the rest
    /// still open: choosing five files and losing all of them because one is
    /// corrupt would be the wrong trade.
    pub(super) fn open_documents(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let mut opened: Vec<PathBuf> = Vec::new();
        for path in paths {
            match self.open_document(path, cx) {
                Ok(source) => opened.push(source),
                Err(failure) => self.notices.push(failure),
            }
        }
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

    fn open_document(&mut self, path: &Path, cx: &mut Context<Self>) -> Result<PathBuf, String> {
        let source = std::path::absolute(path)
            .map_err(|error| format!("{} could not be resolved: {error}", path.display()))?;
        if let Some(index) = self.tabs.tabs().iter().position(|tab| tab.source == source) {
            // Already open. Acrobat raises the tab rather than opening the
            // document twice, and two tabs over one file would be two
            // independent view states over one document.
            self.activate(index, cx);
            return Ok(source);
        }
        let document = Document::open_path(&source)
            .map_err(|error| format!("{} could not be opened: {error}", source.display()))?;
        let mut model = CanvasModel::new(
            document,
            crate::build_registry(),
            ViewSize {
                width: crate::shell::WINDOW_WIDTH,
                height: crate::shell::WINDOW_HEIGHT,
            },
        )
        .map_err(|error| format!("{} could not be opened: {error}", source.display()))?;
        let repaired = repair_notice(&source, model.provenance());
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
        self.search_feedback = None;
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

    /// Activate whichever installed tool carries [`ToolCapability::Snapshot`].
    ///
    /// The menu entry asked the same question before it went live, so a
    /// miss here means the registry changed underneath it. Reported rather
    /// than returned quietly: the user clicked something that did nothing.
    fn take_a_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.active().and_then(|tab| {
            tool_with(
                tab.canvas.read(cx).model.registry(),
                ToolCapability::Snapshot,
            )
        }) else {
            self.notices
                .push("no installed tool takes a snapshot".to_owned());
            cx.notify();
            return;
        };
        let entry = self.active_rail_entry(index, cx);
        self.activate_canvas_tool(index, "Take a Snapshot", entry, cx);
    }

    pub(in crate::shell) fn preferences(&self) -> &Preferences {
        &self.settings.preferences
    }

    pub(in crate::shell) fn show_preferences(
        &mut self,
        category: PreferenceCategory,
        cx: &mut Context<Self>,
    ) {
        self.show_dialog(ShellDialog::Preferences(category), cx);
    }

    fn show_dialog(&mut self, dialog: ShellDialog, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        self.dialog = Some(dialog);
        cx.notify();
    }

    pub(in crate::shell) fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
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
                self.run_shell_view_action(ShellViewAction::SetTheme(theme), cx);
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

    pub(in crate::shell) fn dismiss_notice(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.notices.len() {
            self.notices.remove(index);
            cx.notify();
        }
    }

    /// Ask where the export goes, then run it.
    ///
    /// Nothing is produced until the user has chosen a destination, and the
    /// codec produces every page before this writes any of them, so a page
    /// that fails to render leaves no files at all. A write that fails after
    /// that is disk trouble, not a broken page: it stops on the file it was
    /// on and names it, because the earlier files are already there and the
    /// user needs to know how far the set got.
    fn start_export(&mut self, target: ExportTarget, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let canvas = tab.canvas.clone();
        let extension = canvas
            .read(cx)
            .model
            .registry()
            .codec(target.codec())
            .map(|codec| codec.extension());
        let Some(extension) = extension else {
            report_export_failure(&canvas, CanvasError::UnknownCodec(target.codec()), cx);
            return;
        };
        let directory = tab
            .source
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let suggested = format!("{}.{extension}", tab_title(&tab.source));
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));

        cx.spawn(async move |frame, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    frame
                        .update(cx, |frame, cx| {
                            frame
                                .notices
                                .push(format!("no destination could be chosen: {error}"));
                            cx.notify();
                        })
                        .ok();
                    return;
                }
            };
            frame
                .update(cx, |_frame, cx| run_export(&canvas, target, &path, cx))
                .ok();
        })
        .detach();
    }

    fn run_tab_command(
        &mut self,
        command: TabCommand,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), TabError> {
        self.main_menu_open = false;
        self.tab_context_menu = None;
        match command {
            TabCommand::Close => {
                close_tab(&mut self.tabs, &mut self.search_feedback, index)?;
                self.navigation.document_changed();
                self.observed_view_state = self.active_view_state(cx);
                self.sync_page_entry(cx);
                self.refresh_find(cx);
                refresh_native_menus(cx, self.menu_state(cx));
                cx.notify();
            }
            TabCommand::CloseOthers => {
                close_other_tabs(&mut self.tabs, &mut self.search_feedback, index)?;
                self.navigation.document_changed();
                self.observed_view_state = self.active_view_state(cx);
                self.sync_page_entry(cx);
                self.refresh_find(cx);
                refresh_native_menus(cx, self.menu_state(cx));
                cx.notify();
            }
            TabCommand::CloseAll => {
                self.tabs.close_all();
                self.search_feedback = None;
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

    fn menu_state(&self, cx: &App) -> MenuState {
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
    fn registry_facts(&self, cx: &App) -> RegistryFacts {
        match self.tabs.active() {
            Some(tab) => RegistryFacts::of(tab.canvas.read(cx).model.registry()),
            // With no document there is no tab registry to ask, and the
            // entries still have to say whether their plugin is installed.
            None => self.settings.registry,
        }
    }

    fn apply_theme(&mut self, cx: &mut Context<Self>) {
        let theme = self.shell_view_state.tokens();
        self.search_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.find_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.page_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
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
        let previous_theme = self.shell_view_state.resolved_theme();
        if !self.shell_view_state.apply(action) {
            return;
        }
        if previous_theme != self.shell_view_state.resolved_theme() {
            self.apply_theme(cx);
        }
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
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
        self.page_input.update(cx, |input, cx| {
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

    pub(super) fn run_view_action(&mut self, action: ViewAction, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        canvas.update(cx, |canvas, cx| canvas.run_view_action(action, cx));
        self.page_entry_error = None;
        self.observed_view_state = self.active_view_state(cx);
        self.sync_page_entry(cx);
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    pub(super) fn submit_page_entry(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.active_view_state(cx) else {
            return;
        };
        let input = self.page_input.read(cx).query().to_owned();
        match parse_page_entry(&input, view.page_count) {
            Ok(page) => self.run_view_action(ViewAction::GoToPage(page), cx),
            Err(error) => {
                self.page_entry_error = Some(error);
                cx.notify();
            }
        }
    }

    /// Escape is bound window-wide so it closes the bar from wherever focus
    /// sits, which means a closed bar has to hand the key back.
    pub(in crate::shell) fn close_find_bar(
        &mut self,
        _: &CloseFindBar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.find.is_open() {
            cx.propagate();
            return;
        }
        self.dismiss_find_bar(cx);
    }

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
        self.main_menu_open = false;
        self.tab_context_menu = None;
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

    fn canvases(&self) -> Vec<Entity<Canvas>> {
        self.tabs
            .tabs()
            .iter()
            .map(|tab| tab.canvas.clone())
            .collect()
    }

    fn toggle_main_menu(&mut self, cx: &mut Context<Self>) {
        self.main_menu_open = !self.main_menu_open;
        self.tab_context_menu = None;
        self.canvas_context_menu = None;
        cx.notify();
    }

    fn open_tab_context_menu(
        &mut self,
        index: usize,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if tab_context_entries(index, self.tabs.tabs().len()).is_err() {
            eprintln!(
                "onionskin: {}",
                TabError::OutOfRange {
                    index,
                    count: self.tabs.tabs().len()
                }
            );
            return;
        }
        self.main_menu_open = false;
        self.canvas_context_menu = None;
        self.tab_context_menu = Some(TabContextMenu {
            tab_index: index,
            origin: event.position,
        });
        cx.stop_propagation();
        cx.notify();
    }

    /// The notices waiting to be read, newest last.
    fn render_notices(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    /// File > Open Recent, as a flyout rather than a submenu: the entries are
    /// file names, and the menu schema carries static labels.
    fn render_recent_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            let path = recent.path.display().to_string();
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

    fn dismiss_menus(&mut self, cx: &mut Context<Self>) {
        self.main_menu_open = false;
        self.recent_menu_open = false;
        self.tab_context_menu = None;
        self.canvas_context_menu = None;
        cx.notify();
    }

    fn open_canvas_context_menu(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.tabs.active().is_none() {
            return;
        }
        self.main_menu_open = false;
        self.tab_context_menu = None;
        self.canvas_context_menu = Some(CanvasContextMenu {
            origin: event.position,
        });
        cx.stop_propagation();
        cx.notify();
    }

    /// The menu's live entries are the ones the registry and the selection
    /// answer for, so running one asks the same two sources rather than a
    /// second copy of the rules.
    fn run_canvas_context_command(
        &mut self,
        command: CanvasContextCommand,
        cx: &mut Context<Self>,
    ) {
        self.canvas_context_menu = None;
        // The menu is closed above whatever the command turns out to do, so
        // every exit below has to repaint.
        cx.notify();
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        match command {
            CanvasContextCommand::Copy => {
                let Some(text) = canvas
                    .read(cx)
                    .model
                    .selection_text()
                    .map(str::to_owned)
                    .filter(|text| !text.is_empty())
                else {
                    return;
                };
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            CanvasContextCommand::RotateClockwise => {
                self.run_view_action(ViewAction::RotateClockwise, cx)
            }
            other => {
                let Some(index) = other
                    .capability()
                    .and_then(|capability| tool_with(canvas.read(cx).model.registry(), capability))
                else {
                    return;
                };
                let rail_entry = self.active_rail_entry(index, cx);
                self.activate_canvas_tool(index, other.label(), rail_entry, cx);
            }
        }
    }

    fn canvas_context_menu_entries(&self, cx: &App) -> Vec<CanvasContextEntry> {
        self.tabs
            .active()
            .map(|tab| {
                let model = &tab.canvas.read(cx).model;
                canvas_context_entries(
                    model.registry(),
                    model.selection_text().is_some_and(|text| !text.is_empty()),
                )
            })
            .unwrap_or_default()
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
            .child(div().flex_1())
            .child(
                div()
                    .w(px(320.0))
                    .flex_none()
                    .child(self.search_input.clone()),
            )
    }

    fn search_results(&self, cx: &App) -> Vec<SearchResult> {
        let query = self.search_input.read(cx).query().to_owned();
        let mut results = self
            .tabs
            .active()
            .map(|tab| search_registry(tab.canvas.read(cx).model.registry(), &query))
            .unwrap_or_default();
        if let Some(document_search) = document_search_result(&query) {
            results.push(document_search);
        }
        results
    }

    fn choose_search_result(
        &mut self,
        result: SearchResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(unavailable) =
            unavailable_selection(result.clone(), self.tabs.active().is_some())
        {
            self.search_feedback = Some(unavailable);
            cx.notify();
            return;
        }
        match result {
            SearchResult::Tool { index, name, .. } => {
                let entry = self.active_rail_entry(index, cx);
                if self.activate_canvas_tool(index, name, entry, cx) {
                    self.search_feedback = None;
                    cx.notify();
                }
            }
            SearchResult::DocumentSearch { query } => {
                self.search_feedback = None;
                self.open_find_bar(Some(query), window, cx);
            }
            SearchResult::Command { id, .. } => {
                self.search_feedback = None;
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
                self.rail_state
                    .entries(canvas.model.registry(), canvas.model.active_tool())
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
            self.search_feedback = Some(SearchResult::Unavailable {
                label: name.to_owned(),
                reason: TOOL_ACTIVATION_FAILED,
            });
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
                self.quick_actions_state
                    .entries(tab.canvas.read(cx).model.registry())
            })
            .unwrap_or_default()
    }

    fn all_quick_action_entries(&self, cx: &App) -> Vec<QuickActionEntry> {
        self.tabs
            .active()
            .map(|tab| {
                self.quick_actions_state
                    .all_entries(tab.canvas.read(cx).model.registry())
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
            visibility.rail,
            self.rail_state.expanded(),
            self.navigation_width(visibility.navigation_pane),
            visibility.side_panel,
            self.side_panel_state,
            visibility.page_controls,
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
    /// window and then ask what it holds.
    #[cfg(test)]
    pub(in crate::shell) fn navigation(&self) -> &NavigationPanesState {
        &self.navigation
    }

    #[cfg(test)]
    pub(in crate::shell) fn navigation_mut(&mut self) -> &mut NavigationPanesState {
        &mut self.navigation
    }

    #[cfg(test)]
    pub(in crate::shell) fn active_canvas(&self) -> Option<&Entity<Canvas>> {
        self.tabs.active().map(|tab| &tab.canvas)
    }

    /// The panes' one way back into the frame. Everything a click in a
    /// navigation pane does goes through here, so this file holds which tab
    /// is active and `shell/panes/` holds what the click means.
    pub(in crate::shell) fn run_pane_action(&mut self, action: PaneAction, cx: &mut Context<Self>) {
        let canvas = self.tabs.active().map(|tab| tab.canvas.clone());
        let directory = self
            .tabs
            .active()
            .and_then(|tab| tab.source.parent().map(Path::to_path_buf));
        panes::apply(&mut self.navigation, canvas.as_ref(), directory, action, cx);
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

    fn search_panel_visible(&self, cx: &App) -> bool {
        !self.main_menu_open
            && self.tab_context_menu.is_none()
            && !self.search_input.read(cx).query().trim().is_empty()
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
                        frame.choose_search_result(selection.clone(), window, cx);
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

        if let Some(feedback) = self.search_feedback.as_ref() {
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

    fn render_main_menu(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let max_height = (window.viewport_size().height - px(GLOBAL_BAR_HEIGHT + 8.0)).max(px(0.0));
        let mut panel = div()
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
                                if let Err(error) = frame.run_main_menu_command(command, window, cx)
                                {
                                    eprintln!("onionskin: {error}");
                                }
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

    fn render_tab_context_menu(
        &self,
        menu: TabContextMenu,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let entries = tab_context_entries(menu.tab_index, self.tabs.tabs().len())
            .expect("context-menu targets are validated when opened");
        let size = gpui::size(
            px(TAB_CONTEXT_MENU_WIDTH),
            px(entries.len() as f32 * CONTEXT_MENU_ROW_HEIGHT + 2.0 * CONTEXT_MENU_PADDING),
        );
        let origin = context_menu_origin(menu.origin, size, window.viewport_size());
        let mut panel = div()
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(size.width)
            .p(px(CONTEXT_MENU_PADDING))
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text);

        for (row_index, entry) in entries.into_iter().enumerate() {
            let enabled = entry.availability.is_enabled();
            let command = entry.command;
            let tab_index = entry.tab_index;
            panel = panel.child(
                div()
                    .id(("tab-context-entry", row_index))
                    .h(px(CONTEXT_MENU_ROW_HEIGHT))
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
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        if enabled {
                            if let Err(error) = frame.run_tab_command(command, tab_index, cx) {
                                eprintln!("onionskin: {error}");
                            }
                        }
                    }))
                    .child(entry.label)
                    .when_some(entry.availability.reason(), |row, reason| {
                        row.child(
                            div()
                                .ml_2()
                                .text_xs()
                                .text_color(theme.muted_text)
                                .child(reason),
                        )
                    }),
            );
        }
        panel
    }

    fn render_canvas_context_menu(
        &self,
        menu: CanvasContextMenu,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let entries = self.canvas_context_menu_entries(cx);
        let size = gpui::size(
            px(CANVAS_CONTEXT_MENU_WIDTH),
            px(entries.len() as f32 * CONTEXT_MENU_ROW_HEIGHT + 2.0 * CONTEXT_MENU_PADDING),
        );
        let origin = context_menu_origin(menu.origin, size, window.viewport_size());
        let mut panel = div()
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(size.width)
            .p(px(CONTEXT_MENU_PADDING))
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text);

        for (row_index, entry) in entries.into_iter().enumerate() {
            let enabled = entry.availability.is_enabled();
            let command = entry.command;
            panel = panel.child(
                div()
                    .id(("canvas-context-entry", row_index))
                    .h(px(CONTEXT_MENU_ROW_HEIGHT))
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
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        if enabled {
                            frame.run_canvas_context_command(command, cx);
                        }
                    }))
                    .child(entry.label)
                    .when_some(entry.availability.reason(), |row, reason| {
                        row.child(
                            div()
                                .ml_2()
                                .text_xs()
                                .text_color(theme.muted_text)
                                .child(reason),
                        )
                    }),
            );
        }
        panel
    }
}

impl Render for ShellFrame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let visibility = self.shell_view_state.visibility();
        let document_bounds = document_view_bounds(
            window.viewport_size(),
            visibility.rail,
            self.rail_state.expanded(),
            self.navigation_width(visibility.navigation_pane),
            visibility.side_panel,
            self.side_panel_state,
            visibility.page_controls,
        );
        self.quick_actions_state.constrain_to(document_bounds.size);
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
                        cx.listener(move |frame, event: &gpui::ClickEvent, _window, cx| {
                            if !event.is_right_click() {
                                frame.activate(index, cx);
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |frame, event, _window, cx| {
                            frame.open_tab_context_menu(index, event, cx);
                        }),
                    )
                    .child(tab.title().to_owned()),
            );
        }

        let rail_entries = self.rail_entries(cx);
        let rail_expanded = self.rail_state.expanded();
        let mut body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(visibility.rail, |body| {
                body.child(render_rail(rail_entries, rail_expanded, theme, cx))
            });
        if visibility.navigation_pane {
            // The column is as tall as the body it sits in, which is the one
            // thing the panes cannot work out for themselves and the one
            // thing the thumbnails pane needs to know how many rows to show.
            let height = (window.viewport_size().height - px(GLOBAL_BAR_HEIGHT + TAB_BAR_HEIGHT))
                .max(px(0.0));
            let active_canvas = self.tabs.active().map(|tab| tab.canvas.clone());
            body = body.child(panes::render_navigation_panes(
                &mut self.navigation,
                active_canvas.as_ref(),
                height,
                theme,
                cx,
            ));
        }
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
                theme,
                cx,
            );
            // Summarised only when the bar is on screen to read it.
            let find_summary = self.find.is_open().then(|| {
                FindSummary::new(
                    tab.canvas.read(cx).model.search(),
                    tab.canvas.read(cx).model.viewport().page_count(),
                )
            });
            let find_state = self.find;
            let find_input = self.find_input.clone();
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
                    view.child(render_find_bar(find_state, find_input, &summary, theme, cx))
                });
            let document_column = div()
                .w(document_bounds.size.width)
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .child(canvas_view)
                .when(visibility.page_controls, |column| {
                    column.child(render_page_controls(
                        page_controls_state,
                        self.page_input.clone(),
                        self.page_entry_error.as_ref(),
                        document_bounds.size.width,
                        theme,
                        cx,
                    ))
                });
            body = body.child(document_column);
        } else {
            body = body.child(render_home(&self.home, &self.settings.recents, theme, cx));
        }
        body = body.when(visibility.side_panel, |body| {
            body.child(render_side_panel(self.side_panel_state, theme, cx))
        });

        let frame = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(self.render_global_bar(cx))
            .child(self.render_notices(cx))
            .child(tab_bar)
            .child(body);

        let mut root = div()
            .size_full()
            .relative()
            .on_action(cx.listener(Self::close_find_bar))
            .child(frame);
        if self.main_menu_open
            || self.recent_menu_open
            || self.tab_context_menu.is_some()
            || self.canvas_context_menu.is_some()
        {
            root = root.child(
                div()
                    .id("menu-dismiss-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .on_click(cx.listener(|frame, _event, _window, cx| {
                        frame.dismiss_menus(cx);
                    })),
            );
        }
        if self.main_menu_open {
            root = root.child(self.render_main_menu(window, cx));
        }
        if self.recent_menu_open {
            root = root.child(self.render_recent_menu(cx));
        }
        if let Some(menu) = self.tab_context_menu {
            root = root.child(self.render_tab_context_menu(menu, window, cx));
        }
        if let Some(menu) = self.canvas_context_menu {
            root = root.child(self.render_canvas_context_menu(menu, window, cx));
        }
        if self.search_panel_visible(cx) {
            root = root.child(self.render_search_results(cx));
        }
        if let Some(dialog) = self.dialog {
            root = root.child(render_dialog(self, dialog, theme, cx));
        }
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
    rail_visible: bool,
    rail_expanded: bool,
    navigation_width: Pixels,
    side_panel_visible: bool,
    side_panel: SidePanelState,
    page_controls_visible: bool,
) -> gpui::Bounds<Pixels> {
    let rail = if rail_visible {
        rail_width(rail_expanded)
    } else {
        px(0.0)
    };
    let side_panel = if side_panel_visible {
        side_panel.width()
    } else {
        px(0.0)
    };
    let page_controls = if page_controls_visible {
        px(PAGE_CONTROLS_HEIGHT)
    } else {
        px(0.0)
    };
    let header = px(GLOBAL_BAR_HEIGHT + TAB_BAR_HEIGHT);
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

/// Where a context menu panel of `size` may sit after a click at `click`.
///
/// The canvas menu names all thirteen of parity row 225's entries, which is
/// tall enough to run off the bottom of the window, so the clicked corner
/// is a preference: the panel slides back inside rather than putting
/// entries out of reach.
fn context_menu_origin(
    click: Point<Pixels>,
    size: gpui::Size<Pixels>,
    viewport: gpui::Size<Pixels>,
) -> Point<Pixels> {
    Point {
        x: click.x.min(viewport.width - size.width).max(px(0.0)),
        y: click.y.min(viewport.height - size.height).max(px(0.0)),
    }
}

pub(super) fn tab_context_entries(
    tab_index: usize,
    tab_count: usize,
) -> Result<Vec<TabContextEntry>, TabError> {
    use MenuAvailability::{Disabled, Enabled};

    if tab_index >= tab_count {
        return Err(TabError::OutOfRange {
            index: tab_index,
            count: tab_count,
        });
    }

    Ok(vec![
        TabContextEntry {
            command: TabCommand::Close,
            label: "Close",
            tab_index,
            availability: Enabled,
        },
        TabContextEntry {
            command: TabCommand::CloseOthers,
            label: "Close Others",
            tab_index,
            availability: if tab_count > 1 {
                Enabled
            } else {
                Disabled("No other tabs are open")
            },
        },
        TabContextEntry {
            command: TabCommand::CloseAll,
            label: "Close All",
            tab_index,
            availability: Enabled,
        },
        TabContextEntry {
            command: TabCommand::RevealPath,
            label: "Show Containing Folder",
            tab_index,
            availability: Enabled,
        },
        TabContextEntry {
            command: TabCommand::CopyPath,
            label: "Copy Path",
            tab_index,
            availability: Enabled,
        },
    ])
}

/// Resolution for a raster export. M2 has no export-settings dialog, so this
/// is Acrobat's own default rather than a number picked here; the codec API
/// takes the resolution as an argument so the dialog that lands with M3's
/// `File > Export To` has somewhere to put the user's choice.
const EXPORT_DPI: f32 = 150.0;

/// Run a chosen export and write it, reporting any failure on the document it
/// came from.
fn run_export(canvas: &Entity<Canvas>, target: ExportTarget, path: &Path, cx: &mut App) {
    let exported = canvas.update(cx, |canvas, _cx| {
        canvas.model.export(target.codec(), EXPORT_DPI)
    });
    let result = match exported {
        Ok(files) => write_export(path, &files),
        Err(error) => Err(ExportFailure::Codec(error)),
    };
    if let Err(failure) = result {
        report_export_failure(canvas, failure, cx);
    }
}

fn write_export(chosen: &Path, files: &[ExportedFile]) -> Result<(), ExportFailure> {
    for file in files {
        let path = export_path(chosen, file.page, files.len());
        std::fs::write(&path, &file.bytes)
            .map_err(|source| ExportFailure::Write { path, source })?;
    }
    Ok(())
}

/// Where one exported file goes. A single file takes the name the user chose;
/// a per-page export numbers beside it, one-based like the page controls, so
/// `report.png` becomes `report-001.png`, `report-002.png`.
fn export_path(chosen: &Path, page: Option<PageIndex>, count: usize) -> PathBuf {
    let (Some(page), true) = (page, count > 1) else {
        return chosen.to_path_buf();
    };
    let stem = chosen
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let mut name = format!("{stem}-{:03}", page + 1);
    if let Some(extension) = chosen.extension() {
        name.push('.');
        name.push_str(&extension.to_string_lossy());
    }
    chosen.with_file_name(name)
}

/// Everything that can go wrong once the user has chosen a destination.
///
/// A write names the file it was on. A per-page export is many files, so
/// "export could not be written" alone would not tell the user which of them
/// to look for, nor how far the set got.
#[derive(Debug)]
enum ExportFailure {
    Codec(CanvasError),
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for ExportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(f, "export failed: {error}"),
            Self::Write { path, source } => {
                write!(f, "{} could not be written: {source}", path.display())
            }
        }
    }
}

/// Surface the failure on the document it belongs to, rather than only on
/// stderr where a user will never see it.
fn report_export_failure(canvas: &Entity<Canvas>, failure: impl fmt::Display, cx: &mut App) {
    canvas.update(cx, |canvas, cx| {
        if canvas.model.record_error(failure) {
            cx.notify();
        }
    });
}

fn tab_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn tab_element_id(path: &Path) -> Arc<Path> {
    Arc::from(path)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    #[cfg(feature = "shell-test-support")]
    use gpui::{TestAppContext, VisualTestContext};
    use onionskin_core::{PageLayoutMode, ViewPoint, ViewRotation, ZoomPolicy};

    use crate::preferences::ThemePreference;
    use onionskin_plugin_api::{PluginRegistry, PointerInput, ToolCtx, ToolPlugin};

    use super::*;
    use crate::shell::canvas::CanvasModel;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::canvas::CanvasStatus;

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
            true,
            false,
            px(0.0),
            true,
            SidePanelState::Closed,
            true,
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
            true,
            true,
            px(0.0),
            true,
            SidePanelState::Closed,
            true,
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
            true,
            false,
            px(0.0),
            true,
            SidePanelState::OpenEmpty,
            true,
        );
        assert_eq!(collapsed_open.origin, collapsed_closed.origin);
        assert_eq!(collapsed_open.size, gpui::size(px(732.0), px(736.0)));

        let expanded_open = document_view_bounds(
            viewport,
            true,
            true,
            px(0.0),
            true,
            SidePanelState::OpenEmpty,
            true,
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
            true,
            false,
            px(0.0),
            true,
            SidePanelState::Closed,
            true,
        );
        let with_strip = document_view_bounds(
            viewport,
            true,
            false,
            closed,
            true,
            SidePanelState::Closed,
            true,
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
            false,
            true,
            px(0.0),
            false,
            SidePanelState::OpenEmpty,
            false,
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

    /// A click low enough that a thirteen-entry menu would overhang slides
    /// the panel back inside; a click with room to spare is left alone.
    #[test]
    fn a_context_menu_never_opens_outside_the_window() {
        let viewport = gpui::size(px(800.0), px(600.0));
        let size = gpui::size(
            px(CANVAS_CONTEXT_MENU_WIDTH),
            px(
                CanvasContextCommand::ALL.len() as f32 * CONTEXT_MENU_ROW_HEIGHT
                    + 2.0 * CONTEXT_MENU_PADDING,
            ),
        );

        let roomy = gpui::point(px(100.0), px(50.0));
        assert_eq!(context_menu_origin(roomy, size, viewport), roomy);

        let cornered = context_menu_origin(gpui::point(px(760.0), px(580.0)), size, viewport);
        assert_eq!(cornered.x + size.width, viewport.width);
        assert_eq!(cornered.y + size.height, viewport.height);

        // A window too small for the panel still opens it at the top left,
        // where the first entries are reachable, rather than off-screen.
        let cramped = context_menu_origin(
            gpui::point(px(10.0), px(10.0)),
            size,
            gpui::size(px(120.0), px(120.0)),
        );
        assert_eq!(cramped, gpui::point(px(0.0), px(0.0)));
    }

    /// The entries the menu shows come from the live model, and picking one
    /// reaches the subsystem that owns it: Take A Snapshot activates the
    /// registered snapshot tool, Rotate Clockwise turns the real view.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn canvas_context_entries_come_from_the_live_model_and_run_against_it(cx: &mut TestAppContext) {
        use onionskin_plugin_api::ToolCapability;

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let document = Document::open_path(&path).unwrap();
        let model = CanvasModel::new(
            document,
            crate::build_registry(),
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

        let right_click = MouseDownEvent {
            button: MouseButton::Right,
            position: gpui::point(px(300.0), px(300.0)),
            ..Default::default()
        };
        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.open_canvas_context_menu(&right_click, cx);
            });
        });

        let entries = cx.update(|_window, app| {
            assert!(frame.read(app).canvas_context_menu.is_some());
            frame.read(app).canvas_context_menu_entries(app)
        });
        let live = |command| {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .expect("the entry is present")
                .availability
                .is_enabled()
        };
        assert_eq!(entries.len(), CanvasContextCommand::ALL.len());
        assert!(live(CanvasContextCommand::TakeASnapshot));
        assert!(live(CanvasContextCommand::RotateClockwise));
        // Nothing is selected in a freshly opened document.
        assert!(!live(CanvasContextCommand::Copy));

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_canvas_context_command(CanvasContextCommand::TakeASnapshot, cx);
            });
        });
        cx.update(|_window, app| {
            let frame = frame.read(app);
            assert!(frame.canvas_context_menu.is_none(), "picking closes it");
            let model = &frame.tabs.active().unwrap().canvas.read(app).model;
            assert_eq!(
                model.active_tool(),
                tool_with(model.registry(), ToolCapability::Snapshot)
            );
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.open_canvas_context_menu(&right_click, cx);
                frame.run_canvas_context_command(CanvasContextCommand::RotateClockwise, cx);
            });
        });
        cx.update(|_window, app| {
            let frame = frame.read(app);
            assert!(frame.canvas_context_menu.is_none());
            let rotation = frame
                .tabs
                .active()
                .unwrap()
                .canvas
                .read(app)
                .model
                .view_state()
                .rotation;
            assert_eq!(rotation, ViewRotation::Clockwise90);
        });
    }

    /// A window with the routes the app installs at startup: the keymap's
    /// keybindings and the one action listener behind them.
    #[cfg(feature = "shell-test-support")]
    fn bound_window(
        seeds: &[&str],
        cx: &mut TestAppContext,
    ) -> (gpui::WindowHandle<ShellFrame>, Vec<crate::keymap::Binding>) {
        bound_window_in(seeds, crate::config::ConfigPaths::default(), cx)
    }

    #[cfg(feature = "shell-test-support")]
    fn bound_window_in(
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
        let settings = ShellSettings::load(paths, &crate::build_registry());
        let bindings = settings.bindings.clone();
        let shell_view = ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System);
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
        cx.update(|cx| {
            crate::shell::find_bar::install_keybindings(cx);
            crate::shell::install_command_keybindings(cx, &installed);
            super::super::global_bar::install_native_menus(cx, window, state);
        });
        (window, bindings)
    }

    #[cfg(feature = "shell-test-support")]
    fn keystroke_for(bindings: &[crate::keymap::Binding], id: &str) -> String {
        let binding = bindings
            .iter()
            .find(|binding| binding.id == id)
            .unwrap_or_else(|| panic!("{id} is bound"));
        crate::keymap::platform_keystroke(&binding.keystroke, cfg!(target_os = "macos"))
    }

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
                    .search_input
                    .update(cx, |input, cx| input.set_query("find me".to_owned(), cx));
                window.focus(&frame.search_input.read(cx).focus_handle(cx));
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "edit.select-all"));
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                assert_eq!(
                    frame.search_input.read(cx).selected_range(),
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
                // than pointing at a document that is not there.
                assert_eq!(
                    frame.command_unavailable(MenuCommand::SelectAll, cx),
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
                assert!(!frame.recent_menu_open);
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
                open.search_feedback.is_none(),
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
                frame.close_find_bar(&CloseFindBar, window, cx);
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

    #[test]
    fn composite_layout_origin_reaches_canvas_pointer_mapping() {
        let bounds = document_view_bounds(
            gpui::size(px(1_100.0), px(860.0)),
            true,
            false,
            px(0.0),
            true,
            SidePanelState::OpenEmpty,
            true,
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
    fn switching_and_closing_tabs_keeps_a_valid_active_index() {
        let mut tabs = TabState::new(vec!["one", "two", "three"]);

        assert_eq!(tabs.active_index(), Some(0));
        assert!(tabs.activate(1).unwrap());
        assert_eq!(tabs.close(0).unwrap(), "one");
        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(tabs.active(), Some(&"two"));
        assert_eq!(tabs.close(0).unwrap(), "two");
        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(tabs.active(), Some(&"three"));
        assert_eq!(tabs.close(0).unwrap(), "three");
        assert_eq!(tabs.active_index(), None);
    }

    #[test]
    fn tab_switching_exposes_the_active_documents_page_control_values() {
        let first = CanvasViewState {
            current_page: 0,
            page_count: 1,
            zoom: 1.0,
            zoom_policy: ZoomPolicy::Fixed,
            layout_mode: PageLayoutMode::SinglePageContinuous,
            show_cover: false,
            rotation: ViewRotation::None,
            can_previous_view: false,
            can_next_view: false,
        };
        let second = CanvasViewState {
            current_page: 1,
            page_count: 2,
            zoom: 2.0,
            zoom_policy: ZoomPolicy::Fixed,
            layout_mode: PageLayoutMode::TwoPage,
            show_cover: true,
            rotation: ViewRotation::Clockwise90,
            can_previous_view: true,
            can_next_view: false,
        };
        let mut tabs = TabState::new(vec![first, second]);

        assert_eq!(
            PageControlsState::from_view(*tabs.active().unwrap()).current_page,
            1
        );
        assert!(tabs.activate(1).unwrap());
        let active = PageControlsState::from_view(*tabs.active().unwrap());
        assert_eq!(active.current_page, 2);
        assert_eq!(active.page_count, 2);
        assert_eq!(active.zoom_percent, 200);
        assert!(active.can_previous_view);
    }

    #[test]
    fn close_others_leaves_exactly_the_selected_tab() {
        let mut tabs = TabState::new(vec!["one", "two", "three"]);

        assert_eq!(tabs.close_others(1).unwrap(), vec!["one", "three"]);
        assert_eq!(tabs.tabs(), &["two"]);
        assert_eq!(tabs.active_index(), Some(0));
    }

    #[test]
    fn close_all_leaves_no_invalid_active_tab() {
        let mut tabs = TabState::new(vec!["one", "two"]);

        assert_eq!(tabs.close_all(), vec!["one", "two"]);
        assert!(tabs.is_empty());
        assert_eq!(tabs.active_index(), None);
    }

    #[test]
    fn duplicate_titles_keep_distinct_path_identity() {
        let first = PathBuf::from("/first/report.pdf");
        let second = PathBuf::from("/second/report.pdf");

        assert_eq!(tab_title(&first), tab_title(&second));
        assert_ne!(tab_element_id(&first), tab_element_id(&second));
    }

    #[test]
    fn out_of_range_operations_fail_loudly() {
        let mut tabs = TabState::new(vec!["one"]);

        assert!(matches!(
            tabs.activate(1),
            Err(TabError::OutOfRange { index: 1, count: 1 })
        ));
        assert!(matches!(
            tabs.close(1),
            Err(TabError::OutOfRange { index: 1, count: 1 })
        ));
        assert!(matches!(
            tabs.close_others(1),
            Err(TabError::OutOfRange { index: 1, count: 1 })
        ));
    }

    #[test]
    fn every_tab_context_action_targets_the_clicked_tab() {
        let entries = tab_context_entries(2, 3).unwrap();

        assert_eq!(entries.len(), 5);
        assert!(entries.iter().all(|entry| entry.tab_index == 2));
        assert!(entries.iter().all(|entry| entry.availability.is_enabled()));
    }

    #[test]
    fn close_others_explains_why_it_is_disabled_for_a_single_tab() {
        let entry = tab_context_entries(0, 1)
            .unwrap()
            .into_iter()
            .find(|entry| entry.command == TabCommand::CloseOthers)
            .unwrap();

        assert_eq!(
            entry.availability,
            MenuAvailability::Disabled("No other tabs are open")
        );
    }

    #[test]
    fn an_out_of_range_context_target_fails_loudly() {
        assert!(matches!(
            tab_context_entries(2, 2),
            Err(TabError::OutOfRange { index: 2, count: 2 })
        ));
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

    /// Text is one file and takes the name the user typed. So does a
    /// single-page PNG: numbering `report.png` to `report-001.png` when there
    /// is nothing to disambiguate it from would be surprising.
    #[test]
    fn a_one_file_export_keeps_the_name_the_user_chose() {
        let chosen = Path::new("/exports/report.png");

        assert_eq!(export_path(chosen, None, 1), PathBuf::from(chosen));
        assert_eq!(export_path(chosen, Some(0), 1), PathBuf::from(chosen));
    }

    /// A per-page export numbers beside the chosen name, one-based like the
    /// page controls and zero-padded so a directory listing sorts in page
    /// order rather than putting page 10 before page 2.
    #[test]
    fn a_per_page_export_numbers_one_based_beside_the_chosen_name() {
        let chosen = Path::new("/exports/report.png");

        assert_eq!(
            export_path(chosen, Some(0), 12),
            PathBuf::from("/exports/report-001.png")
        );
        assert_eq!(
            export_path(chosen, Some(9), 12),
            PathBuf::from("/exports/report-010.png")
        );
        assert_eq!(
            export_path(chosen, Some(1_233), 1_234),
            PathBuf::from("/exports/report-1234.png")
        );
    }

    #[test]
    fn a_chosen_name_with_no_extension_still_numbers_its_pages() {
        assert_eq!(
            export_path(Path::new("/exports/report"), Some(1), 2),
            PathBuf::from("/exports/report-002")
        );
    }

    /// A name with dots in it keeps every one of them but the last: the stem
    /// of `q1.2026.png` is `q1.2026`, not `q1`.
    #[test]
    fn a_dotted_name_numbers_on_its_last_extension_only() {
        assert_eq!(
            export_path(Path::new("/exports/q1.2026.png"), Some(0), 2),
            PathBuf::from("/exports/q1.2026-001.png")
        );
    }

    #[test]
    fn a_multi_page_export_writes_one_numbered_file_per_page() {
        let dir = std::env::temp_dir().join(format!("onionskin-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the test can make its own directory");
        let chosen = dir.join("report.png");
        let files = vec![
            ExportedFile {
                page: Some(0),
                bytes: b"first".to_vec(),
            },
            ExportedFile {
                page: Some(1),
                bytes: b"second".to_vec(),
            },
        ];

        write_export(&chosen, &files).expect("the export writes");

        assert!(!chosen.exists(), "the undecorated name should not be used");
        assert_eq!(std::fs::read(dir.join("report-001.png")).unwrap(), b"first");
        assert_eq!(
            std::fs::read(dir.join("report-002.png")).unwrap(),
            b"second"
        );
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    /// A write that fails names the file it was on, so a user whose disk
    /// filled up mid-export knows how far the set got.
    #[test]
    fn a_write_failure_names_the_file_it_stopped_on() {
        let chosen = Path::new("/onionskin-does-not-exist/report.png");
        let files = vec![
            ExportedFile {
                page: Some(0),
                bytes: b"first".to_vec(),
            },
            ExportedFile {
                page: Some(1),
                bytes: b"second".to_vec(),
            },
        ];

        let failure = write_export(chosen, &files).expect_err("an unwritable path is refused");

        assert!(
            failure
                .to_string()
                .starts_with("/onionskin-does-not-exist/report-001.png could not be written: "),
            "{failure}"
        );
    }

    #[cfg(feature = "shell-test-support")]
    fn canvas_for_export(
        registry: PluginRegistry,
        cx: &mut TestAppContext,
    ) -> (
        Entity<Canvas>,
        onionskin_render::BaseRaster,
        &mut VisualTestContext,
    ) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let mut document = Document::open_path(&path).expect("the seed opens");
        // The pixels the canvas would composite for this page at the export's
        // own resolution, taken before the document moves into the model so
        // the comparison is against the same worker and options.
        let on_screen = document
            .render_page_now(0, EXPORT_DPI / 72.0)
            .expect("the seed page renders")
            .raster;
        let model = CanvasModel::new(
            document,
            registry,
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("the canvas model builds");
        let theme =
            ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System).tokens();
        let (canvas, cx) = cx.add_window_view(move |_window, _cx| Canvas::new(model, theme));
        (canvas, on_screen, cx)
    }

    #[cfg(feature = "shell-test-support")]
    fn export_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("onionskin-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the test can make its own directory");
        dir
    }

    /// The whole menu path bar the file dialog, which cannot be driven
    /// headless: the codec id `MenuCommand::Export(Png)` carries, looked up in
    /// the registry the canvas holds, exported and written. The bytes on disk
    /// are the pixels the canvas composites, which is the point of routing
    /// export through `core` rather than beside it.
    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn the_png_menu_entry_writes_the_canvas_paths_own_pixels(cx: &mut TestAppContext) {
        let mut registry = PluginRegistry::new();
        registry.install(&onionskin_codecs_common::CommonCodecsPlugin);
        let (canvas, on_screen, cx) = canvas_for_export(registry, cx);
        let dir = export_dir("png-export");
        let chosen = dir.join("hello.png");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Png, &chosen, app));

        let decoded = image::load_from_memory_with_format(
            &std::fs::read(&chosen).expect("the export reached disk"),
            image::ImageFormat::Png,
        )
        .expect("the export is a PNG")
        .to_rgba8();
        assert_eq!(
            decoded.dimensions(),
            (on_screen.width(), on_screen.height())
        );
        assert_eq!(decoded.into_raw(), on_screen.rgba());
        cx.update(|_window, app| {
            let status = canvas.read(app).model.status();
            assert!(
                !matches!(status, Some(CanvasStatus::Error { .. })),
                "{status:?}"
            );
        });
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    /// Text is one file for the whole document, so it takes the chosen name
    /// unnumbered.
    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn the_text_menu_entry_writes_one_file_at_the_chosen_name(cx: &mut TestAppContext) {
        let mut registry = PluginRegistry::new();
        registry.install(&onionskin_codecs_common::CommonCodecsPlugin);
        let (canvas, _, cx) = canvas_for_export(registry, cx);
        let dir = export_dir("text-export");
        let chosen = dir.join("hello.txt");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Text, &chosen, app));

        assert!(std::fs::read_to_string(&chosen)
            .expect("the export reached disk")
            .contains("Hello"));
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "a whole-document text export is one file"
        );
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    /// A destination that cannot be written surfaces on the document the
    /// export came from, naming the file, rather than only on stderr.
    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn an_unwritable_destination_is_reported_on_the_document(cx: &mut TestAppContext) {
        let mut registry = PluginRegistry::new();
        registry.install(&onionskin_codecs_common::CommonCodecsPlugin);
        let (canvas, _, cx) = canvas_for_export(registry, cx);
        let chosen = Path::new("/onionskin-does-not-exist/hello.txt");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Text, chosen, app));

        cx.update(|_window, app| {
            let Some(CanvasStatus::Error { page, message }) = canvas.read(app).model.status()
            else {
                panic!("the failure did not reach the document");
            };
            assert_eq!(*page, None);
            assert!(
                message.starts_with("/onionskin-does-not-exist/hello.txt could not be written: "),
                "{message}"
            );
        });
    }

    /// With the plugin compiled out the menu entry is disabled, but the run
    /// path still refuses by name rather than writing an empty file.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn exporting_without_the_codec_installed_says_which_one_is_missing(cx: &mut TestAppContext) {
        let (canvas, _, cx) = canvas_for_export(PluginRegistry::new(), cx);
        let dir = export_dir("absent-codec");
        let chosen = dir.join("hello.png");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Png, &chosen, app));

        assert!(!chosen.exists(), "nothing should have been written");
        cx.update(|_window, app| {
            let Some(CanvasStatus::Error { message, .. }) = canvas.read(app).model.status() else {
                panic!("the failure did not reach the document");
            };
            assert_eq!(message, "export failed: no png codec is installed");
        });
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
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
                frame.read(app).search_feedback,
                Some(SearchResult::Unavailable {
                    label: "Absent".to_owned(),
                    reason: TOOL_ACTIVATION_FAILED,
                }),
                "the search panel said nothing about a tool that did not activate"
            );
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.search_feedback = None;
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
                frame.read(app).search_feedback.is_some(),
                "the rail dropped the activation error"
            );
        });
    }
}
