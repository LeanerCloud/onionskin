//! The tab strip's context menu and the page canvas's, and the geometry both
//! panels are placed with.
//!
//! Moved out of `tabs/mod.rs` unchanged. `pub(super)` here reaches
//! `chrome::tabs` and everything under it, which is the scope these items had
//! while they were private items of `chrome::tabs`; the two that were already
//! `pub(super)` in `chrome::tabs` are spelled `pub(in crate::shell::chrome)`
//! to keep the reach they had rather than narrow it.

use super::{ShellFrame, TabCommand, TabError};
use crate::shell::canvas::ViewAction;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::global_bar::MenuAvailability;
use crate::shell::context_menu::{
    canvas_context_entries, tool_with, CanvasContextCommand, CanvasContextEntry,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, Bounds, ClipboardItem, Context, InteractiveElement as _, IntoElement,
    MouseDownEvent, ParentElement as _, Pixels, Point, StatefulInteractiveElement as _,
    Styled as _, Window,
};

impl ShellFrame {
    pub(super) fn open_tab_context_menu(
        &mut self,
        index: usize,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if tab_context_entries(index, self.tabs.tabs().len()).is_err() {
            eprintln!(
                "onionskin: {}",
                TabError::OutOfRange {
                    index,
                    count: self.tabs.tabs().len()
                }
            );
            return;
        }
        self.main_menu_open = false;
        self.canvas_context_menu = None;
        self.tab_context_menu = Some(TabContextMenu {
            tab_index: index,
            origin: event.position,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn dismiss_layer_right_click(
        &mut self,
        event: &MouseDownEvent,
        document_bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.canvas_context_menu.is_some() && document_bounds.contains(&event.position) {
            self.open_canvas_context_menu(event, cx);
        } else {
            self.dismiss_menus(cx);
        }
    }

    pub(super) fn open_canvas_context_menu(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.active().is_none() {
            return;
        }
        self.main_menu_open = false;
        self.tab_context_menu = None;
        self.canvas_context_menu = Some(CanvasContextMenu {
            origin: event.position,
        });
        cx.stop_propagation();
        cx.notify();
    }

    /// The menu's live entries are the ones the registry and the selection
    /// answer for, so running one asks the same two sources rather than a
    /// second copy of the rules.
    pub(super) fn run_canvas_context_command(
        &mut self,
        command: CanvasContextCommand,
        cx: &mut Context<Self>,
    ) {
        self.canvas_context_menu = None;
        // The menu is closed above whatever the command turns out to do, so
        // every exit below has to repaint.
        cx.notify();
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        match command {
            CanvasContextCommand::Copy => {
                let Some(text) = canvas
                    .read(cx)
                    .model
                    .selection_text()
                    .map(str::to_owned)
                    .filter(|text| !text.is_empty())
                else {
                    return;
                };
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            CanvasContextCommand::RotateClockwise => {
                self.run_view_action(ViewAction::RotateClockwise, cx)
            }
            // Spelled out rather than left to a wildcard: every remaining
            // entry activates a tool, and an entry added without a decision
            // here has to be a compile error rather than a silent tool
            // lookup that finds nothing and returns.
            other @ (CanvasContextCommand::CopyWithFormatting
            | CanvasContextCommand::ExportSelectionAs
            | CanvasContextCommand::HighlightText
            | CanvasContextCommand::AddNoteToText
            | CanvasContextCommand::EditText
            | CanvasContextCommand::RedactText
            | CanvasContextCommand::CreateLink
            | CanvasContextCommand::TakeASnapshot
            | CanvasContextCommand::AddBookmark
            | CanvasContextCommand::Print
            | CanvasContextCommand::PageCommands) => {
                let Some(index) = other
                    .capability()
                    .and_then(|capability| tool_with(canvas.read(cx).model.registry(), capability))
                else {
                    return;
                };
                let rail_entry = self.active_rail_entry(index, cx);
                self.activate_canvas_tool(index, other.label(), rail_entry, cx);
            }
        }
    }

    pub(super) fn canvas_context_menu_entries(&self, cx: &App) -> Vec<CanvasContextEntry> {
        self.tabs
            .active()
            .map(|tab| {
                let model = &tab.canvas.read(cx).model;
                canvas_context_entries(
                    model.registry(),
                    model.selection_text().is_some_and(|text| !text.is_empty()),
                )
            })
            .unwrap_or_default()
    }

    pub(super) fn render_tab_context_menu(
        &self,
        menu: TabContextMenu,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let entries = tab_context_entries(menu.tab_index, self.tabs.tabs().len())
            .expect("context-menu targets are validated when opened");
        let size = gpui::size(
            px(TAB_CONTEXT_MENU_WIDTH),
            px(entries.len() as f32 * CONTEXT_MENU_ROW_HEIGHT + 2.0 * CONTEXT_MENU_PADDING),
        );
        let origin = context_menu_origin(menu.origin, size, window.viewport_size());
        let mut panel = div()
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(size.width)
            .p(px(CONTEXT_MENU_PADDING))
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text);

        for (row_index, entry) in entries.into_iter().enumerate() {
            let enabled = entry.availability.is_enabled();
            let command = entry.command;
            let tab_index = entry.tab_index;
            panel = panel.child(
                div()
                    .id(("tab-context-entry", row_index))
                    .h(px(CONTEXT_MENU_ROW_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .rounded_sm()
                    .text_color(if enabled {
                        theme.text
                    } else {
                        theme.disabled_text
                    })
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(move |row| row.bg(theme.selected))
                    })
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        if enabled {
                            frame.run_activation(
                                Activation::TabCommand(command, tab_index),
                                window,
                                cx,
                            );
                        }
                    }))
                    .child(entry.label)
                    .when_some(entry.availability.reason(), |row, reason| {
                        row.child(
                            div()
                                .ml_2()
                                .text_xs()
                                .text_color(theme.muted_text)
                                .child(reason),
                        )
                    }),
            );
        }
        panel
    }

