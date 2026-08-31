//! The page thumbnails pane and its context menu.
//!
//! Rows are a fixed height for the size the pane is set to, so which rows are
//! on screen is arithmetic rather than something only the layout engine
//! knows. That is what keeps a thousand-page document costing a screenful of
//! renders: [`ThumbnailsState::visible_rows`] is the whole rule, and it is
//! the only thing that decides what the worker is asked for.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, img, px, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Point, RenderImage, ScrollWheelEvent,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::PageIndex;

use super::super::canvas::{raster_image, CanvasError};
use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::{empty_message, menu_row, NavigationPanesState, PaneAction};

/// Acrobat's Reduce and Enlarge Page Thumbnails walk a fixed set of sizes.
/// Each is the render zoom and the row height that holds it: a US Letter
/// page at 0.12 is 73 by 95 device pixels, which fits a 112-pixel row with
/// room for the page number under it.
const SIZES: [(f32, f32); 4] = [(0.12, 112.0), (0.18, 148.0), (0.26, 196.0), (0.36, 252.0)];
const DEFAULT_SIZE: usize = 1;
/// One row of slack above and below the visible band, so a scroll of less
/// than a row does not show an empty box before the render lands.
const OVERSCAN: usize = 1;
/// Several screens at any size, so scrolling back up finds the pictures still
/// there, and bounded so a long document does not keep every page it has ever
/// shown.
const MAX_IMAGES: usize = 120;

/// What a click or a scroll in this pane asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) enum ThumbnailAction {
    /// The rows now on screen. Raised by the pane itself when the band it
    /// draws differs from the band it last asked for.
    Show {
        first: usize,
        end: usize,
    },
    Scroll(f32),
    OpenMenu(Point<Pixels>),
    Run(ThumbnailsCommand),
}

/// Parity row 192's menu. Every entry Acrobat names is present; the ones that
/// change the document are disabled until `tools-organize` at M3, rather than
/// hidden, because a missing entry reads as "Onionskin does not have this".
///
/// Ten entries for the row's nine names: the row counts Reduce and Enlarge
/// Page Thumbnails as one item, and they are two commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ThumbnailsCommand {
    InsertPages,
    ExtractPages,
    ReplacePages,
    DeletePages,
    RotatePages,
    CropPages,
    PageProperties,
    EmbedThumbnails,
    ReduceThumbnails,
    EnlargeThumbnails,
}

impl ThumbnailsCommand {
    pub(in crate::shell) const ALL: [Self; 10] = [
        Self::InsertPages,
        Self::ExtractPages,
        Self::ReplacePages,
        Self::DeletePages,
        Self::RotatePages,
        Self::CropPages,
        Self::PageProperties,
        Self::EmbedThumbnails,
        Self::ReduceThumbnails,
        Self::EnlargeThumbnails,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::InsertPages => "Insert Pages",
            Self::ExtractPages => "Extract Pages",
            Self::ReplacePages => "Replace Pages",
            Self::DeletePages => "Delete Pages",
            Self::RotatePages => "Rotate Pages",
            Self::CropPages => "Crop Pages",
            Self::PageProperties => "Page Properties",
            Self::EmbedThumbnails => "Embed All Page Thumbnails",
            Self::ReduceThumbnails => "Reduce Page Thumbnails",
            Self::EnlargeThumbnails => "Enlarge Page Thumbnails",
        }
    }

    /// Whether the entry does anything yet, and what it says when it does
    /// not.
    ///
    /// The two size commands are the pane's own and are live. Every other
    /// entry writes to the document, which no M2 subsystem does: the edit
    /// graph and `tools-organize` are M3, so the reason names that milestone
    /// rather than pretending there is a capability to query.
    pub(in crate::shell) fn availability(self, state: &ThumbnailsState) -> MenuAvailability {
        match self {
            Self::ReduceThumbnails => {
                available(state.size > 0, "Already at the smallest thumbnail size")
            }
            Self::EnlargeThumbnails => available(
                state.size + 1 < SIZES.len(),
                "Already at the largest thumbnail size",
            ),
            _ => MenuAvailability::Disabled("Available in M3 tools-organize"),
        }
    }
}

fn available(live: bool, reason: &'static str) -> MenuAvailability {
    if live {
        MenuAvailability::Enabled
    } else {
        MenuAvailability::Disabled(reason)
    }
}

/// One page's picture, at the size it was rendered.
#[derive(Clone)]
struct Thumbnail {
    image: Arc<RenderImage>,
    width: f32,
    height: f32,
}

