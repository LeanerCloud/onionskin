//! `ShellFrame`'s own state, and the tab list under it.
//!
//! Separated from the frame's behaviour so that adding one field edits a file
//! of declarations rather than the middle of the implementation. `ShellFrame::new`
//! lives here too, because a field that is declared in one file and initialized
//! in another is still a two-file edit. Nothing here moved between scopes:
//! `pub(super)` reaches `chrome::tabs` and everything under it, which is exactly
//! what a private item in `chrome::tabs` reached before the split.

use std::fmt;
use std::path::PathBuf;

use gpui::{AppContext as _, Context, Entity, Window};

use super::context::ContextMenuState;
use super::export::ExportState;
use super::menu::MenuOpenState;
use super::organize::OrganizeDialogs;
use super::tab_title;
use crate::shell::canvas::CanvasViewState;
use crate::shell::chrome::accessible::ShellAccessibility;
use crate::shell::chrome::page_controls::{PageEntryError, PAGE_ENTRY_ID};
use crate::shell::chrome::quick_actions::QuickActionsState;
use crate::shell::chrome::rail::RailState;
use crate::shell::chrome::side_panel::SidePanelState;
use crate::shell::chrome::theme::ShellViewState;
use crate::shell::chrome::tool_search::{SearchInput, SearchResult};
use crate::shell::dialog::ShellDialog;
use crate::shell::find_bar::{FindBarState, FIND_INPUT_ID, FIND_PLACEHOLDER};
use crate::shell::home::HomeState;
use crate::shell::panes::NavigationPanesState;
use crate::shell::{Canvas, ShellSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::chrome) enum TabError {
    OutOfRange {
        index: usize,
        count: usize,
    },
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
            Self::Unavailable(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for TabError {}

pub(super) struct DocumentTab {
    pub(super) source: PathBuf,
    pub(super) title: String,
    pub(super) canvas: Entity<Canvas>,
}

impl DocumentTab {
    pub(super) fn new(source: PathBuf, canvas: Entity<Canvas>) -> Self {
        let title = tab_title(&source);
        Self {
            source,
            title,
            canvas,
        }
    }

    pub(super) fn title(&self) -> &str {
        &self.title
    }
}

pub(super) struct TabState<T> {
    tabs: Vec<T>,
    active: Option<usize>,
}

impl TabState<DocumentTab> {
    /// Point a tab at the file its document now belongs to, after a Save As.
    pub(super) fn retitle(&mut self, index: usize, source: PathBuf) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.title = tab_title(&source);
            tab.source = source;
        }
    }
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

/// The global bar's tool search box, and what the last search there reported.
pub(super) struct ToolSearchState {
    pub(super) search_input: Entity<SearchInput>,
    pub(super) search_feedback: Option<SearchResult>,
}

/// The page-number box in the page controls, and why the last number typed
/// there was refused.
pub(super) struct PageEntryState {
    pub(super) page_input: Entity<SearchInput>,
    pub(super) page_entry_error: Option<PageEntryError>,
}

/// Every loose field that had a second field to group with is in a sub-struct
/// named for the surface it belongs to, and each sub-struct is declared in the
/// file whose code reads it, so that a package adding to one surface edits that
/// file rather than this declaration, which every package is also editing.
///
/// `ToolSearchState` and `PageEntryState` are the exception and do not deliver
/// that: their surfaces are `chrome::tool_search` and `chrome::page_controls`,
/// siblings of `tabs` rather than modules under it, so declaring there would
/// widen `pub(super)` past what these fields had.
///
/// `find` and `find_input` are one surface too and are still two fields: the
/// only spelling that groups them without a stutter renames a leaf, and this
/// restructuring's acceptance rests on renaming none.
pub(in crate::shell) struct ShellFrame {
    pub(super) tabs: TabState<DocumentTab>,
    pub(super) menus: MenuOpenState,
    pub(super) context_menus: ContextMenuState,
    pub(super) tool_search: ToolSearchState,
    pub(super) find: FindBarState,
    pub(super) find_input: Entity<SearchInput>,
    /// The find bar's Replace With.
    pub(super) replace_input: Entity<SearchInput>,
    /// Commenting preferences' author name, as typed and not yet saved.
    pub(super) commenting_author: Entity<SearchInput>,
    /// The comment properties inspector's fields.
    pub(super) inspector: crate::shell::chrome::inspector::InspectorState,
    pub(super) page_entry: PageEntryState,
    pub(super) observed_view_state: Option<CanvasViewState>,
    /// The active document's edit epoch when the frame last looked. An edit
    /// made on the canvas moves no view, so without it the tab's dirty mark,
    /// the Undo entry and an open pane's list would lag behind the edit.
    pub(super) observed_edit_epoch: Option<u64>,
    pub(super) shell_view_state: ShellViewState,
    pub(super) rail_state: RailState,
    pub(super) quick_actions_state: QuickActionsState,
    pub(super) side_panel_state: SidePanelState,
    pub(super) navigation: NavigationPanesState,
    pub(super) settings: ShellSettings,
    /// What the app has to tell the user: a file it repaired to open, a
    /// config file it could not read, a document that would not open. Shown
    /// in the window and dismissed there, not only printed to stderr.
    pub(super) notices: Vec<String>,
    pub(super) dialog: Option<ShellDialog>,
    pub(super) home: HomeState,
    pub(super) export: ExportState,
    pub(super) organize: OrganizeDialogs,
    pub(super) stamps: Option<crate::shell::chrome::stamps_dialog::StampsDialogState>,
    pub(super) summary: Option<crate::shell::chrome::summary_dialog::SummaryDialogState>,
    pub(super) properties: Option<crate::shell::chrome::properties_dialog::PropertiesDialogState>,
    pub(super) bookmark_title: Option<crate::shell::chrome::bookmark_dialog::BookmarkTitleState>,
    /// The Print dialog, while it is open.
    pub(super) print: Option<crate::shell::chrome::print_dialog::PrintDialogState>,
    /// What Page Properties shows, read when it opened.
    pub(super) page_properties: Vec<(String, String)>,
    /// Copy To or Move To Document, while it asks where.
    pub(super) send_pages: Option<crate::shell::chrome::send_pages::SendPagesState>,
    /// A link request taken from the canvas, for the next render to run.
    pub(super) pending_link: Option<onionskin_core::LinkRequest>,
    /// The Trust Manager's prompt, while it asks.
    pub(super) web_link: Option<crate::shell::chrome::web_link_dialog::WebLinkPrompt>,
    /// Create Link or Link Properties, while it is open.
    #[cfg(feature = "tools-edit")]
    pub(super) link_dialog: Option<crate::shell::chrome::link_dialog::LinkDialogState>,
    /// The redaction dialog, while it is open.
    #[cfg(feature = "redact")]
    pub(super) redact: Option<crate::shell::chrome::redact_dialog::RedactDialogState>,
    /// A redaction mark the Redact tool clicked, for the next render to open.
    #[cfg(feature = "redact")]
    pub(super) pending_redaction: Option<onionskin_core::ObjRef>,
    /// A form field's Properties, while open.
    #[cfg(feature = "tools-form")]
    pub(super) field_dialog: Option<crate::shell::chrome::field_dialog::FieldDialogState>,
    /// A field a Prepare Form tool asked Properties for, for the next render.
    #[cfg(feature = "tools-form")]
    pub(super) pending_field: Option<onionskin_core::FieldRequest>,
    /// Add Signature or Add Initials, while it is open.
    #[cfg(feature = "tools-fill-sign")]
    pub(super) signature: Option<crate::shell::chrome::signature_dialog::SignatureDialogState>,
    /// The page-marks dialog, while it is open.
    #[cfg(feature = "tools-edit")]
    pub(super) marks: Option<crate::shell::chrome::marks_dialog::MarksDialogState>,
    /// The Crop Pages dialog, while it is open.
    pub(super) crop: Option<crate::shell::chrome::crop_dialog::CropDialogState>,
    /// The Advanced Search dialog, while it is open.
    pub(super) advanced_search: Option<crate::shell::chrome::advanced_search::AdvancedSearchState>,
    /// The rail tools Manage Tools lists, read when it opened.
    pub(super) managed_tools: Vec<crate::shell::chrome::manage_tools::ManagedTool>,
    /// The Organize Pages grid, while it is open.
    pub(super) page_grid: Option<crate::shell::organize::OrganizeState>,
    /// The skins panel, while it is open.
    pub(super) skins: Option<crate::shell::skins::SkinsState>,
    /// Paper and orientation, which Page Setup and the Print dialog share.
    pub(super) page_setup: crate::shell::chrome::print_dialog::PageSetup,
    /// The question before closing unsaved documents, while it is asked.
    pub(super) unsaved: Option<crate::shell::chrome::file_dialogs::UnsavedState>,
    /// The recovery being offered, and the ones waiting their turn.
    pub(super) recover: Option<crate::shell::chrome::file_dialogs::RecoverState>,
    pub(super) pending_recoveries:
        std::collections::VecDeque<crate::shell::chrome::file_dialogs::RecoverState>,
    /// The accessibility tree the window publishes, its tab order, and the
    /// rectangles the last frame measured.
    pub(super) a11y: ShellAccessibility,
}

pub(super) fn activate_tab<T>(
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

pub(super) fn close_tab<T>(
    tabs: &mut TabState<T>,
    search_feedback: &mut Option<SearchResult>,
    index: usize,
) -> Result<bool, TabError> {
    tabs.close(index)?;
    *search_feedback = None;
    Ok(tabs.is_empty())
}

pub(super) fn close_other_tabs<T>(
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
        // Named from the same constants the accessible description names,
        // so a field and the node describing it cannot be given different
        // identities.
        let find_input =
            cx.new(|cx| SearchInput::with_placeholder(FIND_INPUT_ID, FIND_PLACEHOLDER, theme, cx));
        let replace_input = cx.new(|cx| {
            SearchInput::with_placeholder(
                crate::shell::find_bar::REPLACE_INPUT_ID,
                crate::shell::find_bar::REPLACE_PLACEHOLDER,
                theme,
                cx,
            )
        });
        let page_input =
            cx.new(|cx| SearchInput::with_placeholder(PAGE_ENTRY_ID, "Page", theme, cx));
        let author = settings.preferences.commenting_author.clone();
        let commenting_author = cx.new(|cx| {
            let mut input = SearchInput::with_placeholder(
                crate::shell::preferences_dialog::AUTHOR_FIELD_ID,
                "Your name",
                theme,
                cx,
            );
            input.set_query(author.unwrap_or_default(), cx);
            input
        });
        let inspector = crate::shell::chrome::inspector::InspectorState {
            author: cx.new(|cx| {
                SearchInput::with_placeholder(
                    crate::shell::chrome::inspector::AUTHOR_ID,
                    "Author",
                    theme,
                    cx,
                )
            }),
            subject: cx.new(|cx| {
                SearchInput::with_placeholder(
                    crate::shell::chrome::inspector::SUBJECT_ID,
                    "Subject",
                    theme,
                    cx,
                )
            }),
            filled_from: None,
        };
        cx.observe(&search_input, |frame, _, cx| {
            frame.tool_search.search_feedback = None;
            cx.notify();
        })
        .detach();
        cx.observe(&find_input, |frame, _, cx| {
            frame.find_query_changed(cx);
        })
        .detach();
        cx.observe(&page_input, |frame, _, cx| {
            frame.page_entry.page_entry_error = None;
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
        let mut frame = Self {
            tabs,
            menus: MenuOpenState::default(),
            context_menus: ContextMenuState::default(),
            tool_search: ToolSearchState {
                search_input,
                search_feedback: None,
            },
            find: FindBarState::with_options(settings.preferences.search),
            find_input,
            replace_input,
            commenting_author,
            inspector,
            page_entry: PageEntryState {
                page_input,
                page_entry_error: None,
            },
            observed_view_state,
            observed_edit_epoch: None,
            shell_view_state,
            rail_state: RailState::default(),
            quick_actions_state: QuickActionsState::default(),
            side_panel_state: SidePanelState::default(),
            navigation: NavigationPanesState::default(),
            settings,
            notices,
            dialog: None,
            home: HomeState::default(),
            export: ExportState::default(),
            organize: OrganizeDialogs::default(),
            stamps: None,
            summary: None,
            properties: None,
            bookmark_title: None,
            print: None,
            skins: None,
            page_grid: None,
            page_properties: Vec::new(),
            managed_tools: Vec::new(),
            advanced_search: None,
            crop: None,
            #[cfg(feature = "tools-edit")]
            marks: None,
            pending_link: None,
            web_link: None,
            #[cfg(feature = "tools-edit")]
            link_dialog: None,
            #[cfg(feature = "tools-fill-sign")]
            signature: None,
            #[cfg(feature = "redact")]
            redact: None,
            #[cfg(feature = "redact")]
            pending_redaction: None,
            #[cfg(feature = "tools-form")]
            field_dialog: None,
            #[cfg(feature = "tools-form")]
            pending_field: None,
            send_pages: None,
            page_setup: Default::default(),
            unsaved: None,
            recover: None,
            pending_recoveries: std::collections::VecDeque::new(),
            a11y: ShellAccessibility::new(cx),
        };
        frame.sync_page_entry(cx);
        frame.open_initial_pane(cx);
        let initial: Vec<_> = frame
            .canvases()
            .into_iter()
            .map(|canvas| canvas.entity_id())
            .collect();
        frame.attach_recovery(&initial, cx);
        frame.start_autosave(cx);
        // The chrome takes keyboard focus at launch. Without it GPUI has no
        // focus path to dispatch along, so the shell's own keys, Escape
        // included, would reach nothing until the user clicked a text field.
        window.focus(frame.a11y.focus_handle());
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::super::frame_state::TabState;
    use super::super::{CanvasViewState, PageControlsState, TabError};
    use onionskin_core::{PageLayoutMode, ViewRotation, ZoomPolicy};

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
            auto_scrolling: false,
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
            auto_scrolling: false,
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
}
