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
            let signed = frame
                .tabs
                .active()
                .expect("a tab")
                .canvas
                .read(cx)
                .model
                .path()
                .expect("signed version path");
            let name = signed.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(name, "annotated-after (signed version, Signature1).pdf");
            let expected = std::fs::read(signed_fixture("rsa-sha256.pdf")).unwrap();
            assert_eq!(std::fs::read(&signed).unwrap(), expected);
            let canvas = frame.active_canvas().expect("a tab").clone();
            let rows = canvas.update(cx, |canvas, _| {
                canvas.model.signatures(&Default::default()).unwrap()
            });
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
    assert_eq!(
        buttons,
        ["Add to Trusted Certificates | ", "Close | "],
        "the whole file is already open"
    );
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

static NEXT_DIR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A window on `name` whose settings live in a fresh folder, so trusting a
/// certificate is kept and read back there.
fn window_with_settings(
    name: &str,
    cx: &mut TestAppContext,
) -> (gpui::WindowHandle<ShellFrame>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "onionskin-trust-{name}-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = std::fs::read(signed_fixture(name)).expect("the fixture reads");
    let model = CanvasModel::new(
        onionskin_core::Document::open_bytes(bytes).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let paths = crate::config::ConfigPaths::in_dir(&dir);
    let (window, _) = bound_window_with_models(vec![(PathBuf::from(name), model)], paths, cx);
    window
        .update(cx, |frame, _, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Signatures), cx);
        })
        .unwrap();
    cx.run_until_parked();
    (window, dir)
}

/// Each row's name, status and detail, without the list around them.
fn statuses(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    described(window, "signature-row", cx)
        .into_iter()
        .filter(|row| !row.starts_with("Signatures | "))
        .collect()
}

#[gpui::test]
fn trusting_the_ca_makes_the_signer_known(cx: &mut TestAppContext) {
    use crate::shell::preferences_dialog::PreferenceChange;
    let (window, dir) = window_with_settings("rsa-sha256.pdf", cx);
    assert!(statuses(window, cx)[0].contains("Valid, signer unknown"));

    window
        .update(cx, |frame, _, cx| {
            frame.trust_certificates_in(&signed_fixture("test-ca.pem"), cx)
        })
        .unwrap();
    cx.run_until_parked();
    let row = &statuses(window, cx)[0];
    assert!(row.contains("| Valid. "), "{row}");
    assert!(row.contains("The signer's identity is valid."), "{row}");
    let kept = std::fs::read_to_string(dir.join("trusted-certificates.json")).unwrap();
    assert!(kept.contains("BEGIN CERTIFICATE"));

    act(window, Activation::ShowSignatureProperties(0), cx);
    let facts = described(window, "signature-fact", cx);
    assert!(
        facts
            .iter()
            .any(|fact| fact.starts_with("Trusted through: Ada Signer > Onionskin Test CA")),
        "{facts:?}"
    );
    let buttons = described(window, "signature-properties-", cx);
    assert_eq!(buttons, ["Close | "], "nothing left to trust");
    act(
        window,
        Activation::SignatureProperties(SignaturePropertiesAction::Close),
        cx,
    );

    // Preferences > Signatures lists it; untrusting it for signatures makes
    // the signer unknown again, and removing it empties the list.
    let rows = window
        .update(cx, |frame, _, _| {
            crate::shell::preferences_dialog::rows_for(
                frame.preferences(),
                &[],
                frame.trusted_certificates(),
                crate::preferences::PreferenceCategory::Signatures,
            )
            .into_iter()
            .map(|row| row.label)
            .collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(rows[2], "1 trusted certificate");
    assert!(
        rows[3].starts_with("Onionskin Test CA, issued by "),
        "{rows:?}"
    );
    window
        .update(cx, |frame, _, cx| {
            frame.change_preference(
                PreferenceChange::ToggleTrust {
                    index: 0,
                    certified: false,
                },
                cx,
            )
        })
        .unwrap();
    assert!(statuses(window, cx)[0].contains("Valid, signer unknown"));
    window
        .update(cx, |frame, _, cx| {
            frame.change_preference(PreferenceChange::RemoveTrusted(0), cx);
            assert!(frame.trusted_certificates().is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn add_to_trusted_trusts_the_signer_itself(cx: &mut TestAppContext) {
    let (window, _dir) = window_with_settings("ecdsa-p256.pdf", cx);
    act(window, Activation::ShowSignatureProperties(0), cx);
    let buttons = described(window, "signature-properties-", cx);
    assert_eq!(buttons, ["Add to Trusted Certificates | ", "Close | "]);
    act(
        window,
        Activation::SignatureProperties(SignaturePropertiesAction::AddToTrusted),
        cx,
    );
    let row = &statuses(window, cx)[0];
    assert!(row.contains("| Valid. "), "{row}");

    // A file with no certificate in it is said so.
    let (window, dir) = window_with_settings("rsa-sha256.pdf", cx);
    let junk = dir.join("junk.pem");
    std::fs::write(&junk, "nothing").unwrap();
    window
        .update(cx, |frame, _, cx| {
            frame.trust_certificates_in(&junk, cx);
            frame.trust_certificates_in(&dir.join("missing.pem"), cx);
            assert!(frame
                .notices
                .iter()
                .any(|notice| notice
                    == crate::shell::chrome::tabs::signature_properties::NO_CERTIFICATE));
            assert!(frame
                .notices
                .iter()
                .any(|notice| notice.contains("could not be read")));
        })
        .unwrap();
}

#[gpui::test]
fn signatures_wait_for_validate_all_when_not_verified_on_open(cx: &mut TestAppContext) {
    use crate::preferences::VerificationTime;
    use crate::shell::preferences_dialog::PreferenceChange;
    let (window, _dir) = window_with_settings("rsa-sha256.pdf", cx);
    window
        .update(cx, |frame, _, cx| {
            frame.change_preference(PreferenceChange::VerifyOnOpen(false), cx);
            frame.change_preference(
                PreferenceChange::VerificationTime(VerificationTime::Creation),
                cx,
            );
        })
        .unwrap();
    assert!(statuses(window, cx)[0].contains("Signed, not checked"));
    let validate = described(window, "signature-validate-all", cx);
    assert_eq!(validate, ["Validate All | "]);

    act(window, Activation::Pane(PaneAction::ValidateSignatures), cx);
    assert!(statuses(window, cx)[0].contains("Valid, signer unknown"));
    assert!(described(window, "signature-validate-all", cx).is_empty());
}
