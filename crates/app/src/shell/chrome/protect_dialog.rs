//! Protect Using Password: Acrobat's Password Security settings.
//!
//! Two halves, each turned on by its check box. **Document Open** asks for a
//! password before the document opens. **Permissions** restricts printing,
//! changes and copying behind a permissions password: printing none, at low
//! resolution or at high; changes none, page assembly, form filling, form
//! filling and commenting, or anything but extracting pages; copying; and
//! text access for screen readers. Compatibility picks 256-bit AES (Acrobat
//! X and later) or 128-bit AES (Acrobat 7 and later).
//!
//! Apply saves the document with that security, which is how Acrobat's
//! Save applies it, and the session goes on from the saved file.

use accesskit::Role;
use gpui::{
    div, prelude::FluentBuilder as _, px, Context, Entity, InteractiveElement as _,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::security::{Permissions, Protection, Strength};

use super::accessible::{Activation, Element, TextField};
use super::combine_dialog::button;
use super::{SearchInput, ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// Printing Allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Printing {
    None,
    Low,
    High,
}

impl Printing {
    pub(in crate::shell) const ALL: [Self; 3] = [Self::None, Self::Low, Self::High];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low Resolution (150 dpi)",
            Self::High => "High Resolution",
        }
    }

    fn bits(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Low => Permissions::PRINT,
            Self::High => Permissions::PRINT | Permissions::PRINT_HIGH,
        }
    }
}

/// Changes Allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Changes {
    None,
    Pages,
    Forms,
    Comments,
    Any,
}

impl Changes {
    pub(in crate::shell) const ALL: [Self; 5] = [
        Self::None,
        Self::Pages,
        Self::Forms,
        Self::Comments,
        Self::Any,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Pages => "Inserting, deleting, and rotating pages",
            Self::Forms => "Filling in form fields, and signing existing signature fields",
            Self::Comments => "Commenting, filling in form fields, and signing",
            Self::Any => "Any except extracting pages",
        }
    }

    fn bits(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Pages => Permissions::ASSEMBLE,
            Self::Forms => Permissions::FILL_FORMS,
            Self::Comments => Permissions::ANNOTATE | Permissions::FILL_FORMS,
            Self::Any => {
                Permissions::MODIFY
                    | Permissions::ANNOTATE
                    | Permissions::FILL_FORMS
                    | Permissions::ASSEMBLE
            }
        }
    }
}

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ProtectAction {
    RequireOpen,
    Restrict,
    Strength(Strength),
    Printing(Printing),
    Changes(Changes),
    Copying,
    ScreenReaders,
    Apply,
}

/// The two password fields' ids.
pub(in crate::shell) const OPEN_ID: &str = "protect-open-password";
pub(in crate::shell) const PERMISSIONS_ID: &str = "protect-permissions-password";

/// The permissions the choices make.
pub(in crate::shell) fn permissions(
    printing: Printing,
    changes: Changes,
    copying: bool,
    screen_readers: bool,
) -> Permissions {
    let mut bits = printing.bits() | changes.bits();
    if copying {
        bits |= Permissions::EXTRACT | Permissions::ACCESSIBILITY;
    }
    if screen_readers {
        bits |= Permissions::ACCESSIBILITY;
    }
    Permissions(bits)
}

/// The settings as the dialog holds them, apart from its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct Choices {
    pub(in crate::shell) require_open: bool,
    pub(in crate::shell) restrict: bool,
    pub(in crate::shell) strength: Strength,
    pub(in crate::shell) printing: Printing,
    pub(in crate::shell) changes: Changes,
    pub(in crate::shell) copying: bool,
    pub(in crate::shell) screen_readers: bool,
}

impl Default for Choices {
    /// Acrobat's: nothing asked for yet, and when permissions are
    /// restricted, high-resolution printing, no changes, no copying, and
    /// screen readers let in.
    fn default() -> Self {
        Choices {
            require_open: false,
            restrict: false,
            strength: Strength::Aes256,
            printing: Printing::High,
            changes: Changes::None,
            copying: false,
            screen_readers: true,
        }
    }
}

