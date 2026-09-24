//! Signature Properties: everything validating one signature found, as
//! Acrobat's dialog of the same name lists it, and View Signed Version,
//! which opens the document as that signature signed it.

use accesskit::Role;
use gpui::{
    div, Context, InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _,
};
use onionskin_core::signatures::{Coverage, Validation};

use super::accessible::{Activation, Element};
use super::{ShellFrame, ThemeTokens};
use crate::shell::panes::signatures::{status, SignatureRow, VALIDATION_NOTE};

/// What the dialog's buttons do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SignaturePropertiesAction {
    ViewSignedVersion,
    Close,
}

/// One line of the dialog: a label and what it says.
type Fact = (&'static str, String);

/// Which bytes the signature covers, in words.
pub(in crate::shell) fn coverage(coverage: &Coverage) -> String {
    match coverage {
        Coverage::WholeFile => "The whole document".to_owned(),
        Coverage::Revision { revision, later } => format!(
            "Revision {} of {}: {later} later revision{} added after signing",
            revision + 1,
            revision + 1 + later,
            if *later == 1 { " was" } else { "s were" },
        ),
        Coverage::Invalid(reason) => format!("Not a range a signature may cover: {reason}"),
    }
}

/// What the signature's own CMS states, when it could be read.
fn cms_facts(validation: &Validation) -> Vec<Fact> {
    let check = match &validation.check {
        Ok(check) => check,
        Err(reason) => return vec![("Signature data", format!("Unreadable: {reason}"))],
    };
    let mut facts = Vec::new();
    if let Some(signer) = &check.signer {
        facts.push(("Signed by", signer.display_name().to_owned()));
        facts.push(("Certificate subject", signer.subject.clone()));
        facts.push(("Issued by", signer.issuer.clone()));
        facts.push((
            "Certificate valid",
            format!("{} to {}", signer.not_before, signer.not_after),
        ));
        facts.push(("Serial number", signer.serial.clone()));
    }
    if let Some(algorithm) = check.algorithm {
        facts.push(("Signature algorithm", algorithm.name().to_owned()));
    }
    if let Some(digest) = check.digest {
        let weak = if digest.is_weak() { " (weak)" } else { "" };
        facts.push(("Hash algorithm", format!("{}{weak}", digest.name())));
    }
    if let Some(time) = &check.signing_time {
        facts.push(("Signing time", format!("{time} (the signer's clock)")));
    }
    facts.push((
        "Timestamp",
        if check.has_timestamp {
            "Attached, not checked".to_owned()
        } else {
            "None".to_owned()
        },
    ));
    facts
}

/// What was changed after signing, and whether a certification forbids it.
fn change_facts(validation: &Validation) -> Vec<Fact> {
    let labels = |changes: &std::collections::BTreeSet<_>| {
        changes
            .iter()
            .map(|change: &onionskin_core::signatures::Change| change.label())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut facts = vec![(
        "Changes after signing",
        if validation.changes.is_empty() {
            "None".to_owned()
        } else {
            labels(&validation.changes)
        },
    )];
    if !validation.disallowed.is_empty() {
        facts.push((
            "Not allowed by certification",
            labels(&validation.disallowed),
        ));
    }
    facts
}

/// Every fact the dialog lists for `row`, in order.
pub(in crate::shell) fn facts(row: &SignatureRow) -> Vec<Fact> {
    let mut facts = vec![("Field", row.field.name.clone()), ("Status", status(row))];
    let Some(validation) = &row.validation else {
        return facts;
    };
    facts.push(("Summary", validation.summary()));
    facts.extend(cms_facts(validation));
    if let Some(level) = validation.certification {
        facts.push(("Certification", certification(level).to_owned()));
    }
    facts.push(("Covers", coverage(&validation.coverage)));
    facts.extend(change_facts(validation));
    if let Some(sub_filter) = &validation.sub_filter {
        facts.push(("Format", sub_filter.clone()));
    }
    let stated = [
        ("Reason (as written)", &row.field.reason),
        ("Location (as written)", &row.field.location),
    ];
    facts.extend(
        stated
            .into_iter()
            .filter_map(|(label, value)| value.clone().map(|value| (label, value))),
    );
    facts.push(("Identity", VALIDATION_NOTE.to_owned()));
    facts
}

/// What a certification level allows, as Acrobat words it.
fn certification(level: u8) -> &'static str {
    match level {
        1 => "No changes allowed",
        2 => "Form fill-in and signing allowed",
        _ => "Form fill-in, signing and commenting allowed",
    }
}

/// Whether View Signed Version has something to open: a signature that
/// covers an earlier revision. For one covering the whole file, the signed
/// version is the document already open.
pub(in crate::shell) fn can_view_signed_version(row: &SignatureRow) -> bool {
    row.validation.as_ref().is_some_and(|validation| {
        validation.signed_length.is_some()
            && matches!(validation.coverage, Coverage::Revision { .. })
    })
}

fn buttons(row: &SignatureRow) -> Vec<(&'static str, &'static str, SignaturePropertiesAction)> {
    let mut buttons = Vec::new();
    if can_view_signed_version(row) {
        buttons.push((
            "signature-properties-view",
            "View Signed Version",
            SignaturePropertiesAction::ViewSignedVersion,
        ));
    }
    buttons.push((
        "signature-properties-close",
        "Close",
        SignaturePropertiesAction::Close,
    ));
    buttons
}

pub(in crate::shell) fn accessible(row: &SignatureRow) -> Vec<Element> {
    let facts = facts(row)
        .into_iter()
        .enumerate()
        .map(|(index, (label, value))| {
            Element::new(
                ("signature-fact", index),
                Role::Label,
                format!("{label}: {value}"),
            )
        });
    let buttons = buttons(row).into_iter().map(|(id, label, action)| {
        Element::new(id, Role::Button, label)
            .with_activation(Activation::SignatureProperties(action))
    });
    facts.chain(buttons).collect()
}

pub(in crate::shell) fn render(
    row: &SignatureRow,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1().text_sm();
    for (label, value) in facts(row) {
        list = list.child(
            div()
                .flex()
                .gap_2()
                .px_2()
                .child(
                    div()
                        .w_40()
                        .flex_none()
                        .text_color(theme.secondary_text)
                        .child(label),
                )
                .child(div().text_color(theme.text).child(value)),
        );
    }
    let mut actions = div().flex().justify_end().gap_2().pt_2();
    for (id, label, action) in buttons(row) {
        actions = actions.child(
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |button| button.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::SignatureProperties(action), window, cx);
                }))
                .child(label),
        );
    }
    list.child(actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_core::signatures::Verdict;
    use onionskin_core::SignatureField;

    fn row(coverage: Coverage) -> SignatureRow {
        SignatureRow {
            field: SignatureField {
                name: "Signature1".to_owned(),
                signed: true,
                signer: None,
                reason: Some("Approved".to_owned()),
                location: None,
                signed_at: None,
            },
            validation: Some(Validation {
                field: "Signature1".to_owned(),
                sub_filter: Some("adbe.pkcs7.detached".to_owned()),
                check: Err("no signer info".to_owned()),
                coverage,
                changes: Default::default(),
                certification: Some(2),
                signed_length: Some(10),
                disallowed: Default::default(),
                verdict: Verdict::Unknown("no signer info".to_owned()),
            }),
        }
    }

    #[test]
    fn coverage_counts_revisions_from_one() {
        assert_eq!(coverage(&Coverage::WholeFile), "The whole document");
        assert_eq!(
            coverage(&Coverage::Revision {
                revision: 0,
                later: 1
            }),
            "Revision 1 of 2: 1 later revision was added after signing"
        );
        assert_eq!(
            coverage(&Coverage::Revision {
                revision: 1,
                later: 2
            }),
            "Revision 2 of 4: 2 later revisions were added after signing"
        );
        assert!(coverage(&Coverage::Invalid("gap".into())).ends_with("gap"));
    }

    #[test]
    fn the_signed_version_is_offered_only_for_an_earlier_revision() {
        let whole = row(Coverage::WholeFile);
        assert!(!can_view_signed_version(&whole));
        let earlier = row(Coverage::Revision {
            revision: 0,
            later: 1,
        });
        assert!(can_view_signed_version(&earlier));
        let labels: Vec<_> = accessible(&earlier)
            .into_iter()
            .filter(|element| element.role == Role::Button)
            .map(|element| element.label)
            .collect();
        assert_eq!(labels, ["View Signed Version", "Close"]);
    }

    #[test]
    fn the_facts_label_what_the_file_wrote_and_say_identity_is_unchecked() {
        let listed = facts(&row(Coverage::WholeFile));
        let find = |label| {
            listed
                .iter()
                .find(|(found, _)| *found == label)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(find("Signature data"), Some("Unreadable: no signer info"));
        assert_eq!(
            find("Certification"),
            Some("Form fill-in and signing allowed")
        );
        assert_eq!(find("Reason (as written)"), Some("Approved"));
        assert_eq!(find("Changes after signing"), Some("None"));
        assert_eq!(find("Identity"), Some(VALIDATION_NOTE));
        assert_eq!(certification(1), "No changes allowed");
        assert!(certification(3).contains("commenting"));

        let unsigned = SignatureRow {
            validation: None,
            ..row(Coverage::WholeFile)
        };
        assert_eq!(facts(&unsigned).len(), 2, "field and status only");
    }
}
