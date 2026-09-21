//! The Home view: what the window shows with no document open.
//!
//! Acrobat's Home is the first thing its users meet, and the plan's candor
//! list flags that the M2 text never mentioned it. What it carries here is
//! Recents with the list/thumbnail toggle over them, and Starred: documents
//! the user starred, kept on this machine rather than in a cloud account.
//! Everything cloud-tethered is out of scope, so there are no other
//! sections to show.
//!
//! Thumbnails are the first page of each recent document, rendered once
//! per document and kept until the window closes, including the failures:
//! a file that is gone is gone every time it is asked about, and asking
//! again on every open would turn a missing volume into ten synchronous
//! opens per File > Open. Switching views is the retry.
//!
//! That render is synchronous. With the default list of ten it is a few
//! hundred milliseconds once, on an explicit click, and moving it to the
//! render worker means a worker per recent document for a view the user may
//! never open. Worth revisiting if the list limit grows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, img, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, RenderImage,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{BaseRaster, Document};
use smallvec::smallvec;

use super::chrome::accessible::{Activation, Element, Rects, Surface};
use super::chrome::{ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;
use crate::recents::Recents;

/// How wide a thumbnail card's page is, in pixels. The render is done at the
/// scale that produces this width so the card is not resampling a raster
/// many times its size.
const THUMBNAIL_WIDTH: f32 = 132.0;

/// What the Open button says, and what stands in for an empty list. Named here
/// so the pixels and the node describing them cannot say different things.
const OPEN_LABEL: &str = "Open File…";
const EMPTY_MESSAGE: &str = "No documents yet. Open one, and it will be here next time.";
const NO_STARS: &str = "Star a recent document to keep it here.";

/// What a star button says, for the state it would change.
fn star_label(starred: bool) -> &'static str {
    if starred {
        "Unstar"
    } else {
        "Star"
    }
}

fn file_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum HomeView {
    #[default]
    List,
    Thumbnail,
}

impl HomeView {
    pub(in crate::shell) const ALL: [Self; 2] = [Self::List, Self::Thumbnail];

    fn label(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::Thumbnail => "Thumbnails",
        }
    }
}

/// What a thumbnail card shows: the page, or why there is no page.
type Thumbnail = Result<Arc<RenderImage>, String>;

#[derive(Default)]
pub(in crate::shell) struct HomeState {
    view: HomeView,
    thumbnails: HashMap<PathBuf, Thumbnail>,
}

impl HomeState {
    pub(in crate::shell) fn view(&self) -> HomeView {
        self.view
    }

    /// Switch views, rendering whatever the new one needs and does not
    /// have.
    ///
    /// Choosing Thumbnails is also the retry: a file that was on a volume
    /// that was not mounted gets another chance here, and nowhere else,
    /// because this is the one place the user asked for the work.
    pub(in crate::shell) fn set_view(&mut self, view: HomeView, recents: &Recents) {
        self.view = view;
        if view == HomeView::Thumbnail {
            self.thumbnails.retain(|_, thumbnail| thumbnail.is_ok());
        }
        self.refresh(recents);
    }

    /// Render any thumbnail the current view wants and does not have.
    ///
    /// Called when the recents list changes as well: a document opened
    /// after the last toggle would otherwise sit behind a blank card until
    /// the user switched views and back. A card that already failed keeps
    /// its reason rather than being asked again, so this does no work on a
    /// list it has already seen.
    pub(in crate::shell) fn refresh(&mut self, recents: &Recents) {
        if self.view != HomeView::Thumbnail {
            return;
        }
        self.thumbnails.retain(|path, _| {
            recents
                .documents()
                .iter()
                .any(|recent| &recent.path == path)
        });
        for recent in recents.documents() {
            if !self.thumbnails.contains_key(&recent.path) {
                let thumbnail = render_thumbnail(&recent.path);
                self.thumbnails.insert(recent.path.clone(), thumbnail);
            }
        }
    }

