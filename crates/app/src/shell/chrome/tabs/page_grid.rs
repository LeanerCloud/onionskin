//! The Organize Pages grid's half in the frame: opening it in place of the
//! canvas, the pointer gestures, and the page edits its toolbar runs.
//!
//! Every edit is `tools-organize`'s, called with the grid's selection, so
//! it is one undo step with the same page-tree transformation the menus
//! use. A build without that plugin shows the grid with its editing
//! buttons disabled, saying so.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use onionskin_core::PageIndex;

use super::ShellFrame;
use crate::shell::chrome::accessible::Element;
use crate::shell::chrome::ThemeTokens;
use crate::shell::organize::{self, GridLayout, Held, OrganizeAction, OrganizeState, Reorder};
use crate::shell::panes::{PaneAction, ThumbnailAction};

impl ShellFrame {
    #[cfg(test)]
    pub(in crate::shell) fn organize_state(&self) -> Option<&OrganizeState> {
        self.page_grid.as_ref()
    }

    /// Organize Pages: the grid in place of the page, or the page again.
    pub(super) fn toggle_organize(&mut self, cx: &mut Context<Self>) {
        if self.page_grid.take().is_some() {
            cx.notify();
            return;
        }
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let current = canvas.read(cx).model.viewport().current_page();
        self.page_grid = Some(OrganizeState::new(canvas.entity_id(), current));
        cx.notify();
    }

    pub(in crate::shell) fn run_organize_action(
        &mut self,
        action: OrganizeAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page_count = self.grid_page_count(cx);
        let Some(state) = self.page_grid.as_mut() else {
            return;
        };
        state.error = None;
        match action {
            OrganizeAction::Choose(page) => state.click(page, Held::Nothing),
            OrganizeAction::SelectAll => state.select_all(page_count),
            OrganizeAction::Smaller | OrganizeAction::Larger => {
                self.navigation
                    .thumbnails_mut()
                    .resize(action == OrganizeAction::Larger);
            }
            OrganizeAction::Close => self.page_grid = None,
            OrganizeAction::RotateCounterclockwise
            | OrganizeAction::RotateClockwise
            | OrganizeAction::Delete
            | OrganizeAction::InsertBlank => self.edit_selected_pages(action, cx),
            OrganizeAction::InsertFromFile | OrganizeAction::Replace | OrganizeAction::Extract => {
                self.prompt_for_page_file(action, cx)
            }
        }
        cx.notify();
    }

    fn grid_page_count(&self, cx: &gpui::App) -> usize {
        self.active_canvas()
            .map_or(0, |canvas| canvas.read(cx).model.viewport().page_count())
    }

    fn grid_layout(&self, cx: &gpui::App) -> Option<GridLayout> {
        let state = self.page_grid.as_ref()?;
        let _ = cx;
        Some(GridLayout::new(
            state.size().0,
            self.navigation.thumbnails().cell_size(),
        ))
    }

    /// A press on the grid, in window coordinates.
    pub(in crate::shell) fn grid_press(
        &mut self,
        at: (f32, f32),
        held: Held,
        cx: &mut Context<Self>,
    ) {
        let page_count = self.grid_page_count(cx);
        let Some(layout) = self.grid_layout(cx) else {
            return;
        };
        if let Some(state) = self.page_grid.as_mut() {
            let point = state.to_grid(at);
            state.press(point, held, layout, page_count);
        }
        cx.notify();
    }

    pub(in crate::shell) fn grid_drag(&mut self, at: (f32, f32), cx: &mut Context<Self>) {
        let page_count = self.grid_page_count(cx);
        let Some(layout) = self.grid_layout(cx) else {
            return;
        };
        if let Some(state) = self.page_grid.as_mut() {
            if state.gesture.is_some() {
                let point = state.to_grid(at);
                state.drag_to(point, layout, page_count);
                cx.notify();
            }
        }
    }

    /// The button came up: a drag that moved pages is one reorder, applied
    /// now and only now.
    pub(in crate::shell) fn grid_release(&mut self, at: (f32, f32), cx: &mut Context<Self>) {
        let page_count = self.grid_page_count(cx);
        let Some(layout) = self.grid_layout(cx) else {
            return;
        };
        let reorder = self.page_grid.as_mut().and_then(|state| {
            let point = state.to_grid(at);
            state.release(point, layout, page_count)
        });
        if let Some(reorder) = reorder {
            self.reorder_pages(&reorder, cx);
        }
        cx.notify();
    }

