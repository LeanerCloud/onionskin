//! The attachments pane: list an embedded file, save one out, add one and
//! delete one.
//!
//! Add asks for a file, which is the frame's; Delete rewrites the
//! document's attachment tree through `core::embedded`, leaving the file's
//! bytes in the document unreferenced, as every delete here does.
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

use accesskit::Role;
use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::Attachment;

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::{empty_message, error_message, list, NavigationPanesState, PaneAction};
use crate::a11y::State as A11yState;

/// Said where the list would be when the document embeds no files.
const NO_ATTACHMENTS: &str = "This document has no attachments.";

/// The per-row commands: parity row 195's two at M2, and row 26's Delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum AttachmentCommand {
    Open,
    Save,
    EditDescription,
    Delete,
}

impl AttachmentCommand {
    pub(in crate::shell) const ALL: [Self; 4] =
        [Self::Open, Self::Save, Self::EditDescription, Self::Delete];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Save => "Save",
            Self::EditDescription => "Edit Description…",
            Self::Delete => "Delete",
        }
    }

    /// Delete and Edit Description change the document, so a document that
    /// may not be edited disables them with its reason.
    pub(in crate::shell) fn availability(self, refusal: Option<&'static str>) -> MenuAvailability {
        match (self, refusal) {
            (Self::Open | Self::Save, _) | (Self::Delete | Self::EditDescription, None) => {
                MenuAvailability::Enabled
            }
            (Self::Delete | Self::EditDescription, Some(reason)) => {
                MenuAvailability::Disabled(reason)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) enum AttachmentAction {
    /// Open a PDF attachment in a tab of its own. The frame's, which opens
    /// documents.
    Open(usize),
    Save(usize),
    /// Edit Description, a dialog and so the frame's.
    EditDescription(usize),
    Delete(usize),
    /// Search Attachments: Advanced Search over the PDF attachments.
    Search,
    /// Ask for a file and attach it to the document. The frame's, because it
    /// opens a file dialog.
    Add,
    /// Open the pane's menu at `at`, on the row at `row`, or on none.
    OpenMenu {
        row: Option<usize>,
        at: gpui::Point<gpui::Pixels>,
    },
}

/// What Add is called, in the pane and in its menu.
pub(in crate::shell) const ADD_LABEL: &str = "Add Attachment…";

/// What Add says when the document may not be edited, or nothing.
pub(in crate::shell) fn add_availability(refusal: Option<&'static str>) -> MenuAvailability {
    match refusal {
        Some(reason) => MenuAvailability::Disabled(reason),
        None => MenuAvailability::Enabled,
    }
}

/// The element id one row's command button renders with, unique across the
/// pane so no two buttons are given the same identity.
fn command_id(index: usize, command: AttachmentCommand) -> (&'static str, usize) {
    (
        "attachment-command",
        index * AttachmentCommand::ALL.len() + command as usize,
    )
}

/// What a row's command button runs. Open is listed and does nothing until
/// M5 brings the trust list, so it has nothing to run.
pub(super) fn command_activation(index: usize, command: AttachmentCommand) -> Option<Activation> {
    let action = match command {
        AttachmentCommand::Save => AttachmentAction::Save(index),
        AttachmentCommand::Delete => AttachmentAction::Delete(index),
        AttachmentCommand::Open => AttachmentAction::Open(index),
        AttachmentCommand::EditDescription => AttachmentAction::EditDescription(index),
    };
    Some(Activation::Pane(PaneAction::Attachment(action)))
}

fn add_element(refusal: Option<&'static str>) -> Element {
    let availability = add_availability(refusal);
    let add = Element::new("attachment-add", Role::Button, ADD_LABEL)
        .with_state(A11yState::enabled(availability.is_enabled()))
        .with_activation(Activation::Pane(PaneAction::Attachment(
            AttachmentAction::Add,
        )));
    match availability.reason() {
        Some(reason) => add.with_description(reason),
        None => add,
    }
}

/// What the attachments pane tells a screen reader.
pub(super) fn accessible(
    items: Result<&[Attachment], &String>,
    refusal: Option<&'static str>,
) -> Vec<Element> {
    let items = match items {
        Ok(items) => items,
        Err(message) => {
            return vec![Element::new(
                "attachment-rows-error",
                Role::Alert,
                message.clone(),
            )]
        }
    };
    if items.is_empty() {
        return vec![
            add_element(refusal),
            Element::new("attachment-rows-empty", Role::Label, NO_ATTACHMENTS),
        ];
    }

    vec![
        add_element(refusal),
        Element::new("attachment-rows", Role::List, "Attachments").with_children(
            items
                .iter()
                .enumerate()
                .map(|(index, attachment)| {
                    Element::new(
                        ("attachment-row", index),
                        Role::ListItem,
                        attachment.name.clone(),
                    )
                    .with_description(detail(attachment))
                    .with_children(
                        AttachmentCommand::ALL
                            .into_iter()
                            .map(|command| {
                                let availability = command.availability(refusal);
                                let mut button = Element::new(
                                    command_id(index, command),
                                    Role::Button,
                                    command.label(),
                                )
                                .with_state(A11yState::enabled(availability.is_enabled()));
                                if let Some(activation) = command_activation(index, command) {
                                    button = button.with_activation(activation);
                                }
                                match availability.reason() {
                                    Some(reason) => button.with_description(reason),
                                    None => button,
                                }
                            })
                            .collect(),
                    )
                })
                .collect(),
        ),
    ]
}

pub(super) fn run(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    directory: Option<PathBuf>,
    action: AttachmentAction,
    cx: &mut Context<ShellFrame>,
) {
    if !matches!(action, AttachmentAction::OpenMenu { .. }) {
        state.attachments_menu = None;
    }
    let index = match action {
        AttachmentAction::Save(index) => index,
        AttachmentAction::Delete(index) => {
            delete(state, canvas, index, cx);
            return;
        }
        AttachmentAction::OpenMenu { row, at } => {
            state.attachments_menu = Some(super::attachment_menu::AttachmentsMenu { row, at });
            return;
        }
        // Run by the frame, which asks for the file, opens documents and
        // dialogs.
        AttachmentAction::Add
        | AttachmentAction::Open(_)
        | AttachmentAction::EditDescription(_)
        | AttachmentAction::Search => return,
    };
    let Some(canvas) = canvas.cloned() else {
        return;
    };
    let origin = canvas.entity_id();
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
        let path = match chosen.await {
            Ok(result) => match attachment_destination(result.map_err(|error| error.to_string())) {
                Ok(Some(path)) => path,
                Ok(None) => return,
                Err(error) => {
                    frame
                        .update(cx, |frame, cx| {
                            if frame.is_active_canvas(origin) {
                                frame.report_pane_failure(Some(error), cx);
                            }
                        })
                        .ok();
                    return;
                }
            },
            Err(_) => return,
        };
        frame
            .update(cx, |frame, cx| {
                if !frame.is_active_canvas(origin) {
                    return;
                }
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

fn attachment_destination(
    result: Result<Option<PathBuf>, String>,
) -> Result<Option<PathBuf>, String> {
    result.map_err(|error| format!("no destination could be chosen: {error}"))
}

/// Delete the listed attachment at `index`, by the stream the list read.
fn delete(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    index: usize,
    cx: &mut Context<ShellFrame>,
) {
    let Some(canvas) = canvas else {
        return;
    };
    let Some(stream) = state.attachment_stream(index) else {
        state.feedback = Some(format!("attachment {index} is no longer listed"));
        return;
    };
    super::document_edit(state, canvas, cx, "Delete Attachment", |_, tx| {
        onionskin_core::embedded::remove_attachment(tx, stream)
    });
}

/// The pane's own Add button, above the list and there when it is empty.
fn add_button(
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let availability = add_availability(refusal);
    let enabled = availability.is_enabled();
    let button = div()
        .id("attachment-add")
        .mx_2()
        .my_1()
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
        .child(ADD_LABEL);
    if enabled {
        button
            .cursor_pointer()
            .hover(move |button| button.bg(theme.hover))
            .on_click(cx.listener(|frame, _event, window, cx| {
                frame.run_activation(
                    Activation::Pane(PaneAction::Attachment(AttachmentAction::Add)),
                    window,
                    cx,
                );
            }))
    } else {
        button
    }
}

pub(super) fn render(
    items: Result<&[Attachment], &String>,
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let items = match items {
        Ok(items) => items,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    let add = add_button(refusal, theme, cx);
    if items.is_empty() {
        return div()
            .flex()
            .flex_col()
            .child(add)
            .child(empty_message(NO_ATTACHMENTS, theme))
            .into_any_element();
    }

    let mut body = list("attachment-rows")
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|frame, event: &gpui::MouseDownEvent, _window, cx| {
                frame.run_pane_action(
                    PaneAction::Attachment(AttachmentAction::OpenMenu {
                        row: None,
                        at: event.position,
                    }),
                    cx,
                );
            }),
        )
        .child(add);
    for (index, attachment) in items.iter().enumerate() {
        let mut row = div()
            .id(("attachment-row", index))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |frame, event: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    frame.run_pane_action(
                        PaneAction::Attachment(AttachmentAction::OpenMenu {
                            row: Some(index),
                            at: event.position,
                        }),
                        cx,
                    );
                }),
            )
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
        // Wraps, so a disabled button's reason cannot push the others out of
        // a narrow pane.
        let mut actions = div().flex().flex_wrap().gap_2();
        for command in AttachmentCommand::ALL {
            let availability = command.availability(refusal);
            let enabled = availability.is_enabled();
            let mut button = div()
                .id(command_id(index, command))
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
                    let activation = command_activation(index, command)
                        .expect("an enabled command has something to run");
                    button = button
                        .cursor_pointer()
                        .hover(move |button| button.bg(theme.hover))
                        .on_click(cx.listener(move |frame, _event, window, cx| {
                            frame.run_activation(activation.clone(), window, cx);
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
            page: None,
        }
    }

    /// Save is live at M2, per parity row 195. Open waits on the trust list
    /// and says which milestone brings it, because opening an embedded file
    /// hands document content to the operating system.
    #[test]
    fn open_and_save_are_live_and_the_edits_follow_the_documents_permissions() {
        assert!(AttachmentCommand::Save.availability(None).is_enabled());
        assert!(AttachmentCommand::Delete.availability(None).is_enabled());
        assert_eq!(
            AttachmentCommand::Delete
                .availability(Some("Encrypted"))
                .reason(),
            Some("Encrypted"),
            "a document that may not be edited cannot lose an attachment"
        );

        assert!(AttachmentCommand::Open
            .availability(Some("Encrypted"))
            .is_enabled());
        assert_eq!(
            AttachmentCommand::EditDescription
                .availability(Some("Encrypted"))
                .reason(),
            Some("Encrypted")
        );
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

    #[test]
    fn cancelling_a_destination_prompt_remains_silent() {
        assert_eq!(attachment_destination(Ok(None)), Ok(None));
    }

    #[test]
    fn a_destination_prompt_failure_becomes_pane_feedback() {
        let failure = attachment_destination(Err("dialog unavailable".to_owned()))
            .expect_err("a failed prompt is not cancellation");

        assert_eq!(
            failure,
            "no destination could be chosen: dialog unavailable"
        );
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

    /// One described row per drawn row, each carrying the name and the line
    /// the row draws under it, so a reader hears what the file states about
    /// the attachment rather than only its name.
    #[test]
    fn the_described_rows_are_the_drawn_rows_with_the_detail_line() {
        let items = [
            attachment("data.csv", Some(33), Some("text/csv")),
            attachment("notes.bin", None, None),
        ];

        let described = accessible(Ok(&items), None);
        assert_eq!(described[0].key, gpui::ElementId::from("attachment-add"));
        let rows = &described[1].children;

        assert_eq!(described.len(), 2);
        assert_eq!(described[1].role, Role::List);
        assert_eq!(rows.len(), items.len());
        for (index, (row, item)) in rows.iter().zip(items.iter()).enumerate() {
            assert_eq!(row.key, gpui::ElementId::from(("attachment-row", index)));
            assert_eq!(row.label, item.name);
            assert_eq!(row.description.as_deref(), Some(detail(item).as_str()));
        }
    }

    /// Save runs on the row it was drawn beside, and Open is announced as off
    /// with the reason it waits on M5 rather than being left out of the tree.
    #[test]
    fn each_rows_buttons_are_announced_with_the_action_the_row_runs() {
        let items = [
            attachment("a.csv", None, None),
            attachment("b.csv", None, None),
        ];

        let described = accessible(Ok(&items), None);
        let rows = &described[1].children;

        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.children.len(), AttachmentCommand::ALL.len());
            for (button, command) in row.children.iter().zip(AttachmentCommand::ALL) {
                assert_eq!(button.role, Role::Button);
                assert_eq!(button.label, command.label());
                assert_eq!(
                    button.key,
                    gpui::ElementId::from(command_id(index, command))
                );
                assert_eq!(
                    button.state.disabled,
                    !command.availability(None).is_enabled()
                );
                assert_eq!(
                    button.description.as_deref(),
                    command.availability(None).reason()
                );
            }
        }

        let second_save = rows[1]
            .children
            .iter()
            .find(|button| button.label == AttachmentCommand::Save.label())
            .expect("every row offers Save");
        assert_eq!(
            second_save.activation,
            Some(Activation::Pane(PaneAction::Attachment(
                AttachmentAction::Save(1)
            ))),
            "the button saves the attachment it was drawn beside"
        );

        let open = rows[0]
            .children
            .iter()
            .find(|button| button.label == AttachmentCommand::Open.label())
            .expect("every row lists Open");
        assert!(!open.state.disabled);
        assert_eq!(
            open.activation,
            Some(Activation::Pane(PaneAction::Attachment(
                AttachmentAction::Open(0)
            ))),
            "Open opens the attachment it was drawn beside"
        );
    }

    /// A document with no attachments says so, and a reader that failed says
    /// what went wrong rather than reading as a document with none.
    #[test]
    fn an_empty_list_and_a_failed_read_are_announced_differently() {
        let empty = accessible(Ok(&[]), None);
        assert_eq!(
            empty[0].label, ADD_LABEL,
            "Add is there with nothing to list"
        );
        assert_eq!(empty[1].role, Role::Label);
        assert_eq!(empty[1].label, NO_ATTACHMENTS);

        let failure = "the embedded file tree could not be decoded".to_owned();
        let broken = accessible(Err(&failure), None);
        assert_eq!(broken[0].role, Role::Alert);
        assert_eq!(broken[0].label, failure);
    }
}
