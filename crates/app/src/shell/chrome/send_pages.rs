//! Copy To Document and Move To Document, from the Organize Pages grid:
//! the chosen pages into another open document, at its end (P22, the M3
//! row "Copy or move pages between open documents").
//!
//! Acrobat drags thumbnails from one document's pane to another's. Here the
//! grid's buttons ask which open document, which a keyboard and a screen
//! reader reach as well as a pointer does. The pages go after the target's
//! last page; the grid of the target places them anywhere after that.

use accesskit::Role;
use gpui::{
    div, px, Context, EntityId, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::PageIndex;

use super::accessible::{Activation, Element};
use super::tabs::ShellFrame;
use super::theme::ThemeTokens;

/// The chooser, open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct SendPagesState {
    pub(in crate::shell) moving: bool,
    pub(in crate::shell) pages: Vec<PageIndex>,
    /// The open documents that can take the pages: every other tab not on
    /// the same session, by canvas, with its title.
    pub(in crate::shell) targets: Vec<(EntityId, String)>,
}

/// Said when there is nowhere to send the pages.
pub(in crate::shell) const NO_OTHER_DOCUMENT: &str =
    "No other document is open. Open one to copy pages into it.";

impl SendPagesState {
    /// What the dialog says above the documents.
    pub(in crate::shell) fn summary(&self) -> String {
        let verb = if self.moving { "Move" } else { "Copy" };
        let pages = match self.pages.len() {
            1 => "1 page".to_owned(),
            count => format!("{count} pages"),
        };
        if self.targets.is_empty() {
            return NO_OTHER_DOCUMENT.to_owned();
        }
        format!("{verb} {pages} to the end of:")
    }
}

/// What a finished send says on the notice bar.
pub(in crate::shell) fn done_message(moving: bool, count: usize, target: &str) -> String {
    let verb = if moving { "Moved" } else { "Copied" };
    let pages = if count == 1 { "1 page" } else { "pages" };
    if count == 1 {
        format!("{verb} {pages} to {target}")
    } else {
        format!("{verb} {count} {pages} to {target}")
    }
}

pub(in crate::shell) fn accessible(state: &SendPagesState) -> Vec<Element> {
    let mut rows = vec![Element::new(
        "send-pages-summary",
        Role::Label,
        state.summary(),
    )];
    rows.extend(state.targets.iter().enumerate().map(|(index, (_, title))| {
        Element::new(("send-pages-target", index), Role::Button, title.clone())
            .with_activation(Activation::SendPages(index))
    }));
    rows
}

pub(in crate::shell) fn render(
    state: &SendPagesState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().pb_2().child(state.summary()));
    for (index, (_, title)) in state.targets.iter().enumerate() {
        list = list.child(
            div()
                .id(("send-pages-target", index))
                .h(px(32.0))
                .flex()
                .items_center()
                .px_2()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |row| row.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::SendPages(index), window, cx);
                }))
                .child(title.clone()),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(moving: bool, pages: usize, targets: usize) -> SendPagesState {
        SendPagesState {
            moving,
            pages: (0..pages).collect(),
            targets: (0..targets)
                .map(|index| (EntityId::from(index as u64 + 1), format!("doc{index}.pdf")))
                .collect(),
        }
    }

    #[test]
    fn the_dialog_says_what_it_will_do_or_that_it_cannot() {
        assert_eq!(state(false, 1, 1).summary(), "Copy 1 page to the end of:");
        assert_eq!(state(true, 3, 2).summary(), "Move 3 pages to the end of:");
        assert_eq!(state(true, 3, 0).summary(), NO_OTHER_DOCUMENT);
    }

    #[test]
    fn each_open_document_is_a_button_that_sends_there() {
        let rows = accessible(&state(false, 2, 2));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].label, "doc0.pdf");
        assert_eq!(rows[2].activation, Some(Activation::SendPages(1)));
        assert_eq!(done_message(false, 2, "b.pdf"), "Copied 2 pages to b.pdf");
        assert_eq!(done_message(true, 1, "b.pdf"), "Moved 1 page to b.pdf");
    }
}
