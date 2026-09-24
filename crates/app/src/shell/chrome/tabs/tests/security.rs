//! A document's security on a real window: the password an encrypted
//! document asks for as it opens, Protect Using Password, Remove Security,
//! and the entries that say why they are off.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::password_dialog::PasswordAction;
use crate::shell::chrome::protect_dialog::{Changes, ProtectAction};
use crate::shell::chrome::SearchInput;
use crate::shell::dialog::ShellDialog;

fn fixture(name: &str) -> PathBuf {
    onionskin_corpus_testing::encrypted_fixture(name)
}

fn type_into(input: &Entity<SearchInput>, text: &str, cx: &mut App) {
    input.update(cx, |input, cx| input.set_query(text.to_owned(), cx));
}

fn tab_count(frame: &ShellFrame) -> usize {
    frame.tabs.tabs().len()
}

#[gpui::test]
fn a_protected_document_asks_for_its_password_and_opens_with_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("locked.pdf");
    std::fs::copy(fixture("r6-aes-256-user-password.pdf"), &path).expect("copies");
    let (window, _) = bound_window(&["hello.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            let before = tab_count(frame);
            frame.open_documents(std::slice::from_ref(&path), cx);
            assert_eq!(tab_count(frame), before, "not without its password");
            frame.ask_next_password(window, cx);
            let prompt = frame.password_prompt().expect("asked");
            assert!(prompt.message().contains("locked.pdf"));
            let input = prompt.input.clone();
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
            let tree = frame.accessible(window, cx);
            let field = tree.find(&"document-password".into()).expect("the field");
            assert_eq!(field.role, accesskit::Role::PasswordInput);

            type_into(&input, "guess", cx);
            frame.run_activation(Activation::Password(PasswordAction::Open), window, cx);
            let prompt = frame.password_prompt().expect("still asking");
            assert!(prompt.wrong);
            assert_eq!(prompt.input.read(cx).query(), "", "the field is cleared");
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"document-password-wrong".into()).is_some());

            type_into(&input, "secret", cx);
            frame.run_activation(Activation::Password(PasswordAction::Open), window, cx);
            assert!(frame.password_prompt().is_none());
            assert_eq!(tab_count(frame), before + 1, "open");
            assert_eq!(
                frame
                    .settings
                    .recents
                    .get(0)
                    .map(|recent| recent.path.clone()),
                std::path::absolute(&path).ok()
            );

            frame.pending_passwords.push_back(path.clone());
            frame.ask_next_password(window, cx);
            frame.run_activation(Activation::Password(PasswordAction::Cancel), window, cx);
            assert!(frame.password_prompt().is_none(), "Cancel leaves it closed");
        })
        .unwrap();
}

#[gpui::test]
fn a_document_is_protected_and_its_security_removed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("plan.pdf");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf"),
        &path,
    )
    .expect("copies");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("builds");
    let (window, _) = bound_window_with_models(
        vec![(path.clone(), model)],
        crate::config::ConfigPaths::default(),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(
                frame.command_unavailable(MenuCommand::RemoveSecurity, cx),
                Some(crate::shell::chrome::tabs::NO_SECURITY)
            );
            assert_eq!(
                frame.command_unavailable(MenuCommand::ProtectWithPassword, cx),
                None
            );
            frame
                .run_main_menu_command(MenuCommand::ProtectWithPassword, window, cx)
                .expect("opens");
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"protect-require-open".into()).is_some());
            assert!(
                tree.find(&"protect-open-password".into()).is_none(),
                "not until asked"
            );

            frame.run_activation(Activation::Protect(ProtectAction::Apply), window, cx);
            assert!(frame
                .protect_dialog()
                .and_then(|state| state.error.clone())
                .is_some());

            for action in [
                ProtectAction::RequireOpen,
                ProtectAction::Restrict,
                ProtectAction::Changes(Changes::Comments),
            ] {
                frame.run_activation(Activation::Protect(action), window, cx);
            }
            let tree = frame.accessible(window, cx);
            let restrict = tree.find(&"protect-restrict".into()).expect("listed");
            assert_eq!(restrict.state.toggled, Some(true));
            let state = frame.protect_dialog().expect("open");
            let (open, permissions) = (state.open.clone(), state.permissions.clone());
            type_into(&open, "reader", cx);
            type_into(&permissions, "author", cx);
            frame.run_activation(Activation::Protect(ProtectAction::Apply), window, cx);
            assert!(frame.protect_dialog().is_none(), "{:?}", frame.notices);
            assert!(frame
                .notices
                .last()
                .unwrap()
                .starts_with("Security applied"));
            assert_eq!(
                frame.command_unavailable(MenuCommand::RemoveSecurity, cx),
                None
            );
        })
        .unwrap();

    assert!(
        Document::open_path(&path).is_err(),
        "it needs a password now"
    );
    let reader = Document::open_path_with_password(&path, "reader").expect("opens");
    assert!(reader.edit_refusal().is_some() && reader.permitted().annotate());

    window
        .update(cx, |frame, _, cx| {
            frame.remove_security(cx);
            assert!(frame
                .notices
                .last()
                .unwrap()
                .starts_with("Security removed"));
        })
        .unwrap();
    assert!(Document::open_path(&path).is_ok(), "no password now");
}

#[gpui::test]
fn a_user_cannot_change_the_security_a_permissions_password_set(cx: &mut TestAppContext) {
    let bytes = std::fs::read(fixture("r6-aes-256-print-only.pdf")).expect("reads");
    let (window, _) = bound_window_from_bytes(vec![("restricted.pdf", bytes)], cx);
    window
        .update(cx, |frame, window, cx| {
            let reason = onionskin_core::protection::Refusal::ChangeSecurity.reason();
            assert_eq!(
                frame.command_unavailable(MenuCommand::ProtectWithPassword, cx),
                Some(reason)
            );
            assert_eq!(
                frame.command_unavailable(MenuCommand::RemoveSecurity, cx),
                Some(reason)
            );
            frame.open_protect_dialog(window, cx);
            assert!(frame.protect_dialog().is_none());
            assert_eq!(frame.notices.last().map(String::as_str), Some(reason));
        })
        .unwrap();
}

/// The labels under `prefix` in the accessibility tree.
fn labels_under(
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
                .map(|element| element.label.clone())
                .collect()
        })
        .unwrap()
}

#[gpui::test]
fn the_security_settings_pane_shows_only_on_a_secured_document(cx: &mut TestAppContext) {
    use crate::shell::panes::{NavigationPane, PaneAction};
    let (plain, _) = bound_window(&["hello.pdf"], cx);
    assert!(labels_under(plain, "pane-security-settings", cx).is_empty());

    let bytes = std::fs::read(fixture("r6-aes-256-print-only.pdf")).expect("reads");
    let (window, _) = bound_window_from_bytes(vec![("restricted.pdf", bytes)], cx);
    assert_eq!(
        labels_under(window, "pane-security-settings", cx),
        ["Security Settings"]
    );
    window
        .update(cx, |frame, _, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::SecuritySettings), cx)
        })
        .unwrap();
    let rows = labels_under(window, "security-", cx);
    assert!(
        rows.iter()
            .any(|row| row == "Security Method: Password Security"),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row == "Encryption Level: 256-bit AES"),
        "{rows:?}"
    );

    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::ShowPermissionDetails, window, cx);
            assert_eq!(frame.dialog, Some(ShellDialog::Properties));
            assert_eq!(
                frame.properties.as_ref().map(|state| state.tab),
                Some(crate::shell::chrome::properties_dialog::PropertiesTab::Security)
            );
        })
        .unwrap();
}
