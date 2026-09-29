//! The frame's half of bookmark and attachment authoring: the entries that
//! need a dialog. New and Rename Bookmark ask for a title; Add Attachment
//! asks for a file. Everything else runs in the panes.

use std::path::{Path, PathBuf};

use gpui::{Context, PathPromptOptions, Window};
use onionskin_core::embedded::{add_to_attachments, embed_file, mime_for, NewAttachment};
use onionskin_core::{add_bookmark, rename_bookmark};

use super::ShellFrame;
use crate::shell::chrome::bookmark_dialog::{BookmarkTitleAction, BookmarkTitleState};
use crate::shell::dialog::ShellDialog;
use crate::shell::panes::{bookmark_target, document_edit, BookmarksCommand, PaneAction};

/// A new bookmark's title until the user gives it one, as Acrobat names it.
const UNTITLED: &str = "Untitled";

impl ShellFrame {
    /// New Bookmark and Rename Bookmark, from the bookmarks pane's menu.
    pub(super) fn run_bookmark_dialog_command(
        &mut self,
        command: BookmarksCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = bookmark_target(&self.navigation);
        self.run_pane_action(PaneAction::DismissMenus, cx);
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        match command {
            BookmarksCommand::New => {
                // After the bookmark the menu was opened on, at its level;
                // last at the top level when it was opened on none.
                let (parent, index) = match &target {
                    Some(row) => {
                        let (&last, parent) = row.path.split_last().expect("a row has a path");
                        (parent.to_vec(), Some(last + 1))
                    }
                    None => (Vec::new(), None),
                };
                let added = document_edit(
                    &mut self.navigation,
                    &canvas,
                    cx,
                    "New Bookmark",
                    |page, tx| add_bookmark(tx, &parent, index, UNTITLED, Some(page)),
                );
                if let Some(path) = added {
                    self.open_bookmark_title(path, UNTITLED, window, cx);
                }
            }
            BookmarksCommand::Rename => {
                if let Some(row) = target {
                    self.open_bookmark_title(row.path, &row.title, window, cx);
                }
            }
            BookmarksCommand::Properties => {
                // The style of the title the menu was opened on, refused with
                // the document's own reason when it may not be edited.
                if let Some(row) = target {
                    let refusal = self
                        .active_canvas()
                        .map(|canvas| canvas.read(cx).model.edit_refusal())
                        .flatten();
                    self.open_bookmark_properties(row.path, &row.title, refusal, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn open_bookmark_title(
        &mut self,
        path: Vec<usize>,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_dialog(ShellDialog::BookmarkTitle, window, cx);
        let theme = self.shell_view_state.tokens();
        self.bookmark_title = Some(BookmarkTitleState::new(path, title, theme, cx));
    }

    pub(in crate::shell) fn bookmark_title_dialog(&self) -> Option<&BookmarkTitleState> {
        self.bookmark_title.as_ref()
    }

    pub(in crate::shell) fn run_bookmark_title_action(
        &mut self,
        action: BookmarkTitleAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let BookmarkTitleAction::Submit = action;
        let (Some(state), Some(canvas)) = (
            self.bookmark_title.as_ref(),
            self.tabs.active().map(|tab| tab.canvas.clone()),
        ) else {
            return;
        };
        let path = state.path.clone();
        let renamed = state.title(cx).and_then(|title| {
            document_edit(
                &mut self.navigation,
                &canvas,
                cx,
                "Rename Bookmark",
                |_, tx| rename_bookmark(tx, &path, &title),
            )
            .ok_or_else(|| {
                self.navigation
                    .take_feedback()
                    .unwrap_or_else(|| "The bookmark was not renamed".to_owned())
            })
        });
        match renamed {
            Ok(()) => self.close_dialog(window, cx),
            Err(error) => {
                if let Some(state) = self.bookmark_title.as_mut() {
                    state.error = Some(error);
                }
            }
        }
        cx.notify();
    }

    /// Add Attachment: ask for a file, then attach it.
    pub(super) fn prompt_for_attachment(&mut self, cx: &mut Context<Self>) {
        self.run_pane_action(PaneAction::DismissMenus, cx);
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.attach_file(&path, cx))
                .ok();
        })
        .detach();
    }

    /// Embed the file at `path` in the active document, under its own name,
    /// as one undo step; the pane says why not if it cannot.
    pub(in crate::shell) fn attach_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let (name, data) = match read_attachment(path) {
            Ok(file) => file,
            Err(error) => {
                self.navigation.report(Some(error));
                cx.notify();
                return;
            }
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        document_edit(
            &mut self.navigation,
            &canvas,
            cx,
            "Add Attachment",
            |_, tx| {
                let file = NewAttachment {
                    name: &name,
                    data: &data,
                    mime: mime_for(&name),
                    description: None,
                };
                let spec = embed_file(tx, &file, now)?;
                add_to_attachments(tx, &name, spec)
            },
        );
        cx.notify();
    }
}

/// The file's name and bytes, or a sentence saying which file could not be
/// read.
fn read_attachment(path: &Path) -> Result<(String, Vec<u8>), String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} is not a file", path.display()))?;
    let data = std::fs::read(path)
        .map_err(|error| format!("{} could not be read: {error}", display(path)))?;
    Ok((name, data))
}

fn display(path: &Path) -> String {
    PathBuf::from(path).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_cannot_be_read_is_named_in_the_error() {
        let error = read_attachment(Path::new("/nonexistent/onionskin/notes.txt"))
            .expect_err("no such file");
        assert!(error.contains("notes.txt"), "{error}");
        assert!(
            read_attachment(Path::new("/")).is_err(),
            "a root has no file name"
        );
    }
}
