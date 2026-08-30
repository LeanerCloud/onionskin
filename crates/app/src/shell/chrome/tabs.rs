use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, ClipboardItem, Context, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, WindowHandle,
};

use super::super::Canvas;
use super::global_bar::{
    main_menu_schema, refresh_native_menus, MenuAvailability, MenuCommand, MenuState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabError {
    OutOfRange { index: usize, count: usize },
    CommandUnavailable,
}

impl fmt::Display for TabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { index, count } => {
                write!(f, "tab {index} is outside a {count}-tab window")
            }
            Self::CommandUnavailable => write!(f, "menu command is not available yet"),
        }
    }
}

impl std::error::Error for TabError {}

struct DocumentTab {
    source: PathBuf,
    title: String,
    canvas: Entity<Canvas>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabCommand {
    Close,
    CloseOthers,
    CloseAll,
    RevealPath,
    CopyPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TabContextEntry {
    pub(super) command: TabCommand,
    pub(super) label: &'static str,
    pub(super) tab_index: usize,
    pub(super) availability: MenuAvailability,
}

#[derive(Debug, Clone, Copy)]
struct TabContextMenu {
    tab_index: usize,
    origin: Point<Pixels>,
}

impl DocumentTab {
    fn new(source: PathBuf, canvas: Entity<Canvas>) -> Self {
        let title = tab_title(&source);
        Self {
            source,
            title,
            canvas,
        }
    }

    fn title(&self) -> &str {
        &self.title
    }
}

struct TabState<T> {
    tabs: Vec<T>,
    active: Option<usize>,
}

impl<T> TabState<T> {
    pub fn new(tabs: Vec<T>) -> Self {
        let active = (!tabs.is_empty()).then_some(0);
        Self { tabs, active }
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn tabs(&self) -> &[T] {
        &self.tabs
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active(&self) -> Option<&T> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    pub fn activate(&mut self, index: usize) -> Result<bool, TabError> {
        if index >= self.tabs.len() {
            return Err(TabError::OutOfRange {
                index,
                count: self.tabs.len(),
            });
        }
        if self.active == Some(index) {
            return Ok(false);
        }
        self.active = Some(index);
        Ok(true)
    }

    pub fn close(&mut self, index: usize) -> Result<T, TabError> {
        if index >= self.tabs.len() {
            return Err(TabError::OutOfRange {
                index,
                count: self.tabs.len(),
            });
        }
        let closed = self.tabs.remove(index);
        self.active = match (self.active, self.tabs.is_empty()) {
            (_, true) => None,
            (Some(active), false) if active > index => Some(active - 1),
            (Some(active), false) if active == index => Some(index.min(self.tabs.len() - 1)),
            (active, false) => active,
        };
        Ok(closed)
    }

    pub fn close_others(&mut self, index: usize) -> Result<Vec<T>, TabError> {
        if index >= self.tabs.len() {
            return Err(TabError::OutOfRange {
                index,
                count: self.tabs.len(),
            });
        }
        let kept = self.tabs.remove(index);
        let closed = std::mem::replace(&mut self.tabs, vec![kept]);
        self.active = Some(0);
        Ok(closed)
    }

    pub fn close_all(&mut self) -> Vec<T> {
        self.active = None;
        std::mem::take(&mut self.tabs)
    }
}

pub(in crate::shell) struct ShellFrame {
    tabs: TabState<DocumentTab>,
    main_menu_open: bool,
    tab_context_menu: Option<TabContextMenu>,
}

impl ShellFrame {
    pub(in crate::shell) fn new(tabs: Vec<(PathBuf, Entity<Canvas>)>) -> Self {
        Self {
            tabs: TabState::new(
                tabs.into_iter()
                    .map(|(source, canvas)| DocumentTab::new(source, canvas))
                    .collect(),
            ),
            main_menu_open: false,
            tab_context_menu: None,
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tabs.activate(index).unwrap_or(false) {
            cx.notify();
        }
    }

    pub(super) fn run_native_command(
        window_handle: WindowHandle<Self>,
        command: MenuCommand,
        cx: &mut App,
    ) {
        let result = window_handle.update(cx, |frame, window, cx| {
            frame.run_main_menu_command(command, window, cx)
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("onionskin: {error}"),
            Err(error) => eprintln!("onionskin: cannot run menu command: {error}"),
        }
    }

    fn run_main_menu_command(
        &mut self,
        command: MenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), TabError> {
        let active = self
            .tabs
            .active_index()
            .ok_or(TabError::OutOfRange { index: 0, count: 0 })?;
        match command {
            MenuCommand::CloseTab => self.run_tab_command(TabCommand::Close, active, window, cx),
            MenuCommand::CloseOtherTabs => {
                self.run_tab_command(TabCommand::CloseOthers, active, window, cx)
            }
            MenuCommand::CloseAllTabs => {
                self.run_tab_command(TabCommand::CloseAll, active, window, cx)
            }
            MenuCommand::Open
            | MenuCommand::Undo
            | MenuCommand::Redo
            | MenuCommand::ViewControls
            | MenuCommand::NewWindow
            | MenuCommand::About
            | MenuCommand::KeyboardShortcuts => Err(TabError::CommandUnavailable),
        }
    }

    fn run_tab_command(
        &mut self,
        command: TabCommand,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), TabError> {
        self.main_menu_open = false;
        self.tab_context_menu = None;
        match command {
            TabCommand::Close => {
                self.tabs.close(index)?;
                if self.tabs.is_empty() {
                    window.remove_window();
                } else {
                    refresh_native_menus(cx, self.menu_state());
                    cx.notify();
                }
            }
            TabCommand::CloseOthers => {
                self.tabs.close_others(index)?;
                refresh_native_menus(cx, self.menu_state());
                cx.notify();
            }
            TabCommand::CloseAll => {
                self.tabs.close_all();
                window.remove_window();
            }
            TabCommand::RevealPath => {
                cx.reveal_path(self.tab_source(index)?);
                cx.notify();
            }
            TabCommand::CopyPath => {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    self.tab_source(index)?.to_string_lossy().into_owned(),
                ));
                cx.notify();
            }
        }
        Ok(())
    }

    fn tab_source(&self, index: usize) -> Result<&Path, TabError> {
        self.tabs
            .tabs()
            .get(index)
            .map(|tab| tab.source.as_path())
            .ok_or(TabError::OutOfRange {
                index,
                count: self.tabs.tabs().len(),
            })
    }

    fn menu_state(&self) -> MenuState {
        MenuState::new(self.tabs.tabs().len())
    }

    fn toggle_main_menu(&mut self, cx: &mut Context<Self>) {
        self.main_menu_open = !self.main_menu_open;
        self.tab_context_menu = None;
        cx.notify();
    }

    fn open_tab_context_menu(
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
        self.tab_context_menu = Some(TabContextMenu {
            tab_index: index,
            origin: event.position,
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn dismiss_menus(&mut self, cx: &mut Context<Self>) {
        self.main_menu_open = false;
        self.tab_context_menu = None;
        cx.notify();
    }

    fn render_global_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(40.0))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .bg(gpui::rgb(0x17181a))
            .text_color(gpui::white())
            .child(
                div()
                    .id("main-menu-button")
                    .w(px(32.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .rounded_md()
                    .hover(|button| button.bg(gpui::rgb(0x34363a)))
                    .on_click(cx.listener(|frame, _event, _window, cx| {
                        frame.toggle_main_menu(cx);
                    }))
                    .child("☰"),
            )
            .child(div().text_sm().child("Onionskin"))
    }

    fn render_main_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .absolute()
            .top(px(40.0))
            .left(px(8.0))
            .w(px(420.0))
            .p_2()
            .rounded_md()
            .bg(gpui::rgb(0x292a2d))
            .text_color(gpui::white());

        for section in main_menu_schema(self.menu_state()) {
            panel = panel.child(
                div()
                    .mt_2()
                    .px_2()
                    .text_xs()
                    .text_color(gpui::rgb(0xaeb0b5))
                    .child(section.id.label()),
            );
            for entry in section.entries {
                let command = entry.command;
                let enabled = entry.availability.is_enabled();
                let reason = entry.availability.reason();
                panel = panel.child(
                    div()
                        .id(("main-menu-entry", command as usize))
                        .min_h(px(30.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .rounded_sm()
                        .text_color(if enabled {
                            gpui::rgb(0xffffff)
                        } else {
                            gpui::rgb(0x85878c)
                        })
                        .when(enabled, |row| {
                            row.cursor_pointer()
                                .hover(|row| row.bg(gpui::rgb(0x3a3b3f)))
                        })
                        .on_click(cx.listener(move |frame, _event, window, cx| {
                            if enabled {
                                if let Err(error) = frame.run_main_menu_command(command, window, cx)
                                {
                                    eprintln!("onionskin: {error}");
                                }
                            }
                        }))
                        .child(div().flex_none().child(entry.label))
                        .when_some(reason, |row, reason| {
                            row.child(
                                div()
                                    .ml_3()
                                    .flex_1()
                                    .text_right()
                                    .text_xs()
                                    .text_color(gpui::rgb(0x85878c))
                                    .child(reason),
                            )
                        }),
                );
            }
        }
        panel
    }

    fn render_tab_context_menu(
        &self,
        menu: TabContextMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut panel = div()
            .absolute()
            .left(menu.origin.x)
            .top(menu.origin.y)
            .w(px(230.0))
            .p_1()
            .rounded_md()
            .bg(gpui::rgb(0x292a2d))
            .text_color(gpui::white());

        for (row_index, entry) in tab_context_entries(menu.tab_index, self.tabs.tabs().len())
            .expect("context-menu targets are validated when opened")
            .into_iter()
            .enumerate()
        {
            let enabled = entry.availability.is_enabled();
            let command = entry.command;
            let tab_index = entry.tab_index;
            panel = panel.child(
                div()
                    .id(("tab-context-entry", row_index))
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .rounded_sm()
                    .text_color(if enabled {
                        gpui::rgb(0xffffff)
                    } else {
                        gpui::rgb(0x85878c)
                    })
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(gpui::rgb(0x3a3b3f)))
                    })
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        if enabled {
                            if let Err(error) =
                                frame.run_tab_command(command, tab_index, window, cx)
                            {
                                eprintln!("onionskin: {error}");
                            }
                        }
                    }))
                    .child(entry.label)
                    .when_some(entry.availability.reason(), |row, reason| {
                        row.child(
                            div()
                                .ml_2()
                                .text_xs()
                                .text_color(gpui::rgb(0x85878c))
                                .child(reason),
                        )
                    }),
            );
        }
        panel
    }
}

impl Render for ShellFrame {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tab_bar = div().flex().h(px(36.0)).bg(gpui::rgb(0x202124));
        for (index, tab) in self.tabs.tabs().iter().enumerate() {
            let active = self.tabs.active_index() == Some(index);
            tab_bar = tab_bar.child(
                div()
                    .id(tab_element_id(&tab.source))
                    .px_3()
                    .h_full()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .bg(if active {
                        gpui::rgb(0x3a3b3f)
                    } else {
                        gpui::rgb(0x292a2d)
                    })
                    .text_color(gpui::white())
                    .on_click(
                        cx.listener(move |frame, event: &gpui::ClickEvent, _window, cx| {
                            if !event.is_right_click() {
                                frame.activate(index, cx);
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |frame, event, _window, cx| {
                            frame.open_tab_context_menu(index, event, cx);
                        }),
                    )
                    .child(tab.title().to_owned()),
            );
        }

        let mut frame = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(self.render_global_bar(cx))
            .child(tab_bar);
        if let Some(tab) = self.tabs.active() {
            frame = frame.child(div().flex_1().min_h_0().child(tab.canvas.clone()));
        }

        let mut root = div().size_full().relative().child(frame);
        if self.main_menu_open || self.tab_context_menu.is_some() {
            root = root.child(
                div()
                    .id("menu-dismiss-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .on_click(cx.listener(|frame, _event, _window, cx| {
                        frame.dismiss_menus(cx);
                    })),
            );
        }
        if self.main_menu_open {
            root = root.child(self.render_main_menu(cx));
        }
        if let Some(menu) = self.tab_context_menu {
            root = root.child(self.render_tab_context_menu(menu, cx));
        }
        root
    }
}

pub(super) fn tab_context_entries(
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

fn tab_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn tab_element_id(path: &Path) -> Arc<Path> {
    Arc::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_and_closing_tabs_keeps_a_valid_active_index() {
        let mut tabs = TabState::new(vec!["one", "two", "three"]);

        assert_eq!(tabs.active_index(), Some(0));
        assert!(tabs.activate(1).unwrap());
        assert_eq!(tabs.close(0).unwrap(), "one");
        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(tabs.active(), Some(&"two"));
        assert_eq!(tabs.close(0).unwrap(), "two");
        assert_eq!(tabs.active_index(), Some(0));
        assert_eq!(tabs.active(), Some(&"three"));
        assert_eq!(tabs.close(0).unwrap(), "three");
        assert_eq!(tabs.active_index(), None);
    }

    #[test]
    fn close_others_leaves_exactly_the_selected_tab() {
        let mut tabs = TabState::new(vec!["one", "two", "three"]);

        assert_eq!(tabs.close_others(1).unwrap(), vec!["one", "three"]);
        assert_eq!(tabs.tabs(), &["two"]);
        assert_eq!(tabs.active_index(), Some(0));
    }

    #[test]
    fn close_all_leaves_no_invalid_active_tab() {
        let mut tabs = TabState::new(vec!["one", "two"]);

        assert_eq!(tabs.close_all(), vec!["one", "two"]);
        assert!(tabs.is_empty());
        assert_eq!(tabs.active_index(), None);
    }

    #[test]
    fn duplicate_titles_keep_distinct_path_identity() {
        let first = PathBuf::from("/first/report.pdf");
        let second = PathBuf::from("/second/report.pdf");

        assert_eq!(tab_title(&first), tab_title(&second));
        assert_ne!(tab_element_id(&first), tab_element_id(&second));
    }

    #[test]
    fn out_of_range_operations_fail_loudly() {
        let mut tabs = TabState::new(vec!["one"]);

        assert!(matches!(
            tabs.activate(1),
            Err(TabError::OutOfRange { index: 1, count: 1 })
        ));
        assert!(matches!(
            tabs.close(1),
            Err(TabError::OutOfRange { index: 1, count: 1 })
        ));
        assert!(matches!(
            tabs.close_others(1),
            Err(TabError::OutOfRange { index: 1, count: 1 })
        ));
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
