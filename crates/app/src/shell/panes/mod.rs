//! The left navigation panes: thumbnails, bookmarks, attachments, layers,
//! signatures and search results.
//!
//! One column, one pane at a time, chosen from a strip of buttons that stays
//! visible while the navigation panes are shown. That is Acrobat's shape and
//! the reason most panes hold a snapshot rather than a live borrow: the
//! button switches panes, the switch reads the document once, and drawing a
//! frame after that touches nothing but what was read. The two panes whose
//! contents change while they are open, thumbnails and search results, say
//! so by holding no snapshot at all.
//!
//! The whole surface talks to the rest of the shell through one action type
//! and one entry point, [`apply`]. Everything a pane does to the document is
//! a [`PaneAction`], so the frame that owns the tabs needs one method rather
//! than one per pane, and a pane cannot reach past the canvas it was given.

mod attachments;
mod bookmarks;
mod layers;
mod results;
mod signatures;
mod thumbnails;

use std::path::PathBuf;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Point, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{Attachment, Layer, ObjRef, OutlineItem, PageIndex, SignatureField};

use super::canvas::CanvasError;
use super::chrome::accessible::{Activation, Element, Rects, Surface};
use super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::Canvas;
use crate::a11y::State as A11yState;

pub(in crate::shell) use self::attachments::AttachmentAction;
pub(in crate::shell) use self::layers::LayersCommand;
pub(in crate::shell) use self::thumbnails::ThumbnailAction;

/// The strip of pane buttons, always there while the navigation panes are.
const STRIP_WIDTH: f32 = 48.0;
/// The pane body beside it, when one is open.
const BODY_WIDTH: f32 = 244.0;
const ROW_HEIGHT: f32 = 28.0;
/// Drawn and announced when the open pane's snapshot is not the open pane's.
const NOTHING_YET: &str = "This pane has nothing to show yet.";
/// What the two live panes say with no document behind them.
const NO_DOCUMENT: &str = "No document is open.";

/// Which pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum NavigationPane {
    Thumbnails,
    Bookmarks,
    Attachments,
    Layers,
    Signatures,
    SearchResults,
}

impl NavigationPane {
    /// Acrobat's order down the navigation strip.
    pub(in crate::shell) const ALL: [Self; 6] = [
        Self::Thumbnails,
        Self::Bookmarks,
        Self::Attachments,
        Self::Layers,
        Self::Signatures,
        Self::SearchResults,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Thumbnails => "Page Thumbnails",
            Self::Bookmarks => "Bookmarks",
            Self::Attachments => "Attachments",
            Self::Layers => "Layers",
            Self::Signatures => "Signatures",
            Self::SearchResults => "Search Results",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Thumbnails => "▤",
            Self::Bookmarks => "▸",
            Self::Attachments => "⏚",
            Self::Layers => "◧",
            Self::Signatures => "✎",
            Self::SearchResults => "⌕",
        }
    }

    fn element_id(self) -> &'static str {
        match self {
            Self::Thumbnails => "pane-thumbnails",
            Self::Bookmarks => "pane-bookmarks",
            Self::Attachments => "pane-attachments",
            Self::Layers => "pane-layers",
            Self::Signatures => "pane-signatures",
            Self::SearchResults => "pane-search-results",
        }
    }
}

/// What a pane read from the document when it opened.
///
/// A reader that failed keeps its message rather than leaving the pane
/// blank: a document whose outline cannot be decoded has an outline, and an
/// empty pane would say it has none.
#[derive(Debug, Clone, PartialEq)]
enum PaneContent {
    /// The thumbnails and results panes read nothing here. Pictures arrive
    /// from the worker a row at a time and results are whatever the find has
    /// found so far, both of which change while the pane is open.
    Live,
    Bookmarks(Result<Vec<OutlineItem>, String>),
    Attachments(Result<Vec<Attachment>, String>),
    Layers(Result<Vec<Layer>, String>),
    Signatures(Result<Vec<SignatureField>, String>),
}

/// Everything a pane can ask the shell to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) enum PaneAction {
    /// Show this pane, or close it when it is the one already showing.
    Select(NavigationPane),
    /// Go to a page, from a thumbnail or a bookmark.
    GoToPage(PageIndex),
    /// Make one hit current, by the page it sits on and its position among
    /// that page's hits.
    SelectMatch(PageIndex, usize),
    Attachment(AttachmentAction),
    Layer(LayerAction),
    Thumbnail(ThumbnailAction),
    /// Put away whichever pane-local menu is open.
    DismissMenus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum LayerAction {
    SetVisible { layer: ObjRef, visible: bool },
    OpenMenu(Point<Pixels>),
    Run(LayersCommand),
}

