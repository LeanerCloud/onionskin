//! The Organize Pages grid, drawn and described.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, img, px, Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _,
};

use super::{Gesture, GridLayout, OrganizeState};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element};
use crate::shell::chrome::{ShellFrame, ThemeTokens};
use crate::shell::panes::ThumbnailsState;

/// What a control on the grid does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum OrganizeAction {
    /// Choose one page, as a click does.
    Choose(usize),
    SelectAll,
    RotateCounterclockwise,
    RotateClockwise,
    Delete,
    InsertBlank,
    InsertFromFile,
    Extract,
    Replace,
    /// Crop Pages, on the chosen pages.
    Crop,
    /// Copy or move the chosen pages into another open document.
    CopyTo,
    MoveTo,
    Smaller,
    Larger,
    Close,
}

/// One toolbar button: what it says, what it does, and why it cannot when
/// it cannot.
pub(in crate::shell) struct Button {
    pub(in crate::shell) id: &'static str,
    pub(in crate::shell) label: &'static str,
    pub(in crate::shell) action: OrganizeAction,
    pub(in crate::shell) refusal: Option<&'static str>,
}

/// The toolbar, left to right. `document` is why the document's pages may
/// not be changed, when they may not; a button whose plugin this build lacks
/// says that instead.
pub(in crate::shell) fn buttons(
    document: Option<&'static str>,
    thumbnails: &ThumbnailsState,
) -> Vec<Button> {
    let edits = super::page_edit_refusal(document);
    let button = |id, label, action, refusal| Button {
        id,
        label,
        action,
        refusal,
    };
    let (smaller, larger) = thumbnails.size_limits();
    vec![
        button(
            "organize-rotate-left",
            "Rotate Counterclockwise",
            OrganizeAction::RotateCounterclockwise,
            edits,
        ),
        button(
            "organize-rotate-right",
            "Rotate Clockwise",
            OrganizeAction::RotateClockwise,
            edits,
        ),
        button("organize-delete", "Delete", OrganizeAction::Delete, edits),
        button(
            "organize-insert-blank",
            "Insert Blank Page",
            OrganizeAction::InsertBlank,
            edits,
        ),
        button(
            "organize-insert-file",
            "Insert From File…",
            OrganizeAction::InsertFromFile,
            edits,
        ),
        button(
            "organize-replace",
            "Replace…",
            OrganizeAction::Replace,
            edits,
        ),
        button(
            "organize-extract",
            "Extract…",
            OrganizeAction::Extract,
            edits,
        ),
        button(
            "organize-crop",
            "Crop Pages…",
            OrganizeAction::Crop,
            crate::shell::chrome::crop_dialog::crop_refusal(document),
        ),
        button(
            "organize-copy-to",
            "Copy To Document…",
            OrganizeAction::CopyTo,
            edits,
        ),
        button(
            "organize-move-to",
            "Move To Document…",
            OrganizeAction::MoveTo,
            edits,
        ),
        button(
            "organize-select-all",
            "Select All",
            OrganizeAction::SelectAll,
            None,
        ),
        button(
            "organize-smaller",
            "Smaller Thumbnails",
            OrganizeAction::Smaller,
            smaller,
        ),
        button(
            "organize-larger",
            "Larger Thumbnails",
            OrganizeAction::Larger,
            larger,
        ),
        button(
            "organize-close",
            "Close Organize Pages",
            OrganizeAction::Close,
            None,
        ),
    ]
}

fn activation(action: OrganizeAction) -> Activation {
    Activation::Organize(action)
}

/// The grid, for a screen reader: the toolbar, then one item per page on
/// screen named by its number, with its selection.
pub(in crate::shell) fn accessible(
    state: &OrganizeState,
    thumbnails: &ThumbnailsState,
    page_count: usize,
    edits: Option<&'static str>,
) -> Element {
    let toolbar = buttons(edits, thumbnails)
        .into_iter()
        .map(|button| {
            let element = Element::new(button.id, Role::Button, button.label)
                .with_state(A11yState::enabled(button.refusal.is_none()))
                .with_activation(activation(button.action));
            match button.refusal {
                Some(reason) => element.with_description(reason),
                None => element,
            }
        })
        .collect();
    let layout = GridLayout::new(state.size().0, thumbnails.cell_size());
    let pages = layout
        .visible(state.scroll, state.size().1, page_count)
        .map(|page| {
            Element::new(
                ("organize-page", page),
                Role::ListItem,
                format!("Page {}", page + 1),
            )
            .with_state(A11yState::selected(state.selection.contains(&page)))
            .with_activation(activation(OrganizeAction::Choose(page)))
        })
        .collect();
    let mut grid = Element::new("organize", Role::Group, "Organize Pages")
        .child(
            Element::new("organize-toolbar", Role::Toolbar, "Organize Pages")
                .with_children(toolbar),
        )
        .child(
            Element::new("organize-pages", Role::List, "Pages")
                .with_description(format!(
                    "{} of {page_count} selected",
                    state.selection.len()
                ))
                .with_children(pages),
        );
    if let Some(error) = &state.error {
        grid = grid.child(Element::new("organize-error", Role::Alert, error.clone()));
    }
    grid
}