    pub(super) fn render_canvas_context_menu(
        &self,
        menu: CanvasContextMenu,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.shell_view_state.tokens();
        let entries = self.canvas_context_menu_entries(cx);
        let size = gpui::size(
            px(CANVAS_CONTEXT_MENU_WIDTH),
            px(entries.len() as f32 * CONTEXT_MENU_ROW_HEIGHT + 2.0 * CONTEXT_MENU_PADDING),
        );
        let origin = context_menu_origin(menu.origin, size, window.viewport_size());
        let mut panel = div()
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(size.width)
            .p(px(CONTEXT_MENU_PADDING))
            .rounded_md()
            .bg(theme.raised)
            .text_color(theme.text);

        for (row_index, entry) in entries.into_iter().enumerate() {
            let enabled = entry.availability.is_enabled();
            let command = entry.command;
            panel = panel.child(
                div()
                    .id(("canvas-context-entry", row_index))
                    .h(px(CONTEXT_MENU_ROW_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .rounded_sm()
                    .text_color(if enabled {
                        theme.text
                    } else {
                        theme.disabled_text
                    })
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(move |row| row.bg(theme.selected))
                    })
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        if enabled {
                            frame.run_canvas_context_command(command, cx);
                        }
                    }))
                    .child(entry.label)
                    .when_some(entry.availability.reason(), |row, reason| {
                        row.child(
                            div()
                                .ml_2()
                                .text_xs()
                                .text_color(theme.muted_text)
                                .child(reason),
                        )
                    }),
            );
        }
        panel
    }
}

pub(super) const CONTEXT_MENU_ROW_HEIGHT: f32 = 30.0;

pub(super) const CONTEXT_MENU_PADDING: f32 = 4.0;

pub(super) const CANVAS_CONTEXT_MENU_WIDTH: f32 = 300.0;

pub(super) const TAB_CONTEXT_MENU_WIDTH: f32 = 230.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::chrome) struct TabContextEntry {
    pub(in crate::shell::chrome) command: TabCommand,
    pub(in crate::shell::chrome) label: &'static str,
    pub(in crate::shell::chrome) tab_index: usize,
    pub(in crate::shell::chrome) availability: MenuAvailability,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TabContextMenu {
    pub(super) tab_index: usize,
    pub(super) origin: Point<Pixels>,
}

/// Where the canvas context menu was opened, in frame coordinates.
#[derive(Debug, Clone, Copy)]
pub(super) struct CanvasContextMenu {
    pub(super) origin: Point<Pixels>,
}

/// Where a context menu panel of `size` may sit after a click at `click`.
///
/// The canvas menu names all thirteen of parity row 225's entries, which is
/// tall enough to run off the bottom of the window, so the clicked corner
/// is a preference: the panel slides back inside rather than putting
/// entries out of reach.
pub(super) fn context_menu_origin(
    click: Point<Pixels>,
    size: gpui::Size<Pixels>,
    viewport: gpui::Size<Pixels>,
) -> Point<Pixels> {
    Point {
        x: click.x.min(viewport.width - size.width).max(px(0.0)),
        y: click.y.min(viewport.height - size.height).max(px(0.0)),
    }
}

pub(in crate::shell::chrome) fn tab_context_entries(
    tab_index: usize,
    tab_count: usize,
) -> Result<Vec<TabContextEntry>, TabError> {
    use MenuAvailability::{Disabled, Enabled};

    if tab_index >= tab_count {
        return Err(TabError::OutOfRange {
            index: tab_index,
            count: tab_count,
        });
    }

    Ok(vec![
        TabContextEntry {
            command: TabCommand::Close,
            label: "Close",
            tab_index,
            availability: Enabled,
        },
        TabContextEntry {
            command: TabCommand::CloseOthers,
            label: "Close Others",
            tab_index,
            availability: if tab_count > 1 {
                Enabled
            } else {
                Disabled("No other tabs are open")
            },
        },
        TabContextEntry {
            command: TabCommand::CloseAll,
            label: "Close All",
            tab_index,
            availability: Enabled,
        },
        TabContextEntry {
            command: TabCommand::RevealPath,
            label: "Show Containing Folder",
            tab_index,
            availability: Enabled,
        },
        TabContextEntry {
            command: TabCommand::CopyPath,
            label: "Copy Path",
            tab_index,
            availability: Enabled,
        },
    ])
}
