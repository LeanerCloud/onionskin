//! The frame's half of the comment properties inspector: which comment it
//! shows, and what its controls do to the document and the preferences.

use gpui::{Context, IntoElement as _};
use onionskin_core::properties::{set_properties, CommentProperties};
use onionskin_core::ReadAnnotation;

use super::ShellFrame;
use crate::preferences::CommentDefault;
use crate::shell::chrome::accessible::Element;
use crate::shell::chrome::inspector::{
    self, current, from_rgb, opacity_percent, to_rgb, InspectorAction, PALETTE,
};
use crate::shell::chrome::ThemeTokens;

/// Seconds since the epoch, which `/M` is written from.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

impl ShellFrame {
    /// The comment the inspector shows: the one chosen in the Comments pane.
    fn inspected(&self) -> Option<ReadAnnotation> {
        self.navigation.chosen_comment().cloned()
    }

    /// After a pane action: fill the inspector from the comment now chosen,
    /// if the choice changed.
    pub(super) fn follow_chosen_comment(&mut self, cx: &mut Context<Self>) {
        if let Some(comment) = self.inspected() {
            self.sync_inspector(&comment, cx);
        }
    }

    /// Fill the author and subject fields from the chosen comment, once per
    /// choice, so a redraw never overwrites what is being typed.
    fn sync_inspector(&mut self, comment: &ReadAnnotation, cx: &mut Context<Self>) {
        if self.inspector.filled_from == Some(comment.objref) {
            return;
        }
        self.inspector.filled_from = Some(comment.objref);
        let author = comment.author.clone().unwrap_or_default();
        let subject = comment.subject.clone().unwrap_or_default();
        self.inspector
            .author
            .update(cx, |input, cx| input.set_query(author, cx));
        self.inspector
            .subject
            .update(cx, |input, cx| input.set_query(subject, cx));
    }

    pub(super) fn render_inspector(
        &mut self,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let comment = self.inspected()?;
        self.sync_inspector(&comment, cx);
        let refusal = self.navigation.edit_refusal();
        Some(inspector::render(&comment, &self.inspector, refusal, theme, cx).into_any_element())
    }

    pub(super) fn accessible_inspector(&self, cx: &gpui::App) -> Option<Vec<Element>> {
        let comment = self.inspected()?;
        Some(inspector::accessible(
            &comment,
            &self.inspector,
            self.navigation.edit_refusal(),
            cx,
        ))
    }

    pub(in crate::shell) fn run_inspector_action(
        &mut self,
        action: InspectorAction,
        cx: &mut Context<Self>,
    ) {
        match action {
            InspectorAction::Show => {
                if !self.side_panel_state.is_open() {
                    self.side_panel_state.toggle();
                }
            }
            InspectorAction::Color(index) => {
                if let Some((_, rgb)) = PALETTE.get(index) {
                    let color = from_rgb(*rgb);
                    self.edit_inspected(cx, |properties| properties.color = Some(color));
                }
            }
            InspectorAction::Opacity(percent) => {
                self.edit_inspected(cx, |properties| {
                    properties.opacity = f64::from(percent) / 100.0;
                });
            }
            InspectorAction::SaveText => {
                let author = self.inspector.author.read(cx).query().to_owned();
                let subject = self.inspector.subject.read(cx).query().to_owned();
                self.edit_inspected(cx, |properties| {
                    properties.author = Some(author);
                    properties.subject = Some(subject);
                });
            }
            InspectorAction::MakeDefault => self.make_inspected_default(cx),
        }
        cx.notify();
    }

    /// Change the inspected comment's properties as one undoable edit, and
    /// read the Comments pane again.
    fn edit_inspected(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut CommentProperties),
    ) {
        let (Some(comment), Some(canvas)) = (
            self.inspected(),
            self.tabs.active().map(|tab| tab.canvas.clone()),
        ) else {
            return;
        };
        let mut properties = current(&comment);
        change(&mut properties);
        let result = canvas.update(cx, |canvas, cx| {
            let result = canvas
                .model
                .document_mut()
                .edit_document("Comment Properties", |tx| {
                    set_properties(tx, comment.objref, &properties, now())
                });
            if result.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            result
        });
        match result {
            Ok(()) => {
                self.navigation.report(None);
                self.navigation.reread(&canvas, cx);
            }
            Err(error) => self.navigation.report(Some(error.to_string())),
        }
    }

    /// "Make Current Properties Default": the inspected comment's colour and
    /// opacity become the look of the next comment of its kind, in every tab.
    fn make_inspected_default(&mut self, cx: &mut Context<Self>) {
        let Some(comment) = self.inspected() else {
            return;
        };
        self.settings.preferences.comment_defaults.insert(
            comment.raw_subtype.clone(),
            CommentDefault {
                color: comment.color.map(to_rgb),
                opacity_percent: opacity_percent(&comment),
            },
        );
        self.apply_tool_environment(cx);
        self.save_preferences();
    }
}
