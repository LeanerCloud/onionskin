//! P21 on a real window: Organize Pages opens a grid in the page's place,
//! selects with click, Shift and Cmd, reorders by dragging as one undo
//! step, keeps its selection honest across an undo, asks for only the
//! pages on screen, and the thumbnails pane's page entries run.

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

use super::*;
#[cfg(feature = "tools-organize")]
use crate::shell::chrome::accessible::Activation;
use crate::shell::organize::Held;
#[cfg(feature = "tools-organize")]
use crate::shell::organize::OrganizeAction;
#[cfg(feature = "tools-organize")]
use crate::shell::panes::{PaneAction, ThumbnailAction, ThumbnailsCommand};

/// A document of `count` pages, each saying "Page n" so an order can be
/// read back from the text.
pub(super) fn numbered(count: usize) -> Vec<u8> {
    let contents = (0..count)
        .map(|page| format!("BT /F1 18 Tf 20 40 Td (Page {}) Tj ET", page + 1).into_bytes())
        .collect();
    pdf_pages([0, 0, 200, 100], contents)
}

fn pdf_pages(media_box: [i64; 4], contents: Vec<Vec<u8>>) -> Vec<u8> {
    let dict = |entries: Vec<(&str, Object)>| {
        let mut dict = Dict::new();
        for (key, value) in entries {
            dict.set(Name::new(key), value);
        }
        dict
    };
    let reference = |number: u32| Object::Ref(ObjRef::new(number, 0));
    let count = contents.len();
    let mut objects = vec![
        (
            ObjRef::new(1, 0),
            Object::Dict(dict(vec![
                ("Type", Object::name("Catalog")),
                ("Pages", reference(2)),
            ])),
        ),
        (
            ObjRef::new(3, 0),
            Object::Dict(dict(vec![
                ("Type", Object::name("Font")),
                ("Subtype", Object::name("Type1")),
                ("BaseFont", Object::name("Helvetica")),
            ])),
        ),
    ];
    let mut kids = Vec::new();
    for (page, raw) in contents.into_iter().enumerate() {
        let (content, leaf) = (4 + 2 * page as u32, 5 + 2 * page as u32);
        objects.push((
            ObjRef::new(content, 0),
            Object::Stream(Stream {
                dict: Dict::new(),
                raw,
            }),
        ));
        objects.push((
            ObjRef::new(leaf, 0),
            Object::Dict(dict(vec![
                ("Type", Object::name("Page")),
                ("Parent", reference(2)),
                (
                    "MediaBox",
                    Object::Array(media_box.map(Object::Integer).to_vec()),
                ),
                (
                    "Resources",
                    Object::Dict(dict(vec![(
                        "Font",
                        Object::Dict(dict(vec![("F1", reference(3))])),
                    )])),
                ),
                ("Contents", reference(content)),
            ])),
        ));
        kids.push(reference(leaf));
    }
    objects.push((
        ObjRef::new(2, 0),
        Object::Dict(dict(vec![
            ("Type", Object::name("Pages")),
            ("Count", Object::Integer(count as i64)),
            ("Kids", Object::Array(kids)),
        ])),
    ));
    CosDocument::write_new(&objects, dict(vec![("Root", reference(1))])).expect("writes")
}

/// A single Letter page with visible colored marks at known source coordinates.
pub(super) fn letter_marked() -> Vec<u8> {
    pdf_pages(
        [0, 0, 612, 792],
        vec![b"q 1 0 0 rg 483 399 2 2 re f 0 0 1 rg 199 164 2 2 re f Q".to_vec()],
    )
}