/// The navigation column's own state: which pane is open, what it read, and
/// whatever the open one has to remember between frames.
#[derive(Debug, Default)]
pub(in crate::shell) struct NavigationPanesState {
    active: Option<NavigationPane>,
    content: Option<PaneContent>,
    thumbnails: thumbnails::ThumbnailsState,
    layers_menu: Option<Point<Pixels>>,
    /// What the last action could not do, shown in the pane that raised it.
    /// A refused toggle or a failed extraction has nowhere else to be seen:
    /// the canvas status line sits behind the document, not the pane.
    feedback: Option<String>,
    /// The height the column was last drawn at, which is what decides how
    /// many thumbnail rows are on screen. Set by the frame, which is the
    /// only thing that knows it.
    body_height: f32,
}

impl NavigationPanesState {
    /// The column's width, which the document view has to give up.
    pub(in crate::shell) fn width(&self) -> Pixels {
        px(if self.active.is_some() {
            STRIP_WIDTH + BODY_WIDTH
        } else {
            STRIP_WIDTH
        })
    }

    /// Forget everything read from a document that is no longer showing.
    ///
    /// Called when the active tab changes: the panes are the document's, and
    /// a bookmark list left over from the tab before would navigate this one
    /// to pages that mean nothing in it.
    pub(in crate::shell) fn document_changed(&mut self) {
        self.content = None;
        self.thumbnails.clear();
        self.layers_menu = None;
        self.feedback = None;
    }

    /// Take the thumbnails the worker has answered, as images to paint.
    /// Returns whether anything arrived, which is a repaint.
    pub(in crate::shell) fn collect_thumbnails(&mut self, canvas: &mut Canvas) -> bool {
        self.thumbnails.collect(canvas)
    }

    /// Report what an action could not do, in the pane that raised it.
    pub(in crate::shell) fn report(&mut self, failure: Option<String>) {
        self.feedback = failure;
    }

    fn open(&mut self, pane: NavigationPane, canvas: Option<&mut Canvas>) {
        self.active = Some(pane);
        self.layers_menu = None;
        self.feedback = None;
        self.content = Some(read(pane, canvas));
    }

    fn close(&mut self) {
        self.active = None;
        self.layers_menu = None;
        self.feedback = None;
        self.content = None;
    }

    /// Re-read the open pane, after something changed what it would say.
    fn reread(&mut self, canvas: &Entity<Canvas>, cx: &mut Context<ShellFrame>) {
        let Some(pane) = self.active else {
            return;
        };
        self.content = Some(canvas.update(cx, |canvas, _cx| read(pane, Some(canvas))));
    }

    /// The layers the pane is showing, or nothing when another pane is open
    /// or the reader failed.
    fn layers(&self) -> Option<&[Layer]> {
        match self.content.as_ref()? {
            PaneContent::Layers(Ok(layers)) => Some(layers),
            _ => None,
        }
    }

    /// The name a save dialog should suggest for one listed attachment.
    fn attachment_file_name(&self, index: usize) -> Option<String> {
        match self.content.as_ref()? {
            PaneContent::Attachments(Ok(items)) => {
                items.get(index).map(onionskin_core::Attachment::file_name)
            }
            _ => None,
        }
    }

    /// What the bookmarks pane read, for a test that drove the button that
    /// made it read.
    #[cfg(test)]
    fn bookmarks(&self) -> Option<&[OutlineItem]> {
        match self.content.as_ref()? {
            PaneContent::Bookmarks(Ok(items)) => Some(items),
            _ => None,
        }
    }

    /// Drop every thumbnail picture, keeping the scroll position.
    fn invalidate_thumbnails(&mut self) {
        self.thumbnails.invalidate_images();
    }
}

/// Read what a pane shows, once, when it opens.
fn read(pane: NavigationPane, canvas: Option<&mut Canvas>) -> PaneContent {
    let Some(canvas) = canvas else {
        // No document, so nothing to read. Every pane draws its empty state
        // from an empty list rather than from a missing one.
        return match pane {
            NavigationPane::Thumbnails | NavigationPane::SearchResults => PaneContent::Live,
            NavigationPane::Bookmarks => PaneContent::Bookmarks(Ok(Vec::new())),
            NavigationPane::Attachments => PaneContent::Attachments(Ok(Vec::new())),
            NavigationPane::Layers => PaneContent::Layers(Ok(Vec::new())),
            NavigationPane::Signatures => PaneContent::Signatures(Ok(Vec::new())),
        };
    };
    match pane {
        NavigationPane::Thumbnails | NavigationPane::SearchResults => PaneContent::Live,
        NavigationPane::Bookmarks => PaneContent::Bookmarks(message(canvas.model.outline())),
        NavigationPane::Attachments => {
            PaneContent::Attachments(message(canvas.model.attachments()))
        }
        NavigationPane::Layers => PaneContent::Layers(message(canvas.model.layers())),
        NavigationPane::Signatures => PaneContent::Signatures(message(canvas.model.signatures())),
    }
}

