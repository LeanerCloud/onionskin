mod context;
mod export;
mod frame_state;
mod menu;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, ClipboardItem, Context, Entity, EntityId, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, PathPromptOptions,
    Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _, Window,
};
use onionskin_core::{Document, ViewSize};
use onionskin_plugin_api::ToolCapability;

pub(in crate::shell) use self::frame_state::ShellFrame;
pub(super) use self::frame_state::TabError;
use super::super::canvas::{CanvasModel, CanvasViewState, ViewAction};
use super::super::context_menu::tool_with;
use super::super::dialog::{render_dialog, ShellDialog};
use super::super::find_bar::{
    render_find_bar, Dismiss, FindBarState, FindDirection, FindNextMatch, FindOption,
    FindPreviousMatch, FindSummary, FIND_INPUT_ID, FIND_PLACEHOLDER,
};
use super::super::home::{render_home, HomeState, HomeView};
use super::super::panes::{self, NavigationPanesState, PaneAction};
use super::super::preferences_dialog::PreferenceChange;
use super::super::Canvas;
use super::super::{record_opened, repair_notice, ShellSettings};

pub(super) use self::context::tab_context_entries;

use self::context::TabContextMenu;
use self::export::{export_progress_label, ExportPhaseValue};
use self::frame_state::{activate_tab, close_other_tabs, close_tab, DocumentTab, TabState};
use super::accessible::{
    ActivateFocused, Activation, Element as A11yElement, FocusNext, FocusNextInGroup,
    FocusPrevious, FocusPreviousInGroup, ShellAccessibility, Surface, TextField, SHELL_KEY_CONTEXT,
};
use super::global_bar::{main_menu_schema, refresh_native_menus, MenuCommand, NO_SNAPSHOT_TOOL};
use super::page_controls::{
    self, parse_page_entry, render_page_controls, PageControlsState, PAGE_CONTROLS_HEIGHT,
    PAGE_ENTRY_ID,
};
use super::quick_actions::{
    self, render_quick_actions, QuickAction, QuickActionEntry, QuickActionsState,
};
use super::rail::{self, apply_rail_selection, rail_width, render_rail, RailEntry, RailState};
use super::side_panel::{self, render_side_panel, SidePanelState};
use super::theme::{ShellViewAction, ShellViewState, SurfaceVisibility};
use super::tool_search::{
    document_search_result, search_registry, unavailable_selection, SearchInput, SearchResult,
};
use crate::a11y::{Request as A11yRequest, State as A11yState, Step as A11yStep};
use crate::preferences::{PreferenceCategory, Preferences, ThemePreference};

