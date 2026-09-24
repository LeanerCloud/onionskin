//! The signatures pane: each signature field, and what validating it found.
//!
//! A row leads with the verdict: valid, invalid or unknown, with the
//! reason, and then whether the signer is who their certificate says,
//! judged against the certificates the user trusts in Preferences >
//! Signatures. A signature whose signer chains to none of them is valid
//! with its signer unknown, as Acrobat says. Activating a row opens its
//! properties.

use std::time::SystemTime;

use accesskit::Role;
use gpui::{
    div, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::signatures::{
    identity, identity_sentence, Identity, TrustAnchor, Validation, Verdict, VerificationTime,
};
use onionskin_core::SignatureField;

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{ShellFrame, ThemeTokens};
use super::{empty_message, error_message, list, PaneAction};

/// Said once at the foot of the pane.
pub(in crate::shell) const VALIDATION_NOTE: &str =
    "Signers are identified by the certificates trusted in Preferences > Signatures.";

/// Said where the list would be when the document has no signature fields.
const NO_SIGNATURES: &str = "This document has no signature fields.";

/// What signers are judged by: the Signatures preferences and the trusted
/// certificates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::shell) struct SignatureTrust {
    pub(in crate::shell) anchors: Vec<TrustAnchor>,
    pub(in crate::shell) time: VerificationTime,
    pub(in crate::shell) verify_on_open: bool,
}

impl Default for SignatureTrust {
    /// The preferences' own defaults, with nothing trusted.
    fn default() -> Self {
        Self {
            anchors: Vec::new(),
            time: VerificationTime::Current,
            verify_on_open: true,
        }
    }
}

/// One signature field and, when it is signed, its validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::shell) struct SignatureRow {
    pub(in crate::shell) field: SignatureField,
    /// `None` for an unsigned field, when validating failed as a whole, or
    /// when nothing has asked for it yet.
    pub(in crate::shell) validation: Option<Validation>,
    /// Whether the signer is trusted, for a validated signature.
    pub(in crate::shell) identity: Option<Identity>,
}

/// Pair each field with its validation, by field name, and judge each
/// signer by `trust`. A field nothing validated keeps `None`.
pub(in crate::shell) fn rows(
    fields: Vec<SignatureField>,
    mut validations: Vec<Validation>,
    trust: &SignatureTrust,
) -> Vec<SignatureRow> {
    let now = SystemTime::now();
    fields
        .into_iter()
        .map(|field| {
            let validation = validations
                .iter()
                .position(|found| found.field == field.name)
                .map(|at| validations.swap_remove(at));
            let identity = validation
                .as_ref()
                .map(|validation| identity(validation, &trust.anchors, trust.time, now));
            SignatureRow {
                field,
                validation,
                identity,
            }
        })
        .collect()
}

/// The row's verdict, in a few words.
pub(in crate::shell) fn status(row: &SignatureRow) -> String {
    if !row.field.signed {
        return "Not signed".to_owned();
    }
    let Some(validation) = &row.validation else {
        return "Signed, not checked".to_owned();
    };
    let mut verdict = match &validation.verdict {
        Verdict::Valid if validation.changes.is_empty() => "Valid".to_owned(),
        Verdict::Valid => "Valid, changed after signing".to_owned(),
        Verdict::Invalid(_) => "Invalid".to_owned(),
        Verdict::Unknown(_) => "Validity unknown".to_owned(),
    };
    if validation.verdict == Verdict::Valid {
        match &row.identity {
            Some(Identity::Valid { .. }) => {}
            Some(Identity::Invalid(_)) => verdict.push_str(", signer's identity invalid"),
            Some(Identity::Unknown(_)) | None => verdict.push_str(", signer unknown"),
        }
    }
    let mut status = match validation.certification {
        Some(_) => format!("Certified: {verdict}"),
        None => verdict,
    };
    if validation.is_weak() {
        status.push_str(" (SHA-1, weak)");
    }
    status
}

/// The line under the status: what validating found, or, for a field that
/// was not checked, what the file states about it, each part labelled with
/// the entry it came from. A file is free to write "Verified: signature
/// VALID" into `/Reason`, and unlabelled it would read as this pane's.
pub(in crate::shell) fn detail(row: &SignatureRow) -> String {
    if let Some(validation) = &row.validation {
        return match &row.identity {
            Some(identity) => format!("{} {}", validation.summary(), identity_sentence(identity)),
            None => validation.summary(),
        };
    }
    let field = &row.field;
    let mut parts = Vec::new();
    if let Some(signer) = field.signer.as_ref() {
        parts.push(format!("Name: {signer}"));
    }
    if let Some(signed_at) = field.signed_at.as_ref() {
        parts.push(format!("Time: {signed_at}"));
    }
    if let Some(reason) = field.reason.as_ref() {
        parts.push(format!("Reason: {reason}"));
    }
    if let Some(location) = field.location.as_ref() {
        parts.push(format!("Location: {location}"));
    }
    if parts.is_empty() {
        "The document states nothing else about this field".to_owned()
    } else {
        parts.join(" · ")
    }
}

/// The row's text. A file may leave a field unnamed, and a row with nothing
/// in it is a row nobody can see or hear.
fn name(row: &SignatureRow) -> String {
    if row.field.name.is_empty() {
        "(unnamed field)".to_owned()
    } else {
        row.field.name.clone()
    }
}

