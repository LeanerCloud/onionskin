//! M6 signatures on a real window: the Signatures pane leads each signed
//! field with its verdict, a row opens Signature Properties, and View
//! Signed Version opens the document as the signature signed it.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::signature_properties::SignaturePropertiesAction;
use crate::shell::dialog::ShellDialog;
use crate::shell::panes::{NavigationPane, PaneAction};
use onionskin_corpus_testing::signed_fixture;

fn window(name: &str, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let bytes = std::fs::read(signed_fixture(name)).expect("the fixture reads");
    let window = bound_window_from_bytes(vec![(name, bytes)], cx).0;
    window
        .update(cx, |frame, _, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Signatures), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: Activation, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(action, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
}

/// Every label and description under `prefix` in the accessibility tree.
fn described(
    window: gpui::WindowHandle<ShellFrame>,
    prefix: &str,
    cx: &mut TestAppContext,
) -> Vec<String> {
    window
        .update(cx, |frame, window, cx| {
            frame
                .accessible(window, cx)
                .walk()
                .filter(|element| format!("{:?}", element.key).contains(prefix))
                .map(|element| {
                    format!(
                        "{} | {}",
                        element.label,
                        element.description.clone().unwrap_or_default()
                    )
                })
                .collect()
        })
        .unwrap()
}

#[gpui::test]
fn the_pane_leads_with_each_verdict(cx: &mut TestAppContext) {
    let valid = window("rsa-sha256.pdf", cx);
    let rows = described(valid, "signature-row", cx);
    assert!(
        rows.iter().any(|row| row.contains("Valid, signer unknown")),
        "{rows:?}"
    );

    let tampered = window("tampered.pdf", cx);
    let rows = described(tampered, "signature-row", cx);
    assert!(rows.iter().any(|row| row.contains("| Invalid")), "{rows:?}");

    let annotated = window("annotated-after.pdf", cx);
    let rows = described(annotated, "signature-row", cx);
    assert!(
        rows.iter()
            .any(|row| row.contains("Valid, changed after signing, signer unknown")),
        "{rows:?}"
    );

    let weak = window("rsa-sha1.pdf", cx);
    let rows = described(weak, "signature-row", cx);
    assert!(
        rows.iter().any(|row| row.contains("(SHA-1, weak)")),
        "{rows:?}"
    );
}

#[gpui::test]
fn a_row_opens_its_properties_and_the_signed_version(cx: &mut TestAppContext) {
    let window = window("annotated-after.pdf", cx);
    act(window, Activation::ShowSignatureProperties(0), cx);
    let dialog = window.update(cx, |frame, _, _| frame.dialog).unwrap();
    assert_eq!(dialog, Some(ShellDialog::SignatureProperties));
    let facts = described(window, "signature-fact", cx);
    assert!(
        facts
            .iter()
            .any(|fact| fact.starts_with("Signed by: Ada Signer")),
        "{facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.starts_with("Covers: Revision 2 of 3")),
        "{facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.starts_with("Changes after signing: ")),
        "{facts:?}"
    );

    act(
        window,
        Activation::SignatureProperties(SignaturePropertiesAction::ViewSignedVersion),
        cx,
    );
    window
        .update(cx, |frame, _, cx| {
            assert_eq!(frame.dialog, None);
            assert_eq!(frame.tabs.tabs().len(), 2, "the signed version opened");
            let signed = frame.tabs.active().expect("a tab").source.clone();
            let name = signed.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(name, "annotated-after (signed version, Signature1).pdf");
            let expected = std::fs::read(signed_fixture("rsa-sha256.pdf")).unwrap();
            assert_eq!(std::fs::read(&signed).unwrap(), expected);
            let canvas = frame.active_canvas().expect("a tab").clone();
            let rows = canvas.update(cx, |canvas, _| canvas.model.signatures().unwrap());
            assert!(rows[0].validation.as_ref().unwrap().changes.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn an_unsigned_field_and_close_do_nothing_more(cx: &mut TestAppContext) {
    let unsigned = window("unsigned-field.pdf", cx);
    let rows = described(unsigned, "signature-row", cx);
    assert!(
        rows.iter().any(|row| row.contains("Not signed")),
        "{rows:?}"
    );
    act(unsigned, Activation::ShowSignatureProperties(7), cx);
    let dialog = unsigned.update(cx, |frame, _, _| frame.dialog).unwrap();
    assert_eq!(dialog, None, "no row seven, no dialog");

    let signed = window("rsa-sha256.pdf", cx);
    act(signed, Activation::ShowSignatureProperties(0), cx);
    let buttons = described(signed, "signature-properties-", cx);
    assert_eq!(buttons, ["Close | "], "the whole file is already open");
    act(
        signed,
        Activation::SignatureProperties(SignaturePropertiesAction::Close),
        cx,
    );
    signed
        .update(cx, |frame, _, _| {
            assert_eq!(frame.dialog, None);
            assert_eq!(frame.tabs.tabs().len(), 1);
        })
        .unwrap();
}