/// A reader's failure, as the sentence the pane shows. The error type does
/// not survive into the snapshot: nothing above here would match on it, and
/// keeping it would make the snapshot borrow the canvas's error enum.
fn message<T>(result: Result<T, CanvasError>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

/// Run one pane action against the active document.
///
/// The single entry point from the frame, so the pane surface owns its rules
/// about what a click does and the frame owns only which tab is active.
pub(in crate::shell) fn apply(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    directory: Option<PathBuf>,
    action: PaneAction,
    cx: &mut Context<ShellFrame>,
) {
    match action {
        PaneAction::Select(pane) => {
            if state.active == Some(pane) {
                state.close();
            } else {
                match canvas {
                    Some(canvas) => {
                        canvas.update(cx, |canvas, _cx| state.open(pane, Some(canvas)));
                    }
                    None => state.open(pane, None),
                }
            }
        }
        PaneAction::DismissMenus => {
            state.layers_menu = None;
            state.thumbnails.dismiss_menu();
        }
        PaneAction::GoToPage(page) => {
            navigate(state, canvas, cx, move |canvas| {
                canvas.model.go_to_page(page)
            });
        }
        PaneAction::SelectMatch(page, index) => {
            navigate(state, canvas, cx, move |canvas| {
                canvas.model.select_match(page, index)
            });
        }
        PaneAction::Attachment(action) => {
            attachments::run(state, canvas, directory, action, cx);
        }
        PaneAction::Layer(action) => layers::run(state, canvas, action, cx),
        PaneAction::Thumbnail(action) => thumbnails::run(state, canvas, action, cx),
    }
    cx.notify();
}

/// Move the view, and put whatever it refused into the pane's feedback line.
fn navigate(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
    change: impl FnOnce(&mut Canvas) -> Result<bool, CanvasError>,
) {
    let Some(canvas) = canvas else {
        return;
    };
    let failure = canvas.update(cx, |canvas, cx| {
        let changed = change(canvas);
        let failure = changed.as_ref().err().map(ToString::to_string);
        canvas.handle_change(changed, cx);
        failure
    });
    state.feedback = failure;
}

/// What the navigation column tells a screen reader.
///
/// Built from the same state, and in the same order, as
/// [`render_navigation_panes`]: the strip walks `NavigationPane::ALL` and each
/// pane body walks the list its render walks, so the rectangles the strip
/// reports after prepaint land on the buttons they were measured from.
pub(in crate::shell) fn accessible(
    state: &NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    rects: &Rects,
    cx: &Context<ShellFrame>,
) -> Element {
    let mut strip = Element::new("navigation-pane-strip", Role::TabList, "Navigation Panes")
        .with_children(
            NavigationPane::ALL
                .into_iter()
                .map(|pane| {
                    // The strip draws only `pane.icon()`, which a screen
                    // reader reads as punctuation, so the name is the label.
                    Element::new(pane.element_id(), Role::Tab, pane.label())
                        .with_state(A11yState::selected(state.active == Some(pane)))
                        .with_activation(Activation::Pane(PaneAction::Select(pane)))
                })
                .collect(),
        );
    rects.place(Surface::PaneStrip, &mut strip);

    let mut column = Element::new("navigation-panes", Role::Navigation, "Navigation").child(strip);
    if let Some(pane) = state.active {
        column = column.child(
            Element::new("navigation-pane-body", Role::TabPanel, pane.label())
                .with_children(accessible_body(state, pane, canvas, cx)),
        );
    }
    if let Some(feedback) = state.feedback.as_ref() {
        column = column.child(Element::new(
            "navigation-pane-feedback",
            Role::Alert,
            feedback.clone(),
        ));
    }
    column
}

/// The open pane's own description, dispatched the way [`render_body`]
/// dispatches its drawing.
fn accessible_body(
    state: &NavigationPanesState,
    pane: NavigationPane,
    canvas: Option<&Entity<Canvas>>,
    cx: &Context<ShellFrame>,
) -> Vec<Element> {
    match (pane, state.content.as_ref()) {
        (NavigationPane::Thumbnails, _) => thumbnails::accessible(state, canvas, cx),
        (NavigationPane::SearchResults, _) => results::accessible(canvas, cx),
        (NavigationPane::Bookmarks, Some(PaneContent::Bookmarks(items))) => {
            bookmarks::accessible(items.as_deref())
        }
        (NavigationPane::Attachments, Some(PaneContent::Attachments(items))) => {
            attachments::accessible(items.as_deref())
        }
        (NavigationPane::Layers, Some(PaneContent::Layers(items))) => {
            layers::accessible(items.as_deref(), state.layers_menu.is_some())
        }
        (NavigationPane::Signatures, Some(PaneContent::Signatures(items))) => {
            signatures::accessible(items.as_deref())
        }
        _ => vec![Element::new(
            "navigation-pane-empty",
            Role::Label,
            NOTHING_YET,
        )],
    }
}

/// The navigation column: the button strip, and the open pane beside it.
pub(in crate::shell) fn render_navigation_panes(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    height: Pixels,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    // The frame is the only thing that knows how tall the column ended up,
    // and how many thumbnail rows fit is the one thing that depends on it.
    state.body_height = f32::from(height);

    let mut strip = div()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::PaneStrip, &bounds, window);
        })
        .id("navigation-pane-strip")
        .w(px(STRIP_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .pt_2()
        .gap_1()
        .bg(theme.surface)
        .text_color(theme.text);
    for pane in NavigationPane::ALL {
        let active = state.active == Some(pane);
        strip = strip.child(
            div()
                .id(pane.element_id())
                .w(px(36.0))
                .h(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .cursor_pointer()
                .when(active, |button| button.bg(theme.selected))
                .hover(move |button| button.bg(theme.hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(
                        Activation::Pane(PaneAction::Select(pane)),
                        window,
                        cx,
                    );
                }))
                .child(pane.icon()),
        );
    }

    let body = state.active.map(|pane| {
        let mut body = div()
            .id("navigation-pane-body")
            .relative()
            .w(px(BODY_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.raised)
            .text_color(theme.text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|frame, _event, _window, cx| {
                    frame.run_pane_action(PaneAction::DismissMenus, cx);
                }),
            )
            .child(
                div()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .px_2()
                    .text_sm()
                    .text_color(theme.secondary_text)
                    .child(pane.label()),
            )
            .child(render_body(state, pane, canvas, theme, cx));
        if let Some(feedback) = state.feedback.as_ref() {
            body = body.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .bg(theme.error_surface)
                    .text_color(theme.error_text)
                    .child(feedback.clone()),
            );
        }
        if let Some(at) = state.layers_menu {
            body = body.child(layers::render_menu(at, theme, cx));
        }
        body
    });

    div()
        .h_full()
        .flex_none()
        .flex()
        .child(strip)
        .when_some(body, |column, body| column.child(body))
}

