use std::fmt;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, Div, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{FitMode, PageIndex};

use super::accessible::{Activation, Element, Rects, Surface, TextField};
use super::tabs::ShellFrame;
use super::theme::ThemeTokens;
use super::tool_search::SearchInput;
use crate::a11y::State as A11yState;
use crate::shell::canvas::{CanvasViewState, ViewAction};

pub(super) const PAGE_CONTROLS_HEIGHT: f32 = 48.0;
const FULL_FIT_LABELS_MIN_WIDTH: f32 = 700.0;

/// The element id the page-number field renders with. Named here rather than
/// at the call site so the field and the node describing it cannot be given
/// different identities.
pub(super) const PAGE_ENTRY_ID: &str = "page-entry-input";

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
    pub(super) zoom_percent: Option<u32>,
    pub(super) actual_size: bool,
    pub(super) can_previous_view: bool,
    pub(super) can_next_view: bool,
}

impl PageControlsState {
    pub(super) fn from_view(view: CanvasViewState) -> Self {
        Self {
            current_page: view.current_page + 1,
            page_count: view.page_count,
            zoom_percent: zoom_percent(view.zoom),
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

fn zoom_percent(zoom: f32) -> Option<u32> {
    if !zoom.is_finite() || zoom <= 0.0 {
        return None;
    }
    let percent = (zoom * 100.0).round();
    (percent <= u32::MAX as f32).then_some(percent as u32)
}

/// One button in the page-control row.
///
/// Every one of these is a glyph on screen. Without a name a screen reader
/// reads the glyph, which is punctuation or nothing, so the name lives here
/// beside the glyph rather than being invented somewhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    PreviousView,
    NextView,
    FirstPage,
    PreviousPage,
    GoToPage,
    NextPage,
    LastPage,
    RotateClockwise,
    ActualSize,
    ZoomOut,
    ZoomIn,
    FitPage,
    FitWidth,
    FitHeight,
}

impl Control {
    fn id(self) -> &'static str {
        match self {
            Self::PreviousView => "previous-view",
            Self::NextView => "next-view",
            Self::FirstPage => "first-page",
            Self::PreviousPage => "previous-page",
            Self::GoToPage => "go-to-page",
            Self::NextPage => "next-page",
            Self::LastPage => "last-page",
            Self::RotateClockwise => "rotate-clockwise",
            Self::ActualSize => "actual-size",
            Self::ZoomOut => "zoom-out",
            Self::ZoomIn => "zoom-in",
            Self::FitPage => "fit-page",
            Self::FitWidth => "fit-width",
            Self::FitHeight => "fit-height",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::PreviousView => "Previous View",
            Self::NextView => "Next View",
            Self::FirstPage => "First Page",
            Self::PreviousPage => "Previous Page",
            Self::GoToPage => "Go To Page",
            Self::NextPage => "Next Page",
            Self::LastPage => "Last Page",
            Self::RotateClockwise => "Rotate Clockwise",
            Self::ActualSize => "Actual Size",
            Self::ZoomOut => "Zoom Out",
            Self::ZoomIn => "Zoom In",
            Self::FitPage => "Fit Page",
            Self::FitWidth => "Fit Width",
            Self::FitHeight => "Fit Height",
        }
    }

    /// What is drawn. The fit buttons shorten when the row does; Actual Size
    /// draws its tick as a separate element, so the glyph stays a glyph and
    /// the checked state stays state.
    fn glyph(self, layout: PageControlsLayout) -> &'static str {
        let full = layout == PageControlsLayout::FullLabels;
        match self {
            Self::PreviousView => "↶",
            Self::NextView => "↷",
            Self::FirstPage => "|‹",
            Self::PreviousPage => "‹",
            Self::GoToPage => "Go",
            Self::NextPage => "›",
            Self::LastPage => "›|",
            Self::RotateClockwise => "↻",
            Self::ActualSize => "1:1",
            Self::ZoomOut => "−",
            Self::ZoomIn => "+",
            Self::FitPage if full => "Page",
            Self::FitPage => "P",
            Self::FitWidth if full => "Width",
            Self::FitWidth => "W",
            Self::FitHeight if full => "Height",
            Self::FitHeight => "H",
        }
    }

