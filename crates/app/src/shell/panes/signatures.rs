//! The signatures pane: which signature fields the document has, and what
//! each one says about itself.
//!
//! Read-only, and silent about validity. Parity row 199 puts listing at M2
//! and validation and status reporting at M6, so every string here comes
//! from the form dictionary and none of them is an assessment. The pane
//! carries [`VALIDATION_NOTE`] so a reader is told that, rather than being
//! left to assume a listed signature is a checked one.

use accesskit::Role;
use gpui::{div, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _};
use onionskin_core::SignatureField;

use super::super::chrome::accessible::Element;
use super::super::chrome::ThemeTokens;
use super::{empty_message, error_message, list};

/// Said once at the foot of the pane. Absence of a verdict reads as a
/// verdict, which is the failure this exists to prevent.
pub(super) const VALIDATION_NOTE: &str =
    "Onionskin does not check signatures yet. Validation and signer trust arrive in M6.";

/// Said where the list would be when the document has no signature fields.
const NO_SIGNATURES: &str = "This document has no signature fields.";

/// What the file says about one field. Never whether it verifies.
pub(super) fn status(field: &SignatureField) -> &'static str {
    if field.signed {
        "Signed, not checked"
    } else {
        "Not signed"
    }
}

/// The line under the field name: the signer, the reason and the time, as
/// the document wrote them. Absent entries are left out rather than filled
/// in.
/// Every part is labelled with the entry it came from. A file is free to
/// write "Verified: signature VALID" into `/Reason`, and unlabelled text in
/// this pane would read as Onionskin saying it.
pub(super) fn detail(field: &SignatureField) -> String {
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
fn name(field: &SignatureField) -> String {
    if field.name.is_empty() {
        "(unnamed field)".to_owned()
    } else {
        field.name.clone()
    }
}

/// What the signatures pane tells a screen reader.
///
/// No row activates anything: the pane lists what the form dictionary says
/// and does nothing to it until M6 brings validation. The note is described
/// as the last child, as it is drawn, so a reader is told the listing is not
/// a check.
pub(super) fn accessible(items: Result<&[SignatureField], &String>) -> Vec<Element> {
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
        .map(|(index, field)| {
            Element::new(("signature-row", index), Role::ListItem, name(field))
                .with_description(format!("{}. {}", status(field), detail(field)))
        })
        .collect();
    rows.push(Element::new(
        "signature-validation-note",
        Role::Label,
        VALIDATION_NOTE,
    ));
    vec![Element::new("signature-rows", Role::List, "Signatures").with_children(rows)]
}

pub(super) fn render(
    items: Result<&[SignatureField], &String>,
    theme: ThemeTokens,
) -> gpui::AnyElement {
    let items = match items {
        Ok(items) => items,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    if items.is_empty() {
        return empty_message(NO_SIGNATURES, theme).into_any_element();
    }

    let mut body = list("signature-rows");
    for (index, field) in items.iter().enumerate() {
        body = body.child(
            div()
                .id(("signature-row", index))
                .flex()
                .flex_col()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(theme.text)
                .child(name(field))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.secondary_text)
                        .child(status(field)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child(detail(field)),
                ),
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

    /// The pane distinguishes a signed field from an empty one, and says
    /// nothing about either being valid. "Signed" alone would be read as
    /// "checked", which is why the word "checked" is in the string and why
    /// the note is asserted here rather than left to the layout.
    #[test]
    fn a_signed_field_is_never_reported_as_valid() {
        assert_eq!(status(&field(true)), "Signed, not checked");
        assert_eq!(status(&field(false)), "Not signed");

        for text in [status(&field(true)), status(&field(false)), VALIDATION_NOTE] {
            let lowered = text.to_lowercase();
            assert!(
                !lowered.contains("valid signature") && !lowered.contains("verified"),
                "the pane must claim nothing about validity, said {text:?}"
            );
        }
        assert!(
            VALIDATION_NOTE.contains("M6"),
            "the note names the milestone that brings validation"
        );
    }

    /// Everything on the line is a string the file wrote, and every one of
    /// them says which entry it came from. An unsigned field has none of
    /// them and says so rather than showing empty separators.
    #[test]
    fn the_detail_line_carries_only_what_the_file_wrote_and_labels_it() {
        assert_eq!(
            detail(&field(true)),
            "Name: Ada Lovelace · Time: D:20260101120000Z"
        );
        assert_eq!(
            detail(&field(false)),
            "The document states nothing else about this field"
        );
    }

    /// A file that writes a verdict into `/Reason` gets it back labelled as
    /// its own, because unlabelled it would read as this pane's.
    #[test]
    fn a_reason_that_reads_like_a_verdict_is_labelled_as_the_documents_own() {
        let hostile = SignatureField {
            reason: Some("Verified: signature VALID".to_owned()),
            ..field(true)
        };

        let detail = detail(&hostile);

        assert!(
            detail.contains("Reason: Verified: signature VALID"),
            "said {detail:?}"
        );
        assert!(!detail.starts_with("Verified"), "said {detail:?}");
    }

    /// One described row per drawn row, each saying the field's status and
    /// what the file wrote, and none of them activating anything: the pane is
    /// a listing until M6 brings validation.
    #[test]
    fn each_described_field_says_its_status_and_activates_nothing() {
        let items = [
            field(true),
            SignatureField {
                name: String::new(),
                ..field(false)
            },
        ];

        let described = accessible(Ok(&items));
        let rows = &described[0].children;

        assert_eq!(described.len(), 1);
        assert_eq!(described[0].role, Role::List);
        assert_eq!(rows[0].label, "Approval");
        assert_eq!(rows[1].label, "(unnamed field)");
        for (index, (row, item)) in rows.iter().zip(items.iter()).enumerate() {
            assert_eq!(row.key, gpui::ElementId::from(("signature-row", index)));
            assert_eq!(
                row.description.as_deref(),
                Some(format!("{}. {}", status(item), detail(item)).as_str())
            );
            assert_eq!(row.activation, None);
        }
    }

    /// The note is the last thing described, as it is the last thing drawn.
    /// A reader given only the rows would hear a listing and take it for a
    /// check.
    #[test]
    fn the_validation_note_is_described_after_the_last_field() {
        let items = [field(true), field(false)];

        let described = accessible(Ok(&items));
        let children = &described[0].children;

        assert_eq!(children.len(), items.len() + 1);
        assert_eq!(children[items.len()].label, VALIDATION_NOTE);
    }

    /// A document with no signature fields says so, and a reader that failed
    /// says what went wrong rather than reading as a document with none.
    #[test]
    fn an_empty_list_and_a_failed_read_are_announced_differently() {
        let empty = accessible(Ok(&[]));
        assert_eq!(empty[0].role, Role::Label);
        assert_eq!(empty[0].label, NO_SIGNATURES);

        let failure = "the form dictionary could not be decoded".to_owned();
        let broken = accessible(Err(&failure));
        assert_eq!(broken[0].role, Role::Alert);
        assert_eq!(broken[0].label, failure);
    }
}