const GLOBAL_BAR_HEIGHT: f32 = 40.0;
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
        // Named from the same constants the accessible description names,
        // so a field and the node describing it cannot be given different
        // identities.
        let find_input =
            cx.new(|cx| SearchInput::with_placeholder(FIND_INPUT_ID, FIND_PLACEHOLDER, theme, cx));
        let page_input =
            cx.new(|cx| SearchInput::with_placeholder(PAGE_ENTRY_ID, "Page", theme, cx));
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
            export_job: None,
            next_export_id: 0,
            a11y: ShellAccessibility::new(cx),
        };
        frame.sync_page_entry(cx);
        // The chrome takes keyboard focus at launch. Without it GPUI has no
        // focus path to dispatch along, so the shell's own keys, Escape
        // included, would reach nothing until the user clicked a text field.
        window.focus(frame.a11y.focus_handle());
        frame
    }

    /// What the whole window tells a screen reader.
    ///
    /// Assembled from each surface's own `accessible`, gated by the same
    /// booleans `render` gates the surfaces by, so the tree never describes
    /// something that is not on screen. A modal dialog replaces the chrome
    /// rather than joining it: that is what makes it modal to a screen reader
    /// as well as to a mouse.
    fn accessible(&self, window: &Window, cx: &mut Context<Self>) -> A11yElement {
        let scale = window.scale_factor();
        let title = self.tabs.active().map_or("Onionskin", |tab| tab.title());
        let mut root = A11yElement::new("window", Role::Window, format!("Onionskin, {title}"));

        if let Some(dialog) = self.dialog {
            return root.child(crate::shell::dialog::accessible(self, dialog, cx));
        }

        let visibility = self.shell_view_state.visibility();
        if visibility.global_bar {
            root = root.child(self.accessible_global_bar(cx));
        }
        if !self.notices.is_empty() {
            root = root.child(
                A11yElement::new("notices", Role::List, "Notices").with_children(
                    self.notices
                        .iter()
                        .enumerate()
                        .map(|(index, notice)| {
                            A11yElement::new(("notice", index), Role::ListItem, notice.clone())
                                .child(
                                    A11yElement::new(
                                        ("notice-dismiss", index),
                                        Role::Button,
                                        "Dismiss",
                                    )
                                    .with_activation(Activation::DismissNotice(index)),
                                )
                        })
                        .collect(),
                ),
            );
        }
        if visibility.tab_bar {
            root = root.child(self.accessible_tabs());
        }
        if let Some(job) = &self.export_job {
            let can_cancel = job.phase.load() == ExportPhaseValue::Running;
            root = root.child(
                A11yElement::new(
                    "export-progress",
                    Role::ProgressIndicator,
                    export_progress_label(job),
                )
                .child(
                    A11yElement::new("cancel-export", Role::Button, "Cancel Export")
                        .with_state(A11yState::enabled(can_cancel))
                        .with_activation(Activation::CancelExport),
                ),
            );
        }

        if visibility.rail {
            let mut described =
                rail::accessible(&self.rail_entries(cx), self.rail_state.expanded());
            self.a11y.rects.place(Surface::Rail, &mut described);
            root = root.child(described);
        }
        if visibility.navigation_pane {
            root = root.child(panes::accessible(
                &self.navigation,
                self.tabs.active().map(|tab| &tab.canvas),
                &self.a11y.rects,
                cx,
            ));
        }
        if let Some(tab) = self.tabs.active() {
            let canvas = tab.canvas.clone();
            let title = tab.title().to_owned();
            let with_text = self.a11y.wants_page_text();
            root = root.child(canvas.update(cx, |canvas, _cx| {
                canvas.accessible(&title, scale, with_text)
            }));
            if visibility.quick_actions {
                let mut described = quick_actions::accessible(
                    &self.quick_action_entries(cx),
                    &self.all_quick_action_entries(cx),
                    &self.quick_actions_state,
                );
                self.a11y.rects.place(Surface::QuickActions, &mut described);
                root = root.child(described);
            }
            if self.find.is_open() {
                let summary = FindSummary::new(
                    tab.canvas.read(cx).model.search(),
                    tab.canvas.read(cx).model.viewport().page_count(),
                );
                root = root.child(crate::shell::find_bar::accessible(
                    self.find,
                    &summary,
                    self.find_input.read(cx).query(),
                    &self.a11y.rects,
                ));
            }
            if visibility.page_controls {
                let mut controls = page_controls::accessible(
                    PageControlsState::from_view(tab.canvas.read(cx).model.view_state()),
                    self.page_entry_error.as_ref(),
                    self.page_input.read(cx).query(),
                );
                self.a11y.rects.place(Surface::PageControls, &mut controls);
                root = root.child(controls);
            }
        } else {
            root = root.child(crate::shell::home::accessible(
                &self.home,
                &self.settings.recents,
                self.settings.paths.home.as_deref(),
                &self.a11y.rects,
            ));
        }
        if visibility.side_panel {
            root = root.child(side_panel::accessible(self.side_panel_state));
        }
        if self.main_menu_open {
            root = root.child(self.accessible_main_menu(cx));
        }
        if self.recent_menu_open {
            root = root.child(self.accessible_recent_menu());
        }
        if let Some(menu) = self.tab_context_menu {
            root = root.child(self.accessible_tab_context_menu(menu));
        }
        if self.canvas_context_menu.is_some() {
            root = root.child(self.accessible_canvas_context_menu(cx));
        }
        if self.search_panel_visible(cx) {
            root = root.child(self.accessible_search_results(cx));
        }
        root
    }

    fn accessible_global_bar(&self, cx: &App) -> A11yElement {
        A11yElement::new("global-bar", Role::Toolbar, "Global Bar")
            .child(
                A11yElement::new("main-menu-button", Role::Button, "Main Menu")
                    .with_state(A11yState::toggled(self.main_menu_open))
                    .with_activation(Activation::ToggleMainMenu),
            )
            .child(
                self.search_input
                    .read(cx)
                    .accessible("Search Tools Or Document", TextField::Search),
            )
    }

    fn accessible_tabs(&self) -> A11yElement {
        A11yElement::new("tab-bar", Role::TabList, "Open Documents").with_children(
            self.tabs
                .tabs()
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    A11yElement::new(
                        tab_element_id(&tab.source),
                        Role::Tab,
                        tab.title().to_owned(),
                    )
                    .with_state(A11yState::selected(self.tabs.active_index() == Some(index)))
                    .with_activation(Activation::ActivateTab(index))
                })
                .collect(),
        )
    }

    fn accessible_main_menu(&self, cx: &App) -> A11yElement {
        let mut rows = Vec::new();
        // Two counters, because the panel's children are section headings and
        // entries interleaved while a row's element id counts entries only.
        // The children are described in the panel's order so the rectangles
        // it reports line up, and keyed with the entry counter so the node
        // and the row it describes carry one identity.
        let mut entry_index = 0_usize;
        for (section_index, section) in main_menu_schema(self.menu_state(cx))
            .into_iter()
            .enumerate()
        {
            rows.push(A11yElement::new(
                ("main-menu-section", section_index),
                Role::Label,
                section.id.label(),
            ));
            for entry in section.entries {
                // The rendered row spells a ticked entry "✓ {label}". The
                // node carries the tick as state and the label as a label,
                // so a screen reader says "checked" rather than reading a
                // check mark.
                let mut node = A11yElement::new(
                    ("main-menu-entry", entry_index),
                    Role::MenuItemCheckBox,
                    entry.label,
                )
                .with_state(A11yState {
                    toggled: Some(entry.selected),
                    selected: None,
                    disabled: !entry.availability.is_enabled(),
                })
                .with_activation(Activation::MainMenu(entry.command));
                entry_index += 1;
                if let Some(reason) = entry.availability.reason() {
                    node = node.with_description(reason);
                }
                rows.push(node);
            }
        }
        let mut menu =
            A11yElement::new("main-menu-panel", Role::Menu, "Main Menu").with_children(rows);
        self.a11y.rects.place(Surface::MainMenu, &mut menu);
        menu
    }

    fn accessible_recent_menu(&self) -> A11yElement {
        A11yElement::new("recent-menu", Role::Menu, "Open Recent").with_children(
            self.settings
                .recents
                .documents()
                .iter()
                .enumerate()
                .map(|(index, recent)| {
                    A11yElement::new(("recent-entry", index), Role::MenuItem, recent.title())
                        .with_description(recent.display_path(self.settings.paths.home.as_deref()))
                        .with_activation(Activation::OpenRecent(index))
                })
                .collect(),
        )
    }

    fn accessible_tab_context_menu(&self, menu: TabContextMenu) -> A11yElement {
        let entries = tab_context_entries(menu.tab_index, self.tabs.tabs().len())
            .expect("context-menu targets are validated when opened");
        A11yElement::new("tab-context-menu", Role::Menu, "Tab Commands").with_children(
            entries
                .into_iter()
                .enumerate()
                .map(|(index, entry)| {
                    let mut node =
                        A11yElement::new(("tab-context-entry", index), Role::MenuItem, entry.label)
                            .with_state(A11yState::enabled(entry.availability.is_enabled()))
                            .with_activation(Activation::TabCommand(
                                entry.command,
                                entry.tab_index,
                            ));
                    if let Some(reason) = entry.availability.reason() {
                        node = node.with_description(reason);
                    }
                    node
                })
                .collect(),
        )
    }

    fn accessible_canvas_context_menu(&self, cx: &App) -> A11yElement {
        A11yElement::new("canvas-context-menu", Role::Menu, "Page Commands").with_children(
            self.canvas_context_menu_entries(cx)
                .into_iter()
                .enumerate()
                .map(|(index, entry)| {
                    let mut node = A11yElement::new(
                        ("canvas-context-entry", index),
                        Role::MenuItem,
                        entry.label,
                    )
                    .with_state(A11yState::enabled(entry.availability.is_enabled()))
                    .with_activation(Activation::CanvasContext(entry.command));
                    if let Some(reason) = entry.availability.reason() {
                        node = node.with_description(reason);
                    }
                    node
                })
                .collect(),
        )
    }

    fn accessible_search_results(&self, cx: &App) -> A11yElement {
        let mut panel = A11yElement::new("global-search-results", Role::List, "Search Results")
            .with_children(
                self.search_results(cx)
                    .into_iter()
                    .enumerate()
                    .map(|(index, result)| {
                        A11yElement::new(
                            ("global-search-result", index),
                            Role::ListItem,
                            result.label(),
                        )
                        .with_description(result.detail())
                        .with_activation(Activation::ChooseSearchResult(result))
                    })
                    .collect(),
            );
        if let Some(feedback) = self.search_feedback.as_ref() {
            panel = panel.child(A11yElement::new(
                "global-search-feedback",
                Role::Alert,
                feedback.detail(),
            ));
        }
        panel
    }

    /// Run what activating a control does, whether the control was clicked,
    /// reached with Tab and pressed, or pressed by a screen reader.
    ///
    /// Every click listener in the chrome routes through here and every
    /// accessible node carries the value its listener passes, so the three
    /// paths cannot run different things.
    pub(in crate::shell) fn run_activation(
        &mut self,
        activation: Activation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match activation {
            Activation::ToggleMainMenu => self.toggle_main_menu(cx),
            Activation::MainMenu(command) => {
                if let Err(error) = self.run_main_menu_command(command, window, cx) {
                    eprintln!("onionskin: {error}");
                }
            }
            Activation::ActivateTab(index) => self.activate(index, cx),
            Activation::TabCommand(command, index) => {
                if let Err(error) = self.run_tab_command(command, index, cx) {
                    eprintln!("onionskin: {error}");
                }
            }
            Activation::CanvasContext(command) => self.run_canvas_context_command(command, cx),
            Activation::DismissNotice(index) => self.dismiss_notice(index, cx),
            Activation::OpenRecent(index) => self.open_recent(index, cx),
            Activation::ChooseSearchResult(result) => {
                self.choose_search_result(result, window, cx);
            }
            Activation::Rail(entry) => self.select_rail_entry(entry, cx),
            Activation::ToggleRailExpanded => self.toggle_rail_expanded(cx),
            Activation::QuickAction(entry) => self.select_quick_action(entry, cx),
            Activation::ToggleQuickActionCustomization => {
                self.toggle_quick_action_customization(cx);
            }
            Activation::ToggleQuickActionVisibility(action) => {
                self.toggle_quick_action_visibility(action, cx);
            }
            Activation::ToggleSidePanel => self.toggle_side_panel(cx),
            // A magnification is only offered by the Zoom To dialog, so
            // choosing one dismisses it. Every other view action reaches
            // here from a surface that stays where it is.
            Activation::View(action @ ViewAction::ZoomTo(_)) => {
                self.close_dialog(cx);
                self.run_view_action(action, cx);
            }
            Activation::View(action) => self.run_view_action(action, cx),
            Activation::SubmitPageEntry => self.submit_page_entry(cx),
            Activation::Pane(action) => self.run_pane_action(action, cx),
            Activation::StepFind(direction) => self.step_find(direction, cx),
            Activation::ApplyFindOption(option) => self.apply_find_option(option, cx),
            Activation::DismissFindBar => self.dismiss_find_bar(cx),
            Activation::SetHomeView(view) => self.set_home_view(view, cx),
            Activation::OpenFromHome => self.open_from_home(window, cx),
            Activation::ShowPreferences(category) => self.show_preferences(category, cx),
            Activation::ChangePreference(change) => self.change_preference(change, cx),
            Activation::CloseDialog => self.close_dialog(cx),
            Activation::CancelExport => self.cancel_export(cx),
            Activation::Focus(field) => {
                window.focus(&self.text_field(field).read(cx).focus_handle(cx));
            }
            // The page keys are bound window-wide, so landing on the
            // document is about where the ring is, not about a focus handle
            // of the canvas's own. Returning focus to the chrome is what
            // makes those keys arrive.
            Activation::FocusDocument => {
                window.focus(self.a11y.focus_handle());
                cx.notify();
            }
        }
    }

    /// Tab: move keyboard focus to the next control in reading order.
    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        self.step_focus(A11yStep::Next, window, cx);
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.step_focus(A11yStep::Previous, window, cx);
    }

    /// Right or Down: the next control inside the group focus is in.
    fn focus_next_in_group(
        &mut self,
        _: &FocusNextInGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_focus_within(A11yStep::Next, window, cx);
    }

    fn focus_previous_in_group(
        &mut self,
        _: &FocusPreviousInGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_focus_within(A11yStep::Previous, window, cx);
    }

    /// Moves GPUI's focus as well as the ring's.
    ///
    /// Tabbing out of a text field has to take the field's focus with it, or
    /// the ring lands on a control while the field keeps the keys, and Enter
    /// runs the field's command rather than the focused control.
    fn step_focus(&mut self, step: A11yStep, window: &mut Window, cx: &mut Context<Self>) {
        if !self.a11y.step(step) {
            cx.propagate();
            return;
        }
        self.focus_ring_target(window, cx);
        cx.notify();
    }

    /// The same, inside one group.
    ///
    /// No guard for a focused text field, and that is deliberate. The field
    /// binds the arrows its caret needs in its own key context, which GPUI
    /// resolves ahead of the shell's, so Left and Right never arrive here
    /// while a field has the keys. Refusing the ones that do arrive would
    /// strand the field: the find bar's first stop is its own input, so Up
    /// and Down are how a user reaches the eight controls beside it.
    fn step_focus_within(&mut self, step: A11yStep, window: &mut Window, cx: &mut Context<Self>) {
        if !self.a11y.step_within(step) {
            cx.propagate();
            return;
        }
        self.focus_ring_target(window, cx);
        cx.notify();
    }

    /// Put GPUI's focus where the ring's stop wants the keys to go.
    ///
    /// A stop that is a text field takes the keys itself, so that a screen
    /// reader moving its cursor onto the find field types into the find
    /// field. Every other stop leaves them with the chrome, which is where
    /// the ring's own keys are dispatched from, and moving them off a field
    /// is what stops the field the user has left from keeping Enter.
    fn focus_ring_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.a11y.focused_activation() {
            Some(Activation::Focus(field)) => {
                window.focus(&self.text_field(field).read(cx).focus_handle(cx));
            }
            _ => window.focus(self.a11y.focus_handle()),
        }
    }

    fn text_field(&self, field: TextField) -> &Entity<SearchInput> {
        match field {
            TextField::Search => &self.search_input,
            TextField::Find => &self.find_input,
            TextField::Page => &self.page_input,
        }
    }

    /// Enter or Space: run what clicking the focused control would run.
    ///
    /// Propagates when a text field has focus, so Enter still submits a find
    /// or a page number instead of being eaten by the focus ring.
    fn activate_focused(
        &mut self,
        _: &ActivateFocused,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.text_field_focused(window, cx) {
            cx.propagate();
            return;
        }
        let Some(activation) = self.a11y.focused_activation() else {
            cx.propagate();
            return;
        };
        self.run_activation(activation, window, cx);
    }

    /// The element id of the text field GPUI's focus is in, if it is in one.
    /// The id the field publishes, so the ring and the tree name it the same
    /// way.
    fn focused_text_field(&self, window: &Window, cx: &App) -> Option<gpui::ElementId> {
        [&self.search_input, &self.find_input, &self.page_input]
            .into_iter()
            .find(|input| input.read(cx).focus_handle(cx).is_focused(window))
            .map(|input| input.read(cx).element_id().into())
    }

    fn text_field_focused(&self, window: &Window, cx: &App) -> bool {
        self.focused_text_field(window, cx).is_some()
    }

    /// Serve the accessibility contract without waiting for a frame.
    ///
    /// Scheduled by the adapter's wake, onto the main queue rather than onto
    /// the window's display link: gpui runs a window's display link only
    /// while macOS reports the window visible, so a screen reader working a
    /// window with something in front of it would otherwise be answered when
    /// the user next brought the window forward. Publishing here as well as
    /// in `render` is what lets the tree answer while the window is not
    /// drawing.
    ///
    /// One thing is a frame behind and stays that way: the description takes
    /// its rectangles from `a11y.rects`, which only a drawn frame fills. A
    /// press on a window that is not drawing publishes the right labels,
    /// states and actions with the rectangles of the last frame that drew, so
    /// a reader's cursor is drawn in the old place until the window does.
    pub(in crate::shell) fn serve_accessibility(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_accessibility_requests(window, cx);
        let focused_field = self.focused_text_field(window, cx);
        let described = self.accessible(window, cx);
        self.a11y.publish(&described, focused_field, window, cx);
        // The window may be drawing after all, in which case whatever the
        // request changed has to be drawn.
        cx.notify();
    }

    /// Run what a screen reader asked for.
    ///
    /// The AccessKit handler runs inside an `NSAccessibility` message, with
    /// no `App` in reach, so all it can do is record what was asked and wake
    /// the shell to run it here.
    fn run_accessibility_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (key, request) in self.a11y.take_requests() {
            match request {
                A11yRequest::Focus => {
                    if self.a11y.focus_key(&key) {
                        // The cursor moved, so the keys follow it. A reader
                        // that lands on a control while a text field still
                        // holds GPUI's focus would otherwise leave the next
                        // Enter with the field.
                        self.focus_ring_target(window, cx);
                        cx.notify();
                    }
                }
                A11yRequest::Activate => {
                    if let Some(activation) = self.a11y.activation_for(&key) {
                        self.run_activation(activation, window, cx);
                    }
                }
            }
        }
    }

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
            self.close_dialog(cx);
            return;
        }
        // The panel is open because the search field has something in it, so
        // emptying the field is what closes it. It never shows at the same
        // time as a menu, so where it sits relative to them does not matter.
        if self.search_panel_visible(cx) {
            self.search_input
                .update(cx, |input, cx| input.set_query("", cx));
            cx.notify();
            return;
        }
        if self.tab_context_menu.is_some() || self.canvas_context_menu.is_some() {
            self.tab_context_menu = None;
            self.canvas_context_menu = None;
            cx.notify();
            return;
        }
        if self.main_menu_open || self.recent_menu_open {
            self.main_menu_open = false;
            self.recent_menu_open = false;
            cx.notify();
            return;
        }
        if self.find.is_open() {
            self.dismiss_find_bar(cx);
            return;
        }
        // The two modes that hide the chrome are the last thing Escape
        // closes, innermost first. Without this a window in Full Screen has
        // no chrome to leave it from, and Read Mode ships unbound because
        // Acrobat's Ctrl+H is Hide on macOS.
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

    pub(in crate::shell) fn preferences(&self) -> &Preferences {
        &self.settings.preferences
    }

    fn set_theme(&mut self, theme: ThemePreference, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        self.change_preference(PreferenceChange::Theme(theme), cx);
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
                if let Some(tab) = self.tabs.tabs().get(index) {
                    self.cancel_export_for(tab.canvas.entity_id());
                }
                close_tab(&mut self.tabs, &mut self.search_feedback, index)?;
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
                        .export_job
                        .as_ref()
                        .is_some_and(|job| job.origin != kept.canvas.entity_id())
                    {
                        self.cancel_export(cx);
                    }
                }
                close_other_tabs(&mut self.tabs, &mut self.search_feedback, index)?;
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

    #[cfg(all(test, feature = "shell-test-support"))]
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

    /// The panel belongs to the global bar's search field, so it goes when
    /// the bar does: a query typed before Read Mode was entered would
    /// otherwise leave results hanging under a bar that is no longer there.
    fn search_panel_visible(&self, cx: &App) -> bool {
        self.shell_view_state.visibility().global_bar
            && !self.main_menu_open
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
}

