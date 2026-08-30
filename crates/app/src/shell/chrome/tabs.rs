use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, ClipboardItem, Context, Entity, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, WindowHandle,
};

use super::super::Canvas;
use super::global_bar::{
    main_menu_schema, refresh_native_menus, MenuAvailability, MenuCommand, MenuState,
};
use super::quick_actions::{
    render_quick_actions, QuickAction, QuickActionEntry, QuickActionsState,
};
use super::rail::{apply_rail_selection, rail_width, render_rail, RailEntry, RailState};
use super::side_panel::{render_side_panel, SidePanelState};
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
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(SearchInput::new);
        cx.observe(&search_input, |frame, _, cx| {
            frame.search_feedback = None;
            cx.notify();
        })
        .detach();
        Self {
            tabs: TabState::new(
                tabs.into_iter()
                    .map(|(source, canvas)| DocumentTab::new(source, canvas))
                    .collect(),
            ),
            main_menu_open: false,
            tab_context_menu: None,
            search_input,
            search_feedback: None,
            rail_state: RailState::default(),
            quick_actions_state: QuickActionsState::default(),
            side_panel_state: SidePanelState::default(),
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if activate_tab(&mut self.tabs, &mut self.search_feedback, index) {
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
            MenuCommand::Open
            | MenuCommand::Undo
            | MenuCommand::Redo
            | MenuCommand::ViewControls
            | MenuCommand::NewWindow
            | MenuCommand::About
            | MenuCommand::KeyboardShortcuts => Err(TabError::CommandUnavailable),
        }
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
                    refresh_native_menus(cx, self.menu_state());
                    cx.notify();
                }
            }
            TabCommand::CloseOthers => {
                close_other_tabs(&mut self.tabs, &mut self.search_feedback, index)?;
                refresh_native_menus(cx, self.menu_state());
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

    fn menu_state(&self) -> MenuState {
        MenuState::new(self.tabs.tabs().len())
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
        div()
            .h(px(GLOBAL_BAR_HEIGHT))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .bg(gpui::rgb(0x17181a))
            .text_color(gpui::white())
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
                    .hover(|button| button.bg(gpui::rgb(0x34363a)))
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
        let document =
            document_view_bounds(viewport, self.rail_state.expanded(), self.side_panel_state);
        let bounds = gpui::Bounds {
            origin: Point::default(),
            size: document.size,
        };
        let toolbar_size = self.quick_actions_state.toolbar_size();
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
        let mut panel = div()
            .absolute()
            .top(px(GLOBAL_BAR_HEIGHT))
            .right(px(12.0))
            .w(px(420.0))
            .p_1()
            .rounded_md()
            .occlude()
            .bg(gpui::rgb(0x292a2d))
            .text_color(gpui::white());

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
                    .hover(|row| row.bg(gpui::rgb(0x3a3b3f)))
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        frame.choose_search_result(selection.clone(), cx);
                    }))
                    .child(result.label())
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(0x85878c))
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
                    .bg(gpui::rgb(0x202124))
                    .text_sm()
                    .text_color(gpui::rgb(0xc6c8cd))
                    .child(feedback.detail()),
            );
        }
        panel
    }

    fn render_main_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .absolute()
            .top(px(GLOBAL_BAR_HEIGHT))
            .left(px(8.0))
            .w(px(420.0))
            .p_2()
            .rounded_md()
            .bg(gpui::rgb(0x292a2d))
            .text_color(gpui::white());

        for section in main_menu_schema(self.menu_state()) {
            panel = panel.child(
                div()
                    .mt_2()
                    .px_2()
                    .text_xs()
                    .text_color(gpui::rgb(0xaeb0b5))
                    .child(section.id.label()),
            );
            for entry in section.entries {
                let command = entry.command;
                let enabled = entry.availability.is_enabled();
                let reason = entry.availability.reason();
                panel = panel.child(
                    div()
                        .id(("main-menu-entry", command as usize))
                        .min_h(px(30.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .rounded_sm()
                        .text_color(if enabled {
                            gpui::rgb(0xffffff)
                        } else {
                            gpui::rgb(0x85878c)
                        })
                        .when(enabled, |row| {
                            row.cursor_pointer()
                                .hover(|row| row.bg(gpui::rgb(0x3a3b3f)))
                        })
                        .on_click(cx.listener(move |frame, _event, window, cx| {
                            if enabled {
                                if let Err(error) = frame.run_main_menu_command(command, window, cx)
                                {
                                    eprintln!("onionskin: {error}");
                                }
                            }
                        }))
                        .child(div().flex_none().child(entry.label))
                        .when_some(reason, |row, reason| {
                            row.child(
                                div()
                                    .ml_3()
                                    .flex_1()
                                    .text_right()
                                    .text_xs()
                                    .text_color(gpui::rgb(0x85878c))
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
        let mut panel = div()
            .absolute()
            .left(menu.origin.x)
            .top(menu.origin.y)
            .w(px(230.0))
            .p_1()
            .rounded_md()
            .bg(gpui::rgb(0x292a2d))
            .text_color(gpui::white());

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
                        gpui::rgb(0xffffff)
                    } else {
                        gpui::rgb(0x85878c)
                    })
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(gpui::rgb(0x3a3b3f)))
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
                                .text_color(gpui::rgb(0x85878c))
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
        let document_bounds = document_view_bounds(
            window.viewport_size(),
            self.rail_state.expanded(),
            self.side_panel_state,
        );
        self.quick_actions_state.constrain_to(document_bounds.size);
        let mut tab_bar = div().flex().h(px(TAB_BAR_HEIGHT)).bg(gpui::rgb(0x202124));
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
                    .bg(if active {
                        gpui::rgb(0x3a3b3f)
                    } else {
                        gpui::rgb(0x292a2d)
                    })
                    .text_color(gpui::white())
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
        let mut body =
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(render_rail(rail_entries, rail_expanded, cx));
        if let Some(tab) = self.tabs.active() {
            let canvas = tab.canvas.clone();
            let quick_action_entries = self.quick_action_entries(cx);
            let all_quick_action_entries = self.all_quick_action_entries(cx);
            let quick_actions = render_quick_actions(
                quick_action_entries,
                all_quick_action_entries,
                &self.quick_actions_state,
                cx,
            );
            body = body.child(
                div()
                    .id("document-view")
                    .relative()
                    .w(document_bounds.size.width)
                    .flex_none()
                    .min_w_0()
                    .min_h_0()
                    .child(canvas)
                    .child(quick_actions),
            );
        }
        body = body.child(render_side_panel(self.side_panel_state, cx));

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
            root = root.child(self.render_main_menu(cx));
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
    rail_expanded: bool,
    side_panel: SidePanelState,
) -> gpui::Bounds<Pixels> {
    let rail = rail_width(rail_expanded);
    let header = px(GLOBAL_BAR_HEIGHT + TAB_BAR_HEIGHT);
    gpui::Bounds {
        origin: Point { x: rail, y: header },
        size: gpui::size(
            (viewport.width - rail - side_panel.width()).max(px(0.0)),
            (viewport.height - header).max(px(0.0)),
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

    use onionskin_core::{Document, ViewPoint, ViewSize};
    use onionskin_plugin_api::{PluginRegistry, PointerInput, ToolCtx, ToolPlugin};

    use super::*;
    use crate::shell::canvas::CanvasModel;

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

        let collapsed_closed = document_view_bounds(viewport, false, SidePanelState::Closed);
        assert_eq!(
            collapsed_closed.origin,
            Point {
                x: px(88.0),
                y: px(76.0)
            }
        );
        assert_eq!(collapsed_closed.size, gpui::size(px(972.0), px(784.0)));

        let expanded_closed = document_view_bounds(viewport, true, SidePanelState::Closed);
        assert_eq!(
            expanded_closed.origin,
            Point {
                x: px(240.0),
                y: px(76.0)
            }
        );
        assert_eq!(expanded_closed.size, gpui::size(px(820.0), px(784.0)));

        let collapsed_open = document_view_bounds(viewport, false, SidePanelState::OpenEmpty);
        assert_eq!(collapsed_open.origin, collapsed_closed.origin);
        assert_eq!(collapsed_open.size, gpui::size(px(732.0), px(784.0)));

        let expanded_open = document_view_bounds(viewport, true, SidePanelState::OpenEmpty);
        assert_eq!(expanded_open.origin, expanded_closed.origin);
        assert_eq!(expanded_open.size, gpui::size(px(580.0), px(784.0)));
    }

    #[test]
    fn composite_layout_origin_reaches_canvas_pointer_mapping() {
        let bounds = document_view_bounds(
            gpui::size(px(1_100.0), px(860.0)),
            false,
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
        let mut canvas = Canvas::new(model);
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
}
