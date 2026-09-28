//! File > Properties' half in the frame: reading the document into the
//! dialog, and writing what Apply changed as one undo step. Also the Layers
//! pane's Layer Properties, which reads what the pane already holds.

use std::path::Path;

use gpui::{Context, Window};
use onionskin_core::metadata::{write_initial_view, write_properties};

use super::ShellFrame;
use crate::shell::canvas::CanvasModel;
use crate::shell::chrome::layer_properties_dialog::{
    LayerPropertiesAction, LayerPropertiesState, NO_LAYERS,
};
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
    /// The Security Settings pane's Permission Details: Document
    /// Properties, on its Security tab.
    pub(super) fn show_permission_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_properties_dialog(window, cx);
        if let Some(state) = self.properties.as_mut() {
            state.tab = PropertiesTab::Security;
        }
        cx.notify();
    }

    pub(super) fn open_properties_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((title, path, canvas)) = self
            .tabs
            .active()
            .map(|tab| (tab.title().to_owned(), tab.path(cx), tab.canvas.clone()))
        else {
            return;
        };
        let read = canvas.update(cx, |canvas, _| {
            read_source(&mut canvas.model, &title, path.as_deref())
        });
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

    pub(in crate::shell) fn layer_properties(&self) -> Option<&LayerPropertiesState> {
        self.layer_properties.as_ref()
    }

    /// The Layers pane's Layer Properties, on the first layer, at the
    /// file's defaults.
    pub(super) fn open_layer_properties(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let (layers, refusal) = canvas.update(cx, |canvas, _| {
            (canvas.model.layer_defaults(), canvas.model.edit_refusal())
        });
        match layers {
            Ok(layers) => {
                self.show_dialog(ShellDialog::LayerProperties, window, cx);
                let theme = self.shell_view_state.tokens();
                self.layer_properties = Some(LayerPropertiesState::new(layers, refusal, theme, cx));
            }
            Err(error) => self.notices.push(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn run_layer_properties_action(
        &mut self,
        action: LayerPropertiesAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut state) = self.layer_properties.take() else {
            return;
        };
        match action {
            LayerPropertiesAction::Choose(index) => state.choose(index, cx),
            LayerPropertiesAction::Intent(intent) => state.intent = intent,
            LayerPropertiesAction::DefaultOn(on) => state.default_on = on,
            LayerPropertiesAction::Apply => {
                if let Err(error) = self.apply_layer_properties(&state, cx) {
                    state.error = Some(error);
                } else {
                    self.close_dialog(window, cx);
                    return;
                }
            }
        }
        self.layer_properties = Some(state);
        cx.notify();
    }

    /// Write the chosen layer's properties and read the pane again.
    fn apply_layer_properties(
        &mut self,
        state: &LayerPropertiesState,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let properties = state.properties(cx)?;
        let layer = state.chosen_layer().ok_or(NO_LAYERS)?.id;
        let canvas = self.active_canvas().cloned().ok_or(NO_LAYERS)?;
        canvas
            .update(cx, |canvas, _| {
                canvas.model.set_layer_properties(layer, &properties)
            })
            .map_err(|error| error.to_string())?;
        self.navigation.reread(&canvas, cx);
        Ok(())
    }
}

/// Everything the dialog shows, read once when it opens.
fn read_source(
    model: &mut CanvasModel,
    title: &str,
    path: Option<&Path>,
) -> Result<PropertiesSource, String> {
    let page_count = model.view_state().page_count;
    let edit_refusal = model.edit_refusal();
    let failed =
        |error: onionskin_core::Error| format!("{title}'s properties could not be read: {error}");
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
    let document_facts = document.document_facts();
    let file = vec![
        (
            "Location",
            path.and_then(Path::parent)
                .map_or_else(|| NOT_SET.to_owned(), |dir| dir.display().to_string()),
        ),
        (
            "File Size",
            path.and_then(|path| std::fs::metadata(path).ok())
                .map_or_else(|| NOT_SET.to_owned(), |meta| size_label(meta.len())),
        ),
        ("Pages", page_count.to_string()),
        ("PDF Version", text(document_facts.version.as_deref())),
        ("Page Size", text(document_facts.page_size.as_deref())),
        ("Tagged", yes_no(document_facts.tagged)),
        ("Fast Web View", yes_no(document_facts.linearized)),
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

/// A fact the file either states or does not, which is how Acrobat words
/// these rather than as a value that might be missing.
fn yes_no(value: bool) -> String {
    if value { "Yes" } else { "No" }.to_owned()
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
