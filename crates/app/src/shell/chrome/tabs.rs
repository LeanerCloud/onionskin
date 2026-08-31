use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, ClipboardItem, Context, Entity, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, WindowHandle,
};
use onionskin_plugin_api::{ExportedFile, PageIndex};

use super::super::canvas::{CanvasError, CanvasViewState, ViewAction};
use super::super::Canvas;
use super::global_bar::{
    main_menu_schema, refresh_native_menus, ExportCodecs, ExportTarget, MenuAvailability,
    MenuCommand, MenuState,
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

const GLOBAL_BAR_HEIGHT: f32 = 40.0;
const TAB_BAR_HEIGHT: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabError {
    OutOfRange { index: usize, count: usize },
    CommandUnavailable,
}

impl fmt::Display for TabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { index, count } => {
                write!(f, "tab {index} is outside a {count}-tab window")
            }
            Self::CommandUnavailable => write!(f, "menu command is not available yet"),
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
    search_input: Entity<SearchInput>,
    search_feedback: Option<SearchResult>,
    page_input: Entity<SearchInput>,
    page_entry_error: Option<PageEntryError>,
    observed_view_state: Option<CanvasViewState>,
    shell_view_state: ShellViewState,
    rail_state: RailState,
    quick_actions_state: QuickActionsState,
    side_panel_state: SidePanelState,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        shell_view_state.set_fullscreen(window.is_fullscreen());
        let theme = shell_view_state.tokens();
        let search_input = cx.new(|cx| SearchInput::new(theme, cx));
        let page_input =
            cx.new(|cx| SearchInput::with_placeholder("page-entry-input", "Page", theme, cx));
        cx.observe(&search_input, |frame, _, cx| {
            frame.search_feedback = None;
            cx.notify();
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
        let frame = Self {
            tabs,
            main_menu_open: false,
            tab_context_menu: None,
            search_input,
            search_feedback: None,
            page_input,
            page_entry_error: None,
            observed_view_state,
            shell_view_state,
            rail_state: RailState::default(),
            quick_actions_state: QuickActionsState::default(),
            side_panel_state: SidePanelState::default(),
        };
        frame.sync_page_entry(cx);
        frame
    }

    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if activate_tab(&mut self.tabs, &mut self.search_feedback, index) {
            self.page_entry_error = None;
            self.observed_view_state = self.active_view_state(cx);
            self.sync_page_entry(cx);
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

    fn run_main_menu_command(
        &mut self,
        command: MenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), TabError> {
        let active = self
            .tabs
            .active_index()
            .ok_or(TabError::OutOfRange { index: 0, count: 0 })?;
        match command {
            MenuCommand::CloseTab => self.run_tab_command(TabCommand::Close, active, window, cx),
            MenuCommand::CloseOtherTabs => {
                self.run_tab_command(TabCommand::CloseOthers, active, window, cx)
            }
            MenuCommand::CloseAllTabs => {
                self.run_tab_command(TabCommand::CloseAll, active, window, cx)
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
                let view = self
                    .active_view_state(cx)
                    .expect("view commands require an active document");
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
            command @ (MenuCommand::ToggleNavigationPane
            | MenuCommand::TogglePageControls
            | MenuCommand::ThemeSystem
            | MenuCommand::ThemeLight
            | MenuCommand::ThemeDark
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
            MenuCommand::Open
            | MenuCommand::SaveAs
            | MenuCommand::Undo
            | MenuCommand::Redo
            | MenuCommand::LineWeights
            | MenuCommand::NewWindow
            | MenuCommand::About
            | MenuCommand::KeyboardShortcuts => Err(TabError::CommandUnavailable),
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
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), TabError> {
        self.main_menu_open = false;
        self.tab_context_menu = None;
        match command {
            TabCommand::Close => {
                if close_tab(&mut self.tabs, &mut self.search_feedback, index)? {
                    window.remove_window();
                } else {
                    self.observed_view_state = self.active_view_state(cx);
                    self.sync_page_entry(cx);
                    refresh_native_menus(cx, self.menu_state(cx));
                    cx.notify();
                }
            }
            TabCommand::CloseOthers => {
                close_other_tabs(&mut self.tabs, &mut self.search_feedback, index)?;
                self.observed_view_state = self.active_view_state(cx);
                self.sync_page_entry(cx);
                refresh_native_menus(cx, self.menu_state(cx));
                cx.notify();
            }
            TabCommand::CloseAll => {
                self.tabs.close_all();
                window.remove_window();
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
            self.export_codecs(cx),
        )
    }

    /// Which export formats the active document's registry installed. Derived
    /// from the registry rather than from a hardcoded list, so a build with
    /// `codecs-common` compiled out disables the entries with a reason
    /// instead of offering entries that would fail.
    fn export_codecs(&self, cx: &App) -> ExportCodecs {
        match self.tabs.active() {
            Some(tab) => {
                let model = &tab.canvas.read(cx).model;
                ExportCodecs::installed(|id| model.has_codec(id))
            }
            None => ExportCodecs::default(),
        }
    }

    fn apply_theme(&mut self, cx: &mut Context<Self>) {
        let theme = self.shell_view_state.tokens();
        self.search_input
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

    fn canvas_view_changed(&mut self, cx: &mut Context<Self>) {
        let view = self.active_view_state(cx);
        if self.observed_view_state == view {
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

    fn toggle_main_menu(&mut self, cx: &mut Context<Self>) {
        self.main_menu_open = !self.main_menu_open;
        self.tab_context_menu = None;
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
        self.tab_context_menu = Some(TabContextMenu {
            tab_index: index,
            origin: event.position,
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn dismiss_menus(&mut self, cx: &mut Context<Self>) {
        self.main_menu_open = false;
        self.tab_context_menu = None;
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

    fn choose_search_result(&mut self, result: SearchResult, cx: &mut Context<Self>) {
        match result {
            SearchResult::Tool { index, .. } => {
                let entry = self.active_rail_entry(index, cx);
                if self.activate_canvas_tool(index, entry, cx).is_ok() {
                    self.search_feedback = None;
                    cx.notify();
                }
            }
            deferred => {
                self.search_feedback = unavailable_selection(deferred);
                cx.notify();
            }
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

    fn activate_canvas_tool(
        &mut self,
        index: usize,
        entry: Option<RailEntry>,
        cx: &mut Context<Self>,
    ) -> Result<bool, super::super::canvas::CanvasError> {
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
        if result.is_ok() {
            cx.notify();
        }
        result
    }

    pub(super) fn select_rail_entry(&mut self, entry: RailEntry, cx: &mut Context<Self>) {
        let _ = self.activate_canvas_tool(entry.registry_index, Some(entry), cx);
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
        let _ = self.activate_canvas_tool(index, rail_entry, cx);
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
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        frame.choose_search_result(selection.clone(), cx);
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
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let mut panel = div()
            .absolute()
            .left(menu.origin.x)
            .top(menu.origin.y)
            .w(px(230.0))
            .p_1()
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text);

        for (row_index, entry) in tab_context_entries(menu.tab_index, self.tabs.tabs().len())
            .expect("context-menu targets are validated when opened")
            .into_iter()
            .enumerate()
        {
            let enabled = entry.availability.is_enabled();
            let command = entry.command;
            let tab_index = entry.tab_index;
            panel = panel.child(
                div()
                    .id(("tab-context-entry", row_index))
                    .h(px(30.0))
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
                            if let Err(error) =
                                frame.run_tab_command(command, tab_index, window, cx)
                            {
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
}

impl Render for ShellFrame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let visibility = self.shell_view_state.visibility();
        let document_bounds = document_view_bounds(
            window.viewport_size(),
            visibility.rail,
            self.rail_state.expanded(),
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
            let canvas_view = div()
                .id("document-view")
                .relative()
                .w_full()
                .h(document_bounds.size.height)
                .min_w_0()
                .min_h_0()
                .child(canvas)
                .when(visibility.quick_actions, |view| view.child(quick_actions));
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
            .child(tab_bar)
            .child(body);

        let mut root = div().size_full().relative().child(frame);
        if self.main_menu_open || self.tab_context_menu.is_some() {
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
        if let Some(menu) = self.tab_context_menu {
            root = root.child(self.render_tab_context_menu(menu, cx));
        }
        if self.search_panel_visible(cx) {
            root = root.child(self.render_search_results(cx));
        }
        root
    }
}

fn document_view_bounds(
    viewport: gpui::Size<Pixels>,
    rail_visible: bool,
    rail_expanded: bool,
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
        origin: Point { x: rail, y: header },
        size: gpui::size(
            (viewport.width - rail - side_panel).max(px(0.0)),
            (viewport.height - header - page_controls).max(px(0.0)),
        ),
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
    use onionskin_core::{Document, PageLayoutMode, ViewPoint, ViewRotation, ViewSize, ZoomPolicy};
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

        let collapsed_closed =
            document_view_bounds(viewport, true, false, true, SidePanelState::Closed, true);
        assert_eq!(
            collapsed_closed.origin,
            Point {
                x: px(88.0),
                y: px(76.0)
            }
        );
        assert_eq!(collapsed_closed.size, gpui::size(px(972.0), px(736.0)));

        let expanded_closed =
            document_view_bounds(viewport, true, true, true, SidePanelState::Closed, true);
        assert_eq!(
            expanded_closed.origin,
            Point {
                x: px(240.0),
                y: px(76.0)
            }
        );
        assert_eq!(expanded_closed.size, gpui::size(px(820.0), px(736.0)));

        let collapsed_open =
            document_view_bounds(viewport, true, false, true, SidePanelState::OpenEmpty, true);
        assert_eq!(collapsed_open.origin, collapsed_closed.origin);
        assert_eq!(collapsed_open.size, gpui::size(px(732.0), px(736.0)));

        let expanded_open =
            document_view_bounds(viewport, true, true, true, SidePanelState::OpenEmpty, true);
        assert_eq!(expanded_open.origin, expanded_closed.origin);
        assert_eq!(expanded_open.size, gpui::size(px(580.0), px(736.0)));
        assert_eq!(
            QuickActionsState::default()
                .toolbar_size(expanded_open.size)
                .width,
            expanded_open.size.width
        );
    }

    #[test]
    fn hidden_document_chrome_returns_its_space_to_the_canvas() {
        let bounds = document_view_bounds(
            gpui::size(px(1_100.0), px(860.0)),
            false,
            true,
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
        let mut shell_view = ShellViewState::new(gpui::WindowAppearance::Dark);
        shell_view.apply(ShellViewAction::ToggleReadMode);
        let theme = shell_view.tokens();
        let (frame, cx) = cx.add_window_view(move |window, cx| {
            let canvas = cx.new(|_| Canvas::new(model, theme));
            ShellFrame::new(vec![(path, canvas)], shell_view, window, cx)
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
        let theme = ShellViewState::new(gpui::WindowAppearance::Dark).tokens();
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
        let theme = ShellViewState::new(gpui::WindowAppearance::Dark).tokens();
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
}