    pub(in crate::shell) fn thumbnail(&self, path: &Path) -> Option<&Thumbnail> {
        self.thumbnails.get(path)
    }
}

/// The first page of `path`, small.
///
/// A recent document can have been deleted, moved or damaged since it was
/// last opened, and the card says which rather than showing an empty box.
fn render_thumbnail(path: &Path) -> Thumbnail {
    let mut document =
        Document::open_path(path).map_err(|error| format!("cannot be opened: {error}"))?;
    let geometry = document
        .page_geometry(0)
        .map_err(|error| format!("page one cannot be read: {error}"))?;
    let width = geometry.render_size.0;
    if !(width.is_finite() && width > 0.0) {
        return Err("page one has no width".to_owned());
    }
    let zoom = THUMBNAIL_WIDTH / width as f32;
    let render = document
        .render_page_now(0, zoom)
        .map_err(|error| format!("page one cannot be rendered: {error}"))?;
    image_for(&render.raster)
}

/// GPUI takes image data as BGRA; the renderer produces RGBA.
///
/// A raster carries its own size, and `BaseRaster::new` asserts the bytes
/// match it, so the size check here should never fire. It is a `Result`
/// rather than an assumption because this runs on a click and the card has
/// somewhere to put the reason.
fn image_for(raster: &BaseRaster) -> Thumbnail {
    let mut bgra = raster.rgba().to_vec();
    let expected = raster.width() as usize * raster.height() as usize * 4;
    if bgra.len() != expected {
        return Err(format!(
            "page one rendered {} bytes for a {}x{} image",
            bgra.len(),
            raster.width(),
            raster.height()
        ));
    }
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(raster.width(), raster.height(), bgra)
        .ok_or_else(|| "page one cannot be turned into an image".to_owned())?;
    Ok(Arc::new(RenderImage::new(smallvec![image::Frame::new(
        buffer
    )])))
}

/// What Home tells a screen reader.
///
/// The recents are a list rather than the flat run of buttons they are drawn
/// as, so a screen reader can say how many there are and step through them.
pub(in crate::shell) fn accessible(
    state: &HomeState,
    recents: &Recents,
    home: Option<&Path>,
    rects: &Rects,
) -> Element {
    // The toggles are wrapped rather than left loose: AccessKit gives a tab
    // its tab-group semantics from its `TabList` parent, and without one they
    // read as bare radio buttons.
    let mut views = Element::new("home-views", Role::TabList, "Recents View");
    for (index, view) in HomeView::ALL.into_iter().enumerate() {
        views = views.child(
            Element::new(("home-view", index), Role::Tab, view.label())
                .with_state(A11yState::selected(state.view() == view))
                .with_activation(Activation::SetHomeView(view)),
        );
    }
    let mut root = Element::new("home", Role::Main, "Home").child(views);
    root = root.child(
        Element::new("home-open", Role::Button, OPEN_LABEL)
            .with_activation(Activation::OpenFromHome),
    );

    let thumbnail_view = state.view() == HomeView::Thumbnail;
    let mut list = Element::new("home-recents", Role::List, "Recents");
    for (index, recent) in recents.documents().iter().enumerate() {
        let key = if thumbnail_view {
            ("home-thumbnail", index)
        } else {
            ("home-recent", index)
        };
        // A card that could not be rendered shows the reason where the page
        // would be, so that is what it says instead of the path.
        let description = match state.thumbnail(&recent.path) {
            Some(Err(reason)) if thumbnail_view => reason.clone(),
            _ => recent.display_path(home),
        };
        let starred = recents.is_starred(&recent.path);
        list = list.child(
            Element::new(key, Role::ListItem, recent.title())
                .with_description(description)
                .with_activation(Activation::OpenRecent(index))
                .child(
                    Element::new(("home-star", index), Role::Button, star_label(starred))
                        .with_state(A11yState::toggled(starred))
                        .with_activation(Activation::ToggleStar(recent.path.clone())),
                ),
        );
    }
    rects.place(Surface::Home, &mut list);

    root = root.child(list);
    if recents.is_empty() {
        // The message is drawn where the rows would be, and stays out of the
        // list so that no stale rectangle can be paired with it.
        root = root.child(Element::new("home-empty", Role::Label, EMPTY_MESSAGE));
    }
    root.child(accessible_starred(recents, home))
}