impl std::fmt::Debug for Thumbnail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thumbnail")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(in crate::shell) struct ThumbnailsState {
    /// Pixels scrolled down from the first row.
    scroll: f32,
    size: usize,
    images: BTreeMap<PageIndex, Thumbnail>,
    /// The band the pane last asked the worker for. Kept so the pane asks
    /// once per band rather than once per frame.
    requested: Range<usize>,
    menu: Option<Point<Pixels>>,
}

impl Default for ThumbnailsState {
    fn default() -> Self {
        Self {
            scroll: 0.0,
            size: DEFAULT_SIZE,
            images: BTreeMap::new(),
            requested: 0..0,
            menu: None,
        }
    }
}

impl ThumbnailsState {
    fn row_height(&self) -> f32 {
        SIZES[self.size].1
    }

    fn zoom(&self) -> f32 {
        SIZES[self.size].0
    }

    /// The rows a pane `height` tall shows, given where it is scrolled to.
    ///
    /// One row of overscan at each end, and never past the document. This is
    /// the whole laziness rule: a thousand-page document asks for what this
    /// returns and nothing else.
    fn visible_rows(&self, height: f32, page_count: usize) -> Range<usize> {
        if page_count == 0 || height <= 0.0 {
            return 0..0;
        }
        let row = self.row_height();
        let first = (self.scroll / row).floor().max(0.0) as usize;
        let first = first.saturating_sub(OVERSCAN).min(page_count - 1);
        let spanned = (height / row).ceil() as usize + 1 + OVERSCAN;
        let end = first.saturating_add(spanned).min(page_count);
        first..end
    }

    /// How far the rows may be scrolled, given the pane's height. A document
    /// shorter than the pane scrolls nowhere.
    fn max_scroll(&self, height: f32, page_count: usize) -> f32 {
        (page_count as f32 * self.row_height() - height).max(0.0)
    }

    fn scroll_by(&mut self, delta: f32, height: f32, page_count: usize) {
        self.scroll = (self.scroll - delta).clamp(0.0, self.max_scroll(height, page_count));
    }

    /// Take the pictures the worker has answered and turn them into images
    /// the window can paint. Returns whether anything arrived.
    pub(super) fn collect(&mut self, canvas: &mut Canvas) -> bool {
        let ready = canvas.model.take_thumbnails();
        if ready.is_empty() {
            return false;
        }
        for (page, raster) in ready {
            match raster_image(&raster) {
                Ok((image, width, height)) => {
                    self.images.insert(
                        page,
                        Thumbnail {
                            image,
                            width: width as f32,
                            height: height as f32,
                        },
                    );
                }
                // A picture the window cannot take is the canvas's own error
                // to report; the row keeps its placeholder.
                Err(error) => {
                    canvas.model.record_error(error);
                }
            }
        }
        self.evict();
        true
    }

    /// Keep the cache bounded, dropping the pages furthest from the band
    /// being shown first.
    fn evict(&mut self) {
        while self.images.len() > MAX_IMAGES {
            let middle = self.requested.start.midpoint(self.requested.end);
            let furthest = self
                .images
                .keys()
                .copied()
                .max_by_key(|page| page.abs_diff(middle))
                .expect("the cache is over its limit, so it is not empty");
            self.images.remove(&furthest);
        }
    }

    /// The band the pane last asked for, for the test that proves it asks
    /// for a screenful rather than a document.
    #[cfg(test)]
    pub(super) fn requested_band(&self) -> Range<usize> {
        self.requested.clone()
    }

    #[cfg(test)]
    pub(super) fn set_requested_band(&mut self, band: Range<usize>) {
        self.requested = band;
    }

    #[cfg(test)]
    pub(super) fn has_image(&self, page: PageIndex) -> bool {
        self.images.contains_key(&page)
    }

    #[cfg(test)]
    pub(super) fn image_count(&self) -> usize {
        self.images.len()
    }

    /// Put away the context menu, without touching anything else.
    pub(super) fn dismiss_menu(&mut self) {
        self.menu = None;
    }

    /// Drop everything read from a document that is no longer showing.
    pub(super) fn clear(&mut self) {
        self.scroll = 0.0;
        self.images.clear();
        self.requested = 0..0;
        self.menu = None;
    }

    /// Drop the pictures without dropping the scroll position, which is what
    /// a size change needs: the rows stay where they are, the pictures are
    /// all the wrong size.
    pub(super) fn invalidate_images(&mut self) {
        self.images.clear();
        self.requested = 0..0;
    }
}