impl Choices {
    pub(in crate::shell) fn apply(&mut self, action: ProtectAction) {
        match action {
            ProtectAction::RequireOpen => self.require_open = !self.require_open,
            ProtectAction::Restrict => self.restrict = !self.restrict,
            ProtectAction::Strength(strength) => self.strength = strength,
            ProtectAction::Printing(printing) => self.printing = printing,
            ProtectAction::Changes(changes) => self.changes = changes,
            ProtectAction::Copying => self.copying = !self.copying,
            ProtectAction::ScreenReaders => self.screen_readers = !self.screen_readers,
            ProtectAction::Apply => {}
        }
    }

    /// The protection these choices and the two passwords make, or why they
    /// make none.
    pub(in crate::shell) fn protection(
        &self,
        open: &str,
        permissions_password: &str,
    ) -> Result<Protection, &'static str> {
        if !self.require_open && !self.restrict {
            return Err("Choose a password to open the document, or to restrict it, or both");
        }
        if self.require_open && open.is_empty() {
            return Err("Type the password that opens the document");
        }
        if self.restrict && permissions_password.is_empty() {
            return Err("Type the permissions password");
        }
        if self.require_open && self.restrict && open == permissions_password {
            return Err("The two passwords must differ");
        }
        let (user, owner, allowed) = match (self.require_open, self.restrict) {
            (true, false) => (open, open, Permissions::ALL),
            (require_open, _) => (
                if require_open { open } else { "" },
                permissions_password,
                permissions(
                    self.printing,
                    self.changes,
                    self.copying,
                    self.screen_readers,
                ),
            ),
        };
        Ok(Protection {
            strength: self.strength,
            user_password: user.as_bytes().to_vec(),
            owner_password: owner.as_bytes().to_vec(),
            permissions: allowed,
            encrypt_metadata: true,
        })
    }
}

/// The dialog's state in the frame.
pub(in crate::shell) struct ProtectState {
    pub(in crate::shell) choices: Choices,
    pub(in crate::shell) open: Entity<SearchInput>,
    pub(in crate::shell) permissions: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
}

impl ProtectState {
    pub(in crate::shell) fn text_field(&self, field: TextField) -> Option<&Entity<SearchInput>> {
        match field {
            TextField::OpenPassword => Some(&self.open),
            TextField::PermissionsPassword => Some(&self.permissions),
            _ => None,
        }
    }
}

/// Its password fields, for the focus ring.
pub(in crate::shell) const TEXT_FIELDS: [TextField; 2] =
    [TextField::OpenPassword, TextField::PermissionsPassword];

const STRENGTHS: [(Strength, &str); 2] = [
    (Strength::Aes256, "Acrobat X and later (256-bit AES)"),
    (Strength::Aes128, "Acrobat 7 and later (128-bit AES)"),
];

fn check_box(id: &'static str, label: &'static str, on: bool, action: ProtectAction) -> Element {
    Element::new(id, Role::CheckBox, label)
        .with_state(A11yState::toggled(on))
        .with_activation(Activation::Protect(action))
}