fn toolbar(
    edits: Option<&'static str>,
    thumbnails: &ThumbnailsState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut row = div().flex().flex_wrap().gap_1().p_2().bg(theme.surface);
    for button in buttons(edits, thumbnails) {
        let base = div().id(button.id).px_2().py_1().rounded_sm().text_xs();
        let action = button.action;
        row = row.child(match button.refusal {
            Some(_) => base.text_color(theme.disabled_text).child(button.label),
            None => base
                .cursor_pointer()
                .hover(move |button| button.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(activation(action), window, cx);
                }))
                .child(button.label),
        });
    }
    row
}

/// The grid, drawn: the toolbar, then the cells that are on screen at their
/// arithmetic places, with the drop point or the marquee over them.
pub(in crate::shell) fn render(
    state: &OrganizeState,
    thumbnails: &ThumbnailsState,
    page_count: usize,
    edits: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let (width, height) = state.size();
    let layout = GridLayout::new(width, thumbnails.cell_size());
    let visible = layout.visible(state.scroll, height, page_count);

    // Asking changes the pane's state, which a frame being drawn may not
    // do; deferred to the end of the cycle, as the pane itself does.
    if thumbnails.grid_asks_for(&visible) {
        let (first, end) = (visible.start, visible.end);
        let frame = cx.entity();
        cx.defer(move |cx| {
            frame.update(cx, |frame, cx| frame.show_grid_band(first, end, cx));
        });
    }

    let mut cells = div().relative().size_full();
    for page in visible {
        let (x, y) = layout.origin(page);
        let chosen = state.selection.contains(&page);
        let picture = thumbnails.picture(page);
        cells = cells.child(
            div()
                .id(("organize-page", page))
                .absolute()
                .left(px(x))
                .top(px(y - state.scroll))
                .w(px(layout.cell))
                .h(px(layout.cell))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .rounded_sm()
                .border_2()
                .border_color(if chosen { theme.text } else { theme.surface })
                .when(chosen, |cell| cell.bg(theme.selected))
                .child(match picture {
                    Some((image, w, h)) => img(image).w(px(w)).h(px(h)).into_any_element(),
                    None => div()
                        .w(px(layout.cell * 0.6))
                        .h(px(layout.cell - 28.0))
                        .bg(theme.surface)
                        .into_any_element(),
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child((page + 1).to_string()),
                ),
        );
    }
    match &state.gesture {
        Some(Gesture::Drag { start, now, .. })
            if (now.0 - start.0).hypot(now.1 - start.1) >= 4.0 =>
        {
            // Where the pages would go: a bar at the gap before that slot.
            let slot = layout.drop_slot(*now, page_count);
            let (x, y) = if slot < page_count {
                layout.origin(slot)
            } else {
                let (x, y) = layout.origin(page_count.saturating_sub(1));
                (x + layout.cell + super::GAP, y)
            };
            cells = cells.child(
                div()
                    .absolute()
                    .left(px(x - super::GAP / 2.0 - 2.0))
                    .top(px(y - state.scroll))
                    .w(px(4.0))
                    .h(px(layout.cell))
                    .bg(theme.text),
            );
        }
        Some(Gesture::Marquee { start, now, .. }) => {
            cells = cells.child(
                div()
                    .absolute()
                    .left(px(start.0.min(now.0)))
                    .top(px(start.1.min(now.1) - state.scroll))
                    .w(px((now.0 - start.0).abs()))
                    .h(px((now.1 - start.1).abs()))
                    .border_1()
                    .border_color(theme.text)
                    .bg(theme.subtle_hover),
            );
        }
        _ => {}
    }

    let grid = div()
        .id("organize-grid")
        .size_full()
        .overflow_hidden()
        .on_scroll_wheel(
            cx.listener(move |frame, event: &ScrollWheelEvent, _window, cx| {
                let delta = f32::from(event.delta.pixel_delta(px(layout.cell)).y);
                frame.scroll_grid(delta, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|frame, event: &gpui::MouseDownEvent, _window, cx| {
                let at = (f32::from(event.position.x), f32::from(event.position.y));
                frame.grid_press(at, super::Held::of(&event.modifiers), cx);
            }),
        )
        .on_mouse_move(
            cx.listener(|frame, event: &gpui::MouseMoveEvent, _window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    let at = (f32::from(event.position.x), f32::from(event.position.y));
                    frame.grid_drag(at, cx);
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|frame, event: &gpui::MouseUpEvent, _window, cx| {
                let at = (f32::from(event.position.x), f32::from(event.position.y));
                frame.grid_release(at, cx);
            }),
        )
        // Let go outside the grid: the drag is cancelled, and nothing moved.
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|frame, _event: &gpui::MouseUpEvent, _window, cx| {
                frame.cancel_grid_gesture(cx);
            }),
        )
        .child(cells);
    // Where the grid was drawn, which the pointer arithmetic between
    // frames works from.
    let bounds = state.bounds.clone();
    let grid = div()
        .flex_1()
        .min_h_0()
        .flex()
        .on_children_prepainted(move |children, _window, _cx| {
            if let Some(area) = children.first() {
                bounds.set(Some((
                    f32::from(area.origin.x),
                    f32::from(area.origin.y),
                    f32::from(area.size.width),
                    f32::from(area.size.height),
                )));
            }
        })
        .child(grid);

    div()
        .id("organize")
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(theme.canvas)
        .child(toolbar(edits, thumbnails, theme, cx))
        .when_some(state.error.clone(), |column, error| {
            column.child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.error_text)
                    .child(error),
            )
        })
        .child(grid)
        .into_any_element()
}