    fn activation(self) -> Activation {
        match self {
            Self::PreviousView => Activation::View(ViewAction::PreviousView),
            Self::NextView => Activation::View(ViewAction::NextView),
            Self::FirstPage => Activation::View(ViewAction::FirstPage),
            Self::PreviousPage => Activation::View(ViewAction::PreviousPage),
            Self::GoToPage => Activation::SubmitPageEntry,
            Self::NextPage => Activation::View(ViewAction::NextPage),
            Self::LastPage => Activation::View(ViewAction::LastPage),
            Self::RotateClockwise => Activation::View(ViewAction::RotateClockwise),
            Self::ActualSize => Activation::View(ViewAction::ActualSize),
            Self::ZoomOut => Activation::View(ViewAction::ZoomOut),
            Self::ZoomIn => Activation::View(ViewAction::ZoomIn),
            Self::FitPage => Activation::View(ViewAction::Fit(FitMode::Page)),
            Self::FitWidth => Activation::View(ViewAction::Fit(FitMode::Width)),
            Self::FitHeight => Activation::View(ViewAction::Fit(FitMode::Height)),
        }
    }

    fn enabled(self, state: PageControlsState) -> bool {
        match self {
            Self::PreviousView => state.can_previous_view,
            Self::NextView => state.can_next_view,
            Self::FirstPage | Self::PreviousPage => state.can_previous_page(),
            Self::NextPage | Self::LastPage => state.can_next_page(),
            _ => true,
        }
    }

    /// Why a control is off, for a screen reader to say after the name.
    fn unavailable(self, state: PageControlsState) -> Option<&'static str> {
        if self.enabled(state) {
            return None;
        }
        Some(match self {
            Self::PreviousView => "There is no earlier view to go back to",
            Self::NextView => "There is no later view to go forward to",
            Self::FirstPage | Self::PreviousPage => "This is the first page",
            _ => "This is the last page",
        })
    }

    /// The one control in the row that is on or off rather than just pressed.
    fn toggled(self, state: PageControlsState) -> Option<bool> {
        (self == Self::ActualSize).then_some(state.actual_size)
    }
}

/// One thing in the row, in the order the row builds it.
///
/// The row, its accessible description and the rectangles it reports after
/// prepaint all walk this list, so a control cannot be drawn with one name,
/// announced with another and measured as a third.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Control(Control),
    /// The page-number field.
    PageEntry,
    /// The page count beside the field.
    PageCount,
    /// The zoom percentage beside the zoom buttons.
    ZoomLevel,
}

impl Item {
    /// The row, left to right, in Acrobat's order.
    const ROW: [Item; 17] = [
        Item::Control(Control::PreviousView),
        Item::Control(Control::NextView),
        Item::Control(Control::FirstPage),
        Item::Control(Control::PreviousPage),
        Item::PageEntry,
        Item::PageCount,
        Item::Control(Control::GoToPage),
        Item::Control(Control::NextPage),
        Item::Control(Control::LastPage),
        Item::Control(Control::RotateClockwise),
        Item::Control(Control::ActualSize),
        Item::Control(Control::ZoomOut),
        Item::ZoomLevel,
        Item::Control(Control::ZoomIn),
        Item::Control(Control::FitPage),
        Item::Control(Control::FitWidth),
        Item::Control(Control::FitHeight),
    ];
}

/// What the page controls tell a screen reader.
pub(super) fn accessible(
    state: PageControlsState,
    error: Option<&PageEntryError>,
    page_entry: &str,
) -> Element {
    let mut controls = Element::new("page-controls", Role::Toolbar, "Page Controls").with_children(
        Item::ROW
            .iter()
            .map(|item| describe(*item, state, page_entry))
            .collect(),
    );
    if let Some(error) = error {
        controls = controls.child(Element::new(
            "page-entry-error",
            Role::Alert,
            error.to_string(),
        ));
    }
    controls
}

fn describe(item: Item, state: PageControlsState, page_entry: &str) -> Element {
    match item {
        Item::Control(control) => {
            let mut element = Element::new(control.id(), Role::Button, control.name())
                .with_activation(control.activation())
                .with_state(A11yState {
                    toggled: control.toggled(state),
                    selected: None,
                    disabled: !control.enabled(state),
                });
            if let Some(reason) = control.unavailable(state) {
                element = element.with_description(reason);
            }
            element
        }
        Item::PageEntry => Element::new(PAGE_ENTRY_ID, Role::NumberInput, "Page Number")
            .with_description(if page_entry.is_empty() {
                format!("Page {} of {}", state.current_page, state.page_count)
            } else {
                page_entry.to_owned()
            })
            .with_activation(Activation::Focus(TextField::Page)),
        Item::PageCount => Element::new(
            "page-count",
            Role::Label,
            format!("of {} pages", state.page_count),
        ),
        Item::ZoomLevel => Element::new(
            "zoom-level",
            Role::Label,
            state
                .zoom_percent
                .map(|percent| format!("Zoom {percent} percent"))
                .unwrap_or_else(|| "Zoom unavailable".to_owned()),
        ),
    }
}

