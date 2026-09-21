//! Combine Files and Create PDF From Multiple Files: the list the user builds.
//!
//! One dialog, two entry points, and it says which it was opened as: Create
//! PDF From Multiple Files is Combine under another name, and a user who
//! meets both deserves to know they are the same thing.
//!
//! The list is plain data - [`CombineList`] - so adding, reordering, removing,
//! previewing and expanding a file to a page selection are all tested without
//! a window. The frame owns the file pickers and the background job; this
//! module owns what the list is and what it tells a screen reader.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _,
};

use super::accessible::{Activation, Element, Rects, Surface, TextField};
use super::{SearchInput, ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// Which menu entry opened the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum CombineEntryPoint {
    Combine,
    CreateFromFiles,
}

impl CombineEntryPoint {
    pub(in crate::shell) fn title(self) -> &'static str {
        match self {
            Self::Combine => "Combine Files",
            Self::CreateFromFiles => "Create PDF From Multiple Files",
        }
    }
}

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum CombineAction {
    AddFiles,
    AddFolder,
    Select(usize),
    MoveUp,
    MoveDown,
    Remove,
    /// Apply the page field to the selected file: the per-file expansion.
    ApplyPages,
    Submit,
}

/// One file in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct CombineEntry {
    pub(in crate::shell) path: PathBuf,
    /// How many pages the file has, or why that could not be read. A file
    /// that will not open stays in the list, saying so, rather than vanishing:
    /// the combine names it when it refuses.
    pub(in crate::shell) page_count: Result<usize, String>,
    /// `None` is every page; otherwise these, in this order.
    pub(in crate::shell) pages: Option<Vec<usize>>,
}

impl CombineEntry {
    /// The row's text, which is also its preview: the file, how many pages it
    /// has, and which of them will be used.
    pub(in crate::shell) fn label(&self) -> String {
        let name = self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        match (&self.page_count, &self.pages) {
            (Err(reason), _) => format!("{name} - {reason}"),
            (Ok(count), None) => format!("{name} - {}", plural(*count, "page")),
            (Ok(count), Some(pages)) => format!(
                "{name} - {} of {count}: {}",
                plural(pages.len(), "page"),
                describe_pages(pages)
            ),
        }
    }
}

/// The list the user builds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::shell) struct CombineList {
    pub(in crate::shell) entries: Vec<CombineEntry>,
    pub(in crate::shell) selected: Option<usize>,
}

impl CombineList {
    /// Append `entries`, selecting the first of them so the page field and the
    /// move buttons act on what was just added.
    pub(in crate::shell) fn add(&mut self, entries: impl IntoIterator<Item = CombineEntry>) {
        let first = self.entries.len();
        self.entries.extend(entries);
        if self.entries.len() > first {
            self.selected = Some(first);
        }
    }

    pub(in crate::shell) fn select(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected = Some(index);
        }
    }

    /// Move the selected file one place, keeping it selected. A move off
    /// either end does nothing.
    pub(in crate::shell) fn move_selected(&mut self, earlier: bool) {
        let Some(index) = self.selected else { return };
        let target = if earlier {
            index.checked_sub(1)
        } else {
            Some(index + 1).filter(|target| *target < self.entries.len())
        };
        if let Some(target) = target {
            self.entries.swap(index, target);
            self.selected = Some(target);
        }
    }

    /// Remove the selected file, selecting the one that took its place.
    pub(in crate::shell) fn remove_selected(&mut self) {
        let Some(index) = self.selected else { return };
        self.entries.remove(index);
        self.selected = match self.entries.len() {
            0 => None,
            len => Some(index.min(len - 1)),
        };
    }

    /// Use only the pages `text` names from the selected file - "1-3, 5", one
    /// based, in the order written - or all of them when `text` is empty.
    pub(in crate::shell) fn set_pages(&mut self, text: &str) -> Result<(), String> {
        let index = self.selected.ok_or("Select a file first")?;
        let entry = &mut self.entries[index];
        let count = entry
            .page_count
            .clone()
            .map_err(|reason| format!("That file cannot be expanded: {reason}"))?;
        entry.pages = parse_page_list(text, count)?;
        Ok(())
    }

    /// What the combine is asked for, in list order.
    pub(in crate::shell) fn inputs(&self) -> Vec<(PathBuf, Option<Vec<usize>>)> {
        self.entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.pages.clone()))
            .collect()
    }

    /// Where the output is suggested: beside the first file.
    pub(in crate::shell) fn output_directory(&self) -> Option<&Path> {
        self.entries.first().and_then(|entry| entry.path.parent())
    }
}

