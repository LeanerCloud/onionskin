//! The signatures pane: which signature fields the document has, and what
//! each one says about itself.
//!
//! Read-only, and silent about validity. Parity row 199 puts listing at M2
//! and validation and status reporting at M6, so every string here comes
//! from the form dictionary and none of them is an assessment. The pane
//! carries [`VALIDATION_NOTE`] so a reader is told that, rather than being
//! left to assume a listed signature is a checked one.

use gpui::{div, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _};
use onionskin_core::SignatureField;

use super::super::chrome::ThemeTokens;
use super::{empty_message, error_message, list};

/// Said once at the foot of the pane. Absence of a verdict reads as a
/// verdict, which is the failure this exists to prevent.
pub(super) const VALIDATION_NOTE: &str =
    "Onionskin does not check signatures yet. Validation and signer trust arrive in M6.";

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

pub(super) fn render(
    items: Result<&[SignatureField], &String>,
    theme: ThemeTokens,
) -> gpui::AnyElement {
    let items = match items {
        Ok(items) => items,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    if items.is_empty() {
        return empty_message("This document has no signature fields.", theme).into_any_element();
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
                .child(if field.name.is_empty() {
                    "(unnamed field)".to_owned()
                } else {
                    field.name.clone()
                })
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
}
