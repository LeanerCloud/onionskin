//! The frame's half of Copy To Document and Move To Document.

use gpui::{Context, Window};

use super::page_grid::organize_edits;
use super::ShellFrame;
use crate::shell::chrome::send_pages::{done_message, SendPagesState};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    /// Ask which open document the grid's chosen pages go to.
    pub(super) fn open_send_pages(
        &mut self,
        moving: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pages = self.target_pages(cx);
        let Some(source) = self.active_canvas().cloned() else {
            return;
        };
        if pages.is_empty() {
            return;
        }
        let session = source.read(cx).model.shared_file();
        let targets = self
            .tabs
            .tabs()
            .iter()
            .filter(|tab| !std::rc::Rc::ptr_eq(&tab.canvas.read(cx).model.shared_file(), &session))
            .map(|tab| (tab.canvas.entity_id(), tab.title().to_owned()))
            .collect();
        self.show_dialog(ShellDialog::SendPages { moving }, window, cx);
        self.send_pages = Some(SendPagesState {
            moving,
            pages,
            targets,
        });
    }

    pub(in crate::shell) fn send_pages_dialog(&self) -> Option<&SendPagesState> {
        self.send_pages.as_ref()
    }

    /// Copy or move the pages into the chosen document, then say so.
    pub(super) fn send_pages_to(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.send_pages.clone() else {
            return;
        };
        let Some((target_id, title)) = state.targets.get(index).cloned() else {
            return;
        };
        let (Some(source), Some(target)) = (
            self.active_canvas().cloned(),
            self.tabs
                .tabs()
                .iter()
                .find(|tab| tab.canvas.entity_id() == target_id)
                .map(|tab| tab.canvas.clone()),
        ) else {
            return;
        };
        let refusal = {
            let (source, target) = (source.read(cx), target.read(cx));
            source
                .model
                .read_out_refusal()
                .map(|refusal| refusal.reason())
                .or_else(|| state.moving.then(|| source.model.edit_refusal()).flatten())
                .or_else(|| target.model.edit_refusal())
        };
        let outcome = match refusal {
            Some(reason) => Err(reason.to_owned()),
            None => {
                let (from, to) = (
                    source.read(cx).model.shared_file(),
                    target.read(cx).model.shared_file(),
                );
                let result = organize_edits::send_pages(
                    to.borrow_mut().document_mut(),
                    from.borrow_mut().document_mut(),
                    &state.pages,
                    state.moving,
                );
                result.map_err(|error| error.to_string())
            }
        };
        self.close_dialog(window, cx);
        match outcome {
            Ok(()) => {
                for canvas in [&target, &source] {
                    canvas.update(cx, |canvas, cx| canvas.handle_change(Ok(true), cx));
                }
                self.follow_grid(cx);
                self.notices
                    .push(done_message(state.moving, state.pages.len(), &title));
            }
            Err(message) => self
                .notices
                .push(format!("The pages were not sent: {message}")),
        }
        cx.notify();
    }
}