pub(super) fn run(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    action: ThumbnailAction,
    cx: &mut Context<ShellFrame>,
) {
    match action {
        ThumbnailAction::Show { first, end } => {
            state.thumbnails.requested = first..end;
            request_band(state, canvas, cx);
        }
        ThumbnailAction::Scroll(delta) => {
            let page_count = page_count(canvas, cx);
            let height = state.body_height;
            state.thumbnails.scroll_by(delta, height, page_count);
            state.thumbnails.menu = None;
        }
        ThumbnailAction::OpenMenu(at) => state.thumbnails.menu = Some(at),
        ThumbnailAction::Run(command) => {
            state.thumbnails.menu = None;
            let size = state.thumbnails.size;
            match command {
                ThumbnailsCommand::ReduceThumbnails => {
                    state.thumbnails.size = size.saturating_sub(1);
                }
                ThumbnailsCommand::EnlargeThumbnails => {
                    state.thumbnails.size = (size + 1).min(SIZES.len() - 1);
                }
                // Every other entry is disabled, so nothing can raise it.
                _ => return,
            }
            if state.thumbnails.size != size {
                // The pictures were rendered at the old zoom; keeping them
                // would show a stretched row until each one was replaced.
                // Nothing is asked for here: the rows a bigger size shows are
                // not the rows the old one did, and the frame that draws them
                // is what says which.
                state.thumbnails.invalidate_images();
            }
        }
    }
}

/// Ask the worker for every row in the current band that has no picture and
/// none on the way.
fn request_band(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
) {
    let Some(canvas) = canvas else {
        return;
    };
    let zoom = state.thumbnails.zoom();
    let missing: Vec<PageIndex> = state
        .thumbnails
        .requested
        .clone()
        .filter(|page| !state.thumbnails.images.contains_key(page))
        .collect();
    let outcome = canvas.update(cx, |canvas, _cx| {
        for page in missing {
            if canvas.model.thumbnail_pending(page) {
                continue;
            }
            canvas.model.request_thumbnail(page, zoom)?;
        }
        Ok::<(), CanvasError>(())
    });
    if let Err(error) = outcome {
        state.feedback = Some(error.to_string());
    }
}

fn page_count(canvas: Option<&Entity<Canvas>>, cx: &Context<ShellFrame>) -> usize {
    canvas.map_or(0, |canvas| canvas.read(cx).model.viewport().page_count())
}

pub(super) fn render(
    state: &NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let thumbnails = &state.thumbnails;
    let page_count = page_count(canvas, cx);
    if page_count == 0 {
        return div()
            .flex_1()
            .child(empty_message("No document is open.", theme))
            .into_any_element();
    }
    let current = canvas.map(|canvas| canvas.read(cx).model.viewport().current_page());
    let visible = thumbnails.visible_rows(state.body_height, page_count);
    let row_height = thumbnails.row_height();

    // Asking for a band changes the pane's state, which a frame being drawn
    // may not do. Deferred to the end of the effect cycle, where it becomes
    // an ordinary action; the band it asks for is the one this frame is
    // drawing, and the frame after it asks for nothing, so this settles.
    if visible != thumbnails.requested {
        let (first, end) = (visible.start, visible.end);
        let frame = cx.entity();
        cx.defer(move |cx| {
            frame.update(cx, |frame, cx| {
                frame.run_pane_action(
                    PaneAction::Thumbnail(ThumbnailAction::Show { first, end }),
                    cx,
                );
            });
        });
    }

    let mut rows = div()
        .id("thumbnail-rows")
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .on_scroll_wheel(
            cx.listener(move |frame, event: &ScrollWheelEvent, _window, cx| {
                let delta = f32::from(event.delta.pixel_delta(px(row_height)).y);
                frame.run_pane_action(PaneAction::Thumbnail(ThumbnailAction::Scroll(delta)), cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|frame, event: &gpui::MouseDownEvent, _window, cx| {
                frame.run_pane_action(
                    PaneAction::Thumbnail(ThumbnailAction::OpenMenu(event.position)),
                    cx,
                );
            }),
        );

    // The band starts wherever the scroll left it, so the first drawn row is
    // pushed down by however much of it is scrolled past.
    let above = visible.start as f32 * row_height - thumbnails.scroll;
    rows = rows.child(div().h(px(above.max(0.0))).flex_none());

    for page in visible {
        let picture = thumbnails.images.get(&page).cloned();
        rows = rows.child(
            div()
                .id(("thumbnail-row", page))
                .h(px(row_height))
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .cursor_pointer()
                .when(current == Some(page), |row| row.bg(theme.selected))
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    frame.run_pane_action(PaneAction::GoToPage(page), cx);
                }))
                .child(match picture {
                    Some(picture) => img(picture.image)
                        .w(px(picture.width))
                        .h(px(picture.height))
                        .into_any_element(),
                    // A page whose picture has not arrived keeps its row, so
                    // the rows below do not jump under the pointer when it
                    // does.
                    None => div()
                        .w(px(row_height * 0.62))
                        .h(px(row_height - 24.0))
                        .bg(theme.surface)
                        .into_any_element(),
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child(format!("{}", page + 1)),
                ),
        );
    }

    let mut body = div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(rows);
    if let Some(at) = thumbnails.menu {
        body = body.child(render_menu(thumbnails, at, theme, cx));
    }
    body.into_any_element()
}

