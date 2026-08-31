//! The attachments pane: list an embedded file, and save one out.
//!
//! Save writes to the path the user chose in a dialog and to nowhere else.
//! The suggestion offered to that dialog is the attachment's last path
//! component, because `/UF` is a path and a file naming its attachment
//! `../../etc/passwd` must not be able to steer the dialog.
//!
//! Open is present and disabled. Handing a file the document carries to the
//! operating system to open is the attachment half of the trust surface
//! parity row 445 puts in M5 with `scripting`; shipping it before there is
//! anything to ask would be the one entry here that runs untrusted content.

use std::path::PathBuf;

use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::Attachment;

use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::{empty_message, error_message, list, NavigationPanesState, PaneAction};

/// The two per-row commands parity row 195 names at M2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum AttachmentCommand {
    Open,
    Save,
}

impl AttachmentCommand {
    pub(in crate::shell) const ALL: [Self; 2] = [Self::Open, Self::Save];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Save => "Save",
        }
    }

    pub(in crate::shell) fn availability(self) -> MenuAvailability {
        match self {
            Self::Save => MenuAvailability::Enabled,
            Self::Open => {
                MenuAvailability::Disabled("Available in M5 with the attachment trust list")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum AttachmentAction {
    Save(usize),
}

pub(super) fn run(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    directory: Option<PathBuf>,
    action: AttachmentAction,
    cx: &mut Context<ShellFrame>,
) {
    let AttachmentAction::Save(index) = action;
    let Some(canvas) = canvas.cloned() else {
        return;
    };
    let Some(suggested) = state.attachment_file_name(index) else {
        // The row was drawn from the snapshot, so an index the snapshot does
        // not have means the pane and the click disagree; say so rather than
        // saving whatever is at that position now.
        state.feedback = Some(format!("attachment {index} is no longer listed"));
        return;
    };
    let directory = directory.unwrap_or_else(|| PathBuf::from("."));
    let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));

    cx.spawn(async move |frame, cx| {
        let Ok(Ok(Some(path))) = chosen.await else {
            // A cancelled dialog is not a failure, and a dialog that could
            // not open is reported by the platform layer that opened it.
            return;
        };
        frame
            .update(cx, |frame, cx| {
                let bytes = canvas.update(cx, |canvas, _cx| canvas.model.attachment_bytes(index));
                let outcome = bytes
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| write(&path, &bytes));
                frame.report_pane_failure(outcome.err(), cx);
            })
            .ok();
    })
    .detach();
}

pub(super) fn render(
    items: Result<&[Attachment], &String>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let items = match items {
        Ok(items) => items,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    if items.is_empty() {
        return empty_message("This document has no attachments.", theme).into_any_element();
    }

    let mut body = list("attachment-rows");
    for (index, attachment) in items.iter().enumerate() {
        let mut row = div()
            .id(("attachment-row", index))
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .text_sm()
            .text_color(theme.text)
            .child(attachment.name.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_text)
                    .child(detail(attachment)),
            );
        let mut actions = div().flex().gap_2();
        for command in AttachmentCommand::ALL {
            let availability = command.availability();
            let enabled = availability.is_enabled();
            let mut button = div()
                .id((
                    "attachment-command",
                    index * AttachmentCommand::ALL.len() + command as usize,
                ))
                .px_2()
                .py(px(2.0))
                .rounded_sm()
                .text_xs()
                .bg(theme.surface)
                .text_color(if enabled {
                    theme.text
                } else {
                    theme.disabled_text
                })
                .child(command.label());
            match (enabled, availability.reason()) {
                (true, _) => {
                    button = button
                        .cursor_pointer()
                        .hover(move |button| button.bg(theme.hover))
                        .on_click(cx.listener(move |frame, _event, _window, cx| {
                            frame.run_pane_action(
                                PaneAction::Attachment(AttachmentAction::Save(index)),
                                cx,
                            );
                        }));
                }
                // The reason reads as its own muted line rather than as
                // more of the button's label.
                (false, Some(reason)) => {
                    button = button.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_text)
                            .child(format!("({reason})")),
                    );
                }
                (false, None) => {}
            }
            actions = actions.child(button);
        }
        row = row.child(actions);
        body = body.child(row);
    }
    body.into_any_element()
}

