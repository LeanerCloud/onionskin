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
use super::menu::MenuPanel;
use super::{tab_element_id, ShellFrame};
use crate::a11y::{Request as A11yRequest, State as A11yState, Step as A11yStep};
use crate::shell::canvas::ViewAction;
use crate::shell::chrome::accessible::{
    ActivateFocused, Activation, Element as A11yElement, FocusNext, FocusNextInGroup,
    FocusPrevious, FocusPreviousInGroup, Surface, TextField,
};
use crate::shell::chrome::page_controls::{self, PageControlsState};
use crate::shell::chrome::tool_search::SearchInput;
use crate::shell::chrome::{quick_actions, rail, side_panel};
use crate::shell::find_bar::FindSummary;
use crate::shell::panes;
use crate::shell::panes::{
    start_comment_draft, AttachmentAction, BookmarkAction, BookmarksCommand, CommentAction,
    CommentDraftMode, LayerAction, LayersCommand, PaneAction,
};

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
            root = root.child(self.accessible_tabs(cx));
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
            let mut described = rail::accessible(
                &self.rail_entries(cx),
                self.rail_state.expanded(),
                self.skins.is_some(),
            );
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
            // The grid is in the page's place while it is open, so a screen
            // reader meets one or the other, not both.
            root = root.child(match self.accessible_grid(cx) {
                Some(grid) => grid,
                None => canvas.update(cx, |canvas, cx| {
                    canvas.accessible(&title, scale, with_text, cx)
                }),
            });
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
                    &tab.canvas.read(cx).model.search(),
                    tab.canvas.read(cx).model.viewport().page_count(),
                );
                root = root.child(crate::shell::find_bar::accessible(
                    self.find,
                    &summary,
                    self.find_input.read(cx).query(),
                    &crate::shell::find_bar::ReplaceRow {
                        replacement: self.replace_input.read(cx).query(),
                        refusal: self.replace_refusal(cx),
                    },
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
            root = root.child(side_panel::accessible(
                self.side_panel_state,
                self.active_tool_help(cx),
                self.accessible_side_panel_content(cx),
            ));
        }
        if self.menus.main_menu_open {
            root = root.child(self.accessible_main_menu(cx));
        }
        if self.menus.recent_menu_open {
            root = root.child(self.accessible_recent_menu());
        }
        if let Some(menu) = self.context_menus.tab_context_menu {
            root = root.child(self.accessible_tab_context_menu(menu, cx));
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
        let mut bar = A11yElement::new("global-bar", Role::Toolbar, "Global Bar").child(
            A11yElement::new("main-menu-button", Role::Button, "Main Menu")
                .with_state(A11yState::toggled(self.menus.showing(MenuPanel::Main)))
                .with_activation(Activation::ToggleMainMenu),
        );
        for (id, _glyph, command, availability) in self.global_file_buttons(cx) {
            let name = match command {
                super::MenuCommand::Save => "Save",
                super::MenuCommand::SaveAs => "Save As",
                super::MenuCommand::Undo => "Undo",
                _ => "Redo",
            };
            let button = A11yElement::new(id, Role::Button, name)
                .with_state(A11yState::enabled(availability.is_enabled()))
                .with_activation(Activation::MainMenu(command));
            bar = bar.child(match availability.reason() {
                Some(reason) => button.with_description(reason),
                None => button,
            });
        }
        bar.child(
            A11yElement::new("convert-button", Role::Button, "Convert")
                .with_state(A11yState::toggled(self.menus.showing(MenuPanel::Convert)))
                .with_activation(Activation::ToggleConvertMenu),
        )
        .child(
            self.tool_search
                .search_input
                .read(cx)
                .accessible("Search Tools Or Document", TextField::Search),
        )
    }

    pub(super) fn accessible_tabs(&self, cx: &App) -> A11yElement {
        A11yElement::new("tab-bar", Role::TabList, "Open Documents").with_children(
            self.tabs
                .tabs()
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    let element = A11yElement::new(
                        tab_element_id(tab.canvas.entity_id()),
                        Role::Tab,
                        tab.title().to_owned(),
                    )
                    .with_state(A11yState::selected(self.tabs.active_index() == Some(index)))
                    .with_activation(Activation::ActivateTab(index));
                    // The dot the tab draws is punctuation to a screen
                    // reader; the state is said in words.
                    if Self::is_dirty(&tab.canvas, cx) {
                        element.with_description("Unsaved changes")
                    } else {
                        element
                    }
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
        for (section_index, section) in self.menu_panel_sections(cx).into_iter().enumerate() {
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
                    read_only: false,
                })
                .with_activation(Activation::MainMenu(entry.command));
                entry_index += 1;
                if let Some(reason) = entry.availability.reason() {
                    node = node.with_description(reason);
                }
                rows.push(node);
            }
        }
        let label = match self.menus.panel {
            MenuPanel::Main => "Main Menu",
            MenuPanel::Convert => "Convert",
            MenuPanel::SaveAsOther => "Save as Other",
        };
        let mut menu = A11yElement::new("main-menu-panel", Role::Menu, label).with_children(rows);
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

    pub(super) fn accessible_tab_context_menu(
        &self,
        menu: TabContextMenu,
        cx: &App,
    ) -> A11yElement {
        let has_path = self
            .tabs
            .tabs()
            .get(menu.tab_index)
            .and_then(|tab| tab.path(cx))
            .is_some();
        let entries = tab_context_entries(menu.tab_index, self.tabs.tabs().len(), has_path)
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
            Activation::ToggleConvertMenu => self.toggle_convert_menu(cx),
            Activation::MainMenu(command) => {
                if let Err(error) = self.run_main_menu_command(command, window, cx) {
                    eprintln!("onionskin: {error}");
                }
            }
            Activation::ActivateTab(index) => self.activate(index, cx),
            Activation::TabCommand(command, index) => {
                if let Err(error) = self.request_tab_command(command, index, window, cx) {
                    eprintln!("onionskin: {error}");
                }
            }
            Activation::CanvasContext(command) => {
                self.run_canvas_context_command(command, window, cx)
            }
            Activation::DismissNotice(index) => self.dismiss_notice(index, cx),
            Activation::OpenRecent(index) => self.open_recent(index, cx),
            Activation::OpenStarred(index) => self.open_starred(index, cx),
            Activation::ToggleStar(path) => self.toggle_star(&path, cx),
            Activation::ToggleToolShown(id) => self.toggle_tool_shown(id, cx),
            Activation::AdvancedSearch(action) => self.run_advanced_action(action, window, cx),
            Activation::SendPages(index) => self.send_pages_to(index, window, cx),
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
            Activation::SubmitZoomPercent => self.submit_zoom_percent(window, cx),
            // The pane entries that open a dialog, which is the frame's.
            Activation::Pane(PaneAction::Thumbnail(crate::shell::panes::ThumbnailAction::Run(
                crate::shell::panes::ThumbnailsCommand::CropPages,
            ))) => {
                self.navigation.dismiss_thumbnail_menu();
                self.open_crop_dialog(window, cx);
            }
            Activation::Pane(PaneAction::Layer(LayerAction::Run(LayersCommand::Properties))) => {
                self.run_pane_action(PaneAction::DismissMenus, cx);
                self.open_layer_properties(window, cx);
            }
            Activation::Pane(PaneAction::Bookmark(BookmarkAction::Run(
                command @ (BookmarksCommand::New | BookmarksCommand::Rename),
            ))) => self.run_bookmark_dialog_command(command, window, cx),
            Activation::Pane(PaneAction::Attachment(AttachmentAction::Add)) => {
                self.prompt_for_attachment(cx);
            }
            Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(index))) => {
                self.run_pane_action(PaneAction::DismissMenus, cx);
                self.open_attachment(index, cx);
            }
            Activation::Pane(PaneAction::Attachment(AttachmentAction::EditDescription(index))) => {
                self.run_pane_action(PaneAction::DismissMenus, cx);
                self.open_description_dialog(index, window, cx);
            }
            Activation::Pane(PaneAction::Attachment(AttachmentAction::Search)) => {
                self.run_pane_action(PaneAction::DismissMenus, cx);
                self.search_attachments(window, cx);
            }
            Activation::Pane(PaneAction::Comment(
                action @ (CommentAction::Edit | CommentAction::Reply),
            )) => {
                let mode = if action == CommentAction::Edit {
                    CommentDraftMode::Edit
                } else {
                    CommentDraftMode::Reply
                };
                let theme = self.shell_view_state.tokens();
                start_comment_draft(&mut self.navigation, mode, theme, window, cx);
                cx.notify();
            }
            Activation::Pane(
                action @ PaneAction::Comment(CommentAction::SaveDraft | CommentAction::CancelDraft),
            ) => {
                // The field is gone, so the keys go back to the shell: Undo
                // straight after a reply has to reach the frame.
                self.run_pane_action(action, cx);
                window.focus(self.a11y.focus_handle());
            }
            Activation::Pane(action) => self.run_pane_action(action, cx),
            Activation::StepFind(direction) => self.step_find(direction, cx),
            Activation::ReplaceText { all } => self.replace_text(all, cx),
            Activation::ApplyFindOption(option) => self.apply_find_option(option, cx),
            Activation::DismissFindBar => self.dismiss_find_bar(cx),
            Activation::SetHomeView(view) => self.set_home_view(view, cx),
            Activation::OpenFromHome => self.open_from_home(window, cx),
            Activation::ShowPreferences(category) => self.show_preferences(category, window, cx),
            Activation::ChangePreference(change) => self.change_preference(change, cx),
            Activation::SaveCommentingAuthor => self.save_commenting_author(cx),
            Activation::Inspector(action) => self.run_inspector_action(action, cx),
            Activation::CloseDialog => self.close_dialog(window, cx),
            Activation::SubmitExport => self.submit_export(window, cx),
            Activation::CancelExport => self.cancel_export(cx),
            Activation::Combine(action) => self.run_combine_action(action, cx),
            Activation::Split(action) => self.run_split_action(action, window, cx),
            Activation::Crop(action) => self.run_crop_action(action, window, cx),
            #[cfg(feature = "tools-edit")]
            Activation::Marks(action) => self.run_marks_action(action, window, cx),
            #[cfg(feature = "tools-edit")]
            Activation::Link(action) => self.run_link_action(action, window, cx),
            #[cfg(feature = "tools-fill-sign")]
            Activation::Signature(action) => self.run_signature_action(action, window, cx),
            #[cfg(feature = "redact")]
            Activation::Redact(action) => self.run_redact_action(action, window, cx),
            #[cfg(feature = "tools-form")]
            Activation::FormOption(index) => self.choose_form_option(index, cx),
            #[cfg(feature = "tools-form")]
            Activation::FormSuggestion(index) => self.pick_form_suggestion(index, cx),
            #[cfg(feature = "tools-form")]
            Activation::Field(action) => self.run_field_action(action, window, cx),
            #[cfg(feature = "spelling")]
            Activation::Spelling(action) => self.run_spelling_action(action, cx),
            #[cfg(feature = "tools-edit")]
            Activation::LineStyle(choice) => {
                if let Some(canvas) = self.active_canvas().cloned() {
                    canvas.update(cx, |canvas, cx| canvas.choose_line_style(choice, cx));
                }
            }
            Activation::ToolSetting(id) => self.choose_tool_setting(&id, cx),
            Activation::Password(action) => self.run_password_action(action, window, cx),
            Activation::Protect(action) => self.run_protect_action(action, window, cx),
            Activation::WebLink(action) => self.run_web_link_action(action, window, cx),
            Activation::Description(_) => self.submit_description(window, cx),
            Activation::LayerProperties(action) => {
                self.run_layer_properties_action(action, window, cx)
            }
            Activation::ShowPermissionDetails => self.show_permission_details(window, cx),
            Activation::ShowSignatureProperties(index) => {
                self.show_signature_properties(index, window, cx)
            }
            Activation::SignatureProperties(action) => {
                self.run_signature_properties_action(action, window, cx)
            }
            Activation::Stamps(action) => self.run_stamp_action(action, window, cx),
            Activation::Summary(action) => self.run_summary_action(action, cx),
            Activation::Print(action) => self.run_print_action(action, window, cx),
            Activation::Skins(action) => self.run_skins_action(action, window, cx),
            Activation::Organize(action) => self.run_organize_action(action, window, cx),
            Activation::Properties(action) => self.run_properties_action(action, window, cx),
            Activation::BookmarkTitle(action) => self.run_bookmark_title_action(action, window, cx),
            Activation::File(action) => self.run_file_action(action, window, cx),
            Activation::Focus(field) => {
                if let Some(input) = self.focusable_field(field, cx) {
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
                if let Some(input) = self.focusable_field(field, cx) {
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
            TextField::Replace => Some(&self.replace_input),
            TextField::Page => Some(&self.page_entry.page_input),
            TextField::ExportFirst => self.export.dialog.as_ref().map(|dialog| &dialog.first),
            TextField::ExportLast => self.export.dialog.as_ref().map(|dialog| &dialog.last),
            TextField::ExportDpi => self
                .export
                .dialog
                .as_ref()
                .filter(|dialog| dialog.target.is_raster())
                .map(|dialog| &dialog.dpi),
            TextField::ExportQuality => self
                .export
                .dialog
                .as_ref()
                .filter(|dialog| dialog.target.is_lossy())
                .map(|dialog| &dialog.quality),
            TextField::CombinePages => self.organize.combine.as_ref().map(|dialog| &dialog.pages),
            TextField::SplitValue => self
                .organize
                .split
                .as_ref()
                .filter(|dialog| {
                    dialog.mode != crate::shell::chrome::split_dialog::SplitMode::TopLevelBookmarks
                })
                .map(|dialog| &dialog.value),
            TextField::PropertiesTitle
            | TextField::PropertiesAuthor
            | TextField::PropertiesSubject
            | TextField::PropertiesKeywords
            | TextField::PropertiesCustomKey
            | TextField::PropertiesCustomValue
            | TextField::PropertiesOpenPage => self
                .properties
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            TextField::BookmarkTitle => self.bookmark_title.as_ref().map(|dialog| &dialog.title),
            TextField::ZoomPercent => Some(&self.page_entry.zoom_input),
            TextField::LayerName => self.layer_properties.as_ref().map(|dialog| &dialog.name),
            TextField::AttachmentDescription => {
                self.description.as_ref().map(|dialog| &dialog.text)
            }
            TextField::CommentDraft => self.navigation.comment_draft(),
            TextField::CommentingAuthor => Some(&self.commenting_author),
            TextField::InspectorAuthor => Some(&self.inspector.author),
            TextField::InspectorSubject => Some(&self.inspector.subject),
            TextField::PrintCopies
            | TextField::PrintPages
            | TextField::PrintScale
            | TextField::PrintPosterScale
            | TextField::PrintPosterOverlap
            | TextField::PrintBookletFrom
            | TextField::PrintBookletTo => self
                .print
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            TextField::CropTop
            | TextField::CropBottom
            | TextField::CropLeft
            | TextField::CropRight
            | TextField::CropWidth
            | TextField::CropHeight => self
                .crop
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            #[cfg(feature = "tools-edit")]
            TextField::Mark(field) => self
                .marks
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            #[cfg(feature = "tools-edit")]
            TextField::Link(field) => self
                .link_dialog
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            #[cfg(feature = "tools-fill-sign")]
            TextField::Signature(field) => self
                .signature
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            #[cfg(feature = "redact")]
            TextField::Redact(field) => self
                .redact
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            // The canvas holds it, not the frame; see `focusable_field`.
            #[cfg(feature = "tools-form")]
            TextField::FormField => None,
            #[cfg(feature = "tools-edit")]
            TextField::LineText => None,
            #[cfg(feature = "spelling")]
            TextField::SpellingChangeTo => self.spelling.as_ref().map(|state| &state.change_to),
            #[cfg(feature = "tools-form")]
            TextField::Field(field) => self
                .field_dialog
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            TextField::AdvancedQuery | TextField::AdvancedValue => self
                .advanced_search
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
            TextField::DocumentPassword => {
                self.password_prompt.as_ref().map(|prompt| &prompt.input)
            }
            TextField::OpenPassword | TextField::PermissionsPassword => self
                .protect
                .as_ref()
                .and_then(|dialog| dialog.text_field(field)),
        }
    }

    /// The input to focus for `field`: the frame's own, or the form field
    /// editor the active canvas holds.
    fn focusable_field(&self, field: TextField, cx: &App) -> Option<Entity<SearchInput>> {
        #[cfg(feature = "tools-form")]
        if field == TextField::FormField {
            return self.active_canvas()?.read(cx).field_editor_input();
        }
        #[cfg(feature = "tools-edit")]
        if field == TextField::LineText {
            return self.active_canvas()?.read(cx).line_editor_input();
        }
        #[cfg(not(any(feature = "tools-form", feature = "tools-edit")))]
        let _ = cx;
        self.text_field(field).cloned()
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
            // Nothing in the chrome has the ring, so Enter is the canvas's:
            // the active tool finishes what it has pending. Without this a
            // polygon or connected line built by clicking could never end.
            if self.commit_canvas_tool(cx) {
                return;
            }
            cx.propagate();
            return;
        };
        self.run_activation(activation, window, cx);
    }

    /// Enter for the canvas: the active tool commits its pending gesture.
    pub(super) fn commit_canvas_tool(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return false;
        };
        canvas.update(cx, |canvas, cx| {
            let committed = canvas.model.commit_active_tool();
            if committed {
                canvas.handle_change(Ok(true), cx);
            }
            committed
        })
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
                TextField::ExportQuality,
                TextField::CombinePages,
                TextField::SplitValue,
            ]
            .into_iter()
            .chain(crate::shell::chrome::properties_dialog::TEXT_FIELDS)
            .chain([
                TextField::BookmarkTitle,
                TextField::CommentingAuthor,
                TextField::ZoomPercent,
                TextField::LayerName,
                TextField::AttachmentDescription,
            ])
            .chain(crate::shell::chrome::print_dialog::TEXT_FIELDS)
            .chain(crate::shell::chrome::advanced_search::TEXT_FIELDS)
            .chain(crate::shell::chrome::crop_dialog::TEXT_FIELDS)
            .chain(mark_text_fields())
            .chain(spelling_text_fields())
            .chain([TextField::DocumentPassword])
            .chain(crate::shell::chrome::protect_dialog::TEXT_FIELDS)
            .filter_map(|field| self.text_field(field))
            .find(|input| input.read(cx).focus_handle(cx).is_focused(window))
            .map(|input| input.read(cx).element_id().into());
        }
        [
            &self.tool_search.search_input,
            &self.find_input,
            &self.replace_input,
            &self.page_entry.page_input,
        ]
        .into_iter()
        .chain(self.navigation.comment_draft())
        .chain([&self.inspector.author, &self.inspector.subject])
        .cloned()
        .chain(self.canvas_field_input(cx))
        .find(|input| input.read(cx).focus_handle(cx).is_focused(window))
        .map(|input| input.read(cx).element_id().into())
    }

    /// The form field or line editor's text box on the active canvas, while
    /// one is open.
    fn canvas_field_input(&self, cx: &App) -> Option<Entity<SearchInput>> {
        let canvas = self.active_canvas()?.read(cx);
        #[cfg(feature = "tools-form")]
        if let Some(input) = canvas.field_editor_input() {
            return Some(input);
        }
        #[cfg(feature = "tools-edit")]
        if let Some(input) = canvas.line_editor_input() {
            return Some(input);
        }
        let _ = canvas;
        None
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

/// The plugin dialogs' fields, in a build that has them.
/// Check Spelling's field, in a build that checks spelling.
fn spelling_text_fields() -> Vec<TextField> {
    #[cfg(feature = "spelling")]
    return vec![TextField::SpellingChangeTo];
    #[cfg(not(feature = "spelling"))]
    Vec::new()
}

fn mark_text_fields() -> Vec<TextField> {
    #[cfg_attr(
        not(any(
            feature = "tools-edit",
            feature = "tools-fill-sign",
            feature = "redact",
            feature = "tools-form"
        )),
        allow(unused_mut)
    )]
    let mut fields = Vec::new();
    #[cfg(feature = "tools-edit")]
    fields.extend(
        crate::shell::chrome::marks_dialog::text_fields()
            .chain(crate::shell::chrome::link_dialog::text_fields()),
    );
    #[cfg(feature = "tools-fill-sign")]
    fields.extend(crate::shell::chrome::signature_dialog::text_fields());
    #[cfg(feature = "redact")]
    fields.extend(crate::shell::chrome::redact_dialog::text_fields());
    #[cfg(feature = "tools-form")]
    fields.extend(crate::shell::chrome::field_dialog::text_fields());
    fields
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "shell-test-support")]
    use super::super::tests::{
        bound_window, bound_window_from_bytes, bound_window_in, keystroke_for,
    };
    #[cfg(feature = "shell-test-support")]
    use super::super::{px, Activation, PaneAction, Point, ShellFrame, ViewAction};
    #[cfg(feature = "shell-test-support")]
    use crate::preferences::PreferenceCategory;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::accessible::Element as A11yElement;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::page_controls::PAGE_ENTRY_ID;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::find_bar::FIND_INPUT_ID;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::panes::NavigationPane;
    #[cfg(feature = "shell-test-support")]
    use accesskit::Role;
    #[cfg(feature = "shell-test-support")]
    use gpui::{Focusable as _, TestAppContext};
    #[cfg(feature = "shell-test-support")]
    use onionskin_core::ViewPoint;

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
                        .tool_search
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
                assert!(!frame.menus.main_menu_open, "escape left the menu open");
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
                    .tool_search
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
        let (window, _) = bound_window_from_bytes(
            vec![("text-pages.pdf", crate::shell::fixtures::text_pages_pdf())],
            cx,
        );
        window
            .update(cx, |frame, window, cx| {
                frame.run_view_action(
                    ViewAction::SetLayout(onionskin_core::PageLayoutMode::SinglePage),
                    cx,
                );
                frame.run_view_action(ViewAction::GoToPage(0), cx);
                frame.open_find_bar(Some("alpha".to_owned()), window, cx);
            })
            .unwrap();
        cx.run_until_parked();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            window
                .update(cx, |frame, _window, cx| {
                    let canvas = frame.active_canvas().expect("a tab").clone();
                    canvas.update(cx, |canvas, _cx| {
                        canvas.model.update().expect("the canvas updates");
                    });
                })
                .unwrap();
            cx.run_until_parked();
            let found = window
                .update(cx, |frame, _window, cx| {
                    let canvas = frame.active_canvas().expect("a tab").read(cx);
                    (
                        canvas.model.search().len(),
                        canvas.model.search().is_running(),
                    )
                })
                .unwrap();
            if !found.1 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the walk found {found:?} rather than finishing"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        window
            .update(cx, |frame, _window, cx| {
                let canvas = frame.active_canvas().expect("a tab").read(cx);
                assert_eq!(canvas.model.search().len(), 3);
                assert!(canvas.model.search().failures().is_empty());
                assert!(canvas.model.search().stopped().is_none());
                assert_eq!(canvas.model.search().cursor(), Some((0, 0)));
                assert_eq!(canvas.model.search().current_ordinal(), Some(1));
                assert_eq!(canvas.model.viewport().current_page(), 0);
            })
            .unwrap();
        window
            .update(cx, |frame, window, cx| {
                assert!(frame.a11y.focus_key(&"next-page".into()));
                assert!(
                    frame
                        .find_input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window),
                    "placing the accessibility ring moved GPUI focus off the find field"
                );
            })
            .unwrap();

        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();

        window
            .update(cx, |frame, _window, cx| {
                let canvas = frame.active_canvas().expect("a tab").read(cx);
                assert_eq!(canvas.model.search().cursor(), Some((0, 1)));
                assert_eq!(canvas.model.search().current_ordinal(), Some(2));
                assert_eq!(canvas.model.viewport().current_page(), 0);
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
            .update(cx, |frame, window, cx| {
                frame.show_preferences(PreferenceCategory::General, window, cx);
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
