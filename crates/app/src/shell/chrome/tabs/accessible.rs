//! The accessibility tree the frame publishes, the focus order over it, and
//! the dispatch that runs what a control says it does.
//!
//! `accessible` is the extension point an M3 package adds its child to. Child
//! order is tab order, so a control joins the keyboard path by joining the
//! tree, and `run_activation` is the exhaustive match that makes describing a
//! control without making it operable a compile error.
//!
//! This is `chrome::tabs::accessible`, a child of the frame. The shared
//! vocabulary it is built from, `Activation` and `Element`, lives one level
//! up in `chrome::accessible`.
//!
//! Moved out of `tabs/mod.rs` unchanged. `pub(super)` here reaches
//! `chrome::tabs` and everything under it, which is the scope these items had
//! while they were private items of `chrome::tabs`; the ones that were already
//! `pub(in crate::shell)` keep that spelling, which does not depend on where
//! the item sits.

use accesskit::Role;
use gpui::{App, Context, Entity, Focusable as _, Window};

use super::context::{tab_context_entries, TabContextMenu};
use super::export::{export_progress_label, ExportPhaseValue};
use super::{tab_element_id, ShellFrame};
use crate::a11y::{Request as A11yRequest, State as A11yState, Step as A11yStep};
use crate::shell::canvas::ViewAction;
use crate::shell::chrome::accessible::{
    ActivateFocused, Activation, Element as A11yElement, FocusNext, FocusNextInGroup,
    FocusPrevious, FocusPreviousInGroup, Surface, TextField,
};
use crate::shell::chrome::global_bar::main_menu_schema;
use crate::shell::chrome::page_controls::{self, PageControlsState};
use crate::shell::chrome::tool_search::SearchInput;
use crate::shell::chrome::{quick_actions, rail, side_panel};
use crate::shell::find_bar::FindSummary;
use crate::shell::panes;

