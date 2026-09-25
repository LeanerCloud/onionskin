//! Signature Properties from the Signatures pane, and View Signed Version.
//!
//! The signed version is the file cut where the signature's byte range
//! ends. It is written to its own file under the temporary directory and
//! opened in a new tab, so the document open now is never touched.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use onionskin_core::signatures::Certificate;

use super::ShellFrame;
use crate::shell::chrome::signature_properties::SignaturePropertiesAction;
use crate::shell::dialog::ShellDialog;
use crate::shell::panes::signatures::SignatureRow;

/// Said when the signed version cannot be read out of the file.
pub(in crate::shell) const NO_SIGNED_VERSION: &str =
    "The signed version of this document could not be found.";

/// Said when a file chosen to trust holds no certificate.
pub(in crate::shell) const NO_CERTIFICATE: &str =
    "The file holds no certificate: choose a .pem, .crt, .cer or .der certificate file.";

/// Where a signed version of `source`, signed on `field`, is written.
pub(in crate::shell) fn signed_version_path(
    directory: &Path,
    source: &Path,
    field: &str,
) -> PathBuf {
    let stem = source
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    let field: String = field
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    directory.join(format!("{stem} (signed version, {field}).pdf"))
}

impl ShellFrame {
    pub(in crate::shell) fn trusted_certificates(
        &self,
    ) -> &[onionskin_core::signatures::TrustAnchor] {
        self.settings.trusted.anchors()
    }

    pub(in crate::shell) fn signature_properties(&self) -> Option<&SignatureRow> {
        self.signature_properties.as_ref()
    }

    /// Open Signature Properties on the pane's row `index`.
    pub(super) fn show_signature_properties(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let trust = self.settings.signature_trust();
        let rows = canvas.update(cx, |canvas, _| canvas.model.signatures(&trust));
        match rows.map(|mut rows| (index < rows.len()).then(|| rows.swap_remove(index))) {
            Ok(Some(row)) => {
                self.show_dialog(ShellDialog::SignatureProperties, window, cx);
                self.signature_properties = Some(row);
            }
            Ok(None) => {}
            Err(error) => self.notices.push(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn run_signature_properties_action(
        &mut self,
        action: SignaturePropertiesAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.signature_properties.clone() else {
            return;
        };
        self.close_dialog(window, cx);
        match action {
            SignaturePropertiesAction::ViewSignedVersion => {
                self.view_signed_version(&row.field.name, cx)
            }
            SignaturePropertiesAction::AddToTrusted => {
                let signer = row
                    .validation
                    .and_then(|validation| validation.check.ok())
                    .and_then(|check| check.signer);
                self.trust_certificates(signer.into_iter().collect(), cx);
            }
            SignaturePropertiesAction::Close => {}
        }
    }

    /// Trust `certificates` for approval signatures, keep the list, and
    /// judge every signer again.
    pub(in crate::shell) fn trust_certificates(
        &mut self,
        certificates: Vec<Certificate>,
        cx: &mut Context<Self>,
    ) {
        if certificates.is_empty() {
            self.notices.push(NO_CERTIFICATE.to_owned());
        } else if self.settings.trusted.add(certificates) > 0 {
            self.save_trusted_certificates();
        }
        self.apply_signature_trust(cx);
    }

    /// Preferences > Signatures' Add Certificate: every certificate in the
    /// file at `path`.
    pub(in crate::shell) fn trust_certificates_in(&mut self, path: &Path, cx: &mut Context<Self>) {
        match std::fs::read(path) {
            Ok(bytes) => self.trust_certificates(Certificate::from_file_bytes(&bytes), cx),
            Err(error) => {
                self.notices
                    .push(format!("{} could not be read: {error}", path.display()));
                cx.notify();
            }
        }
    }

    /// Ask which certificate file to trust.
    pub(in crate::shell) fn prompt_for_trusted_certificate(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add Certificate".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.trust_certificates_in(&path, cx))
                .ok();
        })
        .detach();
    }

    pub(in crate::shell) fn save_trusted_certificates(&mut self) {
        if let Some(path) = self.settings.paths.trusted_certificates.as_deref() {
            if let Err(error) = self.settings.trusted.save(path) {
                self.notices.push(error);
            }
        }
    }

    /// Hand the pane what signers are judged by now, and read it again.
    pub(in crate::shell) fn apply_signature_trust(&mut self, cx: &mut Context<Self>) {
        self.navigation
            .set_signature_trust(self.settings.signature_trust());
        if let Some(canvas) = self.active_canvas_entity() {
            self.navigation.reread(&canvas, cx);
        }
        cx.notify();
    }

    /// Write the version `field`'s signature signed and open it.
    fn view_signed_version(&mut self, field: &str, cx: &mut Context<Self>) {
        let Some((title, canvas)) = self
            .tabs
            .active()
            .map(|tab| (tab.title().to_owned(), tab.canvas.clone()))
        else {
            return;
        };
        let bytes = canvas.update(cx, |canvas, _| canvas.model.signed_version(field));
        let Some(bytes) = bytes else {
            self.notices.push(NO_SIGNED_VERSION.to_owned());
            return;
        };
        let directory = std::env::temp_dir().join("onionskin-signed-versions");
        let path = signed_version_path(&directory, Path::new(&title), field);
        let written =
            std::fs::create_dir_all(&directory).and_then(|()| std::fs::write(&path, bytes));
        match written {
            Ok(()) => self.open_documents(&[path], cx),
            Err(error) => self
                .notices
                .push(format!("The signed version could not be written: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_version_is_named_for_its_document_and_field() {
        let path = signed_version_path(
            Path::new("/tmp/x"),
            Path::new("/docs/Contract.pdf"),
            "Sig 1/a",
        );
        assert_eq!(
            path,
            Path::new("/tmp/x/Contract (signed version, Sig_1_a).pdf")
        );
        let unnamed = signed_version_path(Path::new("/t"), Path::new(""), "S");
        assert_eq!(unnamed, Path::new("/t/Document (signed version, S).pdf"));
    }
}
