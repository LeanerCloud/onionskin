//! File > Properties' half in the frame: reading the document into the
//! dialog, and writing what Apply changed as one undo step. Also the Layers
//! pane's Layer Properties, which reads what the pane already holds.

use std::path::Path;

use gpui::{Context, Window};
use onionskin_core::metadata::{write_initial_view, write_properties};

use super::ShellFrame;
use crate::shell::canvas::CanvasModel;
use crate::shell::chrome::properties_dialog::{
    date_label, security_rows, size_label, PropertiesAction, PropertiesDialogState,
    PropertiesFacts, PropertiesSource, PropertiesTab,
};
use crate::shell::dialog::ShellDialog;
use crate::shell::initial_view::initial_pane;
use crate::shell::panes::PaneAction;

/// What an absent fact shows.
const NOT_SET: &str = "—";

impl ShellFrame {
    pub(super) fn open_properties_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, canvas)) = self
            .tabs
            .active()
            .map(|tab| (tab.source.clone(), tab.canvas.clone()))
        else {
            return;
        };
        let read = canvas.update(cx, |canvas, _| read_source(&mut canvas.model, &path));
        match read {
            Ok(source) => {
                self.show_dialog(ShellDialog::Properties, window, cx);
                let theme = self.shell_view_state.tokens();
                self.properties = Some(PropertiesDialogState::new(source, theme, cx));
            }
            Err(error) => self.notices.push(error),
        }
        cx.notify();
    }

    pub(in crate::shell) fn properties_dialog(&self) -> Option<&PropertiesDialogState> {
        self.properties.as_ref()
    }

    pub(in crate::shell) fn run_properties_action(
        &mut self,
        action: PropertiesAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.properties.as_mut() else {
            return;
        };
        state.error = None;
        if action == PropertiesAction::Apply {
            self.apply_properties(window, cx);
            cx.notify();
            return;
        }
        match action {
            PropertiesAction::Tab(tab) => state.tab = tab,
            PropertiesAction::Layout(layout) => state.layout = layout,
            PropertiesAction::Mode(mode) => state.mode = mode,
            PropertiesAction::Fit(fit) => state.fit = fit,
            PropertiesAction::AddCustom => state.error = state.add_custom(cx).err(),
            PropertiesAction::RemoveCustom(index) => {
                if index < state.custom.len() {
                    state.custom.remove(index);
                }
            }
            PropertiesAction::Apply => {}
        }
        cx.notify();
    }

    /// Write what the dialog changed, as one step, and close; or say why
    /// not, in the dialog. A dialog nothing was changed in closes without an
    /// edit, so looking at a document's properties never dirties it.
    fn apply_properties(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(state), Some(canvas)) = (
            self.properties.as_ref(),
            self.tabs.active().map(|tab| tab.canvas.clone()),
        ) else {
            return;
        };
        let (edit, view) = match state.changes(cx) {
            Ok(changes) => changes,
            Err(error) => {
                self.set_properties_error(error);
                return;
            }
        };
        if edit.is_none() && view.is_none() {
            self.close_dialog(window, cx);
            return;
        }
        let now = unix_now();
        let written = canvas.update(cx, |canvas, cx| {
            let result = canvas
                .model
                .document_mut()
                .edit_document("Document Properties", |tx| {
                    if let Some(edit) = &edit {
                        write_properties(tx, edit, now)?;
                    }
                    if let Some(view) = &view {
                        write_initial_view(tx, view)?;
                    }
                    Ok(())
                });
            if result.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            result
        });
        match written {
            Ok(()) => self.close_dialog(window, cx),
            Err(error) => self.set_properties_error(sentence(&error.to_string())),
        }
    }

    fn set_properties_error(&mut self, error: String) {
        if let Some(state) = self.properties.as_mut() {
            state.error = Some(error);
        }
    }

    /// Open the navigation pane the active document asks to open with.
    pub(super) fn open_initial_pane(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let pane = canvas.update(cx, |canvas, _| initial_pane(&mut canvas.model));
        if let Some(pane) = pane.filter(|pane| self.navigation.active() != Some(*pane)) {
            self.run_pane_action(PaneAction::Select(pane), cx);
        }
    }

    /// Layer Properties: each layer, what it shows now, and whether the
    /// document lets the user change that. Read-only: a layer's name and
    /// intent are layer editing, which parity row 204 puts after 1.0.
    pub(in crate::shell) fn layer_property_rows(&self) -> Vec<(String, String)> {
        let layers = self.navigation.layers().unwrap_or_default();
        let mut rows: Vec<(String, String)> = layers
            .iter()
            .map(|layer| {
                let name = if layer.name.is_empty() {
                    "(unnamed layer)".to_owned()
                } else {
                    layer.name.clone()
                };
                let shown = if layer.visible { "Visible" } else { "Hidden" };
                let locked = if layer.locked {
                    ", visibility locked by the document"
                } else {
                    ""
                };
                (name, format!("{shown}{locked}"))
            })
            .collect();
        if rows.is_empty() {
            rows.push(("This document has no layers.".to_owned(), String::new()));
        }
        rows.push((
            "Renaming a layer or changing its intent".to_owned(),
            "arrives after 1.0, with layer editing".to_owned(),
        ));
        rows
    }
}

/// Everything the dialog shows, read once when it opens.
fn read_source(model: &mut CanvasModel, path: &Path) -> Result<PropertiesSource, String> {
    let page_count = model.view_state().page_count;
    let edit_refusal = model.edit_refusal();
    let failed = |error: onionskin_core::Error| {
        format!("{}'s properties could not be read: {error}", path.display())
    };
    let mut document = model.document_mut();
    let info = document.info().map_err(failed)?;
    let view = document.initial_view().map_err(failed)?;
    let fonts = document
        .fonts()
        .map_err(|error| sentence(&error.to_string()));
    let security = security_rows(
        &document.security_facts(),
        document.security_refusal().map(|refusal| refusal.reason()),
    );
    let text = |value: Option<&str>| value.map_or_else(|| NOT_SET.to_owned(), str::to_owned);
    let date = |value: Option<&str>| value.map_or_else(|| NOT_SET.to_owned(), date_label);
    let file = vec![
        (
            "Location",
            path.parent()
                .map_or_else(|| NOT_SET.to_owned(), |dir| dir.display().to_string()),
        ),
        (
            "File Size",
            std::fs::metadata(path)
                .map_or_else(|_| NOT_SET.to_owned(), |meta| size_label(meta.len())),
        ),
        ("Pages", page_count.to_string()),
        ("Created", date(info.created.as_deref())),
        ("Modified", date(info.modified.as_deref())),
        ("Application", text(info.creator.as_deref())),
        ("PDF Producer", text(info.producer.as_deref())),
    ];
    Ok(PropertiesSource {
        tab: PropertiesTab::Description,
        description: info.description,
        custom: info.custom,
        view,
        facts: PropertiesFacts {
            file,
            security,
            fonts,
            page_count,
            edit_refusal,
        },
    })
}

/// A reason as the dialog prints it: a sentence, capitalised.
pub(super) fn sentence(reason: &str) -> String {
    let mut chars = reason.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}
