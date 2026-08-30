use std::fmt;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, Div, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{FitMode, PageIndex};

use super::tabs::ShellFrame;
use super::tool_search::SearchInput;
use crate::shell::canvas::{CanvasViewState, ViewAction};

pub(super) const PAGE_CONTROLS_HEIGHT: f32 = 48.0;
const FULL_FIT_LABELS_MIN_WIDTH: f32 = 700.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageControlsLayout {
    FullLabels,
    CompactLabels,
}

fn page_controls_layout(document_width: Pixels) -> PageControlsLayout {
    if f32::from(document_width) >= FULL_FIT_LABELS_MIN_WIDTH {
        PageControlsLayout::FullLabels
    } else {
        PageControlsLayout::CompactLabels
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PageEntryError {
    Empty,
    InvalidNumber(String),
    OutOfRange { page: usize, page_count: usize },
}

impl fmt::Display for PageEntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Enter a page number"),
            Self::InvalidNumber(value) => write!(f, "{value:?} is not a page number"),
            Self::OutOfRange { page, page_count } => {
                write!(f, "Page {page} is outside this {page_count}-page document")
            }
        }
    }
}

pub(super) fn parse_page_entry(
    input: &str,
    page_count: usize,
) -> Result<PageIndex, PageEntryError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(PageEntryError::Empty);
    }
    let page = input
        .parse::<usize>()
        .map_err(|_| PageEntryError::InvalidNumber(input.to_owned()))?;
    if !(1..=page_count).contains(&page) {
        return Err(PageEntryError::OutOfRange { page, page_count });
    }
    Ok(page - 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PageControlsState {
    pub(super) current_page: usize,
    pub(super) page_count: usize,
    pub(super) zoom_percent: u32,
    pub(super) actual_size: bool,
    pub(super) can_previous_view: bool,
    pub(super) can_next_view: bool,
}

impl PageControlsState {
    pub(super) fn from_view(view: CanvasViewState) -> Self {
        Self {
            current_page: view.current_page + 1,
            page_count: view.page_count,
            zoom_percent: (view.zoom * 100.0).round() as u32,
            actual_size: view.is_actual_size(),
            can_previous_view: view.can_previous_view,
            can_next_view: view.can_next_view,
        }
    }

    fn can_previous_page(self) -> bool {
        self.current_page > 1
    }

    fn can_next_page(self) -> bool {
        self.current_page < self.page_count
    }
}

pub(super) fn render_page_controls(
    state: PageControlsState,
    page_input: Entity<SearchInput>,
    error: Option<&PageEntryError>,
    document_width: Pixels,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let current_and_count = format!("/ {}", state.page_count);
    let zoom = format!("{}%", state.zoom_percent);
    let error = error.map(ToString::to_string);
    let fit_labels = match page_controls_layout(document_width) {
        PageControlsLayout::FullLabels => ("Page", "Width", "Height"),
        PageControlsLayout::CompactLabels => ("P", "W", "H"),
    };

    let controls = controls_row()
        .child(action_button(
            "previous-view",
            "↶",
            state.can_previous_view,
            ViewAction::PreviousView,
            cx,
        ))
        .child(action_button(
            "next-view",
            "↷",
            state.can_next_view,
            ViewAction::NextView,
            cx,
        ))
        .child(action_button(
            "first-page",
            "|‹",
            state.can_previous_page(),
            ViewAction::FirstPage,
            cx,
        ))
        .child(action_button(
            "previous-page",
            "‹",
            state.can_previous_page(),
            ViewAction::PreviousPage,
            cx,
        ))
        .child(div().w(px(42.0)).child(page_input))
        .child(div().min_w(px(34.0)).text_sm().child(current_and_count))
        .child(submit_button(cx))
        .child(action_button(
            "next-page",
            "›",
            state.can_next_page(),
            ViewAction::NextPage,
            cx,
        ))
        .child(action_button(
            "last-page",
            "›|",
            state.can_next_page(),
            ViewAction::LastPage,
            cx,
        ))
        .child(action_button(
            "rotate-clockwise",
            "↻",
            true,
            ViewAction::RotateClockwise,
            cx,
        ))
        .child(action_button(
            "actual-size",
            if state.actual_size { "1:1 ✓" } else { "1:1" },
            true,
            ViewAction::ActualSize,
            cx,
        ))
        .child(action_button(
            "zoom-out",
            "−",
            true,
            ViewAction::ZoomOut,
            cx,
        ))
        .child(div().min_w(px(48.0)).text_center().text_sm().child(zoom))
        .child(action_button("zoom-in", "+", true, ViewAction::ZoomIn, cx))
        .child(action_button(
            "fit-page",
            fit_labels.0,
            true,
            ViewAction::Fit(FitMode::Page),
            cx,
        ))
        .child(action_button(
            "fit-width",
            fit_labels.1,
            true,
            ViewAction::Fit(FitMode::Width),
            cx,
        ))
        .child(action_button(
            "fit-height",
            fit_labels.2,
            true,
            ViewAction::Fit(FitMode::Height),
            cx,
        ));

    div()
        .id("page-controls")
        .h(px(PAGE_CONTROLS_HEIGHT))
        .w_full()
        .flex_none()
        .relative()
        .bg(gpui::rgb(0x202124))
        .text_color(gpui::white())
        .child(controls_scroller(controls))
        .when_some(error, |controls, error| {
            controls.child(
                div()
                    .id("page-entry-error")
                    .absolute()
                    .bottom(px(PAGE_CONTROLS_HEIGHT))
                    .left(px(8.0))
                    .max_w(px(210.0))
                    .p_1()
                    .rounded_sm()
                    .bg(gpui::rgb(0x451a1a))
                    .text_xs()
                    .text_color(gpui::rgb(0xfca5a5))
                    .child(error),
            )
        })
}

fn controls_row() -> Div {
    div()
        .flex_none()
        .min_w_full()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .px_2()
}

fn controls_scroller(controls: impl IntoElement) -> Stateful<Div> {
    div()
        .id("page-controls-scroll")
        .size_full()
        .flex()
        .overflow_x_scroll()
        .child(controls)
}

fn action_button(
    id: &'static str,
    label: impl Into<SharedString>,
    enabled: bool,
    action: ViewAction,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.0))
        .min_w(px(28.0))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .text_sm()
        .text_color(if enabled {
            gpui::rgb(0xffffff)
        } else {
            gpui::rgb(0x696b70)
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(gpui::rgb(0x45464b)))
        })
        .on_click(cx.listener(move |frame, _event, _window, cx| {
            if enabled {
                frame.run_view_action(action, cx);
            }
        }))
        .child(label.into())
}