fn radios<T: Copy + PartialEq>(
    id: &'static str,
    label: &'static str,
    choices: &[(T, &'static str)],
    in_force: T,
    action: impl Fn(T) -> ProtectAction,
) -> Element {
    let rows = choices
        .iter()
        .enumerate()
        .map(|(index, (choice, text))| {
            Element::new((id, index), Role::RadioButton, *text)
                .with_state(A11yState::selected(*choice == in_force))
                .with_activation(Activation::Protect(action(*choice)))
        })
        .collect();
    Element::new(id, Role::RadioGroup, label).with_children(rows)
}

fn printing_choices() -> Vec<(Printing, &'static str)> {
    Printing::ALL
        .map(|choice| (choice, choice.label()))
        .to_vec()
}

fn changes_choices() -> Vec<(Changes, &'static str)> {
    Changes::ALL.map(|choice| (choice, choice.label())).to_vec()
}

pub(in crate::shell) fn accessible(state: &ProtectState, cx: &gpui::App) -> Vec<Element> {
    let choices = state.choices;
    let mut body = vec![radios(
        "protect-compatibility",
        "Compatibility",
        &STRENGTHS,
        choices.strength,
        ProtectAction::Strength,
    )];
    body.push(check_box(
        "protect-require-open",
        "Require a password to open the document",
        choices.require_open,
        ProtectAction::RequireOpen,
    ));
    if choices.require_open {
        body.push(
            state
                .open
                .read(cx)
                .accessible("Document Open Password", TextField::OpenPassword),
        );
    }
    body.push(check_box(
        "protect-restrict",
        "Restrict editing and printing of the document",
        choices.restrict,
        ProtectAction::Restrict,
    ));
    if choices.restrict {
        body.push(state.permissions.read(cx).accessible(
            "Change Permissions Password",
            TextField::PermissionsPassword,
        ));
        body.push(radios(
            "protect-printing",
            "Printing Allowed",
            &printing_choices(),
            choices.printing,
            ProtectAction::Printing,
        ));
        body.push(radios(
            "protect-changes",
            "Changes Allowed",
            &changes_choices(),
            choices.changes,
            ProtectAction::Changes,
        ));
        body.push(check_box(
            "protect-copying",
            "Enable copying of text, images, and other content",
            choices.copying,
            ProtectAction::Copying,
        ));
        body.push(check_box(
            "protect-screen-readers",
            "Enable text access for screen reader devices for the visually impaired",
            choices.screen_readers,
            ProtectAction::ScreenReaders,
        ));
    }
    if let Some(error) = &state.error {
        body.push(Element::new("protect-error", Role::Alert, error.clone()));
    }
    body.push(
        Element::new("protect-apply", Role::Button, "Apply and Save")
            .with_activation(Activation::Protect(ProtectAction::Apply)),
    );
    body.push(
        Element::new("protect-cancel", Role::Button, "Cancel")
            .with_activation(Activation::CloseDialog),
    );
    body
}

/// One clickable row: a check box, or a radio button in a group.
fn row(
    id: impl Into<gpui::ElementId>,
    mark: &'static str,
    label: &'static str,
    action: ProtectAction,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .gap_2()
        .px_1()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |row| row.bg(theme.subtle_hover))
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(Activation::Protect(action), window, cx);
        }))
        .child(mark)
        .child(label)
}