    /// Released outside the grid: a gesture in flight ends having done
    /// nothing.
    pub(in crate::shell) fn cancel_grid_gesture(&mut self, cx: &mut Context<Self>) -> bool {
        match self.page_grid.as_mut() {
            Some(state) if state.gesture.is_some() => {
                state.cancel();
                cx.notify();
                true
            }
            _ => false,
        }
    }

    pub(in crate::shell) fn scroll_grid(&mut self, delta: f32, cx: &mut Context<Self>) {
        let page_count = self.grid_page_count(cx);
        let Some(layout) = self.grid_layout(cx) else {
            return;
        };
        if let Some(state) = self.page_grid.as_mut() {
            state.scroll_by(delta, layout, page_count);
            cx.notify();
        }
    }

    /// The pages the grid now shows, asked for through the pane's cache.
    pub(in crate::shell) fn show_grid_band(
        &mut self,
        first: usize,
        end: usize,
        cx: &mut Context<Self>,
    ) {
        self.run_pane_action(
            PaneAction::Thumbnail(ThumbnailAction::ShowGrid { first, end }),
            cx,
        );
    }

    /// After an edit or an undo: the selection keeps to pages that exist.
    pub(super) fn follow_grid(&mut self, cx: &mut Context<Self>) {
        let page_count = self.grid_page_count(cx);
        let active = self.active_canvas().map(|canvas| canvas.entity_id());
        match self.page_grid.as_mut() {
            Some(state) if Some(state.canvas) != active => self.page_grid = None,
            Some(state) => state.clamp(page_count),
            None => {}
        }
    }