/// A window on the first page of a numbered document.
fn window(pages: usize, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let window = bound_window_from_bytes(vec![("numbered.pdf", numbered(pages))], cx).0;
    window
        .update(cx, |frame, _window, cx| {
            frame.run_view_action(crate::shell::canvas::ViewAction::GoToPage(0), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

/// The pages in order, by what each says.
fn order(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, _window, cx| {
            let canvas = frame.tabs.active().unwrap().canvas.clone();
            canvas.update(cx, |canvas, _| {
                let mut doc = canvas.model.document_mut();
                (0..doc.page_count())
                    .map(|page| {
                        doc.page_text(page)
                            .expect("extracts")
                            .flatten()
                            .text
                            .trim()
                            .to_owned()
                    })
                    .collect()
            })
        })
        .unwrap()
}

/// Open the grid and let it be drawn, so its bounds are the real ones.
fn open_grid(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::OrganizePages, window, cx)
                .expect("opens");
        })
        .unwrap();
    cx.run_until_parked();
}

/// The middle of `page`'s cell, in window coordinates, from where the grid
/// was drawn and the grid's own arithmetic.
fn centre(
    window: gpui::WindowHandle<ShellFrame>,
    page: usize,
    cx: &mut TestAppContext,
) -> (f32, f32) {
    window
        .update(cx, |frame, _window, _cx| {
            let state = frame.organize_state().expect("open");
            let (left, top, width, _) = state.bounds.get().expect("drawn");
            let cell = frame.navigation().thumbnails().cell_size();
            let layout = crate::shell::organize::GridLayout::new(width, cell);
            let (x, y) = layout.origin(page);
            (left + x + cell / 2.0, top + y + cell / 2.0 - state.scroll)
        })
        .unwrap()
}

/// A point `dx` right of `page`'s cell's left edge, at its middle height.
fn into_cell(
    window: gpui::WindowHandle<ShellFrame>,
    page: usize,
    fraction: f32,
    cx: &mut TestAppContext,
) -> (f32, f32) {
    let (x, y) = centre(window, page, cx);
    let cell = window
        .update(cx, |frame, _window, _cx| {
            frame.navigation().thumbnails().cell_size()
        })
        .unwrap();
    (x - cell / 2.0 + cell * fraction, y)
}

fn selected(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<usize> {
    window
        .update(cx, |frame, _window, _cx| {
            frame.organize_state().expect("open").selected()
        })
        .unwrap()
}

#[gpui::test]
fn organize_pages_puts_a_described_grid_in_the_pages_place(cx: &mut TestAppContext) {
    let window = window(6, cx);
    open_grid(window, cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"document".into()).is_none(),
                "the page is not also there"
            );
            let pages = tree.find(&"organize-pages".into()).expect("the grid");
            let labels: Vec<_> = pages
                .children
                .iter()
                .map(|page| page.label.clone())
                .collect();
            assert_eq!(
                labels[..6],
                ["Page 1", "Page 2", "Page 3", "Page 4", "Page 5", "Page 6"]
            );
            assert_eq!(
                pages.children[0].state.selected,
                Some(true),
                "the page on screen"
            );
            assert_eq!(pages.children[1].state.selected, Some(false));
            let delete = tree.find(&"organize-delete".into()).expect("a toolbar");
            #[cfg(feature = "tools-organize")]
            assert!(!delete.state.disabled);
            #[cfg(not(feature = "tools-organize"))]
            assert_eq!(
                delete.description.as_deref(),
                Some(crate::shell::organize::NO_ORGANIZE)
            );
        })
        .unwrap();
    // Organize Pages again puts the page back.
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::OrganizePages, window, cx)
                .expect("closes");
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"organize".into()).is_none());
            assert!(tree.find(&"document".into()).is_some());
        })
        .unwrap();
}