fn check_row(
    id: &'static str,
    label: &'static str,
    on: bool,
    action: ProtectAction,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Stateful<gpui::Div> {
    row(id, if on { "☑" } else { "☐" }, label, action, theme, cx)
}

fn radio_rows<T: Copy + PartialEq>(
    id: &'static str,
    label: &'static str,
    choices: &[(T, &'static str)],
    in_force: T,
    action: impl Fn(T) -> ProtectAction,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut group = div()
        .flex()
        .flex_col()
        .child(div().text_color(theme.muted_text).child(label));
    for (index, (choice, text)) in choices.iter().enumerate() {
        let mark = if *choice == in_force { "◉" } else { "○" };
        group = group.child(row((id, index), mark, text, action(*choice), theme, cx));
    }
    group
}

fn password_row(label: &'static str, input: &Entity<SearchInput>) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .pl_6()
        .child(label)
        .child(div().w(px(200.0)).child(input.clone()))
}

pub(in crate::shell) fn render(
    state: &ProtectState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let choices = state.choices;
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .w(px(520.0))
        .text_sm()
        .child(radio_rows(
            "protect-compatibility",
            "Compatibility",
            &STRENGTHS,
            choices.strength,
            ProtectAction::Strength,
            theme,
            cx,
        ))
        .child(check_row(
            "protect-require-open",
            "Require a password to open the document",
            choices.require_open,
            ProtectAction::RequireOpen,
            theme,
            cx,
        ))
        .when(choices.require_open, |body| {
            body.child(password_row("Document Open Password", &state.open))
        })
        .child(check_row(
            "protect-restrict",
            "Restrict editing and printing of the document",
            choices.restrict,
            ProtectAction::Restrict,
            theme,
            cx,
        ));
    if choices.restrict {
        body = body
            .child(password_row(
                "Change Permissions Password",
                &state.permissions,
            ))
            .child(radio_rows(
                "protect-printing",
                "Printing Allowed",
                &printing_choices(),
                choices.printing,
                ProtectAction::Printing,
                theme,
                cx,
            ))
            .child(radio_rows(
                "protect-changes",
                "Changes Allowed",
                &changes_choices(),
                choices.changes,
                ProtectAction::Changes,
                theme,
                cx,
            ))
            .child(check_row(
                "protect-copying",
                "Enable copying of text, images, and other content",
                choices.copying,
                ProtectAction::Copying,
                theme,
                cx,
            ))
            .child(check_row(
                "protect-screen-readers",
                "Enable text access for screen reader devices for the visually impaired",
                choices.screen_readers,
                ProtectAction::ScreenReaders,
                theme,
                cx,
            ));
    }
    if let Some(error) = &state.error {
        body = body.child(div().text_color(theme.error_text).child(error.clone()));
    }
    body.child(
        div()
            .flex()
            .justify_end()
            .gap_2()
            .child(button(
                "protect-apply",
                "Apply and Save",
                true,
                theme,
                focused,
                cx,
                Activation::Protect(ProtectAction::Apply),
            ))
            .child(button(
                "protect-cancel",
                "Cancel",
                true,
                theme,
                focused,
                cx,
                Activation::CloseDialog,
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_make_acrobats_permission_bits() {
        let none = permissions(Printing::None, Changes::None, false, false);
        assert_eq!(none.0, 0);
        let bits = permissions(Printing::High, Changes::Comments, true, false);
        assert!(bits.print() && bits.print_high() && bits.annotate() && bits.fill_forms());
        assert!(bits.extract() && bits.accessibility() && !bits.modify());
        assert!(permissions(Printing::Low, Changes::Pages, false, true).assemble());
        assert!(!permissions(Printing::Low, Changes::Pages, false, true).print_high());
        let any = permissions(Printing::None, Changes::Any, false, true);
        assert!(any.modify() && any.assemble() && any.accessibility() && !any.extract());
        assert!(permissions(Printing::None, Changes::Forms, false, false).fill_forms());
        let labels: Vec<_> = Changes::ALL.map(Changes::label).to_vec();
        assert_eq!(labels[4], "Any except extracting pages");
        assert_eq!(Printing::Low.label(), "Low Resolution (150 dpi)");
    }

    #[test]
    fn the_passwords_are_checked_before_anything_is_written() {
        let mut choices = Choices::default();
        assert!(choices.protection("", "").is_err(), "nothing asked for");
        choices.apply(ProtectAction::RequireOpen);
        assert_eq!(
            choices.protection("", ""),
            Err("Type the password that opens the document")
        );
        let open_only = choices.protection("open", "").expect("an open password");
        assert_eq!(open_only.user_password, b"open");
        assert_eq!(open_only.owner_password, b"open");
        assert_eq!(open_only.permissions, Permissions::ALL);

        choices.apply(ProtectAction::Restrict);
        assert_eq!(
            choices.protection("open", ""),
            Err("Type the permissions password")
        );
        assert_eq!(
            choices.protection("same", "same"),
            Err("The two passwords must differ")
        );
        choices.apply(ProtectAction::Strength(Strength::Aes128));
        choices.apply(ProtectAction::Printing(Printing::Low));
        choices.apply(ProtectAction::Changes(Changes::Forms));
        choices.apply(ProtectAction::Copying);
        choices.apply(ProtectAction::ScreenReaders);
        choices.apply(ProtectAction::Apply);
        let both = choices.protection("open", "owner").expect("both");
        assert_eq!(both.strength, Strength::Aes128);
        assert_eq!(
            (
                both.user_password.as_slice(),
                both.owner_password.as_slice()
            ),
            (&b"open"[..], &b"owner"[..])
        );
        assert!(both.permissions.fill_forms() && both.permissions.extract());
        assert!(!both.permissions.print_high());

        choices.apply(ProtectAction::RequireOpen);
        let restricted = choices
            .protection("ignored", "owner")
            .expect("permissions only");
        assert!(
            restricted.user_password.is_empty(),
            "opens without a password"
        );
    }
}