    /// Why the grid's page edits cannot run, when they cannot.
    fn grid_edit_refusal(&self, cx: &gpui::App) -> Option<&'static str> {
        organize::page_edit_refusal(
            self.active_canvas()
                .and_then(|canvas| canvas.read(cx).model.edit_refusal()),
        )
    }

    pub(super) fn accessible_grid(&self, cx: &gpui::App) -> Option<Element> {
        let state = self.page_grid.as_ref()?;
        Some(organize::accessible(
            state,
            self.navigation.thumbnails(),
            self.grid_page_count(cx),
            self.grid_edit_refusal(cx),
        ))
    }

    pub(super) fn render_grid(
        &mut self,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let page_count = self.grid_page_count(cx);
        let refusal = self.grid_edit_refusal(cx);
        let state = self.page_grid.as_ref()?;
        Some(organize::render(
            state,
            self.navigation.thumbnails(),
            page_count,
            refusal,
            theme,
            cx,
        ))
    }

    fn reorder_pages(&mut self, reorder: &Reorder, cx: &mut Context<Self>) {
        let pages = reorder.pages.clone();
        let before = reorder.before;
        if self.run_page_edit(cx, move |doc| {
            organize_edits::move_pages(doc, &pages, before)
        }) {
            if let Some(state) = self.page_grid.as_mut() {
                state.follow(reorder);
            }
        }
    }

    /// The pages an edit is about: the grid's selection while it is open,
    /// the page on screen otherwise, as Acrobat's thumbnail menu does.
    fn target_pages(&self, cx: &gpui::App) -> Vec<PageIndex> {
        match &self.page_grid {
            Some(state) => state.selected(),
            None => self
                .active_canvas()
                .map(|canvas| vec![canvas.read(cx).model.viewport().current_page()])
                .unwrap_or_default(),
        }
    }

    fn edit_selected_pages(&mut self, action: OrganizeAction, cx: &mut Context<Self>) {
        let pages = self.target_pages(cx);
        if pages.is_empty() {
            return;
        }
        let after = pages.last().copied().map_or(0, |last| last + 1);
        let first = pages.first().copied();
        let ran = self.run_page_edit(cx, move |doc| match action {
            OrganizeAction::RotateClockwise => organize_edits::rotate(doc, &pages, true),
            OrganizeAction::RotateCounterclockwise => organize_edits::rotate(doc, &pages, false),
            OrganizeAction::Delete => organize_edits::delete(doc, &pages),
            _ => organize_edits::insert_blank(doc, after),
        });
        // What is chosen afterwards: the new page, or the one that took the
        // first deleted page's place.
        let chosen = match action {
            OrganizeAction::InsertBlank => Some(after),
            OrganizeAction::Delete => first,
            _ => None,
        };
        if let (true, Some(page), Some(state)) = (ran, chosen, self.page_grid.as_mut()) {
            // The page count already moved, and the clamp after the edit said
            // so for pages this very edit removed; that is not news.
            state.click(page, Held::Nothing);
            state.error = None;
        }
    }

    /// Insert From File, Replace and Extract ask for a file first.
    fn prompt_for_page_file(&mut self, action: OrganizeAction, cx: &mut Context<Self>) {
        let pages = self.target_pages(cx);
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let source = tab.source.clone();
        let directory = source
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        if action == OrganizeAction::Extract {
            let chosen = cx.prompt_for_new_path(&directory, Some(&extracted_name(&source, &pages)));
            cx.spawn(async move |frame, cx| {
                let Ok(Ok(Some(output))) = chosen.await else {
                    return;
                };
                frame
                    .update(cx, |frame, cx| frame.extract_to(&output, &pages, cx))
                    .ok();
            })
            .detach();
            return;
        }
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    let after = pages.last().copied().map_or(0, |last| last + 1);
                    let targets = pages.clone();
                    let path_for_edit = path.clone();
                    frame.run_page_edit(cx, move |doc| match action {
                        OrganizeAction::Replace => {
                            organize_edits::replace(doc, &path_for_edit, &targets)
                        }
                        _ => organize_edits::insert_from(doc, &path_for_edit, after),
                    });
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// A thumbnails pane menu entry that acts on pages.
    pub(super) fn run_thumbnail_command(
        &mut self,
        command: crate::shell::panes::ThumbnailsCommand,
        cx: &mut Context<Self>,
    ) {
        use crate::shell::panes::ThumbnailsCommand as Command;
        self.navigation.dismiss_thumbnail_menu();
        match command {
            Command::InsertPages => self.prompt_for_page_file(OrganizeAction::InsertFromFile, cx),
            Command::ExtractPages => self.prompt_for_page_file(OrganizeAction::Extract, cx),
            Command::ReplacePages => self.prompt_for_page_file(OrganizeAction::Replace, cx),
            Command::DeletePages => self.edit_selected_pages(OrganizeAction::Delete, cx),
            Command::RotatePages => self.edit_selected_pages(OrganizeAction::RotateClockwise, cx),
            Command::PageProperties => self.open_page_properties(cx),
            Command::EmbedThumbnails => {
                self.run_page_edit(cx, |doc| {
                    onionskin_core::pages::embed_thumbnails(doc)
                        .map(|_| ())
                        .map_err(|source| onionskin_plugin_api::CommandError::Edit {
                            label: "Embed All Page Thumbnails",
                            source,
                        })
                });
            }
            Command::RemoveThumbnails => {
                self.run_page_edit(cx, |doc| {
                    onionskin_core::pages::remove_thumbnails(doc)
                        .map(|_| ())
                        .map_err(|source| onionskin_plugin_api::CommandError::Edit {
                            label: "Remove All Page Thumbnails",
                            source,
                        })
                });
            }
            // Disabled, or the pane's own.
            Command::CropPages | Command::ReduceThumbnails | Command::EnlargeThumbnails => {}
        }
        cx.notify();
    }

    /// Page Properties: the page on screen's size, turn and boxes.
    fn open_page_properties(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let rows = canvas.update(cx, |canvas, _| {
            let page = canvas.model.viewport().current_page();
            let count = canvas.model.viewport().page_count();
            canvas
                .model
                .document_mut()
                .page_geometry(page)
                .map(|geometry| {
                    page_property_rows(
                        page,
                        count,
                        PageFacts {
                            size: geometry.render_size,
                            rotate: geometry.rotate,
                            media_box: geometry.media_box,
                            crop_box: geometry.crop_box,
                        },
                    )
                })
        });
        match rows {
            Ok(rows) => {
                self.dismiss_menus(cx);
                self.page_properties = rows;
                self.dialog = Some(crate::shell::dialog::ShellDialog::PageProperties);
            }
            Err(error) => self.notices.push(error.to_string()),
        }
    }

    pub(in crate::shell) fn page_property_rows(&self) -> Vec<(String, String)> {
        self.page_properties.clone()
    }

    /// Extract the selected pages into `output`, which is then opened.
    pub(super) fn extract_to(
        &mut self,
        output: &Path,
        pages: &[PageIndex],
        cx: &mut Context<Self>,
    ) {
        let pages = pages.to_vec();
        let target = output.to_path_buf();
        if self.run_page_edit(cx, move |doc| organize_edits::extract(doc, &pages, &target)) {
            self.open_documents(&[output.to_path_buf()], cx);
        }
    }

    /// Run a page edit on the active document; what went wrong is said in
    /// the grid, or on the notice bar when the grid is not open.
    pub(super) fn run_page_edit(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(
            &mut onionskin_core::Document,
        ) -> Result<(), onionskin_plugin_api::CommandError>,
    ) -> bool {
        let Some(canvas) = self.active_canvas().cloned() else {
            return false;
        };
        let outcome = canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.edit_pages(edit);
            if outcome.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            outcome
        });
        match outcome {
            Ok(()) => {
                self.follow_grid(cx);
                true
            }
            Err(error) => {
                let message = super::properties::sentence(&error.to_string());
                match self.page_grid.as_mut() {
                    Some(state) => state.error = Some(message),
                    None => self.notices.push(message),
                }
                cx.notify();
                false
            }
        }
    }
}

