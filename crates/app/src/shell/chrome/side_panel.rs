use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    StatefulInteractiveElement as _, Styled as _,
};

use super::tabs::ShellFrame;

pub(super) const CLOSED_WIDTH: f32 = 40.0;
pub(super) const OPEN_WIDTH: f32 = 280.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum SidePanelState {
    Closed,
    #[default]
    OpenEmpty,
}

impl SidePanelState {
    pub(super) fn is_open(self) -> bool {
        self == Self::OpenEmpty
    }

    pub(super) fn toggle(&mut self) {
        *self = if self.is_open() {
            Self::Closed
        } else {
            Self::OpenEmpty
        };
    }

    pub(super) fn width(self) -> Pixels {
        px(if self.is_open() {
            OPEN_WIDTH
        } else {
            CLOSED_WIDTH
        })
    }
}

pub(super) fn render_side_panel(
    state: SidePanelState,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let toggle = div()
        .id("side-panel-toggle")
        .w(px(32.0))
        .h(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .cursor_pointer()
        .hover(|button| button.bg(gpui::rgb(0x45464b)))
        .on_click(cx.listener(|frame, _event, _window, cx| {
            frame.toggle_side_panel(cx);
        }))
        .child(if state.is_open() { "›" } else { "‹" });
    let mut panel = div()
        .id("side-panel")
        .w(state.width())
        .h_full()
        .flex_none()
        .bg(gpui::rgb(0x202124))
        .text_color(gpui::white());

    if state.is_open() {
        panel = panel.child(
            div()
                .h(px(48.0))
                .flex()
                .items_center()
                .justify_between()
                .px_2()
                .child("Panel")
                .child(toggle),
        );
    } else {
        panel = panel
            .flex()
            .items_start()
            .justify_center()
            .pt_2()
            .child(toggle);
    }

    panel
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::{SidePanelState, CLOSED_WIDTH, OPEN_WIDTH};

    #[test]
    fn the_empty_host_opens_by_default_and_toggles_without_content() {
        let mut state = SidePanelState::default();

        assert_eq!(state, SidePanelState::OpenEmpty);
        assert_eq!(state.width(), px(OPEN_WIDTH));

        state.toggle();
        assert_eq!(state, SidePanelState::Closed);
        assert_eq!(state.width(), px(CLOSED_WIDTH));

        state.toggle();
        assert_eq!(state, SidePanelState::OpenEmpty);
    }
}