fn render_body(
    state: &NavigationPanesState,
    pane: NavigationPane,
    canvas: Option<&Entity<Canvas>>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    match (pane, state.content.as_ref()) {
        (NavigationPane::Thumbnails, _) => thumbnails::render(state, canvas, theme, cx),
        (NavigationPane::SearchResults, _) => results::render(canvas, theme, cx),
        (NavigationPane::Bookmarks, Some(PaneContent::Bookmarks(items))) => {
            bookmarks::render(items.as_deref(), theme, cx)
        }
        (NavigationPane::Attachments, Some(PaneContent::Attachments(items))) => {
            attachments::render(items.as_deref(), theme, cx)
        }
        (NavigationPane::Layers, Some(PaneContent::Layers(items))) => {
            layers::render(items.as_deref(), theme, cx)
        }
        (NavigationPane::Signatures, Some(PaneContent::Signatures(items))) => {
            signatures::render(items.as_deref(), theme)
        }
        // The snapshot is always the open pane's, taken when it opened, so
        // the mismatched arms are unreachable rather than a state to draw.
        _ => empty_message(NOTHING_YET, theme).into_any_element(),
    }
}

/// The one shape every pane uses for "nothing here", so a document without
/// bookmarks and a document without attachments read the same way.
fn empty_message(message: &str, theme: ThemeTokens) -> gpui::AnyElement {
    div()
        .p_2()
        .text_xs()
        .text_color(theme.muted_text)
        .child(message.to_owned())
        .into_any_element()
}

/// A reader's failure, drawn where its list would have been.
fn error_message(message: &str, theme: ThemeTokens) -> gpui::AnyElement {
    div()
        .p_2()
        .text_xs()
        .bg(theme.error_surface)
        .text_color(theme.error_text)
        .child(message.to_owned())
        .into_any_element()
}

/// The scrolling body every list pane sits in.
fn list(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
}

/// One menu entry, drawn the way the shell's other context menus draw one: a
/// disabled entry is present, greyed, and carries its reason on a second line
/// rather than being hidden.
fn menu_row(
    menu: &'static str,
    id: usize,
    label: &'static str,
    availability: MenuAvailability,
    activation: Activation,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let enabled = availability.is_enabled();
    let mut row = div()
        .id((menu, id))
        .min_h(px(ROW_HEIGHT))
        .flex()
        .flex_col()
        .justify_center()
        .px_2()
        .rounded_sm()
        .text_sm()
        .text_color(if enabled {
            theme.text
        } else {
            theme.disabled_text
        })
        .child(label);
    if let Some(reason) = availability.reason() {
        row = row.child(
            div()
                .text_xs()
                .text_color(theme.muted_text)
                .child(reason.to_owned()),
        );
    }
    if enabled {
        row = row
            .cursor_pointer()
            .hover(move |row| row.bg(theme.hover))
            .on_click(cx.listener(move |frame, _event, window, cx| {
                frame.run_activation(activation.clone(), window, cx);
            }));
    }
    row
}

