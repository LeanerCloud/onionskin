//! The frame's half of the Link dialog: opening it on a new rectangle, on
//! selected text or on a link the Link tool clicked, and writing the link
//! through `tools-edit`. Also the Edit menu's link commands.

use gpui::{Context, Window};
use onionskin_core::{ObjRef, PageRect};
use onionskin_tools_edit::links as edit_links;

use super::properties::sentence;
use super::ShellFrame;
use crate::shell::chrome::link_dialog::{LinkAction, LinkDialogState, LinkForm, LinkMode};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    /// Open the Link dialog: Create Link on `rect`, or Link Properties on
    /// `link`.
    pub(super) fn open_link_editor(
        &mut self,
        rect: Option<PageRect>,
        link: Option<ObjRef>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let (page_count, current, found) = canvas.update(cx, |canvas, _| {
            let count = canvas.model.viewport().page_count();
            let current = canvas.model.viewport().current_page();
            let found =
                link.map(|link| edit_links::find_link(&mut canvas.model.document_mut(), link));
            (count, current, found)
        });
        let (mode, form, page, url) = match (rect, found) {
            (Some(rect), _) => (
                LinkMode::Create(rect),
                LinkForm::default(),
                (current + 1).to_string(),
                String::new(),
            ),
            (None, Some(Ok(Some(link)))) => {
                let page = match link.target {
                    onionskin_core::links::LinkTarget::Page(page) => page + 1,
                    _ => current + 1,
                };
                let url = match &link.target {
                    onionskin_core::links::LinkTarget::Web(url) => url.clone(),
                    _ => String::new(),
                };
                (
                    LinkMode::Edit {
                        link: link.objref,
                        page: link.page,
                    },
                    LinkForm::of(&link),
                    page.to_string(),
                    url,
                )
            }
            (None, Some(Err(error))) => {
                self.notices.push(sentence(&error.to_string()));
                return;
            }
            _ => return,
        };
        let editing = matches!(mode, LinkMode::Edit { .. });
        self.show_dialog(ShellDialog::Link { editing }, window, cx);
        let theme = self.shell_view_state.tokens();
        self.link_dialog = Some(LinkDialogState::new(
            mode,
            form,
            page_count,
            (page, url),
            theme,
            cx,
        ));
    }

    pub(in crate::shell) fn link_dialog_state(&self) -> Option<&LinkDialogState> {
        self.link_dialog.as_ref()
    }

    pub(super) fn run_link_action(
        &mut self,
        action: LinkAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.link_dialog.as_mut() else {
            return;
        };
        state.error = None;
        match action {
            LinkAction::Submit => match state.request(cx) {
                Ok((target, look)) => {
                    let mode = state.mode;
                    self.write_link(
                        move |doc| match mode {
                            LinkMode::Create(rect) => edit_links::create_link(
                                doc,
                                rect.page,
                                [rect.x0, rect.y0, rect.x1, rect.y1],
                                &target,
                                look,
                            )
                            .map(|_| ()),
                            LinkMode::Edit { link, .. } => {
                                edit_links::edit_link(doc, link, &target, look)
                            }
                        },
                        window,
                        cx,
                    );
                }
                Err(error) => state.error = Some(error),
            },
            LinkAction::Delete => {
                if let LinkMode::Edit { link, page } = state.mode {
                    self.write_link(
                        move |doc| edit_links::delete_link(doc, page, link),
                        window,
                        cx,
                    );
                }
            }
            LinkAction::ChooseFile => self.prompt_for_link_file(cx),
            _ => state.form.apply(action),
        }
        cx.notify();
    }

    /// Choose the file a link opens.
    fn prompt_for_link_file(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    frame.take_link_file(paths.into_iter().next());
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// What the file prompt chose, into the form.
    pub(super) fn take_link_file(&mut self, file: Option<std::path::PathBuf>) {
        if let Some(state) = self.link_dialog.as_mut() {
            state.form.file = file;
        }
    }

    /// Run a link edit, closing the dialog on success and keeping the error
    /// in it otherwise.
    fn write_link(
        &mut self,
        edit: impl FnOnce(
            &mut onionskin_core::Document,
        ) -> Result<(), onionskin_plugin_api::CommandError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let outcome = canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.edit_pages(edit);
            if outcome.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            outcome
        });
        match outcome {
            Ok(()) => self.close_dialog(window, cx),
            Err(error) => {
                if let Some(state) = self.link_dialog.as_mut() {
                    state.error = Some(sentence(&error.to_string()));
                }
            }
        }
    }

    /// Edit > Create Links from URLs, or Remove Web Links, on every page.
    pub(super) fn run_links_command(&mut self, remove: bool, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let mut count = 0;
        let outcome = canvas.update(cx, |canvas, cx| {
            let pages: Vec<usize> = (0..canvas.model.viewport().page_count()).collect();
            let outcome = canvas.model.edit_pages(|doc| {
                count = if remove {
                    edit_links::remove_web_links(doc)?
                } else {
                    edit_links::create_links_from_urls(doc, &pages)?
                };
                Ok(())
            });
            if outcome.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            outcome
        });
        self.notices.push(match outcome {
            Err(error) => sentence(&error.to_string()),
            Ok(()) => links_said(remove, count),
        });
        cx.notify();
    }

    /// The canvas context menu's Create Link: the selected text's own link,
    /// or, with nothing selected, the Link tool.
    pub(super) fn create_link_from_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(canvas) = self.active_canvas().cloned() else {
            return false;
        };
        let rect = canvas.update(cx, |canvas, _| {
            let document = canvas.model.document_mut();
            let quads = document.selection().text_quads();
            let page = quads.first()?.page;
            let mut rect: Option<PageRect> = None;
            for quad in quads.iter().filter(|quad| quad.page == page) {
                for (x, y) in quad.corners {
                    let grown = rect.get_or_insert(PageRect {
                        page,
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                    });
                    grown.x0 = grown.x0.min(x);
                    grown.y0 = grown.y0.min(y);
                    grown.x1 = grown.x1.max(x);
                    grown.y1 = grown.y1.max(y);
                }
            }
            rect
        });
        match rect {
            Some(rect) => {
                self.open_link_editor(Some(rect), None, window, cx);
                true
            }
            None => false,
        }
    }
}

/// What Create Links from URLs or Remove Web Links says it did.
fn links_said(remove: bool, count: usize) -> String {
    let links = match count {
        1 => "1 web link".to_owned(),
        count => format!("{count} web links"),
    };
    match (remove, count) {
        (true, 0) => "There are no web links to remove.".to_owned(),
        (false, 0) => "No web address without a link was found.".to_owned(),
        (true, _) => format!("Removed {links}."),
        (false, _) => format!("Created {links}."),
    }
}

#[cfg(test)]
mod tests {
    use super::links_said;

    #[test]
    fn the_notices_count_what_was_done() {
        assert_eq!(links_said(true, 0), "There are no web links to remove.");
        assert_eq!(links_said(true, 1), "Removed 1 web link.");
        assert_eq!(
            links_said(false, 0),
            "No web address without a link was found."
        );
        assert_eq!(links_said(false, 3), "Created 3 web links.");
    }
}