/// Write the extracted bytes to the path the user chose.
///
/// Its own function so the failure has a seam a test can reach: a write that
/// cannot happen has to name the file it was on, or the pane reports "cannot
/// write" about nothing in particular.
fn write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// The line under the name: what the file states about the attachment, and
/// nothing it does not. A missing size is left out rather than shown as
/// zero, which would be a different claim.
fn detail(attachment: &Attachment) -> String {
    let mut parts = Vec::new();
    if let Some(mime) = attachment.mime.as_ref() {
        parts.push(mime.clone());
    }
    if let Some(size) = attachment.size {
        parts.push(format!("{size} bytes"));
    }
    if let Some(description) = attachment
        .description
        .as_ref()
        .filter(|description| !description.is_empty())
    {
        parts.push(description.clone());
    }
    if parts.is_empty() {
        "The document states nothing about this file".to_owned()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(name: &str, size: Option<u64>, mime: Option<&str>) -> Attachment {
        Attachment {
            name: name.to_owned(),
            description: None,
            size,
            mime: mime.map(str::to_owned),
            stream: 7,
        }
    }

    /// Save is live at M2, per parity row 195. Open waits on the trust list
    /// and says which milestone brings it, because opening an embedded file
    /// hands document content to the operating system.
    #[test]
    fn save_is_live_and_open_is_disabled_naming_the_milestone() {
        assert!(AttachmentCommand::Save.availability().is_enabled());

        let open = AttachmentCommand::Open.availability();
        assert!(!open.is_enabled());
        let reason = open.reason().expect("a disabled command says why");
        assert!(reason.contains("M5"), "said {reason:?}");
    }

    /// The dialog is offered the last path component, so an attachment named
    /// with a traversal cannot suggest a destination outside the folder the
    /// user is looking at.
    #[test]
    fn a_traversing_attachment_name_suggests_only_its_last_component() {
        assert_eq!(
            attachment("../../etc/passwd", None, None).file_name(),
            "passwd"
        );
    }

    /// A write that cannot happen names the file it was on. The pane shows
    /// this sentence and nothing else, so a message that named no file would
    /// leave the user with nowhere to look.
    #[test]
    fn a_write_that_cannot_happen_names_the_file_it_was_on() {
        let missing = std::path::Path::new("/nonexistent-directory/attachment.csv");

        let failure = write(missing, b"payload").expect_err("the directory is not there");

        assert!(failure.contains("attachment.csv"), "said {failure:?}");
        assert!(failure.starts_with("cannot write"), "said {failure:?}");
    }

    #[test]
    fn a_write_that_can_happen_leaves_the_bytes_on_disk() {
        let path = std::env::temp_dir().join("onionskin-p8-attachment.bin");
        let _ = std::fs::remove_file(&path);

        write(&path, b"payload").expect("the temporary directory is writable");

        assert_eq!(std::fs::read(&path).expect("the file is there"), b"payload");
        std::fs::remove_file(&path).expect("the fixture cleans up after itself");
    }

    /// A stated size is shown; an absent one is left out. Showing "0 bytes"
    /// for a file that stated no size would be a claim the file never made.
    #[test]
    fn the_detail_line_states_only_what_the_file_states() {
        assert_eq!(
            detail(&attachment("a.csv", Some(33), Some("text/csv"))),
            "text/csv · 33 bytes"
        );
        assert_eq!(
            detail(&attachment("a.bin", None, Some("text/csv"))),
            "text/csv"
        );
        assert_eq!(
            detail(&attachment("a.bin", None, None)),
            "The document states nothing about this file"
        );
    }
}