#[gpui::test]
fn click_shift_click_and_cmd_click_select_on_the_grid(cx: &mut TestAppContext) {
    // Enough pages for a second row however wide the window is.
    let window = window(40, cx);
    open_grid(window, cx);
    let press = |page: usize, held: Held, cx: &mut TestAppContext| {
        let at = centre(window, page, cx);
        window
            .update(cx, |frame, _window, cx| {
                frame.grid_press(at, held, cx);
                frame.grid_release(at, cx);
            })
            .unwrap();
    };
    press(2, Held::Nothing, cx);
    press(5, Held::Extend, cx);
    assert_eq!(selected(window, cx), [2, 3, 4, 5]);
    press(3, Held::Toggle, cx);
    assert_eq!(selected(window, cx), [2, 4, 5]);
    // A marquee from the gap left of page 1, across page 2 and down a row.
    let (left_of_first, top) = into_cell(window, 0, -0.05, cx);
    let columns = window
        .update(cx, |frame, _window, _cx| {
            let (_, _, width, _) = frame.organize_state().unwrap().bounds.get().unwrap();
            crate::shell::organize::GridLayout::new(
                width,
                frame.navigation().thumbnails().cell_size(),
            )
            .columns
        })
        .unwrap();
    let below_second = centre(window, 1 + columns, cx);
    window
        .update(cx, |frame, _window, cx| {
            frame.grid_press((left_of_first, top), Held::Nothing, cx);
            frame.grid_drag(below_second, cx);
            frame.grid_release(below_second, cx);
        })
        .unwrap();
    assert_eq!(selected(window, cx), [0, 1, columns, columns + 1]);
}

#[cfg(feature = "tools-organize")]
#[gpui::test]
fn a_drag_is_one_reorder_and_one_undo_step(cx: &mut TestAppContext) {
    let window = window(6, cx);
    open_grid(window, cx);
    let start = centre(window, 0, cx);
    // The right half of page 3: after it.
    let drop = into_cell(window, 2, 0.8, cx);
    window
        .update(cx, |frame, _window, cx| {
            frame.grid_press(start, Held::Nothing, cx);
            // Every position on the way is a chance to reorder too early.
            for step in 1..=30 {
                let t = step as f32 / 30.0;
                frame.grid_drag(
                    (
                        start.0 + (drop.0 - start.0) * t,
                        start.1 + (drop.1 - start.1) * t,
                    ),
                    cx,
                );
            }
            frame.grid_release(drop, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        order(window, cx),
        ["Page 2", "Page 3", "Page 1", "Page 4", "Page 5", "Page 6"]
    );
    assert_eq!(selected(window, cx), [2], "the moved page, where it went");
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Undo, window, cx)
                .expect("undoes");
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        order(window, cx),
        ["Page 1", "Page 2", "Page 3", "Page 4", "Page 5", "Page 6"],
        "one undo takes the whole drag back"
    );
    window
        .update(cx, |frame, _window, cx| {
            let canvas = frame.tabs.active().unwrap().canvas.clone();
            assert_eq!(
                canvas.read(cx).model.history_facts().undo,
                None,
                "and there was only the one step"
            );
        })
        .unwrap();
}

#[cfg(feature = "tools-organize")]
#[gpui::test]
fn a_drag_released_outside_changes_nothing(cx: &mut TestAppContext) {
    let window = window(4, cx);
    open_grid(window, cx);
    let (from, to) = (centre(window, 0, cx), centre(window, 3, cx));
    window
        .update(cx, |frame, _window, cx| {
            frame.grid_press(from, Held::Nothing, cx);
            frame.grid_drag(to, cx);
            assert!(frame.cancel_grid_gesture(cx));
            frame.grid_release(to, cx);
        })
        .unwrap();
    assert_eq!(order(window, cx), ["Page 1", "Page 2", "Page 3", "Page 4"]);
}

#[cfg(feature = "tools-organize")]
#[gpui::test]
fn the_toolbar_edits_the_selection_and_an_undo_clamps_it_loudly(cx: &mut TestAppContext) {
    let window = window(4, cx);
    open_grid(window, cx);
    let act = |action: OrganizeAction, cx: &mut TestAppContext| {
        window
            .update(cx, |frame, window, cx| {
                frame.run_activation(Activation::Organize(action), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
    };
    act(OrganizeAction::Choose(3), cx);
    act(OrganizeAction::InsertBlank, cx);
    assert_eq!(order(window, cx).len(), 5);
    assert_eq!(selected(window, cx), [4], "the new page is chosen");
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Undo, window, cx)
                .expect("undoes");
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let state = frame.organize_state().expect("open");
            assert_eq!(state.selected(), [3], "clamped to what exists");
            let tree = frame.accessible(window, cx);
            let error = tree.find(&"organize-error".into()).expect("said");
            assert_eq!(error.label, "1 selected page is no longer in the document");
        })
        .unwrap();
    act(OrganizeAction::Choose(1), cx);
    act(OrganizeAction::Delete, cx);
    assert_eq!(order(window, cx), ["Page 1", "Page 3", "Page 4"]);
    assert_eq!(selected(window, cx), [1], "the page that took its place");
}

