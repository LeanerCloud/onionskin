//! The Home view: what the window shows with no document open.
//!
//! Acrobat's Home is the first thing its users meet, and the plan's candor
//! list flags that the M2 text never mentioned it. What it carries here is
//! the two rows the scoreboard puts in this milestone: Recents, and the
//! list/thumbnail toggle over them. Starred is M3, and everything
//! cloud-tethered is out of scope, so there are no other sections to show.
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

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, img, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, RenderImage,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{BaseRaster, Document};
use smallvec::smallvec;

use super::chrome::{ShellFrame, ThemeTokens};
use crate::recents::Recents;

/// How wide a thumbnail card's page is, in pixels. The render is done at the
/// scale that produces this width so the card is not resampling a raster
/// many times its size.
const THUMBNAIL_WIDTH: f32 = 132.0;

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

pub(in crate::shell) fn render_home(
    state: &HomeState,
    recents: &Recents,
    home: Option<&Path>,
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
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    frame.set_home_view(view, cx);
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
                        frame.open_from_home(window, cx);
                    }))
                    .child("Open File…"),
            ),
        );

    let body = if recents.is_empty() {
        div()
            .text_color(theme.secondary_text)
            .child("No documents yet. Open one, and it will be here next time.")
            .into_any_element()
    } else {
        match state.view() {
            HomeView::List => list(recents, home, theme, cx).into_any_element(),
            HomeView::Thumbnail => thumbnails(state, recents, theme, cx).into_any_element(),
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
}

fn list(
    recents: &Recents,
    home: Option<&Path>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut rows = div().flex().flex_col().gap_1();
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
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    frame.open_recent(index, cx);
                }))
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
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut grid = div().flex().flex_wrap().gap_3();
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
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    frame.open_recent(index, cx);
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
}