impl ShellFrame {
    /// What the whole window tells a screen reader.
    ///
    /// Assembled from each surface's own `accessible`, gated by the same
    /// booleans `render` gates the surfaces by, so the tree never describes
    /// something that is not on screen. A modal dialog replaces the chrome
    /// rather than joining it: that is what makes it modal to a screen reader
    /// as well as to a mouse.
    pub(super) fn accessible(&self, window: &Window, cx: &mut Context<Self>) -> A11yElement {
        let scale = window.scale_factor();
        let title = self.tabs.active().map_or("Onionskin", |tab| tab.title());
        let mut root = A11yElement::new("window", Role::Window, format!("Onionskin, {title}"));

        if let Some(dialog) = self.dialog {
            return root.child(crate::shell::dialog::accessible(
                self,
                dialog,
                &self.a11y.rects,
                cx,
            ));
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
        if let Some(job) = &self.export.export_job {
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
                    self.page_entry.page_entry_error.as_ref(),
                    self.page_entry.page_input.read(cx).query(),
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
        if self.menus.main_menu_open {
            root = root.child(self.accessible_main_menu(cx));
        }
        if self.menus.recent_menu_open {
            root = root.child(self.accessible_recent_menu());
        }
        if let Some(menu) = self.context_menus.tab_context_menu {
            root = root.child(self.accessible_tab_context_menu(menu));
        }
        if self.context_menus.canvas_context_menu.is_some() {
            root = root.child(self.accessible_canvas_context_menu(cx));
        }
        if self.search_panel_visible(cx) {
            root = root.child(self.accessible_search_results(cx));
        }
        root
    }

    pub(super) fn accessible_global_bar(&self, cx: &App) -> A11yElement {
        A11yElement::new("global-bar", Role::Toolbar, "Global Bar")
            .child(
                A11yElement::new("main-menu-button", Role::Button, "Main Menu")
                    .with_state(A11yState::toggled(self.menus.main_menu_open))
                    .with_activation(Activation::ToggleMainMenu),
            )
            .child(
                self.tool_search
                    .search_input
                    .read(cx)
                    .accessible("Search Tools Or Document", TextField::Search),
            )
    }

    pub(super) fn accessible_tabs(&self) -> A11yElement {
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

    pub(super) fn accessible_main_menu(&self, cx: &App) -> A11yElement {
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

    pub(super) fn accessible_recent_menu(&self) -> A11yElement {
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

    pub(super) fn accessible_tab_context_menu(&self, menu: TabContextMenu) -> A11yElement {
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

    pub(super) fn accessible_canvas_context_menu(&self, cx: &App) -> A11yElement {
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

    pub(super) fn accessible_search_results(&self, cx: &App) -> A11yElement {
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
        if let Some(feedback) = self.tool_search.search_feedback.as_ref() {
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
                self.close_dialog(window, cx);
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
            Activation::ShowPreferences(category) => self.show_preferences(category, window, cx),
            Activation::ChangePreference(change) => self.change_preference(change, cx),
            Activation::CloseDialog => self.close_dialog(window, cx),
            Activation::SubmitExport => self.submit_export(window, cx),
            Activation::CancelExport => self.cancel_export(cx),
            Activation::Focus(field) => {
                if let Some(input) = self.text_field(field) {
                    window.focus(&input.read(cx).focus_handle(cx));
                }
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
    pub(super) fn focus_next(
        &mut self,
        _: &FocusNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_focus(A11yStep::Next, window, cx);
    }

    pub(super) fn focus_previous(
        &mut self,
        _: &FocusPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_focus(A11yStep::Previous, window, cx);
    }

    /// Right or Down: the next control inside the group focus is in.
    pub(super) fn focus_next_in_group(
        &mut self,
        _: &FocusNextInGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_focus_within(A11yStep::Next, window, cx);
    }

    pub(super) fn focus_previous_in_group(
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
    pub(super) fn step_focus(
        &mut self,
        step: A11yStep,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moved = if self.dialog == Some(crate::shell::dialog::ShellDialog::Export) {
            self.a11y.step_within(step)
        } else {
            self.a11y.step(step)
        };
        if !moved {
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
    pub(super) fn step_focus_within(
        &mut self,
        step: A11yStep,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
    pub(super) fn focus_ring_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.a11y.focused_activation() {
            Some(Activation::Focus(field)) => {
                if let Some(input) = self.text_field(field) {
                    window.focus(&input.read(cx).focus_handle(cx));
                }
            }
            // A wildcard over twenty-five-odd `Activation` variants, and here
            // the catch-all is the rule rather than a fallthrough: everything
            // that is not a text field leaves the keys with the chrome. What
            // keeps it safe as variants are added is that a text field is not
            // one of them. `TextField` is its own enum and `text_field` below
            // matches it exhaustively, so a fourth field is added there and
            // fails to compile, not added to `Activation` and routed here.
            _ => window.focus(self.a11y.focus_handle()),
        }
    }

    pub(super) fn text_field(&self, field: TextField) -> Option<&Entity<SearchInput>> {
        match field {
            TextField::Search => Some(&self.tool_search.search_input),
            TextField::Find => Some(&self.find_input),
            TextField::Page => Some(&self.page_entry.page_input),
            TextField::ExportFirst => self.export.dialog.as_ref().map(|dialog| &dialog.first),
            TextField::ExportLast => self.export.dialog.as_ref().map(|dialog| &dialog.last),
            TextField::ExportDpi => self
                .export
                .dialog
                .as_ref()
                .filter(|dialog| {
                    dialog.target == crate::shell::chrome::global_bar::ExportTarget::Png
                })
                .map(|dialog| &dialog.dpi),
        }
    }

    /// Enter or Space: run what clicking the focused control would run.
    ///
    /// Propagates when a text field has focus, so Enter still submits a find
    /// or a page number instead of being eaten by the focus ring.
    pub(super) fn activate_focused(
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
    pub(super) fn focused_text_field(&self, window: &Window, cx: &App) -> Option<gpui::ElementId> {
        if self.dialog.is_some() {
            return [
                TextField::ExportFirst,
                TextField::ExportLast,
                TextField::ExportDpi,
            ]
            .into_iter()
            .filter_map(|field| self.text_field(field))
            .find(|input| input.read(cx).focus_handle(cx).is_focused(window))
            .map(|input| input.read(cx).element_id().into());
        }
        [
            &self.tool_search.search_input,
            &self.find_input,
            &self.page_entry.page_input,
        ]
        .into_iter()
        .find(|input| input.read(cx).focus_handle(cx).is_focused(window))
        .map(|input| input.read(cx).element_id().into())
    }

    pub(super) fn text_field_focused(&self, window: &Window, cx: &App) -> bool {
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
    pub(super) fn run_accessibility_requests(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
}
