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
        self.menus.main_menu_open = false;
        self.context_menus.canvas_context_menu = None;
        self.context_menus.tab_context_menu = Some(TabContextMenu {
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
        if self.context_menus.canvas_context_menu.is_some()
            && document_bounds.contains(&event.position)
        {
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
        self.menus.main_menu_open = false;
        self.context_menus.tab_context_menu = None;
        self.context_menus.canvas_context_menu = Some(CanvasContextMenu {
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
        self.context_menus.canvas_context_menu = None;
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
                    model.edit_refusal(),
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

/// Which context menu is open, if either. Only one can be, but they are opened
/// from different surfaces and answer different commands, so each keeps its own
/// field.
#[derive(Default)]
pub(super) struct ContextMenuState {
    pub(super) tab_context_menu: Option<TabContextMenu>,
    pub(super) canvas_context_menu: Option<CanvasContextMenu>,
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

#[cfg(test)]
mod tests {
    use super::super::context::{
        context_menu_origin, tab_context_entries, CANVAS_CONTEXT_MENU_WIDTH, CONTEXT_MENU_PADDING,
        CONTEXT_MENU_ROW_HEIGHT,
    };
    #[cfg(feature = "shell-test-support")]
    use super::super::document_view_bounds;
    #[cfg(feature = "shell-test-support")]
    use super::super::tests::{bound_window, draw_window};
    #[cfg(feature = "shell-test-support")]
    use super::super::MouseButton;
    use super::super::{px, TabCommand, TabError};
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use super::super::{Canvas, Document, ShellFrame, ViewSize};
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use crate::preferences::ThemePreference;
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use crate::shell::canvas::CanvasModel;
    use crate::shell::chrome::global_bar::MenuAvailability;
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use crate::shell::chrome::theme::ShellViewState;
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use crate::shell::context_menu::tool_with;
    use crate::shell::context_menu::CanvasContextCommand;
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use crate::shell::ShellSettings;
    #[cfg(feature = "shell-test-support")]
    use gpui::{TestAppContext, VisualTestContext};
    // `tools-basic` as well: without it the canvas context menu test has no
    // live entry to pick.
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use gpui::{AppContext as _, MouseDownEvent};
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use onionskin_core::ViewRotation;
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    use std::path::Path;

    /// A click low enough that a thirteen-entry menu would overhang slides
    /// the panel back inside; a click with room to spare is left alone.
    #[test]
    fn a_context_menu_never_opens_outside_the_window() {
        let viewport = gpui::size(px(800.0), px(600.0));
        let size = gpui::size(
            px(CANVAS_CONTEXT_MENU_WIDTH),
            px(
                CanvasContextCommand::ALL.len() as f32 * CONTEXT_MENU_ROW_HEIGHT
                    + 2.0 * CONTEXT_MENU_PADDING,
            ),
        );

        let roomy = gpui::point(px(100.0), px(50.0));
        assert_eq!(context_menu_origin(roomy, size, viewport), roomy);

        let cornered = context_menu_origin(gpui::point(px(760.0), px(580.0)), size, viewport);
        assert_eq!(cornered.x + size.width, viewport.width);
        assert_eq!(cornered.y + size.height, viewport.height);

        // A window too small for the panel still opens it at the top left,
        // where the first entries are reachable, rather than off-screen.
        let cramped = context_menu_origin(
            gpui::point(px(10.0), px(10.0)),
            size,
            gpui::size(px(120.0), px(120.0)),
        );
        assert_eq!(cramped, gpui::point(px(0.0), px(0.0)));
    }

    /// The entries the menu shows come from the live model, and picking one
    /// reaches the subsystem that owns it: Take A Snapshot activates the
    /// registered snapshot tool, Rotate Clockwise turns the real view.
    ///
    /// Needs the plugin that registers that tool, for the same reason the
    /// canvas's snapshot-request test does: without it the entry is
    /// correctly not live, and the assertion is about the case where it is.
    #[cfg(all(feature = "shell-test-support", feature = "tools-basic"))]
    #[gpui::test]
    fn canvas_context_entries_come_from_the_live_model_and_run_against_it(cx: &mut TestAppContext) {
        use onionskin_plugin_api::ToolCapability;

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let document = Document::open_path(&path).unwrap();
        let model = CanvasModel::new(
            document,
            crate::build_registry(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
        )
        .unwrap();
        let shell_view = ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System);
        let theme = shell_view.tokens();
        let (frame, cx) = cx.add_window_view(move |window, cx| {
            let canvas = cx.new(|_| Canvas::new(model, theme));
            ShellFrame::new(
                vec![(path, canvas)],
                shell_view,
                ShellSettings::defaults(),
                window,
                cx,
            )
        });

        let right_click = MouseDownEvent {
            button: MouseButton::Right,
            position: gpui::point(px(300.0), px(300.0)),
            ..Default::default()
        };
        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.open_canvas_context_menu(&right_click, cx);
            });
        });

        let entries = cx.update(|_window, app| {
            assert!(frame.read(app).context_menus.canvas_context_menu.is_some());
            frame.read(app).canvas_context_menu_entries(app)
        });
        let live = |command| {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .expect("the entry is present")
                .availability
                .is_enabled()
        };
        assert_eq!(entries.len(), CanvasContextCommand::ALL.len());
        assert!(live(CanvasContextCommand::TakeASnapshot));
        assert!(live(CanvasContextCommand::RotateClockwise));
        // Nothing is selected in a freshly opened document.
        assert!(!live(CanvasContextCommand::Copy));

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.run_canvas_context_command(CanvasContextCommand::TakeASnapshot, cx);
            });
        });
        cx.update(|_window, app| {
            let frame = frame.read(app);
            assert!(
                frame.context_menus.canvas_context_menu.is_none(),
                "picking closes it"
            );
            let model = &frame.tabs.active().unwrap().canvas.read(app).model;
            assert_eq!(
                model.active_tool(),
                tool_with(model.registry(), ToolCapability::Snapshot)
            );
        });

        cx.update(|_window, app| {
            frame.update(app, |frame, cx| {
                frame.open_canvas_context_menu(&right_click, cx);
                frame.run_canvas_context_command(CanvasContextCommand::RotateClockwise, cx);
            });
        });
        cx.update(|_window, app| {
            let frame = frame.read(app);
            assert!(frame.context_menus.canvas_context_menu.is_none());
            let rotation = frame
                .tabs
                .active()
                .unwrap()
                .canvas
                .read(app)
                .model
                .view_state()
                .rotation;
            assert_eq!(rotation, ViewRotation::Clockwise90);
        });
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_second_right_click_inside_the_document_repositions_the_canvas_menu(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let first = gpui::point(px(300.0), px(300.0));
        let second = gpui::point(px(500.0), px(360.0));
        let mut cx = VisualTestContext::from_window(window.into(), cx);

        draw_window(&mut cx);
        cx.simulate_mouse_down(first, MouseButton::Right, gpui::Modifiers::default());
        draw_window(&mut cx);
        cx.simulate_mouse_down(second, MouseButton::Right, gpui::Modifiers::default());

        window
            .update(&mut cx, |frame, _window, _cx| {
                assert_eq!(
                    frame
                        .context_menus
                        .canvas_context_menu
                        .expect("the canvas menu remains open")
                        .origin,
                    second
                );
            })
            .unwrap();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_second_right_click_outside_the_document_dismisses_the_canvas_menu(
        cx: &mut TestAppContext,
    ) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let inside = gpui::point(px(300.0), px(300.0));
        let outside = gpui::point(px(20.0), px(20.0));
        let mut cx = VisualTestContext::from_window(window.into(), cx);

        window
            .update(&mut cx, |frame, window, _cx| {
                let visibility = frame.shell_view_state.visibility();
                assert!(!document_view_bounds(
                    window.viewport_size(),
                    visibility,
                    frame.rail_state.expanded(),
                    frame.navigation_width(visibility.navigation_pane),
                    frame.side_panel_state,
                )
                .contains(&outside));
            })
            .unwrap();

        draw_window(&mut cx);
        cx.simulate_mouse_down(inside, MouseButton::Right, gpui::Modifiers::default());
        draw_window(&mut cx);
        cx.simulate_mouse_down(outside, MouseButton::Right, gpui::Modifiers::default());

        window
            .update(&mut cx, |frame, _window, _cx| {
                assert!(frame.context_menus.canvas_context_menu.is_none());
            })
            .unwrap();
    }

    #[test]
    fn every_tab_context_action_targets_the_clicked_tab() {
        let entries = tab_context_entries(2, 3).unwrap();

        assert_eq!(entries.len(), 5);
        assert!(entries.iter().all(|entry| entry.tab_index == 2));
        assert!(entries.iter().all(|entry| entry.availability.is_enabled()));
    }

    #[test]
    fn close_others_explains_why_it_is_disabled_for_a_single_tab() {
        let entry = tab_context_entries(0, 1)
            .unwrap()
            .into_iter()
            .find(|entry| entry.command == TabCommand::CloseOthers)
            .unwrap();

        assert_eq!(
            entry.availability,
            MenuAvailability::Disabled("No other tabs are open")
        );
    }

    #[test]
    fn an_out_of_range_context_target_fails_loudly() {
        assert!(matches!(
            tab_context_entries(2, 2),
            Err(TabError::OutOfRange { index: 2, count: 2 })
        ));
    }
}