impl Drop for ShellFrame {
    fn drop(&mut self) {
        if let Some(job) = &self.export_job {
            job.phase.cancel();
        }
    }
}

impl Render for ShellFrame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                body.child(render_rail(
                    rail_entries,
                    rail_expanded,
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
                    tab.canvas.read(cx).model.search(),
                    tab.canvas.read(cx).model.viewport().page_count(),
                )
            });
            let find_state = self.find;
            let find_input = self.find_input.clone();
            let export_progress = self.export_job.as_ref().map(|job| {
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
                .child(canvas_view)
                .when(visibility.page_controls, |column| {
                    column.child(render_page_controls(
                        page_controls_state,
                        self.page_input.clone(),
                        self.page_entry_error.as_ref(),
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
            body.child(render_side_panel(self.side_panel_state, theme, cx))
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
            .key_context(SHELL_KEY_CONTEXT)
            .on_action(cx.listener(Self::dismiss_overlay))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_action(cx.listener(Self::focus_next_in_group))
            .on_action(cx.listener(Self::focus_previous_in_group))
            .on_action(cx.listener(Self::activate_focused))
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
    use std::sync::{mpsc, Arc, Condvar, Mutex};

    #[cfg(feature = "shell-test-support")]
    use gpui::{TestAppContext, VisualTestContext};
    use onionskin_core::{PageLayoutMode, ViewPoint, ViewRotation, ZoomPolicy};

    use crate::preferences::ThemePreference;
    use onionskin_plugin_api::{
        CodecPlugin, ExportError, ExportRequest, PageRange, PluginRegistry, PointerInput, ToolCtx,
        ToolPlugin,
    };

    use std::sync::atomic::{AtomicUsize, Ordering};

    use onionskin_plugin_api::{ExportOutputKind, PageIndex};

    use super::context::{
        context_menu_origin, CanvasContextMenu, CANVAS_CONTEXT_MENU_WIDTH, CONTEXT_MENU_PADDING,
        CONTEXT_MENU_ROW_HEIGHT,
    };
    use super::export::{
        export_path, preflight_export_paths, report_export_failure, run_export_worker,
        run_export_worker_observed, ExportFailure, ExportJob, ExportObserver, ExportOutcome,
        ExportPhase, EXPORT_DPI,
    };
    use super::*;
    use crate::shell::canvas::PreparedExport;
    use crate::shell::chrome::global_bar::ExportTarget;
    use crate::shell::chrome::global_bar::MenuAvailability;
    use crate::shell::context_menu::CanvasContextCommand;

    use crate::shell::canvas::CanvasModel;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::canvas::CanvasStatus;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::panes::NavigationPane;

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

    #[cfg(feature = "shell-test-support")]
    fn run_export(canvas: &Entity<Canvas>, target: ExportTarget, path: &Path, cx: &mut App) {
        let prepared = canvas.update(cx, |canvas, _cx| {
            canvas.model.prepare_export(target.codec(), EXPORT_DPI)
        });
        let result = match prepared {
            Ok(prepared) => {
                run_export_worker(prepared, path, &ExportPhase::new(), &AtomicUsize::new(0))
            }
            Err(error) => Err(ExportFailure::Codec(error)),
        };
        if let Err(error) = result {
            report_export_failure(canvas, error, cx);
        }
    }

    struct WorkerCodec {
        kind: ExportOutputKind,
        calls: Arc<Mutex<Vec<PageIndex>>>,
        fail_on: Option<PageIndex>,
        cancel_on: Option<(PageIndex, Arc<ExportPhase>)>,
        on_page: Option<Arc<dyn Fn(PageIndex) + Send + Sync>>,
    }

    struct BlockingCodec {
        kind: ExportOutputKind,
        block_on: PageIndex,
        calls: Arc<Mutex<Vec<PageIndex>>>,
        started: Mutex<Option<mpsc::Sender<()>>>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    #[derive(Default)]
    struct RecordingExportObserver {
        open_writers: AtomicUsize,
        max_open_writers: AtomicUsize,
        before_publish: Option<Arc<dyn Fn() + Send + Sync>>,
        after_publish_started: Option<Arc<dyn Fn() + Send + Sync>>,
        page_completed: Option<Arc<dyn Fn(PageIndex) + Send + Sync>>,
    }

    impl ExportObserver for RecordingExportObserver {
        fn writer_opened(&self) {
            let open = self.open_writers.fetch_add(1, Ordering::AcqRel) + 1;
            self.max_open_writers.fetch_max(open, Ordering::AcqRel);
        }

        fn writer_closed(&self) {
            self.open_writers.fetch_sub(1, Ordering::AcqRel);
        }

        fn page_completed(&self, page: PageIndex) {
            if let Some(callback) = &self.page_completed {
                callback(page);
            }
        }

        fn before_publish(&self) {
            if let Some(callback) = &self.before_publish {
                callback();
            }
        }

        fn after_publish_started(&self) {
            if let Some(callback) = &self.after_publish_started {
                callback();
            }
        }
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

    impl CodecPlugin for WorkerCodec {
        fn id(&self) -> &'static str {
            "worker-test"
        }

        fn name(&self) -> &'static str {
            "Worker Test"
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
            if let Some(on_page) = &self.on_page {
                on_page(page);
            }
            if let Some((cancel_page, phase)) = &self.cancel_on {
                if *cancel_page == page {
                    phase.cancel();
                }
            }
            if self.fail_on == Some(page) {
                return Err(ExportError::Encode {
                    page,
                    source: Box::new(std::io::Error::other("codec failed")),
                });
            }
            Ok(format!("page-{page}").into_bytes())
        }
    }

    fn prepared_worker_export(
        kind: ExportOutputKind,
        calls: Arc<Mutex<Vec<PageIndex>>>,
        fail_on: Option<PageIndex>,
        cancel_on: Option<(PageIndex, Arc<ExportPhase>)>,
        page_count: usize,
    ) -> PreparedExport {
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(page_count))
            .expect("the fixture opens");
        PreparedExport {
            snapshot: document.export_snapshot().expect("the snapshot prepares"),
            codec: Arc::new(WorkerCodec {
                kind,
                calls,
                fail_on,
                cancel_on,
                on_page: None,
            }),
            request: ExportRequest {
                pages: PageRange::whole(page_count).expect("the fixture has pages"),
                dpi: 72.0,
            },
            output_kind: kind,
            page_count,
        }
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
    ///
    /// Needs the plugin that registers that tool, for the same reason the
    /// canvas's snapshot-request test does: without it the entry is
    /// correctly not live, and the assertion is about the case where it is.
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
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

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_second_right_click_inside_the_document_repositions_the_canvas_menu(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let first = gpui::point(px(300.0), px(300.0));
        let second = gpui::point(px(500.0), px(360.0));
        let mut cx = VisualTestContext::from_window(window.into(), cx);

        draw_window(&mut cx);
        cx.simulate_mouse_down(first, MouseButton::Right, gpui::Modifiers::default());
        draw_window(&mut cx);
        cx.simulate_mouse_down(second, MouseButton::Right, gpui::Modifiers::default());

        window
            .update(&mut cx, |frame, _window, _cx| {
                assert_eq!(
                    frame
                        .canvas_context_menu
                        .expect("the canvas menu remains open")
                        .origin,
                    second
                );
            })
            .unwrap();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_second_right_click_outside_the_document_dismisses_the_canvas_menu(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let inside = gpui::point(px(300.0), px(300.0));
        let outside = gpui::point(px(20.0), px(20.0));
        let mut cx = VisualTestContext::from_window(window.into(), cx);

        window
            .update(&mut cx, |frame, window, _cx| {
                let visibility = frame.shell_view_state.visibility();
                assert!(!document_view_bounds(
                    window.viewport_size(),
                    visibility,
                    frame.rail_state.expanded(),
                    frame.navigation_width(visibility.navigation_pane),
                    frame.side_panel_state,
                )
                .contains(&outside));
            })
            .unwrap();

        draw_window(&mut cx);
        cx.simulate_mouse_down(inside, MouseButton::Right, gpui::Modifiers::default());
        draw_window(&mut cx);
        cx.simulate_mouse_down(outside, MouseButton::Right, gpui::Modifiers::default());

        window
            .update(&mut cx, |frame, _window, _cx| {
                assert!(frame.canvas_context_menu.is_none());
            })
            .unwrap();
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
    fn draw_window(cx: &mut VisualTestContext) {
        cx.update(|window, app| {
            window.draw(app).clear();
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
        bound_window_with_models(tabs, paths, cx)
    }

    #[cfg(feature = "shell-test-support")]
    fn bound_window_from_bytes(
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
        // In the order `shell::run` installs them, and all of them: the
        // search field's own bindings were missing here, so a shell binding
        // that shadowed one of the field's keys looked harmless.
        cx.update(|cx| {
            crate::shell::chrome::install_search_keybindings(cx);
            crate::shell::find_bar::install_keybindings(cx);
            crate::shell::chrome::accessible::install_keybindings(cx);
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
            "{\"view.read-mode\": \"cmd-shift-h\"}",
        )
        .expect("the test writes its keymap");
        let (window, bindings) =
            bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::in_dir(&dir), cx);
        window
            .update(cx, |frame, window, cx| {
                // A query left in the global bar's field, so the panel it
                // opens has to go with the bar rather than hang under it.
                frame
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
                |frame, _window, cx| frame.show_dialog(ShellDialog::ZoomTo, cx),
                |frame, _cx| frame.dialog.is_some(),
            ),
            (
                "the canvas context menu",
                |frame, _window, cx| {
                    frame.canvas_context_menu = Some(CanvasContextMenu {
                        origin: gpui::point(px(300.0), px(300.0)),
                    });
                    cx.notify();
                },
                |frame, _cx| frame.canvas_context_menu.is_some(),
            ),
            (
                "the main menu",
                |frame, _window, cx| frame.toggle_main_menu(cx),
                |frame, _cx| frame.main_menu_open,
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

    /// Dynamic Zoom is a drag, so its menu entry selects a tool rather than
    /// changing the view. Pressed on a real window through the binding a
    /// user would give it, and asserted on the tool the canvas ends up with.
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    #[gpui::test]
    fn a_keymap_binding_selects_the_dynamic_zoom_tool(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("dynamic-zoom-keymap");
        std::fs::write(
            dir.join(crate::config::KEYMAP_FILE),
            "{\"view.dynamic-zoom\": \"cmd-shift-z\"}",
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
                frame.search_input.update(cx, |input, cx| {
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
        assert_eq!(active.zoom_percent, Some(200));
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
    /// single-page PNG: numbering `report.png` to `report-01.png` when there
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
            PathBuf::from("/exports/report-01.png")
        );
        assert_eq!(
            export_path(chosen, Some(9), 12),
            PathBuf::from("/exports/report-10.png")
        );
        assert_eq!(
            export_path(chosen, Some(0), 1_234),
            PathBuf::from("/exports/report-0001.png")
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
            PathBuf::from("/exports/report-02")
        );
    }

    /// A name with dots in it keeps every one of them but the last: the stem
    /// of `q1.2026.png` is `q1.2026`, not `q1`.
    #[test]
    fn a_dotted_name_numbers_on_its_last_extension_only() {
        assert_eq!(
            export_path(Path::new("/exports/q1.2026.png"), Some(0), 2),
            PathBuf::from("/exports/q1.2026-01.png")
        );
    }

    #[test]
    fn the_export_phase_has_one_terminal_race_winner() {
        let cancellation_wins = ExportPhase::new();
        assert!(cancellation_wins.cancel());
        assert!(!cancellation_wins.begin_publishing());
        assert_eq!(cancellation_wins.load(), ExportPhaseValue::Cancelling);

        let publication_wins = ExportPhase::new();
        assert!(publication_wins.begin_publishing());
        assert!(!publication_wins.cancel());
        assert_eq!(publication_wins.load(), ExportPhaseValue::Publishing);
    }

    #[test]
    fn duplicate_per_page_destinations_fail_preflight() {
        let path = PathBuf::from("report-01.test");
        let failure = preflight_export_paths(&[path.clone(), path.clone()]).unwrap_err();

        assert!(
            matches!(failure, ExportFailure::Duplicate { path: duplicate } if duplicate == path)
        );
    }

    #[test]
    fn a_per_page_worker_streams_pages_in_absolute_order() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let phase = ExportPhase::new();
        let completed = AtomicUsize::new(0);
        let prepared =
            prepared_worker_export(ExportOutputKind::PerPage, Arc::clone(&calls), None, None, 3);

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &completed).unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(*calls.lock().unwrap(), [0, 1, 2]);
        assert_eq!(completed.load(Ordering::Acquire), 3);
        assert_eq!(
            std::fs::read(dir.path().join("report-01.test")).unwrap(),
            b"page-0"
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-02.test")).unwrap(),
            b"page-1"
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-03.test")).unwrap(),
            b"page-2"
        );
    }

    #[test]
    fn immediate_prepublication_cancellation_wins_for_both_output_kinds() {
        for kind in [ExportOutputKind::Single, ExportOutputKind::PerPage] {
            let dir = tempfile::tempdir().expect("the test directory opens");
            let chosen = dir.path().join("report.test");
            let phase = Arc::new(ExportPhase::new());
            let cancel_phase = Arc::clone(&phase);
            let observer = RecordingExportObserver {
                before_publish: Some(Arc::new(move || {
                    assert!(cancel_phase.cancel());
                })),
                ..RecordingExportObserver::default()
            };
            let prepared =
                prepared_worker_export(kind, Arc::new(Mutex::new(Vec::new())), None, None, 2);

            assert_eq!(
                run_export_worker_observed(
                    prepared,
                    &chosen,
                    &phase,
                    &AtomicUsize::new(0),
                    &observer,
                )
                .unwrap(),
                ExportOutcome::Cancelled
            );
            assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
        }
    }

    #[test]
    fn a_blocked_single_file_worker_keeps_the_destination_absent_until_success() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (started_tx, started_rx) = mpsc::channel();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(1)).unwrap();
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().unwrap(),
            codec: Arc::new(BlockingCodec {
                kind: ExportOutputKind::Single,
                block_on: 0,
                calls,
                started: Mutex::new(Some(started_tx)),
                release: Arc::clone(&release),
            }),
            request: ExportRequest {
                pages: PageRange::whole(1).unwrap(),
                dpi: 72.0,
            },
            output_kind: ExportOutputKind::Single,
            page_count: 1,
        };
        let worker_path = chosen.clone();
        let worker = std::thread::spawn(move || {
            run_export_worker(
                prepared,
                &worker_path,
                &ExportPhase::new(),
                &AtomicUsize::new(0),
            )
        });

        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!chosen.exists());
        let (released, ready) = &*release;
        *released.lock().unwrap() = true;
        ready.notify_all();
        assert_eq!(worker.join().unwrap().unwrap(), ExportOutcome::Complete);
        assert_eq!(std::fs::read(chosen).unwrap(), b"page-0");
    }

    #[test]
    fn a_many_page_worker_never_has_more_than_one_destination_writer_open() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let observer = RecordingExportObserver::default();
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            128,
        );

        assert_eq!(
            run_export_worker_observed(
                prepared,
                &chosen,
                &ExportPhase::new(),
                &AtomicUsize::new(0),
                &observer,
            )
            .unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(observer.open_writers.load(Ordering::Acquire), 0);
        assert_eq!(observer.max_open_writers.load(Ordering::Acquire), 1);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 128);
    }

    #[test]
    fn rollback_preserves_a_completed_page_replaced_by_another_writer() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let first = dir.path().join("report-01.test");
        let phase = Arc::new(ExportPhase::new());
        let replace_path = first.clone();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(3))
            .expect("the fixture opens");
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().expect("the snapshot prepares"),
            codec: Arc::new(WorkerCodec {
                kind: ExportOutputKind::PerPage,
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_on: None,
                cancel_on: Some((1, Arc::clone(&phase))),
                on_page: Some(Arc::new(move |page| {
                    if page == 1 {
                        std::fs::remove_file(&replace_path).unwrap();
                        std::fs::write(&replace_path, b"replacement").unwrap();
                    }
                })),
            }),
            request: ExportRequest {
                pages: PageRange::whole(3).unwrap(),
                dpi: 72.0,
            },
            output_kind: ExportOutputKind::PerPage,
            page_count: 3,
        };

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert_eq!(std::fs::read(first).unwrap(), b"replacement");
        assert!(!dir.path().join("report-02.test").exists());
        assert!(!dir.path().join("report-03.test").exists());
    }

    #[cfg(unix)]
    #[test]
    fn rollback_preserves_a_completed_page_replaced_by_a_symlink() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let first = dir.path().join("report-01.test");
        let target = dir.path().join("target");
        std::fs::write(&target, b"target").unwrap();
        let phase = Arc::new(ExportPhase::new());
        let replace_path = first.clone();
        let link_target = target.clone();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(3)).unwrap();
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().unwrap(),
            codec: Arc::new(WorkerCodec {
                kind: ExportOutputKind::PerPage,
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_on: None,
                cancel_on: Some((1, Arc::clone(&phase))),
                on_page: Some(Arc::new(move |page| {
                    if page == 1 {
                        std::fs::remove_file(&replace_path).unwrap();
                        std::os::unix::fs::symlink(&link_target, &replace_path).unwrap();
                    }
                })),
            }),
            request: ExportRequest {
                pages: PageRange::whole(3).unwrap(),
                dpi: 72.0,
            },
            output_kind: ExportOutputKind::PerPage,
            page_count: 3,
        };

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert!(std::fs::symlink_metadata(&first)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(first).unwrap(), b"target");
    }

    #[test]
    fn rollback_never_deletes_a_completed_page_replaced_by_a_directory() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let first = dir.path().join("report-01.test");
        let phase = Arc::new(ExportPhase::new());
        let replace_path = first.clone();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(3)).unwrap();
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().unwrap(),
            codec: Arc::new(WorkerCodec {
                kind: ExportOutputKind::PerPage,
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_on: None,
                cancel_on: Some((1, Arc::clone(&phase))),
                on_page: Some(Arc::new(move |page| {
                    if page == 1 {
                        std::fs::remove_file(&replace_path).unwrap();
                        std::fs::create_dir(&replace_path).unwrap();
                        std::fs::write(replace_path.join("marker"), b"replacement").unwrap();
                    }
                })),
            }),
            request: ExportRequest {
                pages: PageRange::whole(3).unwrap(),
                dpi: 72.0,
            },
            output_kind: ExportOutputKind::PerPage,
            page_count: 3,
        };

        let failure =
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap_err();
        assert!(failure.to_string().contains("preserved in"), "{failure}");
        let preserved_marker = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("0/marker"))
            .find(|path| path.exists())
            .expect("the substituted directory is retained in quarantine");
        assert_eq!(std::fs::read(preserved_marker).unwrap(), b"replacement");
    }

    #[test]
    fn an_existing_derived_file_fails_before_any_page_runs() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let conflict = dir.path().join("report-02.test");
        std::fs::write(&conflict, b"keep").unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let prepared =
            prepared_worker_export(ExportOutputKind::PerPage, Arc::clone(&calls), None, None, 3);

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(matches!(failure, ExportFailure::Exists { path } if path == conflict));
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&conflict).unwrap(), b"keep");
        assert!(!dir.path().join("report-01.test").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_derived_symlink_fails_before_any_page_runs() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let conflict = dir.path().join("report-02.test");
        std::os::unix::fs::symlink(dir.path().join("missing"), &conflict).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let prepared =
            prepared_worker_export(ExportOutputKind::PerPage, Arc::clone(&calls), None, None, 3);

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(matches!(failure, ExportFailure::Exists { path } if path == conflict));
        assert!(calls.lock().unwrap().is_empty());
        assert!(std::fs::symlink_metadata(conflict)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn a_codec_failure_removes_every_per_page_output() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::clone(&calls),
            Some(1),
            None,
            3,
        );

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(failure.to_string().contains("encoding page 2"));
        assert_eq!(*calls.lock().unwrap(), [0, 1]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancellation_after_page_one_removes_it_and_stops_before_page_three() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let phase = Arc::new(ExportPhase::new());
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::clone(&calls),
            None,
            Some((1, Arc::clone(&phase))),
            3,
        );

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert_eq!(*calls.lock().unwrap(), [0, 1]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancellation_after_the_only_codec_return_publishes_nothing() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let phase = Arc::new(ExportPhase::new());
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            Some((0, Arc::clone(&phase))),
            1,
        );

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert!(!chosen.exists());
    }

    #[test]
    fn publication_preserves_an_existing_single_destination() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        std::fs::write(&chosen, b"keep").unwrap();
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            2,
        );

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(matches!(failure, ExportFailure::Exists { .. }));
        assert_eq!(std::fs::read(chosen).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_late_cancel_cannot_override_single_file_publication() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let phase = Arc::new(ExportPhase::new());
        let cancel_phase = Arc::clone(&phase);
        let observer = RecordingExportObserver {
            after_publish_started: Some(Arc::new(move || {
                assert!(!cancel_phase.cancel());
            })),
            ..RecordingExportObserver::default()
        };
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            2,
        );

        assert_eq!(
            run_export_worker_observed(prepared, &chosen, &phase, &AtomicUsize::new(0), &observer,)
                .unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(std::fs::read(chosen).unwrap(), b"page-0page-1");
    }

    #[test]
    fn cancellation_preserves_an_existing_single_destination() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        std::fs::write(&chosen, b"keep").unwrap();
        let phase = Arc::new(ExportPhase::new());
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            Some((0, Arc::clone(&phase))),
            1,
        );

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert_eq!(std::fs::read(chosen).unwrap(), b"keep");
    }

    #[test]
    fn a_late_cancel_cannot_override_per_page_publication() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let phase = Arc::new(ExportPhase::new());
        let cancel_phase = Arc::clone(&phase);
        let observer = RecordingExportObserver {
            after_publish_started: Some(Arc::new(move || {
                assert!(!cancel_phase.cancel());
            })),
            ..RecordingExportObserver::default()
        };
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            2,
        );

        assert_eq!(
            run_export_worker_observed(prepared, &chosen, &phase, &AtomicUsize::new(0), &observer,)
                .unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-01.test")).unwrap(),
            b"page-0"
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-02.test")).unwrap(),
            b"page-1"
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

    #[cfg(feature = "shell-test-support")]
    fn install_test_export_job(frame: &mut ShellFrame, origin: EntityId) -> Arc<ExportPhase> {
        let phase = Arc::new(ExportPhase::new());
        frame.export_job = Some(ExportJob {
            id: 7,
            origin,
            phase: Arc::clone(&phase),
            completed: Arc::new(AtomicUsize::new(1)),
            total: 3,
            last_displayed: 1,
        });
        phase
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn an_old_progress_timer_cannot_poll_a_rapidly_relaunched_export(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        window
            .update(cx, |frame, _window, cx| {
                let canvas = frame.tabs.tabs()[0].canvas.clone();
                let origin = canvas.entity_id();
                install_test_export_job(frame, origin);
                frame.finish_export(7, origin, &canvas, Ok(ExportOutcome::Complete), cx);
                let completed = Arc::new(AtomicUsize::new(2));
                frame.export_job = Some(ExportJob {
                    id: 8,
                    origin,
                    phase: Arc::new(ExportPhase::new()),
                    completed,
                    total: 3,
                    last_displayed: 0,
                });

                assert_eq!(frame.poll_export_progress(7), (false, false));
                assert_eq!(frame.export_job.as_ref().unwrap().last_displayed, 0);
                assert_eq!(frame.poll_export_progress(8), (true, true));
                assert_eq!(frame.export_job.as_ref().unwrap().last_displayed, 2);
            })
            .unwrap();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn switching_tabs_does_not_cancel_the_origin_export(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);

        let phase = window
            .update(cx, |frame, _window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                frame.activate(1, cx);
                phase
            })
            .unwrap();

        assert_eq!(phase.load(), ExportPhaseValue::Running);
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn a_second_export_is_refused_before_opening_another_prompt(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        window
            .update(cx, |frame, _window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                install_test_export_job(frame, origin);
                frame.start_export(ExportTarget::Png, cx);
            })
            .unwrap();

        assert!(!cx.did_prompt_for_new_path());
        window
            .update(cx, |frame, _window, cx| {
                assert!(matches!(
                    frame.tabs.tabs()[0].canvas.read(cx).model.status(),
                    Some(CanvasStatus::Error { message, .. })
                        if message.contains("already in progress")
                ));
            })
            .unwrap();
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn two_pending_export_prompts_install_only_one_worker(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let dir = tempfile::tempdir().expect("the test directory opens");
        let first = dir.path().join("first.png");
        let second = dir.path().join("second.png");

        window
            .update(cx, |frame, _window, cx| {
                frame.start_export(ExportTarget::Png, cx);
                frame.start_export(ExportTarget::Png, cx);
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(first.clone()));
        cx.simulate_new_path_selection(|_| Some(second.clone()));
        cx.run_until_parked();

        assert!(first.exists());
        assert!(!second.exists());
        window
            .update(cx, |frame, _window, cx| {
                assert!(matches!(
                    frame.tabs.tabs()[0].canvas.read(cx).model.status(),
                    Some(CanvasStatus::Error { message, .. })
                        if message.contains("already in progress")
                ));
            })
            .unwrap();
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn worker_failure_releases_progress_and_the_export_guard(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("failed.test");
        let completed = Arc::new(AtomicUsize::new(0));
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::new(Mutex::new(Vec::new())),
            Some(1),
            None,
            3,
        );
        let result = run_export_worker(prepared, &chosen, &ExportPhase::new(), &completed);
        assert_eq!(completed.load(Ordering::Acquire), 1);
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());

        window
            .update(cx, |frame, window, cx| {
                let canvas = frame.tabs.tabs()[0].canvas.clone();
                let origin = canvas.entity_id();
                frame.export_job = Some(ExportJob {
                    id: 7,
                    origin,
                    phase: Arc::new(ExportPhase::new()),
                    completed: Arc::clone(&completed),
                    total: 3,
                    last_displayed: 0,
                });
                assert_eq!(frame.poll_export_progress(7), (true, true));
                frame.start_export(ExportTarget::Png, cx);
                assert!(frame.export_job.is_some());
                frame.finish_export(7, origin, &canvas, result, cx);
                assert!(frame.export_job.is_none());
                assert!(frame
                    .accessible(window, cx)
                    .find(&"export-progress".into())
                    .is_none());
                frame.start_export(ExportTarget::Png, cx);
            })
            .unwrap();

        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn closing_the_origin_tab_cancels_its_export(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);

        let phase = window
            .update(cx, |frame, _window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                frame.run_tab_command(TabCommand::Close, 0, cx).unwrap();
                phase
            })
            .unwrap();

        assert_eq!(phase.load(), ExportPhaseValue::Cancelling);
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn close_others_and_close_all_cancel_a_removed_origin(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
        let close_others_phase = window
            .update(cx, |frame, _window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                frame
                    .run_tab_command(TabCommand::CloseOthers, 1, cx)
                    .unwrap();
                phase
            })
            .unwrap();
        assert_eq!(close_others_phase.load(), ExportPhaseValue::Cancelling);

        let origin = window
            .update(cx, |frame, _window, _cx| {
                frame.tabs.tabs()[0].canvas.entity_id()
            })
            .unwrap();
        let close_all_phase = window
            .update(cx, |frame, _window, cx| {
                let phase = install_test_export_job(frame, origin);
                frame.run_tab_command(TabCommand::CloseAll, 0, cx).unwrap();
                phase
            })
            .unwrap();
        assert_eq!(close_all_phase.load(), ExportPhaseValue::Cancelling);
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn export_progress_and_cancellation_are_accessible(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        let phase = window
            .update(cx, |frame, window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                let tree = frame.accessible(window, cx);
                assert_eq!(
                    tree.find(&"export-progress".into()).unwrap().label,
                    "Exporting 1 of 3 pages"
                );
                let cancel = tree.find(&"cancel-export".into()).unwrap();
                assert_eq!(cancel.activation, Some(Activation::CancelExport));
                assert!(!cancel.state.disabled);
                frame.run_activation(Activation::CancelExport, window, cx);
                phase
            })
            .unwrap();

        assert_eq!(phase.load(), ExportPhaseValue::Cancelling);
        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                assert_eq!(
                    tree.find(&"export-progress".into()).unwrap().label,
                    "Cancelling export, 1 of 3 pages complete"
                );
                assert!(tree.find(&"cancel-export".into()).unwrap().state.disabled);
            })
            .unwrap();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_blocked_export_leaves_tab_actions_responsive_and_cancel_stops_the_next_page(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
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
                frame.export_job = Some(ExportJob {
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
                frame.start_export(ExportTarget::Png, cx);
                let canvas = frame
                    .tabs
                    .tabs()
                    .iter()
                    .find(|tab| tab.canvas.entity_id() == frame.export_job.as_ref().unwrap().origin)
                    .unwrap()
                    .canvas
                    .clone();
                let origin = frame.export_job.as_ref().unwrap().origin;
                frame.finish_export(7, origin, &canvas, Ok(ExportOutcome::Cancelled), cx);
                assert!(frame.export_job.is_none());
                assert!(frame
                    .accessible(window, cx)
                    .find(&"export-progress".into())
                    .is_none());
                frame.start_export(ExportTarget::Png, cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn stale_export_prompt_after_switching_tabs_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
        let dir = export_dir("stale-export-switch");
        let chosen = dir.join("hello.png");

        window
            .update(cx, |frame, _window, cx| {
                frame.start_export(ExportTarget::Png, cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.activate(1, cx);
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(!chosen.exists(), "a stale export wrote after tab switch");
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn stale_export_prompt_after_closing_the_tab_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
        let dir = export_dir("stale-export-close");
        let chosen = dir.join("hello.png");

        window
            .update(cx, |frame, _window, cx| {
                frame.start_export(ExportTarget::Png, cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.run_tab_command(TabCommand::Close, 0, cx).unwrap();
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(!chosen.exists(), "a stale export wrote after tab close");
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn stale_attachment_prompt_after_switching_tabs_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window_from_bytes(
            vec![
                (
                    "with-attachment.pdf",
                    crate::shell::fixtures::attachment_pdf(),
                ),
                ("other.pdf", crate::shell::fixtures::outline_pdf()),
            ],
            cx,
        );
        let dir = export_dir("stale-attachment-switch");
        let chosen = dir.join("notes.txt");

        window
            .update(cx, |frame, _window, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
                frame.run_pane_action(PaneAction::Attachment(panes::AttachmentAction::Save(0)), cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.activate(1, cx);
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(
            !chosen.exists(),
            "a stale attachment save wrote after tab switch"
        );
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn stale_attachment_prompt_after_closing_the_tab_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window_from_bytes(
            vec![
                (
                    "with-attachment.pdf",
                    crate::shell::fixtures::attachment_pdf(),
                ),
                ("other.pdf", crate::shell::fixtures::outline_pdf()),
            ],
            cx,
        );
        let dir = export_dir("stale-attachment-close");
        let chosen = dir.join("notes.txt");

        window
            .update(cx, |frame, _window, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
                frame.run_pane_action(PaneAction::Attachment(panes::AttachmentAction::Save(0)), cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.run_tab_command(TabCommand::Close, 0, cx).unwrap();
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(
            !chosen.exists(),
            "a stale attachment save wrote after tab close"
        );
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
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

    #[cfg(feature = "shell-test-support")]
    fn focused_key(
        window: gpui::WindowHandle<ShellFrame>,
        cx: &mut TestAppContext,
    ) -> Option<String> {
        window
            .update(cx, |frame, _window, _cx| {
                frame.a11y.focused().map(ToString::to_string)
            })
            .unwrap()
    }

    #[cfg(feature = "shell-test-support")]
    fn current_page(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> usize {
        window
            .update(cx, |frame, _window, cx| {
                frame
                    .active_canvas()
                    .unwrap()
                    .read(cx)
                    .model
                    .view_state()
                    .current_page
            })
            .unwrap()
    }

    /// Tab is the whole keyboard story: before P12 the chrome had one focus
    /// handle and no tab order at all.
    ///
    /// Driven with the real keystroke rather than by calling the handler,
    /// which is how two dead find-bar routes shipped.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn tab_walks_the_chrome_in_reading_order_and_shift_tab_walks_back(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();
        let first = focused_key(window, cx);
        assert_eq!(first.as_deref(), Some("main-menu-button"));

        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();
        let second = focused_key(window, cx);
        assert!(second.is_some());
        assert_ne!(second, first);

        cx.simulate_keystrokes(window.into(), "shift-tab");
        cx.run_until_parked();
        assert_eq!(focused_key(window, cx), first);
    }

    /// Enter on a focused control runs the same thing a click on it runs.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn enter_on_a_focused_control_runs_what_clicking_it_runs(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();

        let before = current_page(window, cx);
        assert!(before > 0, "the fixture opens on the first page");
        window
            .update(cx, |frame, _window, _cx| {
                assert!(
                    frame.a11y.focus_key(&"previous-page".into()),
                    "Previous Page is not in the tab order"
                );
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();

        assert_eq!(
            current_page(window, cx),
            before - 1,
            "Enter on Previous Page did not turn the page"
        );
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn enter_with_nothing_focused_changes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        let before = current_page(window, cx);

        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();

        assert_eq!(current_page(window, cx), before);
    }

    /// The rectangle the tree gives the first page.
    #[cfg(feature = "shell-test-support")]
    fn page_bounds(
        window: gpui::WindowHandle<ShellFrame>,
        cx: &mut TestAppContext,
    ) -> accesskit::Rect {
        window
            .update(cx, |frame, window, cx| {
                frame
                    .accessible(window, cx)
                    .find(&("page", 0usize).into())
                    .expect("the tree carries no page node")
                    .bounds
                    .expect("the page node carries no rectangle")
            })
            .unwrap()
    }

    /// The document's geometry, read off the tree after the view has been
    /// turned and moved.
    ///
    /// `Rects::view_rect` is the only transform placing a page and the words
    /// on it on screen, and a version of it answering one constant
    /// window-sized rectangle for everything left every other test green: the
    /// probe's "the words are inside the page" check is satisfied for free
    /// when both rectangles are the same rectangle, and it looks at the left
    /// and right edges only.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_page_and_its_words_keep_their_places_after_the_view_moves(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();
        // The words are only extracted for something that is listening.
        window
            .update(cx, |frame, _window, _cx| frame.a11y.attach_client())
            .unwrap();
        cx.run_until_parked();

        // Turned and zoomed in, so the page is larger than the view and the
        // scroll below has somewhere to go.
        window
            .update(cx, |frame, window, cx| {
                frame.run_activation(Activation::View(ViewAction::RotateClockwise), window, cx);
                for _ in 0..4 {
                    frame.run_activation(Activation::View(ViewAction::ZoomIn), window, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        let before = page_bounds(window, cx);

        let scrolled = window
            .update(cx, |frame, _window, cx| {
                let canvas = frame.active_canvas().cloned().expect("a document is open");
                canvas.update(cx, |canvas, cx| {
                    let was = canvas.model.viewport().offset();
                    canvas
                        .model
                        // A pan ignores the anchor, so any window point does.
                        .scroll(
                            ViewPoint { x: 0.0, y: -40.0 },
                            false,
                            Point {
                                x: px(1.0),
                                y: px(1.0),
                            },
                        )
                        .expect("the view scrolls");
                    cx.notify();
                    canvas.model.viewport().offset() != was
                })
            })
            .unwrap();
        cx.run_until_parked();
        assert!(
            scrolled,
            "the view had nowhere to scroll, so what follows would prove nothing"
        );

        assert_ne!(
            page_bounds(window, cx),
            before,
            "the page reports the same rectangle after the view scrolled"
        );

        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                let page = tree
                    .find(&("page", 0usize).into())
                    .expect("the tree carries no page node");
                let words = page
                    .children
                    .iter()
                    .find(|child| child.label.contains("Hello Onionskin"))
                    .expect("the page published none of its words");
                let page = page.bounds.expect("the page node carries no rectangle");
                let words = words.bounds.expect("the words carry no rectangle");

                // All four edges. A run that reads as sitting outside the page
                // it is on puts a screen reader's cursor off the document.
                assert!(
                    words.x0 > page.x0 && words.x1 < page.x1,
                    "the words {words:?} are not inside the page {page:?} left to right"
                );
                assert!(
                    words.y0 > page.y0 && words.y1 < page.y1,
                    "the words {words:?} are not inside the page {page:?} top to bottom"
                );

                let size = window.viewport_size();
                let factor = f64::from(window.scale_factor());
                let whole_window = accesskit::Rect::new(
                    0.0,
                    0.0,
                    f64::from(f32::from(size.width)) * factor,
                    f64::from(f32::from(size.height)) * factor,
                );
                assert_ne!(
                    page, whole_window,
                    "the page reports the whole window as its rectangle"
                );
            })
            .unwrap();
    }

    /// A screen reader's press, from the queue the platform's action handler
    /// writes into all the way to the page turning.
    ///
    /// This is how a VoiceOver user operates a control, and it is the only
    /// path with no keyboard and no mouse in it. The unit tests in
    /// `crate::a11y` say which element a request resolves to; this says that
    /// resolving it is followed by running it.
    ///
    /// Nothing here asks for a frame, and that is the point: gpui draws no
    /// frame for a window macOS reports as not visible, so a press that had
    /// to wait for one would sit in the queue until the user brought the
    /// window forward. The adapter wakes the shell on the main queue instead.
    /// A test window that nothing has marked dirty is the closest this
    /// harness gets to that window.
    ///
    /// Both halves are asserted, because they fail separately: the drain,
    /// which is what runs the control, and the publish, which is what the
    /// reader then hears. A shell that drained and published nothing passed
    /// every test in this file.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_screen_reader_press_runs_the_control_it_named_without_waiting_for_a_frame(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        let before = current_page(window, cx);
        assert!(
            before > 0,
            "the fixture opens past the first page, which is what leaves Previous Page live"
        );

        window
            .update(cx, |frame, _window, _cx| {
                frame
                    .a11y
                    .deliver(&"previous-page".into(), accesskit::Action::Click);
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(
            current_page(window, cx),
            before - 1,
            "a screen reader's press on Previous Page did not turn the page"
        );

        // And the tree a reader reads says so. Previous Page is dimmed on
        // the first page, so its own node carries the answer. That this
        // happens without a frame is
        // `serving_a_screen_reader_publishes_the_tree_without_a_frame`: gpui
        // draws every dirty window at the end of its own effect flush under
        // cfg(test) (`app.rs`, `flush_effects`), so a test that lets the
        // flush finish cannot tell which publish it is reading.
        window
            .update(cx, |frame, _window, _cx| {
                let node = frame
                    .a11y
                    .published_node(&"previous-page".into())
                    .expect("the published tree carries no Previous Page node");
                assert!(
                    node.is_disabled(),
                    "the press ran but the tree a reader reads was never republished"
                );
            })
            .unwrap();
    }

    /// The publish half of serving a screen reader off the frame.
    ///
    /// Read inside one window update, before GPUI flushes its effects, which
    /// is the only place in this harness where no frame can have intervened:
    /// under cfg(test) gpui draws every dirty window at the end of a flush,
    /// so after `run_until_parked` a render has always republished and a
    /// shell that published nothing of its own looks identical.
    ///
    /// That the wake is what calls this is the mutation on `Shared::wake`,
    /// which kills the press test above. This is the other link in the same
    /// chain: what the wake calls has to publish, or a reader working a
    /// window that is not drawing hears the state from before the press.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn serving_a_screen_reader_publishes_the_tree_without_a_frame(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                let before = frame
                    .a11y
                    .published_node(&"previous-page".into())
                    .expect("the published tree carries no Previous Page node");
                assert!(
                    !before.is_disabled(),
                    "the fixture opens with Previous Page already dimmed"
                );

                frame
                    .a11y
                    .deliver(&"previous-page".into(), accesskit::Action::Click);
                frame.serve_accessibility(window, cx);

                let after = frame
                    .a11y
                    .published_node(&"previous-page".into())
                    .expect("the published tree carries no Previous Page node");
                assert!(
                    after.is_disabled(),
                    "serving the request ran the control and published nothing about it"
                );
            })
            .unwrap();
    }

    /// The other half of what a screen reader asks for: moving its cursor    /// The other half of what a screen reader asks for: moving its cursor
    /// onto a control has to move the shell's own focus, or the next Enter
    /// runs whatever the ring was left on.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_screen_reader_cursor_moves_the_shell_s_focus(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        assert_ne!(focused_key(window, cx).as_deref(), Some("zoom-in"));

        window
            .update(cx, |frame, _window, _cx| {
                frame
                    .a11y
                    .deliver(&"zoom-in".into(), accesskit::Action::Focus);
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(focused_key(window, cx).as_deref(), Some("zoom-in"));
    }

    /// The tab stops the named container holds, as the published tree has
    /// them. Used to say that focus left a surface rather than that it landed
    /// on one named control, which would pass for the wrong reason the moment
    /// the surface's own order changed.
    #[cfg(feature = "shell-test-support")]
    fn stops_under(
        window: gpui::WindowHandle<ShellFrame>,
        cx: &mut TestAppContext,
        container: &'static str,
    ) -> Vec<String> {
        window
            .update(cx, |frame, window, cx| {
                frame
                    .accessible(window, cx)
                    .find(&container.into())
                    .unwrap_or_else(|| panic!("the tree carries no {container} node"))
                    .walk()
                    .filter(|element| element.is_tab_stop())
                    .map(|element| element.key.to_string())
                    .collect()
            })
            .unwrap()
    }

    /// The arrows move inside the surface focus is in, and Tab leaves it.
    ///
    /// Driven with real keystrokes, and asserting on the whole surface rather
    /// than on one control: a flat ring answers "the control next to it" to
    /// both keys, which is the defect this replaces.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn an_arrow_moves_inside_the_page_controls_and_tab_leaves_them(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        let row = stops_under(window, cx, "page-controls");
        assert!(
            row.len() > 3,
            "the page controls published {} stops, so this would prove little",
            row.len()
        );

        window
            .update(cx, |frame, _window, _cx| {
                assert!(frame.a11y.focus_key(&"first-page".into()));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "right");
        cx.run_until_parked();

        let after_arrow = focused_key(window, cx).expect("the arrow left the ring empty");
        assert_eq!(
            after_arrow,
            row[row
                .iter()
                .position(|key| key == "first-page")
                .expect("First Page is in the row")
                + 1],
            "the arrow did not move to the next control in the row"
        );

        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();

        let after_tab = focused_key(window, cx).expect("Tab left the ring empty");
        assert!(
            !row.contains(&after_tab),
            "Tab stayed inside the page controls, on {after_tab}"
        );

        // The end of the row wraps to the start of the row. A ring that is
        // still flat underneath answers every other arrow correctly and only
        // gives itself away here, by spilling into the surface next door.
        let last = row.last().expect("the row has stops").clone();
        window
            .update(cx, |frame, _window, _cx| {
                assert!(frame
                    .a11y
                    .focus_key(&gpui::ElementId::Name(last.clone().into())));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "right");
        cx.run_until_parked();

        assert_eq!(
            focused_key(window, cx).as_deref(),
            Some(row[0].as_str()),
            "the arrow ran off the end of the page controls instead of wrapping in them"
        );

        // Right stops at the page number field, because Right is that field's
        // own caret key from there on. Down is what walks the whole row. The
        // acceptance script tells a tester exactly this, so it is pinned here
        // rather than left as folklore.
        window
            .update(cx, |frame, window, cx| {
                assert!(frame.a11y.focus_key(&PAGE_ENTRY_ID.into()));
                frame.focus_ring_target(window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "right");
        cx.run_until_parked();
        assert_eq!(
            focused_key(window, cx).as_deref(),
            Some(PAGE_ENTRY_ID),
            "Right moved the ring out of the page number field instead of the caret"
        );

        cx.simulate_keystrokes(window.into(), "down");
        cx.run_until_parked();
        assert_ne!(
            focused_key(window, cx).as_deref(),
            Some(PAGE_ENTRY_ID),
            "Down did not carry the ring out of the page number field"
        );
    }

    /// The defect in the ledger: with a pane open, every row was a tab stop,
    /// so Tab crossed the pane one page at a time. Tab now steps over the
    /// whole pane.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn tab_steps_over_an_open_pane_rather_than_through_its_rows(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        window
            .update(cx, |frame, _window, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Thumbnails), cx);
            })
            .unwrap();
        cx.run_until_parked();
        let rows = stops_under(window, cx, "thumbnail-rows");
        assert!(
            rows.len() > 1,
            "the pane published {} rows, so Tab skipping them would prove nothing",
            rows.len()
        );
        assert_eq!(rows[0], "thumbnail-row-0");

        window
            .update(cx, |frame, _window, _cx| {
                assert!(
                    frame.a11y.focus_key(&("thumbnail-row", 0_usize).into()),
                    "the first thumbnail row is not in the tab order"
                );
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();

        let after = focused_key(window, cx).expect("Tab left the ring empty");
        assert!(
            !rows.contains(&after),
            "Tab moved to the next row, {after}, instead of leaving the pane"
        );

        // And the arrows are what reaches the rest of the pane.
        window
            .update(cx, |frame, _window, _cx| {
                assert!(frame.a11y.focus_key(&("thumbnail-row", 0_usize).into()));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "down");
        cx.run_until_parked();

        assert_eq!(focused_key(window, cx).as_deref(), Some(rows[1].as_str()));

        // And they stay in the pane: the last row wraps to the first rather
        // than falling into whatever the pane is next to.
        cx.simulate_keystrokes(window.into(), "down");
        cx.run_until_parked();

        assert_eq!(
            focused_key(window, cx).as_deref(),
            Some(rows[0].as_str()),
            "the arrow left the pane at its last row"
        );
    }

    /// The ledger's focus-dispatch defect: an AccessKit focus request moved
    /// the ring but left GPUI's focus in the text field the user had been
    /// typing in, so the next Enter went to the field and the control the
    /// reader was sitting on never ran.
    ///
    /// Previous Page rather than Zoom In, which is what the ledger names:
    /// whether a zoom step is allowed depends on the raster ceiling of a page
    /// the render worker measures on a thread of its own, so a run that got
    /// there first and a run that did not disagree about whether Zoom In did
    /// anything. Turning a page is the same route with an answer that does
    /// not depend on the worker.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_screen_reader_cursor_takes_the_keys_off_a_text_field(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        let before = current_page(window, cx);
        assert!(before > 0, "the fixture opens past the first page");
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(None, window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                assert!(
                    frame.text_field_focused(window, cx),
                    "the find bar did not take the keys, so this would prove nothing"
                );
            })
            .unwrap();

        window
            .update(cx, |frame, _window, _cx| {
                frame
                    .a11y
                    .deliver(&"previous-page".into(), accesskit::Action::Focus);
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            focused_key(window, cx).as_deref(),
            Some("previous-page"),
            "the reader's cursor did not reach the control"
        );
        window
            .update(cx, |frame, window, cx| {
                assert!(
                    !frame.text_field_focused(window, cx),
                    "the field still holds the keys after the reader moved off it"
                );
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();

        assert_eq!(
            current_page(window, cx),
            before - 1,
            "Enter after the reader moved onto Previous Page did not turn the page:              the field kept the keys"
        );
    }

    /// A reader moving its cursor onto a text field has to put the keys in
    /// the field, or it types into nothing.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_screen_reader_cursor_on_a_text_field_gives_it_the_keys(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| {
                frame
                    .a11y
                    .deliver(&"global-search-input".into(), accesskit::Action::Focus);
            })
            .unwrap();
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                assert!(
                    frame
                        .search_input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window),
                    "the reader's cursor left the search field without the keys"
                );
            })
            .unwrap();
    }

    /// The same contract the other way round: GPUI's focus moving into a
    /// field has to move the reader's cursor there, or the tree keeps naming
    /// the control the ring was left on.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn focusing_a_text_field_moves_the_published_cursor_onto_it(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        window
            .update(cx, |frame, _window, _cx| {
                assert!(frame.a11y.focus_key(&"zoom-in".into()));
            })
            .unwrap();

        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(None, window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(
            focused_key(window, cx).as_deref(),
            Some(FIND_INPUT_ID),
            "the ring still sits on the control it was left on"
        );
        // The ring is not what a screen reader reads. Only the focus node of
        // the published update leaves the process, and passing `None` there
        // used to break no test.
        window
            .update(cx, |frame, _window, _cx| {
                assert_eq!(
                    frame.a11y.published_focus().map(|key| key.to_string()),
                    Some(FIND_INPUT_ID.to_owned()),
                    "the published tree does not name the field as focused"
                );
            })
            .unwrap();
    }

    /// A keymap that binds a bare arrow still gets it.
    ///
    /// The keymap installs its bindings with no key context, and a binding
    /// with no context matches at the full depth of the context stack
    /// (`keymap.rs`, `binding_enabled`), which is deeper than the shell's own
    /// `OnionskinShell`. Ties at equal depth go to whichever was installed
    /// later, and `shell::run` installs the keymap's after the ring's. So the
    /// user's binding wins twice over, and reordering the two installs is
    /// what this test is here to catch. Driven through a real keymap file and
    /// a real keystroke, because which of two bindings wins is a question
    /// only dispatch can answer.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_keymap_binding_on_a_bare_arrow_still_runs(cx: &mut TestAppContext) {
        let dir = crate::config::test_dir("arrow-keymap");
        let file = dir.join(crate::config::KEYMAP_FILE);
        std::fs::write(&file, "{\"view.previous-page\": \"up\"}")
            .expect("the test can write its own keymap");
        let (window, bindings) = bound_window_in(
            &["two-page.pdf"],
            crate::config::ConfigPaths::in_dir(&dir),
            cx,
        );
        let _ = std::fs::remove_file(&file);
        assert_eq!(
            keystroke_for(&bindings, "view.previous-page"),
            "up",
            "the keymap file did not reach the window"
        );
        cx.run_until_parked();
        let before = current_page(window, cx);
        assert!(before > 0, "the fixture opens past the first page");

        cx.simulate_keystrokes(window.into(), "up");
        cx.run_until_parked();

        assert_eq!(
            current_page(window, cx),
            before - 1,
            "the ring took the arrow the keymap bound"
        );
    }

    /// Every stop the tree publishes can be reached with the keyboard.
    ///
    /// The sweep the ring exists for. A ring that leads somewhere it cannot
    /// leave is worse than no ring: the controls past the dead end are
    /// published, announced, and unreachable, and nothing else here would
    /// notice. Walks Tab once per group and the arrows once per stop in the
    /// widest group, which covers every group and every stop in it.
    ///
    /// With the find bar open, because its first stop is its text field,
    /// which is the shape that dead-ends.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn every_published_stop_can_be_reached_from_the_keyboard(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        sweep(window, cx, "tab", "down");
        sweep(window, cx, "shift-tab", "up");

        // Again with the find bar open, which is the surface that dead-ends:
        // its first stop is its own text field.
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(Some("page".to_owned()), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        sweep(window, cx, "tab", "down");
        sweep(window, cx, "shift-tab", "up");
    }

    /// Walk `across` once per group and `along` once per stop in the widest
    /// group, which covers every group and every stop in it, and assert that
    /// the walk reached everything the tree publishes.
    ///
    /// Run in both directions, because a ring can wrap forwards and strand
    /// backwards.
    #[cfg(feature = "shell-test-support")]
    fn sweep(
        window: gpui::WindowHandle<ShellFrame>,
        cx: &mut TestAppContext,
        across: &str,
        along: &str,
    ) {
        let stops = stops_under(window, cx, "window");
        let sizes = window
            .update(cx, |frame, _window, _cx| frame.a11y.group_sizes())
            .unwrap();
        assert!(
            stops.len() > 15 && sizes.len() > 3,
            "the window published {} stops in {} groups, which would prove little",
            stops.len(),
            sizes.len()
        );
        let widest = sizes.iter().copied().max().unwrap_or(0);

        let mut reached = std::collections::BTreeSet::new();
        for _ in 0..sizes.len() {
            cx.simulate_keystrokes(window.into(), across);
            cx.run_until_parked();
            reached.extend(focused_key(window, cx));
            for _ in 0..widest {
                cx.simulate_keystrokes(window.into(), along);
                cx.run_until_parked();
                reached.extend(focused_key(window, cx));
            }
        }

        let missing: Vec<&String> = stops
            .iter()
            .filter(|stop| !reached.contains(*stop))
            .collect();
        assert!(
            missing.is_empty(),
            "{} of {} published stops cannot be reached with {across} and {along}: {missing:?}",
            missing.len(),
            stops.len()
        );
    }

    /// The find bar is the surface that dead-ends: its first stop is its text
    /// field, so Tab enters the group there and GPUI focus goes into the
    /// input. The arrows have to keep working from inside it or the eight
    /// controls beside it are published and unreachable.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_arrows_walk_the_find_bar_out_of_its_own_field(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(Some("page".to_owned()), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let bar = stops_under(window, cx, "find-bar");
        assert!(bar.len() > 4, "the find bar published {} stops", bar.len());
        assert_eq!(
            bar[0], FIND_INPUT_ID,
            "the find bar no longer starts with its field, so this proves nothing"
        );

        window
            .update(cx, |frame, window, cx| {
                assert!(frame.a11y.focus_key(&FIND_INPUT_ID.into()));
                frame.focus_ring_target(window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                assert!(
                    frame.text_field_focused(window, cx),
                    "the ring on the field did not give it the keys"
                );
            })
            .unwrap();

        let mut walked = vec![FIND_INPUT_ID.to_owned()];
        for _ in 1..bar.len() {
            cx.simulate_keystrokes(window.into(), "down");
            cx.run_until_parked();
            walked.push(focused_key(window, cx).expect("the arrow left the ring empty"));
        }

        assert_eq!(walked, bar, "the arrows did not walk the find bar in order");
    }

    /// And the arrows the field itself needs stay with the field: Left and
    /// Right move the caret, not the ring. Deleting the guard that does this
    /// used to break no test.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_find_field_keeps_the_arrows_that_move_its_caret(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(Some("page".to_owned()), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let before = window
            .update(cx, |frame, window, cx| {
                assert!(frame.a11y.focus_key(&FIND_INPUT_ID.into()));
                frame.focus_ring_target(window, cx);
                frame.find_input.read(cx).selected_range()
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), "left");
        cx.run_until_parked();

        assert_eq!(
            focused_key(window, cx).as_deref(),
            Some(FIND_INPUT_ID),
            "Left moved the focus ring instead of the caret"
        );
        window
            .update(cx, |frame, _window, cx| {
                let after = frame.find_input.read(cx).selected_range();
                assert_ne!(after, before, "Left did not move the caret");
                assert_eq!(
                    frame.find_input.read(cx).query(),
                    "page",
                    "the query changed while walking the caret"
                );
            })
            .unwrap();
    }

    /// The UI-thread extraction the ledger records: the shell parsed every
    /// visible page's content stream on every frame, whether or not anything
    /// was listening, so a scroll paid for text nobody could hear.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_page_s_text_is_not_extracted_until_a_screen_reader_attaches(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();

        assert_eq!(
            extracted_pages(window, cx),
            0,
            "the shell parsed a page's content stream with nothing listening"
        );
        assert_eq!(
            page_text(window, cx),
            vec!["Page 1 has not been read yet".to_owned()],
            "the page said something other than that it had not been read"
        );

        window
            .update(cx, |frame, _window, _cx| {
                frame.a11y.attach_client();
            })
            .unwrap();
        cx.run_until_parked();

        assert!(
            extracted_pages(window, cx) > 0,
            "the shell never extracted the page after a client attached"
        );
        let text = page_text(window, cx);
        assert!(
            text.iter().any(|run| run.contains("Hello Onionskin")),
            "the page published no words to the attached client: {text:?}"
        );
    }

    #[cfg(feature = "shell-test-support")]
    fn extracted_pages(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> usize {
        window
            .update(cx, |frame, _window, cx| {
                frame
                    .active_canvas()
                    .unwrap()
                    .read(cx)
                    .model
                    .extracted_pages()
            })
            .unwrap()
    }

    /// Every run of words the tree carries under the first page.
    #[cfg(feature = "shell-test-support")]
    fn page_text(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
        window
            .update(cx, |frame, window, cx| {
                frame
                    .accessible(window, cx)
                    .find(&("page", 0usize).into())
                    .expect("the tree carries no page node")
                    .children
                    .iter()
                    .map(|child| child.label.clone())
                    .collect()
            })
            .unwrap()
    }

    /// Escape peels overlays off one at a time, topmost first. Before P12 the
    /// only thing it closed was the find bar, and menus were dismissed by
    /// clicking an invisible layer no keyboard could reach.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn escape_closes_the_menu_first_and_then_the_find_bar(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(None, window, cx);
                frame.toggle_main_menu(cx);
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |frame, _window, _cx| {
                assert!(!frame.main_menu_open, "escape left the menu open");
                assert!(frame.find.is_open(), "escape closed two things at once");
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |frame, _window, _cx| {
                assert!(!frame.find.is_open(), "escape left the find bar open");
            })
            .unwrap();
    }

    /// The global search panel had no keyboard way out: it is open because
    /// the field has something in it, and only the mouse could empty it.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn escape_closes_the_search_panel_before_the_find_bar(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(None, window, cx);
                frame
                    .search_input
                    .update(cx, |input, cx| input.set_query("zoom", cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |frame, _window, cx| {
                assert!(frame.search_panel_visible(cx), "the panel did not open");
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |frame, _window, cx| {
                assert!(!frame.search_panel_visible(cx), "the panel is still open");
                assert!(frame.find.is_open(), "escape closed two things at once");
            })
            .unwrap();
    }

    /// A dialog had no keyboard way out at all before P12: it closed by
    /// clicking outside it or by clicking its Close button.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn escape_closes_a_dialog(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        window
            .update(cx, |frame, _window, cx| {
                frame.show_preferences(PreferenceCategory::General, cx);
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, _cx| assert!(frame.dialog.is_none()))
            .unwrap();
    }

    /// Tab out of a text field has to take the field's focus with it. Without
    /// that the ring lands on a control while the field keeps the keys, and
    /// the user is on something Enter cannot operate with no way back.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn tab_out_of_a_text_field_takes_the_keys_with_it(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        let before = current_page(window, cx);
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(None, window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                assert!(
                    !frame.text_field_focused(window, cx),
                    "tab left the keys with the find field"
                );
                assert!(frame.a11y.focus_key(&"previous-page".into()));
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();

        assert_eq!(
            current_page(window, cx),
            before - 1,
            "Enter after tabbing out of the field did not run the focused control"
        );
    }

    /// The find field binds its own key context, which has to beat the
    /// shell's: Enter there finds the next match rather than pressing
    /// whatever the focus ring is on.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn enter_in_the_find_field_finds_rather_than_pressing_the_focused_control(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&["two-page.pdf"], cx);
        cx.run_until_parked();
        let before = current_page(window, cx);
        window
            .update(cx, |frame, window, cx| {
                frame.open_find_bar(Some("page".to_owned()), window, cx);
                assert!(frame.a11y.focus_key(&"previous-page".into()));
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();

        assert_eq!(
            current_page(window, cx),
            before,
            "Enter in the find field turned the page"
        );
        window
            .update(cx, |frame, _window, cx| {
                assert!(
                    frame
                        .active_canvas()
                        .unwrap()
                        .read(cx)
                        .model
                        .search()
                        .current_ordinal()
                        .is_some(),
                    "Enter in the find field did not step the search"
                );
            })
            .unwrap();
    }

    /// A modal dialog replaces the chrome in the tree rather than joining it,
    /// which is what makes it modal to a screen reader as well as to a mouse.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn an_open_dialog_is_the_only_thing_the_tree_offers(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();

        let chrome = window
            .update(cx, |frame, window, cx| {
                frame
                    .accessible(window, cx)
                    .find(&"page-controls".into())
                    .is_some()
            })
            .unwrap();
        assert!(
            chrome,
            "the page controls were not in the tree to begin with"
        );

        window
            .update(cx, |frame, _window, cx| {
                frame.show_preferences(PreferenceCategory::General, cx);
            })
            .unwrap();
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                assert!(
                    tree.find(&"dialog".into()).is_some(),
                    "the dialog is not in the tree"
                );
                assert!(
                    tree.find(&"page-controls".into()).is_none(),
                    "the chrome is still reachable behind a modal dialog"
                );
            })
            .unwrap();
    }

    /// The document is a tab stop of its own, so a keyboard user can land on
    /// the page rather than tabbing past it, and it announces as a document
    /// rather than as the group AccessKit's role mapping would otherwise
    /// leave it as.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_document_is_in_the_tree_and_in_the_tab_order(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                let document = tree.find(&"document".into()).expect("no document node");
                assert_eq!(document.role, Role::Document);
                assert_eq!(document.role_description, Some("document"));
                assert!(document.label.contains("hello.pdf"));
                assert!(document.is_tab_stop());
                let page = tree.find(&("page", 0usize).into()).expect("no page node");
                assert_eq!(page.label, "Page 1 of 1");
                assert_eq!(page.role_description, Some("page"));
                assert!(page.bounds.is_some(), "the page node carries no rectangle");
            })
            .unwrap();
    }

    /// The page's own words, in the tree the shell builds.
    ///
    /// The headline of the whole package: a document a screen reader can read
    /// rather than a rectangle it announces the name of. Publishing no text
    /// nodes at all survived the entire lib suite, and only the probe noticed,
    /// which is a job the probe runs with `continue-on-error` in CI. So it is
    /// asserted here as well, inside the gate that fails the build.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_page_publishes_its_own_words_under_the_page_they_are_on(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();
        // The words are extracted for a listening client and not before.
        // That nothing is extracted without one is
        // `a_page_s_text_is_not_extracted_until_a_screen_reader_attaches`.
        window
            .update(cx, |frame, _window, _cx| frame.a11y.attach_client())
            .unwrap();
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                let page = tree
                    .find(&("page", 0usize).into())
                    .expect("the tree carries no page node");
                let runs: Vec<(String, &str)> = page
                    .children
                    .iter()
                    .filter(|child| child.role == Role::Label)
                    .map(|child| (child.key.to_string(), child.label.as_str()))
                    .collect();

                assert!(
                    runs.iter()
                        .any(|(_, text)| text.contains("Hello Onionskin")),
                    "the page published none of its own words; it published {runs:?}"
                );
                // Keyed by the page they sit under, so the identifier a screen
                // reader reads back names the page it is looking at.
                assert!(
                    runs.iter().all(|(key, _)| key.starts_with("page-0-text-")),
                    "a run of text is keyed away from its page: {runs:?}"
                );
            })
            .unwrap();
    }

    /// Every tab stop has to have something to run, or Tab lands somewhere
    /// Enter cannot leave.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn every_tab_stop_carries_an_activation(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                let stops: Vec<&A11yElement> = tree
                    .walk()
                    .filter(|element| element.is_tab_stop())
                    .collect();
                assert!(stops.len() > 10, "the tab order is {} long", stops.len());
                for stop in stops {
                    assert!(
                        stop.activation.is_some(),
                        "{} is a tab stop with nothing to run",
                        stop.key
                    );
                }
            })
            .unwrap();
    }

    /// Two nodes sharing a key would share an AccessKit id, and a screen
    /// reader would lose its place between them.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn no_two_nodes_in_the_published_tree_share_a_key(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                let mut keys: Vec<String> =
                    tree.walk().map(|element| element.key.to_string()).collect();
                let count = keys.len();
                keys.sort();
                keys.dedup();
                assert_eq!(keys.len(), count, "the tree published a duplicate key");
            })
            .unwrap();
    }

    /// The other half of the tab-order contract: anything the shell says can
    /// be activated has to be reachable to activate it.
    ///
    /// A role missing from `a11y::tree::is_focusable` fails here, which is
    /// how a whole pane of bookmarks was found carrying activations nothing
    /// could run.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn everything_with_an_action_is_reachable_or_disabled(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();
        // Open every pane in turn, so their rows are in the tree to check.
        for pane in crate::shell::panes::NavigationPane::ALL {
            window
                .update(cx, |frame, _window, cx| {
                    frame.run_pane_action(PaneAction::Select(pane), cx);
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |frame, window, cx| {
                    for element in frame.accessible(window, cx).walk() {
                        if element.activation.is_none() || element.state.disabled {
                            continue;
                        }
                        assert!(
                            element.is_tab_stop(),
                            "{} can be activated but nothing can reach it; its role {:?} is not in is_focusable",
                            element.key,
                            element.role
                        );
                    }
                })
                .unwrap();
        }
    }

    /// A node with no name is a node a screen reader announces as its role
    /// and nothing else.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn nothing_in_the_tree_is_announced_without_a_name(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        cx.run_until_parked();

        window
            .update(cx, |frame, window, cx| {
                for element in frame.accessible(window, cx).walk() {
                    assert!(
                        !element.label.trim().is_empty(),
                        "{} is published with no name",
                        element.key
                    );
                }
            })
            .unwrap();
    }
}
