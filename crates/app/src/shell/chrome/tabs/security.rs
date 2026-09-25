//! A document's security in the frame: asking for the password an encrypted
//! document opens with, Protect Using Password, and Remove Security.

use gpui::{AppContext as _, Context, Focusable as _, Window};

use super::ShellFrame;
use crate::shell::chrome::password_dialog::{PasswordAction, PasswordPrompt, PASSWORD_ID};
use crate::shell::chrome::protect_dialog::{
    Choices, ProtectAction, ProtectState, OPEN_ID, PERMISSIONS_ID,
};
use crate::shell::chrome::SearchInput;
use crate::shell::dialog::ShellDialog;

/// What Remove Security says on a document with none.
pub(in crate::shell) const NO_SECURITY: &str = "The document has no security to remove";

impl ShellFrame {
    pub(in crate::shell) fn password_prompt(&self) -> Option<&PasswordPrompt> {
        self.password_prompt.as_ref()
    }

    pub(in crate::shell) fn protect_dialog(&self) -> Option<&ProtectState> {
        self.protect.as_ref()
    }

    /// A masked field, focused.
    fn password_field(
        &self,
        id: &'static str,
        placeholder: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<SearchInput> {
        let theme = self.shell_view_state.tokens();
        cx.new(|cx| {
            let mut input = SearchInput::with_placeholder(id, placeholder, theme, cx);
            input.set_masked(true);
            input
        })
    }

    /// Ask for the next queued document's password, once nothing else is
    /// being asked.
    pub(super) fn ask_next_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        let Some(target) = self.pending_passwords.pop_front() else {
            return;
        };
        self.show_dialog(ShellDialog::DocumentPassword, window, cx);
        let input = self.password_field(PASSWORD_ID, "Password", cx);
        window.focus(&input.focus_handle(cx));
        self.password_prompt = Some(PasswordPrompt {
            target,
            input,
            wrong: false,
        });
    }

    pub(in crate::shell) fn run_password_action(
        &mut self,
        action: PasswordAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self.password_prompt.as_mut() else {
            return;
        };
        if action == PasswordAction::Cancel {
            self.close_dialog(window, cx);
            return;
        }
        let password = prompt.input.read(cx).query().to_owned();
        let target = prompt.target.clone();
        match self.open_with_password(&target, &password, cx) {
            Ok(()) => self.close_dialog(window, cx),
            Err(None) => {
                if let Some(prompt) = self.password_prompt.as_mut() {
                    prompt.wrong = true;
                    prompt
                        .input
                        .update(cx, |input, cx| input.set_query(String::new(), cx));
                }
                cx.notify();
            }
            Err(Some(failure)) => {
                self.notices.push(failure);
                self.close_dialog(window, cx);
            }
        }
    }

    pub(super) fn open_protect_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        if let Some(refusal) = canvas.read(cx).model.security_refusal() {
            self.notices.push(refusal.to_owned());
            cx.notify();
            return;
        }
        self.show_dialog(ShellDialog::Protect, window, cx);
        let open = self.password_field(OPEN_ID, "Password", cx);
        let permissions = self.password_field(PERMISSIONS_ID, "Password", cx);
        self.protect = Some(ProtectState {
            choices: Choices::default(),
            open,
            permissions,
            error: None,
        });
    }

    pub(in crate::shell) fn run_protect_action(
        &mut self,
        action: ProtectAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.protect.as_mut() else {
            return;
        };
        state.error = None;
        state.choices.apply(action);
        if action != ProtectAction::Apply {
            cx.notify();
            return;
        }
        let open = state.open.read(cx).query().to_owned();
        let permissions = state.permissions.read(cx).query().to_owned();
        let protection = match state.choices.protection(&open, &permissions) {
            Ok(protection) => protection,
            Err(reason) => {
                state.error = Some(reason.to_owned());
                cx.notify();
                return;
            }
        };
        match self.save_security(Some(&protection), cx) {
            Ok(()) => {
                self.notices
                    .push("Security applied: the document was saved with it".to_owned());
                self.close_dialog(window, cx);
            }
            Err(error) => {
                if let Some(state) = self.protect.as_mut() {
                    state.error = Some(error);
                }
                cx.notify();
            }
        }
    }

    /// Remove Security: save the document with none.
    pub(super) fn remove_security(&mut self, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        let encrypted = self
            .active_canvas()
            .is_some_and(|canvas| canvas.read(cx).model.security_facts().level.is_some());
        let outcome = if encrypted {
            self.save_security(None, cx)
                .map(|()| "Security removed: the document was saved without it".to_owned())
        } else {
            Err(NO_SECURITY.to_owned())
        };
        self.notices.push(outcome.unwrap_or_else(|error| error));
        cx.notify();
    }

    fn save_security(
        &mut self,
        protection: Option<&onionskin_core::security::Protection>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let canvas = self.active_canvas().cloned().ok_or("No document is open")?;
        canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.save_with_security(protection);
            canvas.handle_change(Ok(true), cx);
            outcome.map_err(|error| {
                super::properties::sentence(&format!("The document was not saved: {error}"))
            })
        })
    }
}
