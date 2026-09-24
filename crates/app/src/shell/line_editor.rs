//! Editing a line of text where it is: the editor the Edit Text tool opens
//! over the line it was clicked on, holding what the line says.
//!
//! Enter keeps what was typed, Escape leaves the line as it was, and a
//! press anywhere else on the canvas keeps it, as leaving the line in
//! Acrobat does. A change the document refuses (a character no font can
//! draw, a line that changed meanwhile) is said on the canvas's status
//! line, and the line stays as it was.

use gpui::{
    actions, div, px, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, ParentElement as _, Styled as _, Window,
};

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::StatefulInteractiveElement as _;
use onionskin_core::{TextEditRequest, ViewRect, ViewSize};

use super::chrome::accessible::{Activation, Element as A11yElement, Rects, TextField};
use super::chrome::{SearchInput, ThemeTokens};
use super::line_style::{Picked, StyleChoice, StyleList};
use super::Canvas;
use crate::a11y::State as A11yState;

actions!(onionskin_line_editor, [CommitLine, CancelLine]);

const LINE_KEY_CONTEXT: &str = "OnionskinLineEditor";

/// The id the editor's text box publishes.
pub(super) const LINE_EDITOR_ID: &str = "line-editor";

pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("enter", CommitLine, Some(LINE_KEY_CONTEXT)),
        KeyBinding::new("escape", CancelLine, Some(LINE_KEY_CONTEXT)),
    ]);
}

/// The open editor: the line it edits, its text box, and the font, size
/// and colour picked for it.
pub(super) struct LineEditor {
    pub(super) request: TextEditRequest,
    pub(super) input: Entity<SearchInput>,
    pub(super) picked: Picked,
}

impl Canvas {
    /// Open the editor on the line a tool asked for, if one did.
    pub(super) fn answer_text_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.model.document_mut().take_text_edit_request() else {
            return;
        };
        let theme = self.theme;
        let text = request.text.clone();
        let input = cx.new(|cx| {
            let mut input =
                SearchInput::with_placeholder(LINE_EDITOR_ID, "Line of text", theme, cx);
            input.set_query(text, cx);
            input
        });
        window.focus(&input.read(cx).focus_handle(cx));
        self.line_editor = Some(LineEditor {
            request,
            input,
            picked: Picked::default(),
        });
        cx.notify();
    }

    /// Write what the editor holds into the line, and close it.
    pub(super) fn commit_line_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.line_editor.take() else {
            return;
        };
        let typed = editor.input.read(cx).query().to_owned();
        let result = self
            .model
            .edit_text_line(&editor.request, &typed, editor.picked.style());
        self.handle_change(result, cx);
    }

    /// Leave the line as it was.
    pub(super) fn cancel_line_editor(&mut self, cx: &mut Context<Self>) {
        if self.line_editor.take().is_some() {
            cx.notify();
        }
    }

    /// Pick a font, size or colour for the line being edited.
    pub(in crate::shell) fn choose_line_style(
        &mut self,
        choice: StyleChoice,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.line_editor.as_mut() {
            editor.picked.pick(choice);
            cx.notify();
        }
    }

    /// The editor's text box, while one is open.
    pub(in crate::shell) fn line_editor_input(&self) -> Option<Entity<SearchInput>> {
        Some(self.line_editor.as_ref()?.input.clone())
    }

    /// What the editor tells a screen reader: a text box over the line.
    pub(super) fn line_editor_node(&self, scale: f32, cx: &gpui::App) -> Option<A11yElement> {
        let editor = self.line_editor.as_ref()?;
        let mut node = editor
            .input
            .read(cx)
            .accessible("Line of text", TextField::LineText);
        node.bounds = self
            .model
            .view_rect(editor.request.page, editor.request.bounds)
            .map(|(origin, width, height)| {
                let rect = ViewRect {
                    origin,
                    size: ViewSize { width, height },
                };
                Rects::view_rect(rect, self.model.canvas_origin(), scale)
            });
        Some(node.with_children(style_nodes(editor.picked)))
    }

    /// The editor, over the line in canvas coordinates.
    pub(super) fn render_line_editor(
        &self,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let editor = self.line_editor.as_ref()?;
        let (at, width, height) = self
            .model
            .view_rect(editor.request.page, editor.request.bounds)?;
        Some(
            div()
                .key_context(LINE_KEY_CONTEXT)
                .on_action(cx.listener(|canvas, _: &CommitLine, _window, cx| {
                    canvas.commit_line_editor(cx);
                }))
                .on_action(cx.listener(|canvas, _: &CancelLine, _window, cx| {
                    canvas.cancel_line_editor(cx);
                }))
                // A press inside places the cursor; it is not the press
                // elsewhere that keeps the change.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .absolute()
                .left(px(at.x))
                .top(px(at.y))
                .w(px(width.max(160.0)))
                .min_h(px(height))
                .p_1()
                .rounded_sm()
                .bg(theme.raised)
                .border_1()
                .border_color(theme.selected)
                .flex()
                .flex_col()
                .gap_1()
                .child(editor.input.clone())
                .children(StyleList::ALL.map(|list| style_row(list, editor.picked, theme, cx))),
        )
    }
}

fn chip_id(choice: StyleChoice) -> (&'static str, usize) {
    let list = match choice.list {
        StyleList::Font => "line-style-font",
        StyleList::Size => "line-style-size",
        StyleList::Colour => "line-style-colour",
    };
    (list, choice.index)
}

/// The three lists to a screen reader: each entry a radio button that
/// picks itself.
fn style_nodes(picked: Picked) -> Vec<A11yElement> {
    StyleList::ALL
        .into_iter()
        .map(|list| {
            A11yElement::new(
                chip_id(StyleChoice {
                    list,
                    index: usize::MAX,
                })
                .0,
                Role::RadioGroup,
                list.label(),
            )
            .with_children(
                list.names()
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let choice = StyleChoice { list, index };
                        A11yElement::new(chip_id(choice), Role::RadioButton, name)
                            .with_state(A11yState::selected(picked.is_picked(choice)))
                            .with_activation(Activation::LineStyle(choice))
                    })
                    .collect(),
            )
        })
        .collect()
}

fn style_row(
    list: StyleList,
    picked: Picked,
    theme: ThemeTokens,
    cx: &mut Context<Canvas>,
) -> gpui::Div {
    let mut row = div()
        .flex()
        .flex_wrap()
        .gap_1()
        .text_xs()
        .child(div().text_color(theme.muted_text).child(list.label()));
    for (index, name) in list.names().into_iter().enumerate() {
        let choice = StyleChoice { list, index };
        row = row.child(
            div()
                .id(chip_id(choice))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .when(picked.is_picked(choice), |chip| chip.bg(theme.selected))
                .hover(move |chip| chip.bg(theme.hover))
                .on_click(cx.listener(move |canvas, _, _, cx| {
                    canvas.choose_line_style(choice, cx);
                }))
                .child(name),
        );
    }
    row
}