/// What activating a row does: a signed field's properties.
fn activation(index: usize, row: &SignatureRow) -> Option<Activation> {
    row.validation
        .is_some()
        .then_some(Activation::ShowSignatureProperties(index))
}

/// Whether Validate All has something to do: a signed field not checked.
fn unchecked(items: &[SignatureRow]) -> bool {
    items
        .iter()
        .any(|row| row.field.signed && row.validation.is_none())
}

const VALIDATE_ALL: &str = "Validate All";

/// What the signatures pane tells a screen reader.
pub(super) fn accessible(items: Result<&[SignatureRow], &String>) -> Vec<Element> {
    let items = match items {
        Ok(items) => items,
        Err(message) => {
            return vec![Element::new(
                "signature-rows-error",
                Role::Alert,
                message.clone(),
            )]
        }
    };
    if items.is_empty() {
        return vec![Element::new(
            "signature-rows-empty",
            Role::Label,
            NO_SIGNATURES,
        )];
    }

    let mut rows: Vec<Element> = items
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let element = Element::new(("signature-row", index), Role::ListItem, name(row))
                .with_description(format!("{}. {}", status(row), detail(row)));
            match activation(index, row) {
                Some(activation) => element.with_activation(activation),
                None => element,
            }
        })
        .collect();
    if unchecked(items) {
        rows.push(
            Element::new("signature-validate-all", Role::Button, VALIDATE_ALL)
                .with_activation(Activation::Pane(PaneAction::ValidateSignatures)),
        );
    }
    rows.push(Element::new(
        "signature-validation-note",
        Role::Label,
        VALIDATION_NOTE,
    ));
    vec![Element::new("signature-rows", Role::List, "Signatures").with_children(rows)]
}

pub(super) fn render(
    items: Result<&[SignatureRow], &String>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let items = match items {
        Ok(items) => items,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    if items.is_empty() {
        return empty_message(NO_SIGNATURES, theme).into_any_element();
    }

    let mut body = list("signature-rows");
    for (index, row) in items.iter().enumerate() {
        let mut line = div()
            .id(("signature-row", index))
            .flex()
            .flex_col()
            .px_2()
            .py_1()
            .text_sm()
            .text_color(theme.text)
            .child(name(row))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.secondary_text)
                    .child(status(row)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_text)
                    .child(detail(row)),
            );
        if let Some(activation) = activation(index, row) {
            line = line
                .cursor_pointer()
                .hover(move |line| line.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(activation.clone(), window, cx);
                }));
        }
        body = body.child(line);
    }
    if unchecked(items) {
        body = body.child(
            div()
                .id("signature-validate-all")
                .mx_2()
                .px_2()
                .py_1()
                .rounded_sm()
                .text_sm()
                .cursor_pointer()
                .hover(move |button| button.bg(theme.subtle_hover))
                .on_click(cx.listener(|frame, _event, window, cx| {
                    frame.run_activation(
                        Activation::Pane(PaneAction::ValidateSignatures),
                        window,
                        cx,
                    );
                }))
                .child(VALIDATE_ALL),
        );
    }
    body.child(
        div()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(theme.muted_text)
            .child(VALIDATION_NOTE),
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(signed: bool) -> SignatureField {
        SignatureField {
            name: "Approval".to_owned(),
            signed,
            signer: signed.then(|| "Ada Lovelace".to_owned()),
            reason: None,
            location: None,
            signed_at: signed.then(|| "D:20260101120000Z".to_owned()),
        }
    }

    fn unchecked(signed: bool) -> SignatureRow {
        SignatureRow {
            field: field(signed),
            validation: None,
            identity: None,
        }
    }

    #[test]
    fn a_field_nothing_checked_says_so_and_labels_what_the_file_wrote() {
        assert_eq!(status(&unchecked(true)), "Signed, not checked");
        assert_eq!(status(&unchecked(false)), "Not signed");
        assert_eq!(
            detail(&unchecked(true)),
            "Name: Ada Lovelace · Time: D:20260101120000Z"
        );
        assert_eq!(
            detail(&unchecked(false)),
            "The document states nothing else about this field"
        );
        let hostile = SignatureRow {
            field: SignatureField {
                reason: Some("Verified: signature VALID".to_owned()),
                ..field(true)
            },
            validation: None,
            identity: None,
        };
        assert!(detail(&hostile).contains("Reason: Verified: signature VALID"));
        assert_eq!(activation(0, &hostile), None, "nothing to show");
    }

    #[test]
    fn an_empty_list_and_a_failed_read_are_announced_differently() {
        let empty = accessible(Ok(&[]));
        assert_eq!(empty[0].role, Role::Label);
        assert_eq!(empty[0].label, NO_SIGNATURES);

        let failure = "the form dictionary could not be decoded".to_owned();
        let broken = accessible(Err(&failure));
        assert_eq!(broken[0].role, Role::Alert);
        assert_eq!(broken[0].label, failure);

        let unnamed = SignatureRow {
            field: SignatureField {
                name: String::new(),
                ..field(false)
            },
            validation: None,
            identity: None,
        };
        let described = accessible(Ok(std::slice::from_ref(&unnamed)));
        assert_eq!(described[0].children[0].label, "(unnamed field)");
        assert_eq!(described[0].children[1].label, VALIDATION_NOTE);
    }
}