/// The Starred section, for a screen reader.
fn accessible_starred(recents: &Recents, home: Option<&Path>) -> Element {
    let mut starred = Element::new("home-starred", Role::List, "Starred");
    for (index, path) in recents.starred().iter().enumerate() {
        starred = starred.child(
            Element::new(
                ("home-starred-row", index),
                Role::ListItem,
                file_title(path),
            )
            .with_description(crate::recents::abbreviate_path(path, home))
            .with_activation(Activation::OpenStarred(index))
            .child(
                Element::new(("home-unstar", index), Role::Button, star_label(true))
                    .with_state(A11yState::toggled(true))
                    .with_activation(Activation::ToggleStar(path.clone())),
            ),
        );
    }
    if recents.starred().is_empty() {
        starred = starred.with_description(NO_STARS);
    }
    starred
}

pub(in crate::shell) fn render_home(
    state: &HomeState,
    recents: &Recents,
    home: Option<&Path>,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut toggle = div().flex().gap_1();
    for (index, view) in HomeView::ALL.into_iter().enumerate() {
        let selected = state.view() == view;
        toggle = toggle.child(
            div()
                .id(("home-view", index))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .when(selected, |button| button.bg(theme.selected))
                .hover(move |button| button.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::SetHomeView(view), window, cx);
                }))
                .child(view.label()),
        );
    }

    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .pb_3()
        .child(div().text_lg().child("Recents"))
        .child(
            div().flex().gap_3().child(toggle).child(
                div()
                    .id("home-open")
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(theme.selected)
                    .hover(move |button| button.bg(theme.hover))
                    .on_click(cx.listener(|frame, _event, window, cx| {
                        frame.run_activation(Activation::OpenFromHome, window, cx);
                    }))
                    .child(OPEN_LABEL),
            ),
        );

    let body = if recents.is_empty() {
        div()
            .text_color(theme.secondary_text)
            .child(EMPTY_MESSAGE)
            .into_any_element()
    } else {
        match state.view() {
            HomeView::List => list(recents, home, rects, theme, cx).into_any_element(),
            HomeView::Thumbnail => thumbnails(state, recents, rects, theme, cx).into_any_element(),
        }
    };

    div()
        .id("home")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p_4()
        .bg(theme.surface)
        .text_color(theme.text)
        .child(header)
        .child(body)
        .child(starred(recents, home, theme, cx))
}

/// A star that toggles without opening the row it sits on.
fn star(
    id: impl Into<gpui::ElementId>,
    path: &Path,
    starred: bool,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let path = path.to_path_buf();
    div()
        .id(id)
        .px_1()
        .cursor_pointer()
        .text_color(if starred {
            theme.text
        } else {
            theme.muted_text
        })
        .hover(move |star| star.bg(theme.subtle_hover))
        .on_click(cx.listener(move |frame, _event, window, cx| {
            cx.stop_propagation();
            frame.run_activation(Activation::ToggleStar(path.clone()), window, cx);
        }))
        .child(if starred { "★" } else { "☆" })
}

/// The Starred section, drawn.
fn starred(
    recents: &Recents,
    home: Option<&Path>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut section = div()
        .pt_4()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_lg().pb_2().child("Starred"));
    if recents.starred().is_empty() {
        return section.child(div().text_color(theme.secondary_text).child(NO_STARS));
    }
    for (index, path) in recents.starred().iter().enumerate() {
        section = section.child(
            div()
                .id(("home-starred-row", index))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |row| row.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::OpenStarred(index), window, cx);
                }))
                .child(star(("home-unstar", index), path, true, theme, cx))
                .child(div().flex_none().child(file_title(path)))
                .child(
                    div()
                        .flex_1()
                        .text_right()
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child(crate::recents::abbreviate_path(path, home)),
                ),
        );
    }
    section
}