/// A page list as a user writes one: one-based numbers and ranges separated by
/// commas, in the order they should appear. Empty means every page.
pub(in crate::shell) fn parse_page_list(
    text: &str,
    count: usize,
) -> Result<Option<Vec<usize>>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let mut pages = Vec::new();
    for part in text
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let range = parse_range(part, count)?;
        pages.extend(range.map(|page| page - 1));
    }
    if pages.is_empty() {
        return Err("Name at least one page".to_owned());
    }
    Ok(Some(pages))
}

fn parse_range(part: &str, count: usize) -> Result<RangeInclusive<usize>, String> {
    let number = |text: &str| -> Result<usize, String> {
        let page: usize = text
            .trim()
            .parse()
            .map_err(|_| format!("{:?} is not a page number", text.trim()))?;
        if (1..=count).contains(&page) {
            Ok(page)
        } else {
            Err(format!("There is no page {page}; the file has {count}"))
        }
    };
    match part.split_once('-') {
        Some((first, last)) => {
            let (first, last) = (number(first)?, number(last)?);
            if first > last {
                return Err(format!("{part} runs backwards"));
            }
            Ok(first..=last)
        }
        None => {
            let page = number(part)?;
            Ok(page..=page)
        }
    }
}

/// "1-3, 5" for zero-based `[0, 1, 2, 4]`: runs collapsed, one based.
fn describe_pages(pages: &[usize]) -> String {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for page in pages.iter().map(|page| page + 1) {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == page => *last = page,
            _ => runs.push((page, page)),
        }
    }
    runs.iter()
        .map(|(first, last)| {
            if first == last {
                first.to_string()
            } else {
                format!("{first}-{last}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// The dialog's state in the frame.
pub(in crate::shell) struct CombineDialogState {
    pub(in crate::shell) entry_point: CombineEntryPoint,
    pub(in crate::shell) list: CombineList,
    pub(super) pages: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
    /// A combine is running in the background; Combine is not offered twice.
    pub(in crate::shell) running: bool,
}

impl CombineDialogState {
    pub(super) fn new(
        entry_point: CombineEntryPoint,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let pages = cx.new(|cx| {
            SearchInput::with_placeholder("combine-pages", "All pages, or e.g. 1-3, 5", theme, cx)
        });
        Self {
            entry_point,
            list: CombineList::default(),
            pages,
            error: None,
            running: false,
        }
    }

    pub(super) fn pages_text(&self, cx: &gpui::App) -> String {
        self.pages.read(cx).query().to_owned()
    }
}

/// The controls, in the order they are drawn and tabbed through.
fn buttons(state: &CombineDialogState) -> Vec<(&'static str, &'static str, CombineAction, bool)> {
    let has_selection = state.list.selected.is_some();
    let submit = match state.entry_point {
        CombineEntryPoint::Combine => "Combine",
        CombineEntryPoint::CreateFromFiles => "Create",
    };
    vec![
        (
            "combine-add-files",
            "Add Files…",
            CombineAction::AddFiles,
            true,
        ),
        (
            "combine-add-folder",
            "Add Folder…",
            CombineAction::AddFolder,
            true,
        ),
        (
            "combine-move-up",
            "Move Up",
            CombineAction::MoveUp,
            has_selection,
        ),
        (
            "combine-move-down",
            "Move Down",
            CombineAction::MoveDown,
            has_selection,
        ),
        (
            "combine-remove",
            "Remove",
            CombineAction::Remove,
            has_selection,
        ),
        (
            "combine-apply-pages",
            "Use These Pages",
            CombineAction::ApplyPages,
            has_selection,
        ),
        (
            "combine-submit",
            submit,
            CombineAction::Submit,
            !state.list.entries.is_empty() && !state.running,
        ),
    ]
}

/// A line saying what the dialog is, for the entry point that is Combine
/// under another name.
fn entry_note(state: &CombineDialogState) -> Option<&'static str> {
    (state.entry_point == CombineEntryPoint::CreateFromFiles)
        .then_some("Creates one PDF from several: the same as Combine Files.")
}

pub(in crate::shell) fn accessible(
    state: &CombineDialogState,
    rects: &Rects,
    cx: &gpui::App,
) -> Vec<Element> {
    let mut body = Vec::new();
    if let Some(note) = entry_note(state) {
        body.push(Element::new("combine-note", Role::Label, note));
    }
    let rows = state
        .list
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            Element::new(("combine-row", index), Role::ListItem, entry.label())
                .with_state(A11yState::selected(state.list.selected == Some(index)))
                .with_activation(Activation::Combine(CombineAction::Select(index)))
        })
        .collect();
    body.push(Element::new("combine-list", Role::List, "Files to combine").with_children(rows));
    body.push(
        state
            .pages
            .read(cx)
            .accessible("Pages from the selected file", TextField::CombinePages),
    );
    if let Some(error) = &state.error {
        body.push(Element::new("combine-error", Role::Alert, error.clone()));
    }
    for (id, label, action, enabled) in buttons(state) {
        let button = Element::new(id, Role::Button, label);
        body.push(if enabled {
            button.with_activation(Activation::Combine(action))
        } else {
            button.with_state(A11yState::enabled(false))
        });
    }
    for (row, bounds) in body.iter_mut().zip(rects.of(Surface::CombineDialog)) {
        row.bounds = Some(bounds);
    }
    body
}

pub(in crate::shell) fn render(
    state: &CombineDialogState,
    rects: Rects,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::CombineDialog, &bounds, window);
        })
        .flex()
        .flex_col()
        .gap_2();
    if let Some(note) = entry_note(state) {
        body = body.child(div().id("combine-note").child(note));
    }
    let mut list = div().id("combine-list").flex().flex_col().gap_1();
    if state.list.entries.is_empty() {
        list = list.child(
            div()
                .text_color(theme.muted_text)
                .child("Add files or a folder to begin."),
        );
    }
    for (index, entry) in state.list.entries.iter().enumerate() {
        let selected = state.list.selected == Some(index);
        list = list.child(
            div()
                .id(("combine-row", index))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .when(selected, |row| row.bg(theme.selected))
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(
                        Activation::Combine(CombineAction::Select(index)),
                        window,
                        cx,
                    );
                }))
                .child(entry.label()),
        );
    }
    body = body.child(list);
    body = body.child(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child("Pages from the selected file")
            .child(state.pages.clone()),
    );
    if let Some(error) = &state.error {
        body = body.child(
            div()
                .id("combine-error")
                .text_color(theme.error_text)
                .child(error.clone()),
        );
    }
    let mut row = div().flex().flex_wrap().gap_2();
    for (id, label, action, enabled) in buttons(state) {
        row = row.child(button(
            id,
            label,
            enabled,
            theme,
            focused,
            cx,
            Activation::Combine(action),
        ));
    }
    body.child(row)
}

