//! Bookmark Properties: the dialog, and the pane commands that open it.
//!
//! Separate from the pane's own menu module because this one writes to the
//! document, through the same public writer the core tests read back.

use gpui::{Context, Window};

use onionskin_core::set_bookmark_style;

use super::ShellFrame;
use crate::shell::chrome::bookmark_properties::{
    BookmarkPropertiesAction, BookmarkPropertiesState,
};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    pub(in crate::shell) fn bookmark_properties(&self) -> Option<&BookmarkPropertiesState> {
        self.bookmark_properties.as_ref()
    }

    /// Open Properties on the bookmark at `path`, titled as the pane shows it.
    pub(super) fn open_bookmark_properties(
        &mut self,
        path: Vec<usize>,
        title: &str,
        refusal: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_dialog(ShellDialog::BookmarkProperties, window, cx);
        self.bookmark_properties = Some(BookmarkPropertiesState::new(path, title, refusal));
    }

    pub(super) fn run_bookmark_properties_action(
        &mut self,
        action: BookmarkPropertiesAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.bookmark_properties.as_mut() else {
            return;
        };
        match action {
            BookmarkPropertiesAction::Colour(colour) => state.colour = colour,
            BookmarkPropertiesAction::ToggleBold => state.bold = !state.bold,
            BookmarkPropertiesAction::ToggleItalic => state.italic = !state.italic,
            BookmarkPropertiesAction::Apply => {
                let (path, bold, italic, rgb) = {
                    let state = self.bookmark_properties.as_ref().expect("has state");
                    (state.path.clone(), state.bold, state.italic, state.rgb())
                };
                let Some(canvas) = self.active_canvas().cloned() else {
                    return;
                };
                let result = canvas.update(cx, |canvas, cx| {
                    let result = canvas
                        .model
                        .document_mut()
                        .edit_document("Bookmark Properties", |tx| {
                            set_bookmark_style(tx, &path, bold, italic, rgb)
                        });
                    if result.is_ok() {
                        canvas.handle_change(Ok(true), cx);
                    }
                    result
                });
                match result {
                    Ok(_) => {
                        self.close_dialog(window, cx);
                        self.navigation.reread(&canvas, cx);
                    }
                    Err(error) => {
                        if let Some(state) = self.bookmark_properties.as_mut() {
                            state.error = Some(error.to_string());
                        }
                    }
                }
            }
        }
        cx.notify();
    }
}