fn list(
    recents: &Recents,
    home: Option<&Path>,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut rows =
        div()
            .flex()
            .flex_col()
            .gap_1()
            .on_children_prepainted(move |bounds, window, _cx| {
                rects.record(Surface::Home, &bounds, window);
            });
    for (index, recent) in recents.documents().iter().enumerate() {
        rows = rows.child(
            div()
                .id(("home-recent", index))
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |row| row.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::OpenRecent(index), window, cx);
                }))
                .child(star(
                    ("home-star", index),
                    &recent.path,
                    recents.is_starred(&recent.path),
                    theme,
                    cx,
                ))
                .child(div().flex_none().child(recent.title()))
                .child(
                    div()
                        .flex_1()
                        .text_right()
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child(recent.display_path(home)),
                ),
        );
    }
    rows
}

fn thumbnails(
    state: &HomeState,
    recents: &Recents,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut grid =
        div()
            .flex()
            .flex_wrap()
            .gap_3()
            .on_children_prepainted(move |bounds, window, _cx| {
                rects.record(Surface::Home, &bounds, window);
            });
    for (index, recent) in recents.documents().iter().enumerate() {
        let page = match state.thumbnail(&recent.path) {
            Some(Ok(image)) => img(Arc::clone(image))
                .w(px(THUMBNAIL_WIDTH))
                .into_any_element(),
            Some(Err(reason)) => div()
                .w(px(THUMBNAIL_WIDTH))
                .p_1()
                .text_xs()
                .text_color(theme.error_text)
                .child(reason.clone())
                .into_any_element(),
            None => div()
                .w(px(THUMBNAIL_WIDTH))
                .h(px(THUMBNAIL_WIDTH))
                .bg(theme.canvas)
                .into_any_element(),
        };
        grid = grid.child(
            div()
                .id(("home-thumbnail", index))
                .w(px(THUMBNAIL_WIDTH + 16.0))
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .p_2()
                .rounded_sm()
                .cursor_pointer()
                .bg(theme.raised)
                .hover(move |card| card.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::OpenRecent(index), window, cx);
                }))
                .child(page)
                .child(div().text_xs().child(recent.title())),
        );
    }
    grid
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    fn seed(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(name)
    }

    /// The toggle is not decoration: switching to thumbnails produces a real
    /// page image for every recent document, at the width the card draws.
    #[test]
    fn switching_to_thumbnails_renders_the_first_page_of_each_recent() {
        let mut recents = Recents::default();
        recents
            .record(&seed("hello.pdf"), UNIX_EPOCH, 10)
            .expect("the seed path records");
        let mut state = HomeState::default();

        state.set_view(HomeView::Thumbnail, &recents);

        let thumbnail = state
            .thumbnail(&seed("hello.pdf"))
            .expect("the card asked for a thumbnail")
            .as_ref()
            .expect("the seed renders");
        let size = thumbnail.size(0);
        assert_eq!(u32::from(size.width), THUMBNAIL_WIDTH as u32);
        assert!(u32::from(size.height) > 0);
    }

    /// A failure is kept rather than retried on every refresh, and choosing
    /// the view again is the retry.
    ///
    /// refresh runs whenever a document is opened, so a list of ten files on
    /// an unmounted volume would otherwise mean ten opens of nothing per
    /// File > Open. The file is made to appear between the two calls,
    /// because a re-render of a still-missing file fails again and would
    /// look exactly like keeping the failure.
    #[test]
    fn a_failed_card_is_retried_when_the_view_is_chosen_again_and_not_before() {
        let dir = crate::config::test_dir("home-retry");
        let path = dir.join("appears-later.pdf");
        let _ = std::fs::remove_file(&path);
        let mut recents = Recents::default();
        recents
            .record(&path, UNIX_EPOCH, 10)
            .expect("the path records");
        let mut state = HomeState::default();
        state.set_view(HomeView::Thumbnail, &recents);
        assert!(
            state.thumbnail(&path).is_some_and(Result::is_err),
            "a missing file should have failed"
        );

        std::fs::copy(seed("hello.pdf"), &path).expect("the file appears");
        state.refresh(&recents);

        assert!(
            state.thumbnail(&path).is_some_and(Result::is_err),
            "refresh rendered a card it already had, which is the work \
             every open would repeat"
        );

        state.set_view(HomeView::Thumbnail, &recents);

        assert!(
            state.thumbnail(&path).is_some_and(Result::is_ok),
            "choosing the view again did not retry the failure"
        );
    }

    /// A recent document that has since been deleted still gets a card, with
    /// the reason on it. Dropping the row would look like the app forgot the
    /// file rather than that the file is gone.
    #[test]
    fn a_recent_document_that_is_gone_says_so_on_its_card() {
        let mut recents = Recents::default();
        recents
            .record(&seed("not-here.pdf"), UNIX_EPOCH, 10)
            .expect("the path records even though the file is missing");
        let mut state = HomeState::default();

        state.set_view(HomeView::Thumbnail, &recents);

        let failure = state
            .thumbnail(&seed("not-here.pdf"))
            .expect("the card asked")
            .as_ref()
            .expect_err("the file does not exist");
        assert!(failure.contains("cannot be opened"), "{failure}");
    }

    /// The row draws the name on the left and the path on the right, and a
    /// screen reader reaching only one of the two cannot tell two files with
    /// the same name apart.
    #[test]
    fn a_recent_row_announces_both_its_title_and_its_path() {
        let mut recents = Recents::default();
        recents
            .record(&seed("hello.pdf"), UNIX_EPOCH, 10)
            .expect("the seed path records");
        let state = HomeState::default();

        let described = accessible(&state, &recents, None, &Rects::default());

        let row = described.find(&("home-recent", 0usize).into()).unwrap();
        assert_eq!(row.role, Role::ListItem);
        assert_eq!(row.label, "hello.pdf");
        assert_eq!(
            row.description.as_deref(),
            Some(seed("hello.pdf").display().to_string().as_str())
        );
        assert_eq!(row.activation, Some(Activation::OpenRecent(0)));
    }

    /// The rows are keyed by the view that drew them, so the rectangles that
    /// view reported after prepaint cannot be paired with the other view's
    /// nodes.
    #[test]
    fn the_described_rows_are_keyed_as_the_view_that_drew_them() {
        let mut recents = Recents::default();
        recents
            .record(&seed("hello.pdf"), UNIX_EPOCH, 10)
            .expect("the seed path records");
        let mut state = HomeState::default();

        let listed = accessible(&state, &recents, None, &Rects::default());
        state.set_view(HomeView::Thumbnail, &recents);
        let carded = accessible(&state, &recents, None, &Rects::default());

        assert!(listed.find(&("home-recent", 0usize).into()).is_some());
        assert!(listed.find(&("home-thumbnail", 0usize).into()).is_none());
        assert!(carded.find(&("home-thumbnail", 0usize).into()).is_some());
        assert!(carded.find(&("home-recent", 0usize).into()).is_none());
    }

    /// The card shows the reason where the page would be, so that is what it
    /// says. Announcing the path instead would leave the failure invisible.
    #[test]
    fn a_card_that_could_not_be_rendered_says_why_instead_of_where_it_is() {
        let mut recents = Recents::default();
        recents
            .record(&seed("not-here.pdf"), UNIX_EPOCH, 10)
            .expect("the path records even though the file is missing");
        let mut state = HomeState::default();
        state.set_view(HomeView::Thumbnail, &recents);

        let described = accessible(&state, &recents, None, &Rects::default());

        let description = described
            .find(&("home-thumbnail", 0usize).into())
            .unwrap()
            .description
            .clone()
            .expect("the card says something");
        assert!(description.contains("cannot be opened"), "{description}");
    }

    /// The toggle shows which view is on with a background colour, which is
    /// nothing at all to a screen reader.
    #[test]
    fn the_view_toggle_carries_which_view_is_showing_as_state() {
        let recents = Recents::default();
        let mut state = HomeState::default();
        state.set_view(HomeView::Thumbnail, &recents);

        let described = accessible(&state, &recents, None, &Rects::default());

        let list = described.find(&("home-view", 0usize).into()).unwrap();
        let thumbnails = described.find(&("home-view", 1usize).into()).unwrap();
        assert_eq!(list.label, "List");
        assert_eq!(list.state.selected, Some(false));
        assert_eq!(thumbnails.label, "Thumbnails");
        assert_eq!(thumbnails.state.selected, Some(true));
        assert_eq!(
            thumbnails.activation,
            Some(Activation::SetHomeView(HomeView::Thumbnail))
        );
    }

    #[test]
    fn an_empty_home_says_so_and_still_offers_the_way_to_open_a_document() {
        let described = accessible(
            &HomeState::default(),
            &Recents::default(),
            None,
            &Rects::default(),
        );

        assert_eq!(
            described.find(&"home-empty".into()).map(|node| &node.label),
            Some(&EMPTY_MESSAGE.to_owned())
        );
        assert!(described
            .find(&"home-recents".into())
            .unwrap()
            .children
            .is_empty());
        let open = described.find(&"home-open".into()).unwrap();
        assert_eq!(open.label, OPEN_LABEL);
        assert_eq!(open.activation, Some(Activation::OpenFromHome));
    }

    /// The list view asks for nothing, so opening the app on Home costs no
    /// renders at all.
    #[test]
    fn the_list_view_renders_no_pages() {
        let mut recents = Recents::default();
        recents
            .record(&seed("hello.pdf"), UNIX_EPOCH, 10)
            .expect("the seed path records");
        let mut state = HomeState::default();

        state.set_view(HomeView::List, &recents);

        assert!(state.thumbnail(&seed("hello.pdf")).is_none());
    }

    /// Every recent row carries its star, and a starred document is listed
    /// under Starred with the way to open it and to unstar it.
    #[test]
    fn a_starred_recent_is_marked_on_its_row_and_listed_under_starred() {
        let mut recents = Recents::default();
        for name in ["hello.pdf", "minimal.pdf"] {
            recents
                .record(&seed(name), UNIX_EPOCH, 10)
                .expect("the seed path records");
        }
        recents.toggle_star(&seed("hello.pdf")).expect("stars");

        let described = accessible(&HomeState::default(), &recents, None, &Rects::default());

        let star = |index: usize| {
            described
                .find(&("home-star", index).into())
                .expect("each row has a star")
                .clone()
        };
        // Most recent first: minimal, then hello.
        assert_eq!(star(0).label, "Star");
        assert_eq!(star(0).state.toggled, Some(false));
        assert_eq!(star(1).label, "Unstar");
        assert_eq!(
            star(1).activation,
            Some(Activation::ToggleStar(seed("hello.pdf")))
        );
        let starred = described.find(&"home-starred".into()).expect("a section");
        assert_eq!(starred.children.len(), 1);
        assert_eq!(starred.children[0].label, "hello.pdf");
        assert_eq!(
            starred.children[0].activation,
            Some(Activation::OpenStarred(0))
        );
        assert!(starred.description.is_none());
    }

    #[test]
    fn with_nothing_starred_the_section_says_how_to_star() {
        let described = accessible(
            &HomeState::default(),
            &Recents::default(),
            None,
            &Rects::default(),
        );
        let starred = described.find(&"home-starred".into()).expect("a section");
        assert!(starred.children.is_empty());
        assert_eq!(starred.description.as_deref(), Some(NO_STARS));
    }
}