fn render_menu(
    state: &ThumbnailsState,
    at: Point<Pixels>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut menu = div()
        .id("thumbnail-context-menu")
        .absolute()
        .top(at.y)
        .left(px(4.0))
        .w(px(228.0))
        .p_1()
        .rounded_md()
        .occlude()
        .bg(theme.raised)
        .text_color(theme.text);
    for (index, command) in ThumbnailsCommand::ALL.into_iter().enumerate() {
        menu = menu.child(menu_row(
            "thumbnail-menu-entry",
            index,
            command.label(),
            command.availability(state),
            theme,
            cx,
            move |frame, cx| {
                frame.run_pane_action(PaneAction::Thumbnail(ThumbnailAction::Run(command)), cx);
            },
        ));
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(size: usize, scroll: f32) -> ThumbnailsState {
        ThumbnailsState {
            scroll,
            size,
            ..ThumbnailsState::default()
        }
    }

    /// The laziness claim, as arithmetic: a thousand-page document shows a
    /// screenful of rows and asks for nothing else.
    #[test]
    fn a_thousand_page_document_shows_only_the_rows_on_screen() {
        let pane = state(DEFAULT_SIZE, 0.0);

        let visible = pane.visible_rows(700.0, 1_000);

        assert_eq!(visible.start, 0);
        // 700 over 148 is 4.7 rows, rounded up, plus one partial row and one
        // of overscan.
        assert_eq!(visible.end, 7);
        assert!(
            visible.len() < 20,
            "a screenful, not a document: {visible:?}"
        );
    }

    /// Scrolled into the middle, the band moves with the scroll and keeps a
    /// row of slack above it.
    #[test]
    fn scrolling_moves_the_band_and_keeps_a_row_of_slack_above_it() {
        let pane = state(DEFAULT_SIZE, 148.0 * 40.0);

        let visible = pane.visible_rows(700.0, 1_000);

        assert_eq!(visible.start, 39, "one row of overscan above the first");
        assert_eq!(visible.end, 46);
    }

    #[test]
    fn the_band_never_runs_past_the_last_page() {
        let pane = state(DEFAULT_SIZE, 148.0 * 998.0);

        let visible = pane.visible_rows(700.0, 1_000);

        assert_eq!(visible.end, 1_000);
        assert!(visible.start < 1_000);
    }

    #[test]
    fn a_document_shorter_than_the_pane_shows_every_page_and_scrolls_nowhere() {
        let mut pane = state(DEFAULT_SIZE, 0.0);

        assert_eq!(pane.visible_rows(700.0, 3), 0..3);
        assert_eq!(pane.max_scroll(700.0, 3), 0.0);

        pane.scroll_by(-500.0, 700.0, 3);
        assert_eq!(pane.scroll, 0.0, "there is nothing below to scroll to");
        assert_eq!(pane.visible_rows(700.0, 3), 0..3);
    }

    /// A wheel that would take the rows past the end stops at the end, and
    /// one that would take them above the first stops at the first.
    #[test]
    fn scrolling_clamps_at_both_ends() {
        let mut pane = state(DEFAULT_SIZE, 0.0);

        pane.scroll_by(-1_000_000.0, 700.0, 1_000);
        assert_eq!(pane.scroll, pane.max_scroll(700.0, 1_000));

        pane.scroll_by(1_000_000.0, 700.0, 1_000);
        assert_eq!(pane.scroll, 0.0);
    }

    /// A pane with no height, or a document with no pages, asks for nothing
    /// rather than for row zero of an empty list.
    #[test]
    fn an_empty_document_or_a_collapsed_pane_shows_no_rows() {
        let pane = state(DEFAULT_SIZE, 0.0);

        assert_eq!(pane.visible_rows(700.0, 0), 0..0);
        assert_eq!(pane.visible_rows(0.0, 1_000), 0..0);
    }

    /// A bigger size shows fewer rows and renders them larger, which is the
    /// point of the command.
    #[test]
    fn each_size_step_changes_the_rows_and_the_render_zoom_together() {
        let small = state(0, 0.0);
        let large = state(SIZES.len() - 1, 0.0);

        assert!(small.row_height() < large.row_height());
        assert!(small.zoom() < large.zoom());
        assert!(small.visible_rows(700.0, 1_000).len() > large.visible_rows(700.0, 1_000).len());
    }

    /// Parity row 192 names nine items; the size item is two commands, so ten
    /// entries carry them. Every one is present, and every disabled one says
    /// why.
    #[test]
    fn every_menu_entry_is_present_and_every_disabled_one_says_why() {
        let pane = state(DEFAULT_SIZE, 0.0);

        assert_eq!(ThumbnailsCommand::ALL.len(), 10);
        for command in ThumbnailsCommand::ALL {
            assert!(!command.label().is_empty());
            let availability = command.availability(&pane);
            if !availability.is_enabled() {
                assert!(
                    !availability.reason().expect("disabled says why").is_empty(),
                    "{} has an empty reason",
                    command.label()
                );
            }
        }
    }

    /// The page-mutating entries wait on M3 and say so. Asserted on the
    /// milestone rather than the whole sentence: pinning the prose would keep
    /// passing once `tools-organize` lands, which is the state this test
    /// exists to catch.
    #[test]
    fn every_page_editing_entry_is_disabled_and_names_the_milestone() {
        let pane = state(DEFAULT_SIZE, 0.0);

        for command in [
            ThumbnailsCommand::InsertPages,
            ThumbnailsCommand::ExtractPages,
            ThumbnailsCommand::ReplacePages,
            ThumbnailsCommand::DeletePages,
            ThumbnailsCommand::RotatePages,
            ThumbnailsCommand::CropPages,
            ThumbnailsCommand::PageProperties,
            ThumbnailsCommand::EmbedThumbnails,
        ] {
            let reason = command
                .availability(&pane)
                .reason()
                .expect("a page-editing entry is disabled");
            assert!(
                reason.contains("M3"),
                "{} should name the milestone it waits on, said {reason:?}",
                command.label()
            );
        }
    }

    /// The two size commands are the pane's own, so they are live, and they
    /// stop at the ends of the size list with a reason rather than silently
    /// doing nothing.
    #[test]
    fn the_size_commands_are_live_until_the_ends_of_the_size_list() {
        let middle = state(DEFAULT_SIZE, 0.0);
        assert!(ThumbnailsCommand::ReduceThumbnails
            .availability(&middle)
            .is_enabled());
        assert!(ThumbnailsCommand::EnlargeThumbnails
            .availability(&middle)
            .is_enabled());

        let smallest = state(0, 0.0);
        assert!(!ThumbnailsCommand::ReduceThumbnails
            .availability(&smallest)
            .is_enabled());
        assert!(ThumbnailsCommand::EnlargeThumbnails
            .availability(&smallest)
            .is_enabled());

        let largest = state(SIZES.len() - 1, 0.0);
        assert!(ThumbnailsCommand::ReduceThumbnails
            .availability(&largest)
            .is_enabled());
        assert!(!ThumbnailsCommand::EnlargeThumbnails
            .availability(&largest)
            .is_enabled());
    }

    /// The cache is bounded, and what it drops is the page furthest from what
    /// is being shown rather than whatever the map happened to hold first.
    #[test]
    fn the_image_cache_drops_the_pages_furthest_from_the_band() {
        let mut pane = state(DEFAULT_SIZE, 0.0);
        pane.requested = 500..510;
        for page in 0..MAX_IMAGES + 5 {
            pane.images.insert(page * 10, thumbnail());
        }

        pane.evict();

        assert_eq!(pane.images.len(), MAX_IMAGES);
        assert!(
            pane.images.contains_key(&500),
            "a page in the band survives"
        );
        // Page 1240 is 735 rows from the middle of the band and page 0 is
        // 505, so the far end goes first and the near end stays. A cache
        // that dropped by key order would have taken page 0.
        assert!(!pane.images.contains_key(&1_240), "the furthest page went");
        assert!(pane.images.contains_key(&0), "the nearer end stayed");
    }

    fn thumbnail() -> Thumbnail {
        Thumbnail {
            image: Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
                image::RgbaImage::new(1, 1)
            )])),
            width: 1.0,
            height: 1.0,
        }
    }
}
