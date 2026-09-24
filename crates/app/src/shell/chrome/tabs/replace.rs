//! The find bar's Replace and Replace All: the frame's half.
//!
//! The text looked for is the find bar's query, with its Match Case and
//! Whole Word; what replaces it is the Replace With field. Replace takes
//! the match the find bar has on screen, or the first one when none is;
//! Replace All takes every one, as one undo step. The find runs again
//! afterwards, so what is highlighted is what is left.

use gpui::{App, Context};

use super::ShellFrame;

/// What the Replace buttons say in a build that cannot edit text.
pub(in crate::shell) const NO_TEXT_EDIT: &str = "No installed plugin edits text";

impl ShellFrame {
    /// Why Replace is off, or `None` when it is live.
    pub(in crate::shell) fn replace_refusal(&self, cx: &App) -> Option<&'static str> {
        if !cfg!(feature = "tools-edit") {
            return Some(NO_TEXT_EDIT);
        }
        self.active_canvas()?.read(cx).model.edit_refusal()
    }

    pub(super) fn replace_text(&mut self, all: bool, cx: &mut Context<Self>) {
        let notice = self.replace_in_active(all, cx);
        self.notices.push(notice);
        self.run_find_query(cx);
        cx.notify();
    }

    /// Replace on the active document, and say how it went.
    fn replace_in_active(&mut self, all: bool, cx: &mut Context<Self>) -> String {
        if let Some(refusal) = self.replace_refusal(cx) {
            return format!("Nothing was replaced: {refusal}");
        }
        let needle = self.find_input.read(cx).query().to_owned();
        if needle.trim().is_empty() {
            return "Type the text to find, then what replaces it".to_owned();
        }
        let replacement = self.replace_input.read(cx).query().to_owned();
        let options = self.find.options();
        let Some(canvas) = self.active_canvas().cloned() else {
            return "No document is open".to_owned();
        };
        let on_screen = {
            let model = &canvas.read(cx).model;
            let search = model.search();
            search.current().and_then(|hit| {
                let corners = hit.quads.first()?.corners;
                let x = corners.iter().map(|corner| corner.0).sum::<f64>() / 4.0;
                let y = corners.iter().map(|corner| corner.1).sum::<f64>() / 4.0;
                Some((hit.page, (x, y)))
            })
        };
        let request = text_edits::Request {
            needle,
            replacement,
            case_sensitive: options.case_sensitive,
            whole_word: options.whole_word,
            all,
            on_screen,
        };
        canvas.update(cx, |canvas, cx| {
            let mut replaced = 0;
            let outcome = canvas.model.edit_pages(|doc| {
                replaced = text_edits::replace(doc, &request)?;
                Ok(())
            });
            let notice = match &outcome {
                Ok(()) => text_edits::count(replaced),
                Err(error) => super::properties::sentence(&error.to_string()),
            };
            if outcome.is_ok() && replaced > 0 {
                canvas.handle_change(Ok(true), cx);
            }
            notice
        })
    }
}

/// Replacing, through `tools-edit` when it is built in.
mod text_edits {
    use onionskin_core::Document;
    use onionskin_plugin_api::CommandError;

    #[cfg_attr(not(feature = "tools-edit"), allow(dead_code))]
    pub(super) struct Request {
        pub(super) needle: String,
        pub(super) replacement: String,
        pub(super) case_sensitive: bool,
        pub(super) whole_word: bool,
        pub(super) all: bool,
        /// The find bar's current match: its page and a point in it.
        pub(super) on_screen: Option<(usize, (f64, f64))>,
    }

    pub(super) fn count(replaced: usize) -> String {
        match replaced {
            0 => "No match to replace".to_owned(),
            1 => "Replaced 1 match".to_owned(),
            count => format!("Replaced {count} matches"),
        }
    }

    #[cfg(feature = "tools-edit")]
    pub(super) fn replace(doc: &mut Document, request: &Request) -> Result<usize, CommandError> {
        use onionskin_core::text_edit::MatchOptions;
        use onionskin_tools_edit::text;

        let options = MatchOptions {
            case_sensitive: request.case_sensitive,
            whole_word: request.whole_word,
        };
        let found = text::find(doc, &request.needle, options)?;
        if request.all {
            return text::replace(doc, &found, &request.replacement, "Replace All");
        }
        let chosen = request
            .on_screen
            .and_then(|(page, at)| text::match_at(doc, &found, page, at))
            .or_else(|| found.first().cloned());
        match chosen {
            Some(chosen) => text::replace(doc, &[chosen], &request.replacement, "Replace"),
            None => Ok(0),
        }
    }

    #[cfg(not(feature = "tools-edit"))]
    pub(super) fn replace(_: &mut Document, _: &Request) -> Result<usize, CommandError> {
        Err(CommandError::Failed {
            label: "Replace",
            reason: super::NO_TEXT_EDIT.to_owned(),
        })
    }
}