/// A thousand-page document: the grid asks for the pages on screen, and
/// only those, through the pane's cache.
#[gpui::test]
fn a_thousand_page_grid_asks_for_a_screenful(cx: &mut TestAppContext) {
    let window = window(1000, cx);
    open_grid(window, cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, _cx| {
            let band = frame.navigation().thumbnails().grid_band();
            assert!(!band.is_empty(), "it asked");
            let state = frame.organize_state().expect("open");
            let (_, _, width, height) = state.bounds.get().expect("drawn");
            let cell = frame.navigation().thumbnails().cell_size();
            let layout = crate::shell::organize::GridLayout::new(width, cell);
            assert_eq!(band, layout.visible(0.0, height, 1000), "what it draws");
            assert!(
                band.len() < 100,
                "a screenful and a row either side, not {band:?}"
            );
            assert_eq!(band.start, 0);
        })
        .unwrap();
}

/// The pane's page entries run on the page on screen, and Crop Pages alone
/// is still disabled, on M5.
#[cfg(feature = "tools-organize")]
#[gpui::test]
fn the_thumbnail_menus_page_entries_run_and_crop_still_waits(cx: &mut TestAppContext) {
    let window = window(3, cx);
    let run = |command: ThumbnailsCommand, cx: &mut TestAppContext| {
        window
            .update(cx, |frame, _window, cx| {
                frame.run_pane_action(PaneAction::Thumbnail(ThumbnailAction::Run(command)), cx);
            })
            .unwrap();
        cx.run_until_parked();
    };
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(
                Activation::Pane(PaneAction::Select(
                    crate::shell::panes::NavigationPane::Thumbnails,
                )),
                window,
                cx,
            );
            frame.run_pane_action(
                PaneAction::Thumbnail(ThumbnailAction::OpenMenu(gpui::point(px(10.0), px(10.0)))),
                cx,
            );
            let tree = frame.accessible(window, cx);
            let menu = tree.find(&"thumbnail-context-menu".into()).expect("open");
            for entry in &menu.children {
                if entry.label == "Crop Pages" {
                    assert!(entry.state.disabled);
                    assert!(entry
                        .description
                        .as_deref()
                        .unwrap_or_default()
                        .contains("M5"));
                } else if !entry.label.contains("Thumbnails") || entry.label.contains("All") {
                    assert!(!entry.state.disabled, "{} is live", entry.label);
                }
            }
        })
        .unwrap();
    run(ThumbnailsCommand::RotatePages, cx);
    run(ThumbnailsCommand::DeletePages, cx);
    assert_eq!(order(window, cx), ["Page 2", "Page 3"]);
    run(ThumbnailsCommand::EmbedThumbnails, cx);
    window
        .update(cx, |frame, _window, cx| {
            let canvas = frame.tabs.active().unwrap().canvas.clone();
            canvas.update(cx, |canvas, _| {
                let mut document = canvas.model.document_mut();
                let structure = document.structure().expect("structure");
                let page = structure.page(0).expect("page");
                assert!(page.dict.get(b"Thumb").is_some(), "embedded");
            });
        })
        .unwrap();
    run(ThumbnailsCommand::PageProperties, cx);
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(
                frame.dialog,
                Some(crate::shell::dialog::ShellDialog::PageProperties)
            );
            let tree = frame.accessible(window, cx);
            let rows: Vec<_> = tree
                .walk()
                .filter(|element| {
                    element.label.starts_with("Page:") || element.label.starts_with("Size:")
                })
                .map(|element| element.label.clone())
                .collect();
            assert_eq!(rows[0], "Page: 1 of 2");
        })
        .unwrap();
}
