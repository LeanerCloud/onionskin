//! `ShellFrame`'s own state, and the tab list under it.
//!
//! Separated from the frame's behaviour so that adding one field edits a file
//! of declarations rather than the middle of the implementation. Nothing here
//! moved between scopes: `pub(super)` reaches `chrome::tabs` and everything
//! under it, which is exactly what a private item in `chrome::tabs` reached
//! before the split.

use std::fmt;
use std::path::PathBuf;

use gpui::Entity;

use super::context::{CanvasContextMenu, TabContextMenu};
use super::export::ExportJob;
use super::tab_title;
use crate::shell::canvas::CanvasViewState;
use crate::shell::chrome::accessible::ShellAccessibility;
use crate::shell::chrome::page_controls::PageEntryError;
use crate::shell::chrome::quick_actions::QuickActionsState;
use crate::shell::chrome::rail::RailState;
use crate::shell::chrome::side_panel::SidePanelState;
use crate::shell::chrome::theme::ShellViewState;
use crate::shell::chrome::tool_search::{SearchInput, SearchResult};
use crate::shell::dialog::ShellDialog;
use crate::shell::find_bar::FindBarState;
use crate::shell::home::HomeState;
use crate::shell::panes::NavigationPanesState;
use crate::shell::{Canvas, ShellSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::chrome) enum TabError {
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
    pub(super) tabs: TabState<DocumentTab>,
    pub(super) main_menu_open: bool,
    pub(super) tab_context_menu: Option<TabContextMenu>,
    pub(super) canvas_context_menu: Option<CanvasContextMenu>,
    pub(super) search_input: Entity<SearchInput>,
    pub(super) search_feedback: Option<SearchResult>,
    pub(super) find: FindBarState,
    pub(super) find_input: Entity<SearchInput>,
    pub(super) page_input: Entity<SearchInput>,
    pub(super) page_entry_error: Option<PageEntryError>,
    pub(super) observed_view_state: Option<CanvasViewState>,
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
    pub(super) recent_menu_open: bool,
    pub(super) home: HomeState,
    pub(super) export_job: Option<ExportJob>,
    pub(super) next_export_id: u64,
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