/// `report.pdf` pages 2 to 4 is `report pages 2-4.pdf`.
fn extracted_name(source: &Path, pages: &[PageIndex]) -> String {
    let stem = source
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    match (pages.first(), pages.last()) {
        (Some(first), Some(last)) if first == last => format!("{stem} page {}.pdf", first + 1),
        (Some(first), Some(last)) => format!("{stem} pages {}-{}.pdf", first + 1, last + 1),
        _ => format!("{stem} pages.pdf"),
    }
}

/// The page edits, through `tools-organize` when it is built in, and a
/// refusal naming it when it is not.
mod organize_edits {
    use std::path::Path;

    use onionskin_core::{Document, PageIndex};
    use onionskin_plugin_api::CommandError;

    /// US Letter, the blank page Acrobat's Insert Blank Page makes.
    #[cfg(feature = "tools-organize")]
    const LETTER: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

    #[cfg(feature = "tools-organize")]
    pub(super) fn move_pages(
        doc: &mut Document,
        pages: &[PageIndex],
        before: PageIndex,
    ) -> Result<(), CommandError> {
        onionskin_tools_organize::move_pages(doc, pages, before)
    }

    #[cfg(feature = "tools-organize")]
    pub(super) fn rotate(
        doc: &mut Document,
        pages: &[PageIndex],
        clockwise: bool,
    ) -> Result<(), CommandError> {
        use onionskin_tools_organize::Turn;
        let turn = if clockwise {
            Turn::Clockwise
        } else {
            Turn::Counterclockwise
        };
        onionskin_tools_organize::rotate_pages(doc, pages, turn)
    }

    #[cfg(feature = "tools-organize")]
    pub(super) fn delete(doc: &mut Document, pages: &[PageIndex]) -> Result<(), CommandError> {
        onionskin_tools_organize::delete_pages(doc, pages)
    }

    #[cfg(feature = "tools-organize")]
    pub(super) fn insert_blank(doc: &mut Document, at: PageIndex) -> Result<(), CommandError> {
        onionskin_tools_organize::insert_blank_pages(doc, at, 1, LETTER)
    }

    #[cfg(feature = "tools-organize")]
    pub(super) fn insert_from(
        doc: &mut Document,
        path: &Path,
        at: PageIndex,
    ) -> Result<(), CommandError> {
        onionskin_tools_organize::insert_pages_from(doc, path, None, at)
    }

    #[cfg(feature = "tools-organize")]
    pub(super) fn replace(
        doc: &mut Document,
        path: &Path,
        targets: &[PageIndex],
    ) -> Result<(), CommandError> {
        let mut source = Document::open_path(path).map_err(|source| CommandError::Edit {
            label: "Replace Pages",
            source,
        })?;
        let pages: Vec<PageIndex> = (0..targets.len().min(source.page_count())).collect();
        onionskin_tools_organize::replace_pages_from(
            doc,
            &mut source,
            &pages,
            &targets[..pages.len()],
        )
    }

