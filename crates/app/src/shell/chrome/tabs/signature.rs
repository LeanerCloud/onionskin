//! The frame's half of Add Signature and Add Initials: opening the dialog,
//! drawing on its pad, saving what it makes into the signature library, and
//! choosing the Sign tool to place it.

use super::ShellFrame;

/// What Add Signature says in a build with no tool that places one.
pub(in crate::shell) const NO_SIGN_TOOL: &str = "The Fill & Sign plugin is not installed";

#[cfg(not(feature = "tools-fill-sign"))]
impl ShellFrame {
    /// Disabled without the plugin, saying so; reached some other way, it
    /// says so too.
    pub(super) fn open_signature_dialog(
        &mut self,
        _initials: bool,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.notices.push(NO_SIGN_TOOL.to_owned());
        cx.notify();
    }
}

#[cfg(feature = "tools-fill-sign")]
mod dialog {
    use std::path::{Path, PathBuf};

    use gpui::{Context, PathPromptOptions, Window};
    use onionskin_plugin_api::{tool_with, ToolCapability};
    use onionskin_tools_fill_sign::signature::{SignatureKind, SignatureLibrary};

    use super::super::properties::sentence;
    use super::ShellFrame;
    use crate::shell::chrome::signature_dialog::{
        kind, Made, PadEvent, SignatureAction, SignatureDialogState, SignatureForm,
    };
    use crate::shell::dialog::ShellDialog;

    const NO_FOLDER: &str = "There is no folder to keep signatures in";

    fn library(data: &Option<PathBuf>) -> Result<SignatureLibrary, String> {
        data.as_deref()
            .map(SignatureLibrary::in_data_dir)
            .ok_or_else(|| NO_FOLDER.to_owned())
    }

    /// The page an image or PDF file makes: a PDF's own first page, or an
    /// image made into one.
    fn page_of(file: &Path) -> Result<Vec<u8>, String> {
        let bytes = std::fs::read(file)
            .map_err(|error| format!("{} could not be read: {error}", file.display()))?;
        if bytes.starts_with(b"%PDF") {
            Ok(bytes)
        } else {
            super::super::create::import(&bytes)
        }
    }

    /// What saving says it did.
    fn saved_said(kind: SignatureKind) -> String {
        format!(
            "Saved your {}. Click where it goes.",
            kind.label().to_lowercase()
        )
    }

    impl ShellFrame {
        /// Edit > Add Signature, or Add Initials.
        pub(in crate::shell::chrome::tabs) fn open_signature_dialog(
            &mut self,
            initials: bool,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let kind = kind(initials);
            let saved = library(&self.settings.paths.data)
                .ok()
                .is_some_and(|library| library.get(kind).is_some());
            self.show_dialog(ShellDialog::Signature { initials }, window, cx);
            let theme = self.shell_view_state.tokens();
            self.signature = Some(SignatureDialogState::new(
                SignatureForm::new(kind, saved),
                theme,
                cx,
            ));
        }

        pub(in crate::shell) fn signature_dialog(&self) -> Option<&SignatureDialogState> {
            self.signature.as_ref()
        }

        pub(in crate::shell::chrome::tabs) fn run_signature_action(
            &mut self,
            action: SignatureAction,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let Some(state) = self.signature.as_mut() else {
                return;
            };
            state.error = None;
            match action {
                SignatureAction::Save => self.save_signature(window, cx),
                SignatureAction::ClearSaved => self.clear_saved_signature(cx),
                SignatureAction::ChooseImage => self.prompt_for_signature_image(cx),
                _ => state.form.apply(action),
            }
            cx.notify();
        }

        /// A pointer event on the drawing pad.
        pub(in crate::shell) fn signature_pad(&mut self, event: PadEvent, cx: &mut Context<Self>) {
            let Some(state) = self.signature.as_mut() else {
                return;
            };
            match event {
                PadEvent::Down(at) => {
                    if let Some(at) = state.on_pad(at) {
                        state.form.pad_down(at);
                    }
                }
                PadEvent::Move(at) => {
                    if let Some(at) = state.on_pad(at) {
                        state.form.pad_move(at);
                    }
                }
                PadEvent::Up => state.form.pad_up(),
            }
            cx.notify();
        }

        fn save_signature(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            let Some(state) = self.signature.as_ref() else {
                return;
            };
            let kind = state.form.kind;
            let saved = state
                .request(cx)
                .and_then(|made| match made {
                    Made::Page(pdf) => Ok(pdf),
                    Made::File(file) => page_of(&file),
                })
                .and_then(|pdf| {
                    library(&self.settings.paths.data)?
                        .save(kind, &pdf)
                        .map_err(|error| sentence(&error.to_string()))
                });
            match saved {
                Ok(()) => {
                    self.close_dialog(window, cx);
                    let said = match self.choose_sign_tool(kind, cx) {
                        Ok(()) => saved_said(kind),
                        Err(error) => error,
                    };
                    self.notices.push(said);
                }
                Err(error) => {
                    if let Some(state) = self.signature.as_mut() {
                        state.error = Some(error);
                    }
                }
            }
        }

        fn clear_saved_signature(&mut self, cx: &mut Context<Self>) {
            let Some(state) = self.signature.as_mut() else {
                return;
            };
            let kind = state.form.kind;
            match library(&self.settings.paths.data)
                .and_then(|library| library.clear(kind).map_err(|error| error.to_string()))
            {
                Ok(()) => {
                    state.form.saved = false;
                    self.notices.push(format!(
                        "Cleared your saved {}.",
                        kind.label().to_lowercase()
                    ));
                }
                Err(error) => state.error = Some(error),
            }
            cx.notify();
        }

        /// Choose `kind` on the Sign tool and switch to it.
        fn choose_sign_tool(
            &mut self,
            kind: SignatureKind,
            cx: &mut Context<Self>,
        ) -> Result<(), String> {
            let canvas = self.active_canvas().cloned().ok_or("No document is open")?;
            let index = tool_with(
                canvas.read(cx).model.registry(),
                ToolCapability::AddSignature,
            )
            .ok_or(super::NO_SIGN_TOOL)?;
            canvas.update(cx, |canvas, _| canvas.model.choose_tool(index, kind.id()));
            let entry = self.active_rail_entry(index, cx);
            self.activate_canvas_tool(index, "Sign Yourself", entry, cx);
            Ok(())
        }

        fn prompt_for_signature_image(&mut self, cx: &mut Context<Self>) {
            let chosen = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Choose".into()),
            });
            cx.spawn(async move |frame, cx| {
                let Ok(Ok(Some(paths))) = chosen.await else {
                    return;
                };
                frame
                    .update(cx, |frame, cx| {
                        frame.take_signature_image(paths.into_iter().next());
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }

        /// What the image prompt chose, into the form.
        pub(in crate::shell::chrome::tabs) fn take_signature_image(
            &mut self,
            file: Option<PathBuf>,
        ) {
            if let Some(state) = self.signature.as_mut() {
                state.form.image = file;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn saving_says_what_was_saved_and_a_missing_file_is_named() {
            assert_eq!(
                saved_said(SignatureKind::Initials),
                "Saved your initials. Click where it goes."
            );
            assert_eq!(library(&None).unwrap_err(), NO_FOLDER);
            let error = page_of(Path::new("/no/such/sig.png")).unwrap_err();
            assert!(
                error.starts_with("/no/such/sig.png could not be read"),
                "{error}"
            );
        }
    }
}
