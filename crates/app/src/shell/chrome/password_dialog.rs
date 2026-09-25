//! The password an encrypted document asks for as it opens.
//!
//! Acrobat's prompt: the document's name, a masked field, Open and Cancel.
//! A wrong password leaves the prompt up, saying so, and clears the field.
//! Either password opens the document: the user password under its
//! permissions, the permissions password with all of them.

use std::path::PathBuf;
use std::sync::Arc;

use accesskit::Role;
use gpui::{div, px, Context, Entity, ParentElement as _, Styled as _};

use super::accessible::{Activation, Element, TextField};
use super::combine_dialog::button;
use super::{SearchInput, ShellFrame, ThemeTokens};

/// The password field's id.
pub(in crate::shell) const PASSWORD_ID: &str = "document-password";

/// A protected document waiting for its password, retaining attachment bytes
/// across a parent-tab switch or retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) enum OpenTarget {
    File(PathBuf),
    Attachment { name: String, bytes: Arc<Vec<u8>> },
}

impl OpenTarget {
    pub(in crate::shell) fn display_name(&self) -> String {
        match self {
            Self::File(path) => path.to_string_lossy().into_owned(),
            Self::Attachment { name, .. } => name.clone(),
        }
    }
}

/// What a control in the prompt does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PasswordAction {
    Open,
    Cancel,
}

const BUTTONS: [(&str, &str, PasswordAction); 2] = [
    ("document-password-open", "Open", PasswordAction::Open),
    ("document-password-cancel", "Cancel", PasswordAction::Cancel),
];

/// The prompt's state in the frame.
pub(in crate::shell) struct PasswordPrompt {
    pub(in crate::shell) target: OpenTarget,
    pub(in crate::shell) input: Entity<SearchInput>,
    /// Whether the last password tried was wrong.
    pub(in crate::shell) wrong: bool,
}

impl PasswordPrompt {
    /// What the prompt says above the field.
    pub(in crate::shell) fn message(&self) -> String {
        message_for(&self.target.display_name())
    }
}

fn message_for(name: &str) -> String {
    let name = PathBuf::from(name).file_name().map_or_else(
        || name.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    format!("'{name}' is protected. Enter a password to open it.")
}

const WRONG: &str = "Incorrect password. Try again.";

pub(in crate::shell) fn accessible(prompt: &PasswordPrompt, cx: &gpui::App) -> Vec<Element> {
    let mut body = vec![
        Element::new("document-password-message", Role::Label, prompt.message()),
        prompt
            .input
            .read(cx)
            .accessible("Password", TextField::DocumentPassword),
    ];
    if prompt.wrong {
        body.push(Element::new("document-password-wrong", Role::Alert, WRONG));
    }
    for (id, label, action) in BUTTONS {
        body.push(
            Element::new(id, Role::Button, label).with_activation(Activation::Password(action)),
        );
    }
    body
}

pub(in crate::shell) fn render(
    prompt: &PasswordPrompt,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .w(px(380.0))
        .child(prompt.message())
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child("Password")
                .child(div().w(px(240.0)).child(prompt.input.clone())),
        );
    if prompt.wrong {
        body = body.child(div().text_color(theme.error_text).child(WRONG));
    }
    let mut buttons = div().flex().justify_end().gap_2();
    for (id, label, action) in BUTTONS {
        buttons = buttons.child(button(
            id,
            label,
            true,
            theme,
            focused,
            cx,
            Activation::Password(action),
        ));
    }
    body.child(buttons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_names_the_document() {
        assert_eq!(
            message_for("/a/b/plan.pdf"),
            "'plan.pdf' is protected. Enter a password to open it."
        );
        assert!(message_for("/").contains("'/'"));
    }
}