    #[cfg(feature = "tools-organize")]
    pub(super) fn extract(
        doc: &mut Document,
        pages: &[PageIndex],
        path: &Path,
    ) -> Result<(), CommandError> {
        onionskin_tools_organize::extract_pages_to(doc, pages, path, false)
    }

    #[cfg(not(feature = "tools-organize"))]
    fn missing() -> Result<(), CommandError> {
        Err(CommandError::Failed {
            label: "Organize Pages",
            reason: crate::shell::organize::NO_ORGANIZE.to_owned(),
        })
    }

    #[cfg(not(feature = "tools-organize"))]
    pub(super) fn move_pages(
        _: &mut Document,
        _: &[PageIndex],
        _: PageIndex,
    ) -> Result<(), CommandError> {
        missing()
    }

    #[cfg(not(feature = "tools-organize"))]
    pub(super) fn rotate(_: &mut Document, _: &[PageIndex], _: bool) -> Result<(), CommandError> {
        missing()
    }

    #[cfg(not(feature = "tools-organize"))]
    pub(super) fn delete(_: &mut Document, _: &[PageIndex]) -> Result<(), CommandError> {
        missing()
    }

    #[cfg(not(feature = "tools-organize"))]
    pub(super) fn insert_blank(_: &mut Document, _: PageIndex) -> Result<(), CommandError> {
        missing()
    }

    #[cfg(not(feature = "tools-organize"))]
    pub(super) fn insert_from(
        _: &mut Document,
        _: &Path,
        _: PageIndex,
    ) -> Result<(), CommandError> {
        missing()
    }

    #[cfg(not(feature = "tools-organize"))]
    pub(super) fn replace(_: &mut Document, _: &Path, _: &[PageIndex]) -> Result<(), CommandError> {
        missing()
    }

    #[cfg(not(feature = "tools-organize"))]
    pub(super) fn extract(_: &mut Document, _: &[PageIndex], _: &Path) -> Result<(), CommandError> {
        missing()
    }
}

/// What Page Properties reads from a page's geometry.
struct PageFacts {
    /// As displayed, turned.
    size: (f64, f64),
    rotate: i32,
    media_box: [f64; 4],
    crop_box: Option<[f64; 4]>,
}

/// What Page Properties lists about one page.
fn page_property_rows(page: PageIndex, count: usize, facts: PageFacts) -> Vec<(String, String)> {
    let inches = |points: f64| points / 72.0;
    let (width, height) = facts.size;
    let boxed = |[x0, y0, x1, y1]: [f64; 4]| format!("{x0} {y0} {x1} {y1}");
    vec![
        ("Page".to_owned(), format!("{} of {count}", page + 1)),
        (
            "Size".to_owned(),
            format!(
                "{width:.0} x {height:.0} points ({:.2} x {:.2} in)",
                inches(width),
                inches(height)
            ),
        ),
        (
            "Rotation".to_owned(),
            format!("{} degrees", facts.rotate.rem_euclid(360)),
        ),
        ("Media box".to_owned(), boxed(facts.media_box)),
        (
            "Crop box".to_owned(),
            facts
                .crop_box
                .map_or_else(|| "Same as the media box".to_owned(), boxed),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_properties_list_the_pages_size_turn_and_boxes() {
        let rows = page_property_rows(
            2,
            5,
            PageFacts {
                size: (792.0, 612.0),
                rotate: 90,
                media_box: [0.0, 0.0, 612.0, 792.0],
                crop_box: None,
            },
        );
        assert_eq!(rows[0], ("Page".to_owned(), "3 of 5".to_owned()));
        assert_eq!(rows[1].1, "792 x 612 points (11.00 x 8.50 in)");
        assert_eq!(rows[2].1, "90 degrees");
        assert_eq!(rows[4].1, "Same as the media box");
    }

    #[test]
    fn an_extract_is_named_after_its_pages() {
        let source = Path::new("/a/report.pdf");
        assert_eq!(extracted_name(source, &[1, 2, 3]), "report pages 2-4.pdf");
        assert_eq!(extracted_name(source, &[4]), "report page 5.pdf");
        assert_eq!(extracted_name(source, &[]), "report pages.pdf");
    }
}