pub(super) fn render_page_controls(
    state: PageControlsState,
    page_input: Entity<SearchInput>,
    error: Option<&PageEntryError>,
    document_width: Pixels,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let layout = page_controls_layout(document_width);
    let error = error.map(ToString::to_string);

    let mut row = controls_row().on_children_prepainted(move |bounds, window, _cx| {
        rects.record(Surface::PageControls, &bounds, window);
    });
    for item in Item::ROW {
        row = match item {
            Item::Control(control) => row.child(action_button(control, layout, state, theme, cx)),
            Item::PageEntry => row.child(div().w(px(42.0)).child(page_input.clone())),
            Item::PageCount => row.child(
                div()
                    .min_w(px(34.0))
                    .text_sm()
                    .child(format!("/ {}", state.page_count)),
            ),
            Item::ZoomLevel => row.child(
                div().min_w(px(48.0)).text_center().text_sm().child(
                    state
                        .zoom_percent
                        .map(|percent| format!("{percent}%"))
                        .unwrap_or_else(|| "Zoom".to_owned()),
                ),
            ),
        };
    }

    div()
        .id("page-controls")
        .h(px(PAGE_CONTROLS_HEIGHT))
        .w_full()
        .flex_none()
        .relative()
        .bg(theme.surface)
        .text_color(theme.text)
        .child(controls_scroller(row))
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
                    .bg(theme.error_surface)
                    .text_xs()
                    .text_color(theme.error_text)
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
    control: Control,
    layout: PageControlsLayout,
    state: PageControlsState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let enabled = control.enabled(state);
    let activation = control.activation();
    div()
        .id(control.id())
        .h(px(28.0))
        .min_w(px(28.0))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .rounded_sm()
        .text_sm()
        .text_color(if enabled {
            theme.text
        } else {
            theme.disabled_text
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |button| button.bg(theme.hover))
        })
        .on_click(cx.listener(move |frame, _event, window, cx| {
            if enabled {
                frame.run_activation(activation.clone(), window, cx);
            }
        }))
        .child(control.glyph(layout))
        // The tick is its own element rather than part of the glyph, so the
        // control's name stays a name and its checked state stays state.
        .when(control.toggled(state) == Some(true), |button| {
            button.child("✓")
        })
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

    fn state(current_page: usize, page_count: usize) -> PageControlsState {
        PageControlsState::from_view(view(current_page, page_count))
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
        assert_eq!(actual.zoom_percent, Some(100));
        assert!(actual.actual_size);

        let mut fitted = view(0, 1);
        fitted.zoom_policy = ZoomPolicy::Fit(FitMode::Page);
        assert!(!PageControlsState::from_view(fitted).actual_size);
    }

    #[test]
    fn non_finite_zoom_is_not_cast_to_zero() {
        for zoom in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0] {
            let mut view = view(0, 1);
            view.zoom = zoom;
            let state = PageControlsState::from_view(view);
            let described = accessible(state, None, "");
            let zoom = described.find(&"zoom-level".into()).unwrap();

            assert_eq!(state.zoom_percent, None);
            assert_eq!(zoom.label, "Zoom unavailable");
        }
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

    /// Every one of these buttons is a glyph. A screen reader reading the
    /// glyph would say "left single quotation mark", so each one has to carry
    /// a name that is not its glyph.
    #[test]
    fn every_glyph_button_is_announced_by_name_and_not_by_its_glyph() {
        let described = accessible(state(1, 3), None, "");

        for item in Item::ROW {
            let Item::Control(control) = item else {
                continue;
            };
            let node = described
                .find(&control.id().into())
                .unwrap_or_else(|| panic!("{} is not in the description", control.id()));
            assert_eq!(node.label, control.name());
            assert_ne!(node.label, control.glyph(PageControlsLayout::FullLabels));
            assert!(
                node.label.chars().any(char::is_alphabetic),
                "{}",
                control.id()
            );
        }
    }

    /// The fit buttons shorten to a single letter on a narrow window. What a
    /// screen reader says does not shorten with them, because the name is not
    /// the glyph.
    #[test]
    fn the_compact_layout_shortens_the_glyphs_and_not_the_names() {
        let described = accessible(state(1, 3), None, "");

        assert_eq!(
            Control::FitWidth.glyph(PageControlsLayout::CompactLabels),
            "W"
        );
        assert_eq!(
            Control::FitWidth.glyph(PageControlsLayout::FullLabels),
            "Width"
        );
        assert_eq!(
            described.find(&"fit-width".into()).unwrap().label,
            "Fit Width"
        );
    }

    #[test]
    fn actual_size_carries_its_tick_as_state_rather_than_in_its_name() {
        let on = accessible(state(1, 3), None, "");
        let off = {
            let mut fitted = view(0, 1);
            fitted.zoom_policy = ZoomPolicy::Fit(FitMode::Page);
            accessible(PageControlsState::from_view(fitted), None, "")
        };

        let on = on.find(&"actual-size".into()).unwrap();
        let off = off.find(&"actual-size".into()).unwrap();
        assert_eq!(on.state.toggled, Some(true));
        assert_eq!(off.state.toggled, Some(false));
        assert_eq!(on.label, off.label);
        assert!(!on.label.contains('✓'));
    }

    #[test]
    fn a_control_at_a_document_boundary_is_disabled_and_says_why() {
        let first = accessible(state(0, 3), None, "");
        let previous = first.find(&"previous-page".into()).unwrap();
        assert!(previous.state.disabled);
        assert_eq!(
            previous.description.as_deref(),
            Some("This is the first page")
        );

        let last = accessible(state(2, 3), None, "");
        let next = last.find(&"next-page".into()).unwrap();
        assert!(next.state.disabled);
        assert_eq!(next.description.as_deref(), Some("This is the last page"));
        assert!(!last.find(&"previous-page".into()).unwrap().state.disabled);
    }

    /// Both halves of "one list drives both": the description has exactly one
    /// node per rendered item, in the same order, so the rectangles the row
    /// reports after prepaint land on the right nodes.
    #[test]
    fn the_description_has_one_node_per_row_item_in_row_order() {
        let described = accessible(state(1, 3), None, "");

        assert_eq!(described.children.len(), Item::ROW.len());
        let keys: Vec<String> = described
            .children
            .iter()
            .map(|child| child.key.to_string())
            .collect();
        assert_eq!(keys[0], "previous-view");
        assert_eq!(keys[4], PAGE_ENTRY_ID);
        assert_eq!(keys[Item::ROW.len() - 1], "fit-height");
    }

    /// An error banner is an extra child, appended after the row, so it must
    /// never be mistaken for a row item and given a row item's rectangle.
    #[test]
    fn the_page_entry_error_is_announced_as_an_alert_after_the_row() {
        let error = PageEntryError::OutOfRange {
            page: 4,
            page_count: 3,
        };
        let described = accessible(state(1, 3), Some(&error), "4");

        assert_eq!(described.children.len(), Item::ROW.len() + 1);
        let alert = described.find(&"page-entry-error".into()).unwrap();
        assert_eq!(alert.role, Role::Alert);
        assert_eq!(alert.label, "Page 4 is outside this 3-page document");
    }

    #[test]
    fn the_page_field_reads_the_page_it_is_on_until_the_user_types() {
        let empty = accessible(state(0, 12), None, "");
        let typed = accessible(state(0, 12), None, "7");

        let empty = empty.find(&PAGE_ENTRY_ID.into()).unwrap();
        assert_eq!(empty.label, "Page Number");
        assert_eq!(empty.description.as_deref(), Some("Page 1 of 12"));
        assert_eq!(
            typed
                .find(&PAGE_ENTRY_ID.into())
                .unwrap()
                .description
                .as_deref(),
            Some("7")
        );
    }

    /// The click listener and the accessible description read `activation()`
    /// from the same table, so a keyboard or screen-reader press cannot run
    /// something the mouse would not.
    #[test]
    fn every_described_control_carries_the_action_its_click_runs() {
        let described = accessible(state(1, 3), None, "");

        for item in Item::ROW {
            let Item::Control(control) = item else {
                continue;
            };
            let node = described.find(&control.id().into()).unwrap();
            assert_eq!(node.activation.as_ref(), Some(&control.activation()));
        }
        assert_eq!(
            described.find(&"go-to-page".into()).unwrap().activation,
            Some(Activation::SubmitPageEntry)
        );
    }

    #[test]
    fn control_ids_are_unique() {
        let mut ids: Vec<&str> = Item::ROW
            .iter()
            .filter_map(|item| match item {
                Item::Control(control) => Some(control.id()),
                _ => None,
            })
            .collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), count);
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