fn submit_button(cx: &mut Context<ShellFrame>) -> impl IntoElement {
    div()
        .id("go-to-page")
        .h(px(28.0))
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .text_sm()
        .cursor_pointer()
        .hover(|button| button.bg(gpui::rgb(0x45464b)))
        .on_click(cx.listener(|frame, _event, _window, cx| {
            frame.submit_page_entry(cx);
        }))
        .child("Go")
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "shell-test-support")]
    use gpui::{Render, ScrollHandle, TestAppContext, Window};
    use onionskin_core::{PageLayoutMode, ViewRotation, ZoomPolicy};

    use super::*;

    fn view(current_page: usize, page_count: usize) -> CanvasViewState {
        CanvasViewState {
            current_page,
            page_count,
            zoom: 1.0,
            zoom_policy: ZoomPolicy::Fixed,
            layout_mode: PageLayoutMode::SinglePageContinuous,
            show_cover: false,
            rotation: ViewRotation::None,
            can_previous_view: false,
            can_next_view: false,
        }
    }

    #[test]
    fn page_entry_is_one_based_and_fails_loudly() {
        assert_eq!(parse_page_entry("2", 3), Ok(1));
        assert_eq!(parse_page_entry(" 3 ", 3), Ok(2));
        assert_eq!(parse_page_entry("", 3), Err(PageEntryError::Empty));
        assert_eq!(
            parse_page_entry("two", 3),
            Err(PageEntryError::InvalidNumber("two".to_owned()))
        );
        assert_eq!(
            parse_page_entry("0", 3),
            Err(PageEntryError::OutOfRange {
                page: 0,
                page_count: 3
            })
        );
        assert_eq!(
            parse_page_entry("4", 3),
            Err(PageEntryError::OutOfRange {
                page: 4,
                page_count: 3
            })
        );
    }

    #[test]
    fn boundary_pages_and_history_availability_follow_the_canvas() {
        let first = PageControlsState::from_view(view(0, 3));
        assert!(!first.can_previous_page());
        assert!(first.can_next_page());
        assert!(!first.can_previous_view);

        let mut last_view = view(2, 3);
        last_view.can_previous_view = true;
        let last = PageControlsState::from_view(last_view);
        assert!(last.can_previous_page());
        assert!(!last.can_next_page());
        assert!(last.can_previous_view);
    }

    #[test]
    fn actual_size_is_exactly_fixed_one_hundred_percent() {
        let actual = PageControlsState::from_view(view(0, 1));
        assert_eq!(actual.zoom_percent, 100);
        assert!(actual.actual_size);

        let mut fitted = view(0, 1);
        fitted.zoom_policy = ZoomPolicy::Fit(FitMode::Page);
        assert!(!PageControlsState::from_view(fitted).actual_size);
    }

    #[test]
    fn narrow_document_width_uses_the_bounded_compact_layout() {
        assert_eq!(
            page_controls_layout(px(0.0)),
            PageControlsLayout::CompactLabels
        );
        assert_eq!(
            page_controls_layout(px(FULL_FIT_LABELS_MIN_WIDTH - 1.0)),
            PageControlsLayout::CompactLabels
        );
        assert_eq!(
            page_controls_layout(px(FULL_FIT_LABELS_MIN_WIDTH)),
            PageControlsLayout::FullLabels
        );
    }

    #[cfg(feature = "shell-test-support")]
    struct ControlsLayoutProbe {
        fitted_scroller: ScrollHandle,
        fitted_row: ScrollHandle,
        overflowing_scroller: ScrollHandle,
        overflowing_row: ScrollHandle,
    }

    #[cfg(feature = "shell-test-support")]
    impl Render for ControlsLayoutProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(
                    div().w(px(200.0)).h(px(PAGE_CONTROLS_HEIGHT)).child(
                        controls_scroller(
                            controls_row()
                                .id("fitted-controls-row")
                                .track_scroll(&self.fitted_row)
                                .child(div().flex_none().size(px(20.0)))
                                .child(div().flex_none().size(px(20.0))),
                        )
                        .track_scroll(&self.fitted_scroller),
                    ),
                )
                .child(
                    div().w(px(100.0)).h(px(PAGE_CONTROLS_HEIGHT)).child(
                        controls_scroller(
                            controls_row()
                                .id("overflowing-controls-row")
                                .track_scroll(&self.overflowing_row)
                                .child(div().flex_none().size(px(80.0)))
                                .child(div().flex_none().size(px(80.0))),
                        )
                        .track_scroll(&self.overflowing_scroller),
                    ),
                )
        }
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn controls_center_when_they_fit_and_scroll_to_both_ends_when_they_do_not(
        cx: &mut TestAppContext,
    ) {
        let fitted_scroller = ScrollHandle::new();
        let fitted_row = ScrollHandle::new();
        let overflowing_scroller = ScrollHandle::new();
        let overflowing_row = ScrollHandle::new();
        let (_, cx) = cx.add_window_view(|_window, _cx| ControlsLayoutProbe {
            fitted_scroller: fitted_scroller.clone(),
            fitted_row: fitted_row.clone(),
            overflowing_scroller: overflowing_scroller.clone(),
            overflowing_row: overflowing_row.clone(),
        });
        cx.run_until_parked();

        let fitted_bounds = fitted_row.bounds();
        let fitted_first = fitted_row.bounds_for_item(0).unwrap();
        let fitted_last = fitted_row.bounds_for_item(1).unwrap();
        let leading_space = f32::from(fitted_first.left() - fitted_bounds.left());
        let trailing_space = f32::from(fitted_bounds.right() - fitted_last.right());
        assert!((leading_space - trailing_space).abs() < 0.01);
        assert_eq!(fitted_scroller.max_offset().width, px(0.0));

        let viewport = overflowing_scroller.bounds();
        let first = overflowing_row.bounds_for_item(0).unwrap();
        let last = overflowing_row.bounds_for_item(1).unwrap();
        let max_scroll = overflowing_scroller.max_offset().width;
        assert!(max_scroll > px(0.0));
        assert!(first.left() >= viewport.left());
        assert!(first.left() < viewport.right());
        assert!(last.left() - max_scroll < viewport.right());
        assert!(last.right() - max_scroll <= viewport.right());
        assert!(last.right() - max_scroll > viewport.left());
    }
}
