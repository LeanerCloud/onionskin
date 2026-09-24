//! The Security Settings pane: on a secured document, what its security
//! allows, as Acrobat's pane of the same name summarizes it, and Permission
//! Details, which opens the Security tab of Document Properties.
//!
//! The strip shows the pane's button only for a secured document.

use accesskit::Role;
use gpui::{
    div, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{ShellFrame, ThemeTokens};
use super::{empty_message, list};

/// The rows the pane lists: the Security tab's own.
pub(super) type SecurityRows = Vec<(&'static str, String)>;

/// Said at the head of the pane on a secured document.
const SECURED: &str =
    "This document is secured. What it allows is below; some features are unavailable.";
/// Said for a document with no security, which the strip does not offer
/// the pane for but a switched tab can still leave open.
const NOT_SECURED: &str = "This document has no security.";
const DETAILS: &str = "Permission Details";

/// Whether `rows` describe a secured document.
pub(super) fn secured(rows: &SecurityRows) -> bool {
    rows.iter()
        .any(|(label, value)| *label == "Security Method" && value != "No Security")
}

pub(super) fn accessible(rows: &SecurityRows) -> Vec<Element> {
    if !secured(rows) {
        return vec![Element::new("security-empty", Role::Label, NOT_SECURED)];
    }
    let mut described = vec![Element::new("security-summary", Role::Label, SECURED)];
    described.extend(rows.iter().enumerate().map(|(index, (label, value))| {
        Element::new(
            ("security-row", index),
            Role::Label,
            format!("{label}: {value}"),
        )
    }));
    described.push(
        Element::new("security-details", Role::Button, DETAILS)
            .with_activation(Activation::ShowPermissionDetails),
    );
    described
}

pub(super) fn render(
    rows: &SecurityRows,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    if !secured(rows) {
        return empty_message(NOT_SECURED, theme).into_any_element();
    }
    let mut body = list("security-rows").child(
        div()
            .px_2()
            .py_1()
            .text_sm()
            .text_color(theme.text)
            .child(SECURED),
    );
    for (label, value) in rows {
        body = body.child(
            div()
                .px_2()
                .text_xs()
                .flex()
                .flex_col()
                .child(div().text_color(theme.secondary_text).child(*label))
                .child(div().text_color(theme.text).child(value.clone())),
        );
    }
    body.child(
        div()
            .id("security-details")
            .mx_2()
            .mt_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .text_sm()
            .cursor_pointer()
            .hover(move |button| button.bg(theme.subtle_hover))
            .on_click(cx.listener(|frame, _event, window, cx| {
                frame.run_activation(Activation::ShowPermissionDetails, window, cx);
            }))
            .child(DETAILS),
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_document_says_it_has_no_security() {
        let plain = vec![("Security Method", "No Security".to_owned())];
        assert!(!secured(&plain));
        assert_eq!(accessible(&plain)[0].label, NOT_SECURED);

        let secured_rows = vec![
            ("Security Method", "Password Security".to_owned()),
            ("Printing", "Not Allowed".to_owned()),
        ];
        let described = accessible(&secured_rows);
        let labels: Vec<_> = described
            .iter()
            .map(|element| element.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                SECURED,
                "Security Method: Password Security",
                "Printing: Not Allowed",
                DETAILS
            ]
        );
    }
}