/// The described half of a pane's context menu, built from the same label,
/// availability and action each drawn row takes, so the menu cannot be drawn
/// with one set of entries and announced with another.
fn menu_element(
    id: &'static str,
    name: &'static str,
    entry: &'static str,
    entries: impl IntoIterator<Item = (&'static str, MenuAvailability, Activation)>,
) -> Element {
    Element::new(id, Role::Menu, name).with_children(
        entries
            .into_iter()
            .enumerate()
            .map(|(index, (label, availability, activation))| {
                let row = Element::new((entry, index), Role::MenuItem, label)
                    .with_state(A11yState::enabled(availability.is_enabled()))
                    .with_activation(activation);
                match availability.reason() {
                    Some(reason) => row.with_description(reason),
                    None => row,
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "shell-test-support")]
    use crate::preferences::ThemePreference;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::ShellSettings;

    /// The strip is always there, so the column never collapses to nothing
    /// and the buttons stay reachable with every pane closed.
    #[test]
    fn the_column_keeps_its_strip_when_no_pane_is_open() {
        let mut state = NavigationPanesState::default();

        assert_eq!(state.active, None);
        assert_eq!(state.width(), px(STRIP_WIDTH));

        state.open(NavigationPane::Bookmarks, None);
        assert_eq!(state.active, Some(NavigationPane::Bookmarks));
        assert_eq!(state.width(), px(STRIP_WIDTH + BODY_WIDTH));

        state.close();
        assert_eq!(state.width(), px(STRIP_WIDTH));
    }

    /// Parity rows 191 to 228 name six panes. Pinned to the count in the rows
    /// rather than to `ALL`, which would agree with itself while quietly
    /// dropping one.
    #[test]
    fn every_named_pane_has_a_button_a_label_and_its_own_element_id() {
        assert_eq!(NavigationPane::ALL.len(), 6);

        let mut ids: Vec<&str> = NavigationPane::ALL
            .iter()
            .map(|pane| pane.element_id())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 6, "two panes share an element id");

        for pane in NavigationPane::ALL {
            assert!(!pane.label().is_empty());
            assert!(!pane.icon().is_empty());
        }
    }

    /// Switching documents drops what the pane read from the old one: a
    /// bookmark list from another tab would navigate this one to pages that
    /// mean nothing in it.
    #[test]
    fn switching_documents_drops_the_snapshot_but_keeps_the_pane_open() {
        let mut state = NavigationPanesState::default();
        state.open(NavigationPane::Bookmarks, None);
        state.feedback = Some("stale".to_owned());

        state.document_changed();

        assert_eq!(state.active, Some(NavigationPane::Bookmarks));
        assert_eq!(state.content, None);
        assert_eq!(state.feedback, None);
    }

    #[cfg(feature = "shell-test-support")]
    fn frame_over(
        bytes: Vec<u8>,
        cx: &mut gpui::TestAppContext,
    ) -> (Entity<ShellFrame>, &mut gpui::VisualTestContext) {
        use gpui::AppContext as _;
        use onionskin_core::{Document, ViewSize};
        use onionskin_plugin_api::PluginRegistry;

        use super::super::canvas::CanvasModel;
        use super::super::chrome::ShellViewState;

        let document = Document::open_bytes(bytes).expect("the fixture opens");
        let model = CanvasModel::new(
            document,
            PluginRegistry::new(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("the canvas starts");
        let shell_view = ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System);
        let theme = shell_view.tokens();
        cx.add_window_view(move |window, cx| {
            let canvas = cx.new(|_| Canvas::new(model, theme));
            ShellFrame::new(
                vec![(PathBuf::from("fixture.pdf"), canvas)],
                shell_view,
                ShellSettings::defaults(),
                window,
                cx,
            )
        })
    }

    /// The button opens the pane and reads the document once; the same button
    /// puts it away. Driven through a real frame rather than by calling
    /// `open`, so the reader, the snapshot and the button are all on the path.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_pane_button_reads_the_document_and_the_same_button_closes_it(
        cx: &mut gpui::TestAppContext,
    ) {
        let (frame, cx) = frame_over(super::super::fixtures::outline_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let state = frame.read(app).navigation();
            let items = state.bookmarks().expect("the pane read the outline");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].page, Some(0));
            assert_eq!(items[1].page, Some(2), "the second bookmark names page 3");
            assert_eq!(state.width(), px(STRIP_WIDTH + BODY_WIDTH));
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let state = frame.read(app).navigation();
            assert_eq!(state.active, None);
            assert_eq!(state.content, None);
            assert_eq!(state.width(), px(STRIP_WIDTH));
        });
    }

    /// Clicking a bookmark moves the view to the page the file named, and it
    /// is a navigation, so Previous View comes back from it.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn clicking_a_bookmark_goes_to_the_page_it_names(cx: &mut gpui::TestAppContext) {
        let (frame, cx) = frame_over(super::super::fixtures::outline_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            });
        });
        cx.run_until_parked();

        let target = cx.update(|_window, app| {
            let frame = frame.read(app);
            assert_eq!(
                frame
                    .active_canvas()
                    .expect("a tab")
                    .read(app)
                    .model
                    .viewport()
                    .current_page(),
                0,
                "the view starts on the first page, so the jump has somewhere to go"
            );
            let items = frame
                .navigation()
                .bookmarks()
                .expect("the pane read the outline");
            bookmarks::rows(items)[1].page.expect("the row navigates")
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::GoToPage(target), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let frame = frame.read(app);
            let canvas = frame.active_canvas().expect("a tab").read(app);
            assert_eq!(canvas.model.viewport().current_page(), 2);
            assert!(
                canvas.model.can_previous_view(),
                "the jump is a navigation Previous View returns from"
            );
            assert_eq!(frame.navigation().feedback, None);
        });
    }

    /// The laziness claim, through the real pane on a real document: opening
    /// the thumbnails of a thousand-page file asks the worker for the rows on
    /// screen and for nothing else.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_thumbnails_pane_asks_only_for_the_rows_it_shows(cx: &mut gpui::TestAppContext) {
        let (frame, cx) = frame_over(super::super::fixtures::many_pages_pdf(1_000), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Thumbnails), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let frame = frame.read(app);
            let band = frame.navigation().thumbnails.requested_band();
            assert!(!band.is_empty(), "the pane asked for the rows it draws");
            assert!(band.len() < 20, "a screenful, not a document: {band:?}");

            let canvas = frame.active_canvas().expect("a tab").read(app);
            for page in band.clone() {
                assert!(
                    canvas.model.thumbnail_pending(page)
                        || frame.navigation().thumbnails.has_image(page),
                    "page {page} is on screen and was never asked for"
                );
            }
            for page in [band.end + 50, 500, 999] {
                assert!(
                    !canvas.model.thumbnail_pending(page),
                    "page {page} is nowhere near the screen and was asked for anyway"
                );
            }
        });
    }

    /// The whole thumbnail path, end to end: the pane asks, the render worker
    /// answers on its own channel, and the collector the canvas observer runs
    /// turns the raster into a picture the pane holds.
    ///
    /// Driven rather than reasoned about, because every step of it is on a
    /// different thread or a different callback: a green unit test on the
    /// band arithmetic would say nothing about whether a picture ever
    /// arrives.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_requested_thumbnail_arrives_and_becomes_a_picture(cx: &mut gpui::TestAppContext) {
        let (frame, cx) = frame_over(super::super::fixtures::outline_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Thumbnails), cx);
            });
        });
        // The band comes from the pane's own frame, deferred out of the
        // render that drew it, so nothing here says which rows to ask for.
        cx.run_until_parked();
        let band =
            cx.update(|_window, app| frame.read(app).navigation().thumbnails.requested_band());
        assert_eq!(
            band,
            0..3,
            "every row of a three-page document is on screen"
        );

        // The worker is a real thread, so the pictures arrive when they
        // arrive. Collect on a deadline rather than after a fixed wait.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            cx.update(|_window, app| {
                frame.update(app, |frame, cx| frame.collect_thumbnails(cx));
            });
            cx.run_until_parked();
            let arrived =
                cx.update(|_window, app| frame.read(app).navigation().thumbnails.image_count());
            if arrived == band.len() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "only {arrived} of the {} requested thumbnails ever arrived",
                band.len()
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        cx.update(|_window, app| {
            let frame = frame.read(app);
            let canvas = frame.active_canvas().expect("a tab").read(app);
            for page in 0..3 {
                assert!(
                    frame.navigation().thumbnails.has_image(page),
                    "page {page} was asked for and never drew"
                );
                assert!(
                    !canvas.model.thumbnail_pending(page),
                    "a collected thumbnail is no longer outstanding"
                );
            }
        });
    }

    /// Clicking a result makes that hit current, on the cursor the find bar
    /// reads. Driven through the frame's own action so the whole path is
    /// under test: the row's coordinates, the canvas call and the reveal.
    ///
    /// The hit clicked is on the second page and is not the first hit found,
    /// because a click that always landed on the first hit would satisfy a
    /// weaker test and is exactly what a broken row-to-cursor mapping does.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn clicking_a_search_result_makes_that_hit_current(cx: &mut gpui::TestAppContext) {
        let (frame, cx) = frame_over(super::super::fixtures::text_pages_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::SearchResults), cx);
            });
            let canvas = frame.read(app).active_canvas().expect("a tab").clone();
            canvas.update(app, |canvas, _cx| {
                canvas
                    .model
                    .start_search("alpha", onionskin_core::SearchOptions::default())
                    .expect("the search worker starts");
            });
        });

        // The walk runs on its own thread; the canvas applies what it has
        // produced on every update.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            cx.update(|_window, app| {
                let canvas = frame.read(app).active_canvas().expect("a tab").clone();
                canvas.update(app, |canvas, _cx| {
                    canvas.model.update().expect("the canvas updates");
                });
            });
            cx.run_until_parked();
            let found = cx.update(|_window, app| {
                let canvas = frame.read(app).active_canvas().expect("a tab").read(app);
                (
                    canvas.model.search().len(),
                    canvas.model.search().is_running(),
                )
            });
            if found == (3, false) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the walk found {found:?} rather than three hits and a finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::SelectMatch(1, 0), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let canvas = frame.read(app).active_canvas().expect("a tab").read(app);
            assert_eq!(canvas.model.search().cursor(), Some((1, 0)));
            assert_eq!(
                canvas
                    .model
                    .search()
                    .current()
                    .expect("the hit is there")
                    .page,
                1
            );
            // The cursor the pane moved is the one the find bar steps from.
            assert_eq!(canvas.model.search().current_ordinal(), Some(3));
        });
    }

    /// A toggle in the pane reaches the document and puts the pane's own
    /// pictures back in step: they were rendered under the visibility that
    /// just changed, so a pane that kept them would show the old layers
    /// beside a document showing the new ones.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn toggling_a_layer_in_the_pane_reaches_the_document_and_drops_its_pictures(
        cx: &mut gpui::TestAppContext,
    ) {
        let (frame, cx) = frame_over(super::super::fixtures::optional_content_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Layers), cx);
                // Stand in for a pane that has already drawn a row: the band
                // is what a toggle has to make it ask for again.
                frame.navigation_mut().thumbnails.set_requested_band(0..1);
            });
        });
        cx.run_until_parked();

        let layer = cx.update(|_window, app| {
            let layers = frame
                .read(app)
                .navigation()
                .layers()
                .expect("the pane read the layers");
            assert_eq!(layers.len(), 1);
            assert!(layers[0].visible, "the file shows it by default");
            layers[0].id
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(
                    PaneAction::Layer(LayerAction::SetVisible {
                        layer,
                        visible: false,
                    }),
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let frame = frame.read(app);
            let layers = frame
                .navigation()
                .layers()
                .expect("the pane read the layers");
            assert!(
                !layers[0].visible,
                "the pane re-read the document rather than keeping what it drew"
            );
            assert_eq!(
                frame.navigation().thumbnails.requested_band(),
                0..0,
                "the pictures were produced under the old visibility"
            );
            assert_eq!(frame.navigation().thumbnails.image_count(), 0);
            assert_eq!(frame.navigation().feedback, None);
        });
    }

    /// A group the document locked refuses the toggle, and says so where the
    /// user clicked. Silently ignoring it would leave a control that looks
    /// live and does nothing.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_locked_layer_refuses_the_toggle_and_says_so_in_the_pane(cx: &mut gpui::TestAppContext) {
        let (frame, cx) = frame_over(super::super::fixtures::locked_layer_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Layers), cx);
            });
        });
        cx.run_until_parked();

        let layer = cx.update(|_window, app| {
            let layers = frame
                .read(app)
                .navigation()
                .layers()
                .expect("the pane read the layers");
            assert!(layers[0].locked);
            assert!(!layers::availability(&layers[0]).is_enabled());
            layers[0].id
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(
                    PaneAction::Layer(LayerAction::SetVisible {
                        layer,
                        visible: false,
                    }),
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            let frame = frame.read(app);
            let feedback = frame
                .navigation()
                .feedback
                .as_deref()
                .expect("a refused toggle says why");
            assert!(feedback.contains("locked"), "said {feedback:?}");
            assert!(
                frame.navigation().layers().expect("the layers read")[0].visible,
                "the visibility the document locked is unchanged"
            );
        });
    }

    /// The strip draws six glyphs and nothing else, so a reader given the
    /// buttons as drawn hears punctuation. Every button is announced by its
    /// pane's name, and the open one is the one announced as selected.
    ///
    /// Driven through a real frame because the strip's description has to
    /// agree with the strip the frame drew, not with a state built here.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_pane_strip_button_is_announced_by_its_pane_name_and_not_by_its_glyph(
        cx: &mut gpui::TestAppContext,
    ) {
        let (frame, cx) = frame_over(super::super::fixtures::outline_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                let described = accessible(
                    frame.navigation(),
                    frame.active_canvas(),
                    &Rects::default(),
                    cx,
                );

                let strip = described
                    .find(&"navigation-pane-strip".into())
                    .expect("the strip is described");
                assert_eq!(strip.children.len(), NavigationPane::ALL.len());
                for (button, pane) in strip.children.iter().zip(NavigationPane::ALL) {
                    assert_eq!(button.key, gpui::ElementId::from(pane.element_id()));
                    assert_eq!(button.label, pane.label());
                    assert_ne!(button.label, pane.icon(), "the glyph is not a name");
                    assert!(
                        button.label.chars().any(char::is_alphabetic),
                        "{} is announced as {:?}",
                        pane.element_id(),
                        button.label
                    );
                    assert_eq!(
                        button.state.selected,
                        Some(pane == NavigationPane::Bookmarks)
                    );
                    assert_eq!(
                        button.activation,
                        Some(Activation::Pane(PaneAction::Select(pane)))
                    );
                }
            });
        });
    }

    /// The open pane is described as the panel the strip's selected button
    /// opened, and its rows are the rows the frame drew: the band the pane
    /// asked the worker for is the band it is drawing.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn the_open_panes_described_rows_are_the_rows_the_frame_drew(
        cx: &mut gpui::TestAppContext,
    ) {
        let (frame, cx) = frame_over(super::super::fixtures::many_pages_pdf(1_000), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Thumbnails), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                let band = frame.navigation().thumbnails.requested_band();
                assert!(!band.is_empty(), "the frame drew rows to describe");
                let described = accessible(
                    frame.navigation(),
                    frame.active_canvas(),
                    &Rects::default(),
                    cx,
                );

                let body = described
                    .find(&"navigation-pane-body".into())
                    .expect("an open pane is described");
                assert_eq!(body.label, NavigationPane::Thumbnails.label());

                let rows = described
                    .find(&"thumbnail-rows".into())
                    .expect("the rows are described");
                assert_eq!(rows.children.len(), band.len());
                for (row, page) in rows.children.iter().zip(band) {
                    assert_eq!(row.label, format!("Page {}", page + 1));
                    assert_eq!(
                        row.activation,
                        Some(Activation::Pane(PaneAction::GoToPage(page)))
                    );
                }
                let current = frame
                    .active_canvas()
                    .map(|canvas| canvas.read(cx).model.viewport().current_page())
                    .expect("a tab is open");
                let selected: Vec<&str> = rows
                    .children
                    .iter()
                    .filter(|row| row.state.selected == Some(true))
                    .map(|row| row.label.as_str())
                    .collect();
                assert_eq!(
                    selected,
                    [format!("Page {}", current + 1).as_str()],
                    "exactly the page the view is on is announced as selected"
                );
            });
        });
    }

    /// A pane the strip has not opened has no panel, so a reader is not
    /// offered a body that is not on screen.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_closed_column_describes_its_strip_and_no_panel(cx: &mut gpui::TestAppContext) {
        let (frame, cx) = frame_over(super::super::fixtures::outline_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                let described = accessible(
                    frame.navigation(),
                    frame.active_canvas(),
                    &Rects::default(),
                    cx,
                );

                assert_eq!(described.children.len(), 1);
                assert!(described.find(&"navigation-pane-body".into()).is_none());
                for pane in NavigationPane::ALL {
                    let button = described
                        .find(&pane.element_id().into())
                        .unwrap_or_else(|| panic!("{} is described", pane.element_id()));
                    assert_eq!(button.state.selected, Some(false));
                }
            });
        });
    }

    /// What an action refused is announced as an alert, because the pane's
    /// feedback line is the only place it is shown and a reader that missed
    /// it would be left with a control that looks live and does nothing.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_refused_action_is_announced_as_an_alert(cx: &mut gpui::TestAppContext) {
        let (frame, cx) = frame_over(super::super::fixtures::locked_layer_pdf(), cx);

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Layers), cx);
            });
        });
        cx.run_until_parked();

        let layer = cx.update(|_window, app| {
            frame
                .read(app)
                .navigation()
                .layers()
                .expect("the pane read the layers")[0]
                .id
        });
        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_pane_action(
                    PaneAction::Layer(LayerAction::SetVisible {
                        layer,
                        visible: false,
                    }),
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                let feedback = frame
                    .navigation()
                    .feedback
                    .clone()
                    .expect("a refused toggle says why");
                let described = accessible(
                    frame.navigation(),
                    frame.active_canvas(),
                    &Rects::default(),
                    cx,
                );

                let alert = described
                    .find(&"navigation-pane-feedback".into())
                    .expect("the feedback line is announced");
                assert_eq!(alert.role, Role::Alert);
                assert_eq!(alert.label, feedback);
            });
        });
    }

    /// The two panes whose contents change while they are open hold no
    /// snapshot, so they cannot show a stale one.
    #[test]
    fn the_live_panes_hold_nothing_and_the_reading_panes_hold_a_list() {
        for pane in [NavigationPane::Thumbnails, NavigationPane::SearchResults] {
            assert_eq!(read(pane, None), PaneContent::Live, "{}", pane.label());
        }

        assert_eq!(
            read(NavigationPane::Bookmarks, None),
            PaneContent::Bookmarks(Ok(Vec::new()))
        );
        assert_eq!(
            read(NavigationPane::Layers, None),
            PaneContent::Layers(Ok(Vec::new()))
        );
    }
}