/// A dialog button: live, or drawn muted and taking no click.
pub(in crate::shell) fn button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    theme: ThemeTokens,
    focused: Option<&gpui::ElementId>,
    cx: &mut Context<ShellFrame>,
    activation: Activation,
) -> gpui::AnyElement {
    let base = div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .when(focused == Some(&id.into()), |button| {
            button.bg(theme.selected)
        });
    if !enabled {
        return base
            .text_color(theme.muted_text)
            .child(label)
            .into_any_element();
    }
    base.cursor_pointer()
        .hover(move |button| button.bg(theme.subtle_hover))
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(activation.clone(), window, cx);
        }))
        .child(label)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, count: usize) -> CombineEntry {
        CombineEntry {
            path: PathBuf::from(format!("/files/{name}")),
            page_count: Ok(count),
            pages: None,
        }
    }

    fn names(list: &CombineList) -> Vec<String> {
        list.entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn adding_selects_the_first_file_added() {
        let mut list = CombineList::default();
        list.add([entry("a.pdf", 1)]);
        list.add([entry("b.pdf", 2), entry("c.pdf", 3)]);
        assert_eq!(names(&list), ["a.pdf", "b.pdf", "c.pdf"]);
        assert_eq!(list.selected, Some(1));
    }

    #[test]
    fn moving_keeps_the_file_selected_and_stops_at_either_end() {
        let mut list = CombineList::default();
        list.add([entry("a.pdf", 1), entry("b.pdf", 1), entry("c.pdf", 1)]);
        list.move_selected(false);
        assert_eq!(names(&list), ["b.pdf", "a.pdf", "c.pdf"]);
        assert_eq!(list.selected, Some(1));
        list.move_selected(true);
        list.move_selected(true);
        assert_eq!(
            names(&list),
            ["a.pdf", "b.pdf", "c.pdf"],
            "the first cannot go earlier"
        );
        assert_eq!(list.selected, Some(0));
    }

    #[test]
    fn removing_selects_what_took_its_place() {
        let mut list = CombineList::default();
        list.add([entry("a.pdf", 1), entry("b.pdf", 1)]);
        list.select(1);
        list.remove_selected();
        assert_eq!(names(&list), ["a.pdf"]);
        assert_eq!(list.selected, Some(0));
        list.remove_selected();
        assert_eq!(list.selected, None);
        list.remove_selected();
        assert!(list.entries.is_empty());
    }

    /// The per-file expansion: pages picked, in the order written.
    #[test]
    fn a_file_expands_to_the_pages_written_in_the_order_written() {
        let mut list = CombineList::default();
        list.add([entry("a.pdf", 10)]);
        list.set_pages("9-10, 1, 3").expect("parses");
        assert_eq!(list.inputs()[0].1, Some(vec![8, 9, 0, 2]));
        assert_eq!(list.entries[0].label(), "a.pdf - 4 pages of 10: 9-10, 1, 3");
        list.set_pages("").expect("clears");
        assert_eq!(list.inputs()[0].1, None);
        assert_eq!(list.entries[0].label(), "a.pdf - 10 pages");
    }

    #[test]
    fn a_page_list_says_what_is_wrong_with_it() {
        assert_eq!(
            parse_page_list("1, 12", 10),
            Err("There is no page 12; the file has 10".to_owned())
        );
        assert_eq!(
            parse_page_list("4-2", 10),
            Err("4-2 runs backwards".to_owned())
        );
        assert!(parse_page_list("one", 10).is_err());
        assert_eq!(
            parse_page_list(" , ", 10),
            Err("Name at least one page".to_owned())
        );
        assert_eq!(
            parse_page_list("0", 10),
            Err("There is no page 0; the file has 10".to_owned())
        );
    }

    #[test]
    fn a_file_that_would_not_open_says_so_and_cannot_be_expanded() {
        let mut list = CombineList::default();
        list.add([CombineEntry {
            path: PathBuf::from("/files/broken.pdf"),
            page_count: Err("does not open".to_owned()),
            pages: None,
        }]);
        assert_eq!(list.entries[0].label(), "broken.pdf - does not open");
        assert!(list.set_pages("1").is_err());
    }

    #[test]
    fn the_output_is_suggested_beside_the_first_file() {
        let mut list = CombineList::default();
        assert_eq!(list.output_directory(), None);
        list.add([entry("a.pdf", 1)]);
        assert_eq!(list.output_directory(), Some(Path::new("/files")));
    }
}
