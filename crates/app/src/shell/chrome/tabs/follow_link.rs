//! Following a link, and the Trust Manager's say over the ones that leave
//! the document.
//!
//! A tool raises a link request (the Hand tool's click, the Link tool's
//! drag or click); the canvas's notification brings the frame here to take
//! it, and the next render, which has the window a dialog needs, runs it.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use onionskin_core::links::{link_at, LinkTarget};
use onionskin_core::LinkRequest;

use super::ShellFrame;
use crate::shell::canvas::ViewAction;
use crate::shell::chrome::web_link_dialog::{
    decide, host, Decision, WebLinkAction, WebLinkPrompt, BLOCKED,
};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    /// Take the active canvas's link request, for the next render to run.
    pub(super) fn collect_link_request(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let request = canvas.update(cx, |canvas, _| {
            canvas.model.document_mut().take_link_request()
        });
        if let Some(request) = request {
            self.pending_link = Some(request);
            cx.notify();
        }
    }

    /// Run a link request taken earlier. Called from render, which has the
    /// window a dialog needs.
    pub(super) fn run_pending_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        let Some(request) = self.pending_link.take() else {
            return;
        };
        match request {
            LinkRequest::Follow(at) => self.follow_link_at(at, window, cx),
            LinkRequest::Create(rect) => self.open_link_editor(Some(rect), None, window, cx),
            LinkRequest::Edit(link) => self.open_link_editor(None, Some(link), window, cx),
        }
    }

    /// Go where the link under `at` goes, if there is one.
    fn follow_link_at(
        &mut self,
        at: onionskin_core::PagePoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let links = canvas.update(cx, |canvas, _| canvas.model.document_mut().links());
        let target = match links {
            Ok(links) => link_at(&links, at.page, (at.x, at.y)).map(|link| link.target.clone()),
            Err(error) => {
                self.notices
                    .push(format!("The link could not be read: {error}"));
                None
            }
        };
        if let Some(target) = target {
            self.follow(target, window, cx);
        }
        cx.notify();
    }

    /// Go to `target`.
    pub(in crate::shell) fn follow(
        &mut self,
        target: LinkTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            LinkTarget::Page(page) => self.run_view_action(ViewAction::GoToPage(page), cx),
            LinkTarget::Web(url) => {
                let site = host(&url);
                match decide(&self.settings.preferences, &site) {
                    Decision::Open => self.open_web_link(&url, cx),
                    Decision::Block => self.notices.push(BLOCKED.to_owned()),
                    Decision::Ask => {
                        self.show_dialog(ShellDialog::WebLink, window, cx);
                        self.web_link = Some(WebLinkPrompt { url, host: site });
                    }
                }
            }
            LinkTarget::File(file) => self.open_linked_file(&file, cx),
            LinkTarget::Other(kind) => self.notices.push(format!(
                "This link runs a {kind} action, which Onionskin does not run."
            )),
        }
    }

    fn open_web_link(&mut self, url: &str, cx: &mut Context<Self>) {
        cx.open_url(url);
        self.notices.push(format!("Opened {url}"));
    }

    /// A linked file opens here if it is a PDF beside this one, or at the
    /// path it names; Onionskin opens nothing else.
    fn open_linked_file(&mut self, file: &str, cx: &mut Context<Self>) {
        let folder = if Path::new(file).is_absolute() {
            PathBuf::new()
        } else if let Some(folder) = self
            .tabs
            .active()
            .and_then(|tab| tab.path(cx))
            .and_then(|path| path.parent().map(Path::to_path_buf))
        {
            folder
        } else {
            self.notices
                .push("Save this document before following a relative link".to_owned());
            cx.notify();
            return;
        };
        let path = linked_path(&folder, file);
        let is_pdf = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
        if !is_pdf {
            self.notices.push(format!(
                "This link opens {file}; Onionskin opens PDF files only."
            ));
        } else if !path.exists() {
            self.notices.push(format!(
                "This link opens {}, which is not there.",
                path.display()
            ));
        } else {
            self.open_documents(&[path], cx);
        }
    }

    pub(in crate::shell) fn web_link_prompt(&self) -> Option<&WebLinkPrompt> {
        self.web_link.as_ref()
    }

    pub(super) fn run_web_link_action(
        &mut self,
        action: WebLinkAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self.web_link.clone() else {
            return;
        };
        self.close_dialog(window, cx);
        match action {
            WebLinkAction::Open => self.open_web_link(&prompt.url, cx),
            WebLinkAction::AlwaysAllow => {
                self.settings.preferences.trusted_sites.insert(prompt.host);
                if let Some(path) = self.settings.paths.preferences.as_deref() {
                    if let Err(error) = self.settings.preferences.save(path) {
                        self.notices.push(error.to_string());
                    }
                }
                self.open_web_link(&prompt.url, cx);
            }
            WebLinkAction::Cancel => {}
        }
        cx.notify();
    }

    /// Open the Link dialog: on a new rectangle, or on an existing link.
    #[cfg(not(feature = "tools-edit"))]
    fn open_link_editor(
        &mut self,
        _: Option<onionskin_core::PageRect>,
        _: Option<onionskin_core::ObjRef>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.notices.push(
            crate::shell::chrome::crop_dialog::crop_refusal(None)
                .unwrap_or_default()
                .to_owned(),
        );
    }
}

/// `file` as a path: as written when absolute, else beside the document.
fn linked_path(folder: &Path, file: &str) -> PathBuf {
    let path = PathBuf::from(file);
    if path.is_absolute() {
        path
    } else {
        folder.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_link_is_beside_the_document() {
        assert_eq!(
            linked_path(Path::new("/cases"), "exhibit.pdf"),
            PathBuf::from("/cases/exhibit.pdf")
        );
        assert_eq!(
            linked_path(Path::new("/cases"), "/other/a.pdf"),
            PathBuf::from("/other/a.pdf")
        );
    }
}
