//! The frame's half of Crop Pages: opening the dialog on the pages chosen,
//! and running the crop it asks for through `tools-edit`.

use gpui::{Context, Focusable as _, Window};
use onionskin_core::pages::shown_margins;

use super::ShellFrame;
use crate::shell::chrome::crop_dialog::{CropAction, CropDialogState, CropRequest};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    /// Open the dialog on the grid's selection, or the page on screen, with
    /// the first page's crop box as its margins.
    pub(super) fn open_crop_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pages = self.target_pages(cx);
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let Some(&first) = pages.first() else {
            return;
        };
        let (page_count, measured) = canvas.update(cx, |canvas, _| {
            let count = canvas.model.viewport().page_count();
            let measured = canvas
                .model
                .document_mut()
                .page_geometry(first)
                .map(|page| {
                    let media = page.media_box;
                    let (width, height) = (media[2] - media[0], media[3] - media[1]);
                    let size = if page.rotate % 180 == 0 {
                        (width, height)
                    } else {
                        (height, width)
                    };
                    let crop = page.crop_box.unwrap_or(media);
                    (shown_margins(media, crop, page.rotate), size)
                })
                .ok();
            (count, measured)
        });
        let Some((margins, size)) = measured else {
            return;
        };
        self.show_dialog(ShellDialog::CropPages, window, cx);
        let theme = self.shell_view_state.tokens();
        let state = CropDialogState::new(pages, page_count, margins, size, theme, cx);
        window.focus(&state.top.read(cx).focus_handle(cx));
        self.crop = Some(state);
    }

    pub(in crate::shell) fn crop_dialog(&self) -> Option<&CropDialogState> {
        self.crop.as_ref()
    }

    pub(super) fn run_crop_action(
        &mut self,
        action: CropAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.crop.as_mut() else {
            return;
        };
        state.error = None;
        match action {
            CropAction::Submit => match state.request(cx) {
                Ok(request) => self.crop_pages(request, window, cx),
                Err(error) => state.error = Some(error),
            },
            CropAction::SetToZero => state.zero(cx),
            _ => state.form.apply(action),
        }
        cx.notify();
    }

    /// Crop, then close; a refusal stays in the dialog, which keeps what was
    /// typed.
    fn crop_pages(&mut self, request: CropRequest, window: &mut Window, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let outcome = canvas.update(cx, |canvas, cx| {
            let outcome = canvas
                .model
                .edit_pages(|doc| crop_edits::apply(doc, &request));
            if outcome.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            outcome
        });
        match outcome {
            Ok(()) => {
                self.close_dialog(window, cx);
                self.follow_grid(cx);
            }
            Err(error) => {
                if let Some(state) = self.crop.as_mut() {
                    state.error = Some(super::properties::sentence(&error.to_string()));
                }
            }
        }
    }
}

/// The crop, through `tools-edit` when it is built in, and a refusal naming
/// it when it is not.
mod crop_edits {
    use onionskin_core::Document;
    use onionskin_plugin_api::CommandError;

    use crate::shell::chrome::crop_dialog::CropRequest;

    #[cfg(feature = "tools-edit")]
    pub(super) fn apply(doc: &mut Document, request: &CropRequest) -> Result<(), CommandError> {
        use onionskin_tools_edit::{crop_pages, crop_to_content, CropPages};
        match request.margins {
            Some(margins) => crop_pages(
                doc,
                &CropPages {
                    pages: request.pages.clone(),
                    which: request.which,
                    margins,
                    page_size: request.page_size,
                },
            ),
            None => crop_to_content(doc, &request.pages, request.which),
        }
    }

    #[cfg(not(feature = "tools-edit"))]
    pub(super) fn apply(_: &mut Document, _: &CropRequest) -> Result<(), CommandError> {
        Err(CommandError::Failed {
            label: "Crop Pages",
            reason: crate::shell::chrome::crop_dialog::NO_EDIT_TOOLS.to_owned(),
        })
    }
}
