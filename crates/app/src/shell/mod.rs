//! GPUI adapters and the first windowed document shell.

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    canvas as gpui_canvas, div, fill, outline, point, px, size, App, AppContext as _, Application,
    BorderStyle, Bounds, ClipboardItem, Context, DispatchPhase, Hsla, Image, ImageFormat,
    InteractiveElement as _, IntoElement, KeyBinding, Keystroke, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PinchEvent, Pixels, Point,
    Render, ScrollWheelEvent, Styled as _, Timer, TitlebarOptions, TouchPhase, Window,
    WindowBounds, WindowOptions,
};
use onionskin_core::{Document, Provenance, ViewPoint, ViewRect, ViewSize};
use onionskin_plugin_api::PluginRegistry;

use accesskit::Role;

use self::canvas::{CanvasError, CanvasModel, CanvasStatus, OverlayPaint, PaintList, ViewAction};
use self::chrome::accessible::{Activation, Element as A11yElement, Rects};
use self::chrome::{
    command_defaults, command_for_id, install_native_menus, install_search_keybindings, MenuState,
    QuickAction, RegistryFacts, RunCommand, ShellFrame, ShellViewState, ThemeTokens,
};
use crate::config::ConfigPaths;
use crate::keymap::{platform_keystroke, Binding, Keymap};
use crate::preferences::Preferences;
use crate::recents::Recents;

pub mod canvas;
mod chrome;
mod context_menu;
mod dialog;
mod find_bar;
#[cfg(test)]
mod fixtures;
mod home;
pub mod input;
mod panes;
mod preferences_dialog;

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 860.0;
const SCROLL_LINE_HEIGHT: f32 = 30.0;
const POLL_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug)]
pub enum ShellError {
    Open {
        path: PathBuf,
        source: onionskin_core::Error,
    },
    ResolvePath {
        path: PathBuf,
        source: std::io::Error,
    },
    Canvas(CanvasError),
    Window(String),
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(f, "cannot open PDF {}: {source}", path.display())
            }
            Self::ResolvePath { path, source } => {
                write!(f, "cannot resolve PDF path {}: {source}", path.display())
            }
            Self::Canvas(error) => write!(f, "cannot start canvas: {error}"),
            Self::Window(error) => write!(f, "cannot create window: {error}"),
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open { source, .. } => Some(source),
            Self::ResolvePath { source, .. } => Some(source),
            Self::Canvas(error) => Some(error),
            Self::Window(_) => None,
        }
    }
}

impl From<CanvasError> for ShellError {
    fn from(error: CanvasError) -> Self {
        Self::Canvas(error)
    }
}

/// What the app read from disk before the window opened: the three config
/// files, and everything that went wrong reading them.
///
/// Loaded once and handed to the frame, rather than read where each is
/// needed, so a session's keymap and preferences cannot change under it and
/// every failure is reportable in one place.
pub(in crate::shell) struct ShellSettings {
    pub(in crate::shell) paths: ConfigPaths,
    pub(in crate::shell) preferences: Preferences,
    pub(in crate::shell) recents: Recents,
    /// What the app's own registry answers, for the menus to consult when no
    /// document is open and there is no tab's registry to ask.
    pub(in crate::shell) registry: RegistryFacts,
    /// The keystrokes in force, already checked against GPUI's parser.
    pub(in crate::shell) bindings: Vec<Binding>,
    pub(in crate::shell) notices: Vec<String>,
}

impl ShellSettings {
    pub(in crate::shell) fn load(paths: ConfigPaths, registry: &PluginRegistry) -> Self {
        let mut notices = Vec::new();
        let (preferences, preference_errors) = Preferences::load(paths.preferences.as_deref());
        notices.extend(preference_errors.iter().map(ToString::to_string));
        let (mut recents, recent_errors) = Recents::load(paths.recents.as_deref());
        notices.extend(recent_errors.iter().map(ToString::to_string));
        recents.truncate(preferences.recent_documents);

        let defaults = command_defaults(
            registry
                .commands()
                .iter()
                .map(|command| (command.id, command.keybind)),
        );
        let macos = cfg!(target_os = "macos");
        let keymap = Keymap::load(&defaults, paths.keymap.as_deref(), macos);
        notices.extend(keymap.errors().iter().map(ToString::to_string));
        let (bindings, unbindable) = bindable(keymap.bindings());
        notices.extend(unbindable);

        Self {
            paths,
            preferences,
            recents,
            registry: RegistryFacts::of(registry),
            bindings,
            notices,
        }
    }

    /// Gated on the feature its callers are gated on: every test that uses
    /// this is a windowed one, and a `--features shell` test build would
    /// otherwise compile it with nothing calling it.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn defaults() -> Self {
        Self::load(ConfigPaths::default(), &PluginRegistry::new())
    }
}

/// The bindings GPUI can take, and a message for each one it cannot.
///
/// `KeyBinding::new` panics on a keystroke it cannot parse, and the
/// keystrokes come from a file the user edits, so they are parsed here
/// first. A binding naming a command with no home in the menus is dropped
/// the same way: nothing would run it.
fn bindable(bindings: &[Binding]) -> (Vec<Binding>, Vec<String>) {
    let macos = cfg!(target_os = "macos");
    let mut usable = Vec::new();
    let mut rejected = Vec::new();
    for binding in bindings {
        let keystroke = platform_keystroke(&binding.keystroke, macos);
        if Keystroke::parse(&keystroke).is_err() {
            rejected.push(format!(
                "{} is bound to {}, which is not a keystroke this platform can read",
                binding.id, binding.keystroke
            ));
            continue;
        }
        if command_for_id(binding.id).is_none() {
            rejected.push(format!(
                "{} is bound to {}, but no menu entry runs it",
                binding.id, binding.keystroke
            ));
            continue;
        }
        usable.push(binding.clone());
    }
    (usable, rejected)
}

/// Bind everything the keymap ended up with. Window-wide, with no key
/// context: a shortcut works wherever focus sits, and a text field that
/// wants a keystroke for itself binds it in its own context, which wins.
pub(in crate::shell) fn install_command_keybindings(cx: &mut App, bindings: &[Binding]) {
    let macos = cfg!(target_os = "macos");
    cx.bind_keys(bindings.iter().filter_map(|binding| {
        // Total over what `bindable` hands back: it drops and reports the
        // bindings no menu entry runs, so nothing is dropped silently here.
        let command = command_for_id(binding.id)?;
        Some(KeyBinding::new(
            &platform_keystroke(&binding.keystroke, macos),
            RunCommand { command },
            None,
        ))
    }));
}

/// Open a document the way the Page Display preferences say to.
///
/// Applied here rather than inside `CanvasModel::new` so the model keeps one
/// opening behaviour and the preference stays the shell's: the same call
/// runs for a document from the command line and one from File > Open.
pub(in crate::shell) fn apply_page_display(
    model: &mut CanvasModel,
    preferences: &Preferences,
) -> Result<(), CanvasError> {
    model.set_layout_mode(preferences.layout)?;
    match preferences.zoom.fit() {
        Some(fit) => model.fit(fit)?,
        None => model.actual_size()?,
    };
    Ok(())
}

/// What a repaired open owes the user: that the file was broken, and what
/// was done about it.
///
/// Shown for every document that arrives repaired, whether it came from the
/// command line or from File > Open. Decision 10 says a repaired file opens;
/// it does not say it opens silently.
pub(in crate::shell) fn repair_notice(path: &Path, provenance: &Provenance) -> Option<String> {
    let report = provenance.report()?;
    Some(format!(
        "{} was repaired to open it: {}",
        path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned()
        ),
        report.summary()
    ))
}

pub struct Canvas {
    model: CanvasModel,
    polling: bool,
    theme: ThemeTokens,
}

impl Canvas {
    fn new(model: CanvasModel, theme: ThemeTokens) -> Self {
        Self {
            model,
            polling: false,
            theme,
        }
    }

    fn set_theme(&mut self, theme: ThemeTokens, cx: &mut Context<Self>) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        cx.notify();
    }

    fn prepare_paint(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) -> PaintList {
        let result = self
            .resize_for_bounds(bounds)
            .and_then(|()| self.model.update())
            .and_then(|()| self.model.paint_list());
        match result {
            Ok(paint) => {
                self.arm_poll(cx);
                paint
            }
            Err(error) => {
                self.record_error(error, cx);
                PaintList::default()
            }
        }
    }

    fn resize_for_bounds(&mut self, bounds: Bounds<Pixels>) -> Result<(), CanvasError> {
        self.model.resize(
            ViewPoint {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
            },
            ViewSize {
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            },
        )
    }

    fn handle_change(&mut self, result: Result<bool, CanvasError>, cx: &mut Context<Self>) {
        match result {
            Ok(false) => {}
            Ok(true) => match self.model.update() {
                Ok(()) => {
                    self.arm_poll(cx);
                    cx.notify();
                }
                Err(error) => self.record_error(error, cx),
            },
            Err(error) => self.record_error(error, cx),
        }
        self.copy_pending_snapshot(cx);
    }

    /// The snapshot tool raises a request rather than holding a render
    /// handle, so producing the pixels and reaching the clipboard is the
    /// shell's half of the contract.
    ///
    /// Drained here because a request can only come out of a tool's
    /// pointer handlers, and every pointer path routes through
    /// `handle_change`. On the frames that raise nothing this costs one
    /// `Option::take`.
    fn copy_pending_snapshot(&mut self, cx: &mut Context<Self>) {
        match self.model.take_snapshot_png() {
            Ok(None) => {}
            Ok(Some(png)) => cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                ImageFormat::Png,
                png,
            ))),
            Err(error) => self.record_error(error, cx),
        }
    }

    fn activate_tool(&mut self, index: usize, cx: &mut Context<Self>) -> Result<bool, CanvasError> {
        match self.model.activate_tool(index) {
            Ok(changed) => {
                if changed {
                    cx.notify();
                }
                Ok(changed)
            }
            Err(error) => {
                self.record_error(&error, cx);
                Err(error)
            }
        }
    }

    fn run_view_action(&mut self, action: ViewAction, cx: &mut Context<Self>) {
        let result = match action {
            ViewAction::PreviousView => self.model.previous_view(),
            ViewAction::NextView => self.model.next_view(),
            ViewAction::FirstPage => self.model.first_page(),
            ViewAction::PreviousPage => self.model.previous_page(),
            ViewAction::NextPage => self.model.next_page(),
            ViewAction::LastPage => self.model.last_page(),
            ViewAction::GoToPage(page) => self.model.go_to_page(page),
            ViewAction::RotateClockwise => self.model.rotate_clockwise(),
            ViewAction::ActualSize => self.model.actual_size(),
            ViewAction::ZoomOut => self.model.zoom_out(),
            ViewAction::ZoomIn => self.model.zoom_in(),
            ViewAction::Fit(mode) => self.model.fit(mode),
            ViewAction::SetLayout(mode) => self.model.set_layout_mode(mode),
            ViewAction::SetShowCover(show_cover) => self.model.set_show_cover(show_cover),
        };
        self.handle_change(result, cx);
    }

    fn record_error(&mut self, error: impl fmt::Display, cx: &mut Context<Self>) {
        if self.model.record_error(error) {
            cx.notify();
        }
    }

    fn arm_poll(&mut self, cx: &mut Context<Self>) {
        if self.polling || !self.model.has_pending_work() {
            return;
        }
        self.polling = true;
        cx.spawn(async move |entity, cx| loop {
            Timer::after(POLL_INTERVAL).await;
            let keep_polling = entity
                .update(cx, |canvas, cx| match canvas.model.update() {
                    Ok(()) => {
                        let pending = canvas.model.poll_again(Instant::now());
                        if !pending {
                            canvas.polling = false;
                        }
                        cx.notify();
                        pending
                    }
                    Err(error) => {
                        canvas.polling = false;
                        canvas.record_error(error, cx);
                        false
                    }
                })
                .unwrap_or(false);
            if !keep_polling {
                break;
            }
        })
        .detach();
    }

    /// What the document tells a screen reader.
    ///
    /// The page node is a `Role::Document` with a role description of its
    /// own, because AccessKit maps that role to `NSAccessibilityGroupRole`
    /// and AppKit would otherwise answer "group": the defect M1's spike
    /// recorded. Its children are one node per visible page and, under each,
    /// one node per run of text, so a screen reader navigates the page rather
    /// than being handed it as a single string.
    ///
    pub(in crate::shell) fn accessible(&mut self, title: &str, scale: f32) -> A11yElement {
        let origin = self.model.canvas_origin();
        let page_count = self.model.viewport().page_count();
        let mut document = A11yElement::new("document", Role::Document, title.to_owned())
            .with_activation(Activation::FocusDocument);
        let pages = match self.model.accessible_pages() {
            Ok(pages) => pages,
            Err(error) => {
                return document.child(A11yElement::new(
                    "document-unreadable",
                    Role::Alert,
                    format!("This document cannot be laid out: {error}"),
                ));
            }
        };
        for outline in pages {
            let number = outline.page + 1;
            let mut page = A11yElement::new(
                ("page", outline.page),
                Role::Region,
                format!("Page {number} of {page_count}"),
            )
            .with_role_description("page");
            page.bounds = Some(Rects::view_rect(outline.rect, origin, scale));
            match outline.text {
                // An unmeasured page has no layout to place its words in, so
                // it carries no runs. Saying so keeps "still loading" from
                // sounding like "this page has no text on it".
                Ok(_) if !outline.measured => {
                    page = page.child(A11yElement::new(
                        ("page-loading", outline.page),
                        Role::Label,
                        format!("Page {number} is still loading"),
                    ));
                }
                Ok(runs) => {
                    for (index, run) in runs.into_iter().enumerate() {
                        let mut node = A11yElement::new(
                            gpui::ElementId::NamedInteger(
                                format!("page-{number}-text").into(),
                                index as u64,
                            ),
                            Role::Label,
                            run.text,
                        );
                        node.bounds = run.rect.map(|rect| Rects::view_rect(rect, origin, scale));
                        page = page.child(node);
                    }
                }
                Err(error) => {
                    page = page.child(A11yElement::new(
                        ("page-unreadable", outline.page),
                        Role::Alert,
                        format!("The text on page {number} cannot be read: {error}"),
                    ));
                }
            }
            document = document.child(page);
        }
        if let Some(status) = self.model.status() {
            document = document.child(A11yElement::new(
                "document-status",
                Role::Alert,
                status_text(status),
            ));
        }
        document
    }

    fn local_point(&self, point: Point<Pixels>) -> ViewPoint {
        let origin = self.model.canvas_origin();
        ViewPoint {
            x: f32::from(point.x) - origin.x,
            y: f32::from(point.y) - origin.y,
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .model
            .pointer_down(event.position, event.pressure, event.modifiers);
        self.handle_change(result, cx);
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.model.pointer_move(
            event.position,
            event.pressure,
            event.modifiers,
            event.dragging(),
        );
        self.handle_change(result, cx);
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .model
            .pointer_up(event.position, event.pressure, event.modifiers);
        self.handle_change(result, cx);
    }

    fn on_mouse_up_out(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.model.cancel_pointer_gesture();
        self.handle_change(Ok(changed), cx);
    }

    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(SCROLL_LINE_HEIGHT));
        let result = self.model.scroll(
            ViewPoint {
                x: f32::from(delta.x),
                y: f32::from(delta.y),
            },
            event.modifiers.control || event.modifiers.platform,
            self.local_point(event.position),
        );
        self.handle_change(result.map(|()| true), cx);
    }

    fn on_pinch(&mut self, event: &PinchEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.phase != TouchPhase::Moved {
            return;
        }
        let result = self
            .model
            .pinch(event.delta, self.local_point(event.position));
        self.handle_change(result, cx);
    }
}

impl Render for Canvas {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prepare_entity = cx.entity();
        let exit_entity = cx.entity();
        let error_entity = cx.entity();
        let status = self.model.status().map(status_text);
        let theme = self.theme;
        let mut root = div()
            .id("canvas")
            .size_full()
            .overflow_hidden()
            .bg(self.theme.canvas)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|canvas, event, window, cx| canvas.on_mouse_down(event, window, cx)),
            )
            .on_mouse_move(
                cx.listener(|canvas, event, window, cx| canvas.on_mouse_move(event, window, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|canvas, event, window, cx| canvas.on_mouse_up(event, window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|canvas, event, window, cx| canvas.on_mouse_up_out(event, window, cx)),
            )
            .on_scroll_wheel(
                cx.listener(|canvas, event, window, cx| canvas.on_scroll(event, window, cx)),
            )
            .on_pinch(cx.listener(|canvas, event, window, cx| canvas.on_pinch(event, window, cx)))
            .child(
                gpui_canvas(
                    move |bounds, _window, cx| {
                        prepare_entity.update(cx, |canvas, cx| canvas.prepare_paint(bounds, cx))
                    },
                    move |bounds, paint: PaintList, window, cx| {
                        let exit_entity = exit_entity.clone();
                        window.on_mouse_event(
                            move |_event: &MouseExitEvent, phase, _window, cx| {
                                if phase != DispatchPhase::Capture {
                                    return;
                                }
                                exit_entity.update(cx, |canvas, cx| {
                                    let changed = canvas.model.cancel_pointer_gesture();
                                    canvas.handle_change(Ok(changed), cx);
                                });
                            },
                        );
                        for page in paint.pages {
                            window.paint_quad(fill(
                                window_bounds(bounds.origin, page.rect),
                                gpui::white(),
                            ));
                        }
                        for tile in &paint.tiles {
                            let page = tile.page;
                            let painted = window.with_content_mask(
                                Some(gpui::ContentMask {
                                    bounds: window_bounds(bounds.origin, tile.clip_rect),
                                }),
                                |window| {
                                    window.paint_image(
                                        window_bounds(bounds.origin, tile.rect),
                                        gpui::Corners::default(),
                                        tile.image.clone(),
                                        0,
                                        false,
                                    )
                                },
                            );
                            // Whether the GPU can take this frame's image is a
                            // runtime condition, not a claim the canvas gets to
                            // make: an unwrap here took the window down over a
                            // tile. Drop it, keep the rest of the page, and say
                            // so on the status line the next frame.
                            if let Err(error) = painted {
                                let entity = error_entity.clone();
                                cx.defer(move |cx| {
                                    entity.update(cx, |canvas, cx| {
                                        canvas.record_error(format!("page {page}: {error}"), cx);
                                    });
                                });
                            }
                        }
                        paint_overlays(&paint.overlays, bounds.origin, theme, window);
                        for highlight in paint.highlights {
                            window.paint_quad(fill(
                                window_bounds(bounds.origin, highlight.rect),
                                Hsla::from(if highlight.current {
                                    theme.search_highlight_current
                                } else {
                                    theme.search_highlight
                                }),
                            ));
                        }
                    },
                )
                .size_full(),
            );
        if let Some(status) = status {
            root = root.child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .left(px(8.0))
                    .max_w(px(700.0))
                    .p_2()
                    .bg(self.theme.canvas_error_surface)
                    .text_color(self.theme.canvas_error_text)
                    .child(status),
            );
        }
        root
    }
}

pub fn run<I, P>(paths: I) -> Result<(), ShellError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let prepared = prepare_tabs(paths)?;
    // Every tab builds its own registry; this one is the app's, and only its
    // command list is read, to resolve the keymap against.
    let mut settings = ShellSettings::load(ConfigPaths::resolve(), &crate::build_registry());
    let mut prepared = prepared;
    for (path, model) in &mut prepared {
        settings
            .notices
            .extend(repair_notice(path, model.provenance()));
        if let Err(error) = apply_page_display(model, &settings.preferences) {
            settings.notices.push(format!(
                "{} opened at the default view: {error}",
                path.display()
            ));
        }
    }
    settings.notices.extend(record_opened(
        &mut settings.recents,
        prepared.iter().map(|(path, _)| path.as_path()),
        settings.preferences.recent_documents,
        settings.paths.recents.as_deref(),
    ));
    // On the notice bar for the user, and on stderr for whoever started the
    // app from a terminal: a keymap that did not load is worth both.
    for notice in &settings.notices {
        eprintln!("onionskin: {notice}");
    }
    let launch_error = Rc::new(RefCell::new(None));
    let error_slot = Rc::clone(&launch_error);

    Application::new().run(move |cx: &mut App| {
        install_search_keybindings(cx);
        find_bar::install_keybindings(cx);
        chrome::install_a11y_keybindings(cx);
        install_command_keybindings(cx, &settings.bindings);
        cx.on_window_closed(|cx| {
            if should_quit_after_window_closed(cx.windows().len()) {
                cx.quit();
            }
        })
        .detach();

        let shell_view_state =
            ShellViewState::new(cx.window_appearance(), settings.preferences.theme);
        let theme = shell_view_state.tokens();
        let menu_state = MenuState::new(
            prepared.len(),
            prepared.first().map(|(_, model)| model.view_state()),
            shell_view_state,
            // Every quick action is on until the user hides one.
            [true; QuickAction::ALL.len()],
            prepared.first().map_or(settings.registry, |(_, model)| {
                RegistryFacts::of(model.registry())
            }),
            settings.recents.documents().len(),
        );
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Onionskin".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let tabs = prepared
                    .into_iter()
                    .map(|(path, model)| (path, cx.new(|_cx| Canvas::new(model, theme))))
                    .collect();
                cx.new(|cx| ShellFrame::new(tabs, shell_view_state, settings, window, cx))
            },
        );
        match result {
            Ok(window) => {
                install_native_menus(cx, window, menu_state);
                cx.activate(true);
                // A build with the probe feature on reads its own
                // accessibility tree back off the window and exits. It walks
                // this boot path rather than a copy of it, so what it reports
                // is what the app publishes.
                #[cfg(all(feature = "a11y-probe", target_os = "macos"))]
                crate::a11y::probe::arm(cx);
            }
            Err(error) => {
                *error_slot.borrow_mut() = Some(ShellError::Window(error.to_string()));
                cx.quit();
            }
        }
    });

    let launch_error = launch_error.borrow_mut().take();
    match launch_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn prepare_tabs<I, P>(paths: I) -> Result<Vec<(PathBuf, CanvasModel)>, ShellError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let paths: Vec<PathBuf> = paths
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .collect();

    paths
        .into_iter()
        .map(|path| {
            let document = Document::open_path(&path).map_err(|source| ShellError::Open {
                path: path.clone(),
                source,
            })?;
            let source_path =
                std::path::absolute(&path).map_err(|source| ShellError::ResolvePath {
                    path: path.clone(),
                    source,
                })?;
            let model = CanvasModel::new(
                document,
                crate::build_registry(),
                ViewSize {
                    width: WINDOW_WIDTH,
                    height: WINDOW_HEIGHT,
                },
            )?;
            Ok((source_path, model))
        })
        .collect()
}

/// Record documents as opened and save the list, reporting a save that
/// failed rather than dropping it.
pub(in crate::shell) fn record_opened<'a>(
    recents: &mut Recents,
    paths: impl Iterator<Item = &'a Path>,
    limit: usize,
    file: Option<&Path>,
) -> Vec<String> {
    let now = SystemTime::now();
    let mut changed = false;
    let mut notices = Vec::new();
    for path in paths {
        match recents.record(path, now, limit) {
            Ok(recorded) => changed |= recorded,
            Err(error) => notices.push(error.to_string()),
        }
    }
    let Some(file) = file.filter(|_| changed) else {
        return notices;
    };
    if let Err(error) = recents.save(file) {
        notices.push(error.to_string());
    }
    notices
}

fn should_quit_after_window_closed(open_window_count: usize) -> bool {
    open_window_count == 0
}

/// Paint what the active tool asked for, over the page rasters.
///
/// A text selection is a filled polygon rather than a rectangle because a
/// rotated view, or a page that draws its text on a slant, turns a glyph's
/// quad into one; a marquee is the dashed rectangle Acrobat draws.
fn paint_overlays(
    overlays: &[OverlayPaint],
    origin: Point<Pixels>,
    theme: ThemeTokens,
    window: &mut Window,
) {
    for overlay in overlays {
        match overlay {
            OverlayPaint::Quads(quads) => {
                for corners in quads {
                    // `/QuadPoints` order is upper-left, upper-right,
                    // lower-left, lower-right; a path has to walk the
                    // perimeter, so the last two swap.
                    let [top_left, top_right, bottom_left, bottom_right] =
                        corners.map(|corner| window_point_at(origin, corner));
                    let mut path = gpui::Path::new(top_left);
                    path.line_to(top_right);
                    path.line_to(bottom_right);
                    path.line_to(bottom_left);
                    window.paint_path(path, theme.selection);
                }
            }
            OverlayPaint::AntsRect(rect) => window.paint_quad(outline(
                window_bounds(origin, *rect),
                theme.drag_preview,
                BorderStyle::Dashed,
            )),
        }
    }
}

fn window_point_at(origin: Point<Pixels>, at: ViewPoint) -> Point<Pixels> {
    point(origin.x + px(at.x), origin.y + px(at.y))
}

fn window_bounds(origin: Point<Pixels>, rect: ViewRect) -> Bounds<Pixels> {
    Bounds {
        origin: point(origin.x + px(rect.origin.x), origin.y + px(rect.origin.y)),
        size: size(px(rect.size.width), px(rect.size.height)),
    }
}

fn status_text(status: &CanvasStatus) -> String {
    match status {
        CanvasStatus::Warning { page, message } => format!("page {page}: {message}"),
        CanvasStatus::Error {
            page: Some(page),
            message,
        } => format!("page {page}: {message}"),
        CanvasStatus::Error {
            page: None,
            message,
        } => message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_real_seeds_prepare_as_two_document_tabs() {
        let tabs = prepare_tabs([seed("hello.pdf"), seed("two-page.pdf")]).unwrap();

        assert_eq!(tabs.len(), 2);
        assert!(tabs.iter().all(|(path, _)| path.is_absolute()));
        assert_eq!(tabs[0].0.file_name().unwrap(), "hello.pdf");
        assert_eq!(tabs[1].0.file_name().unwrap(), "two-page.pdf");
    }

    #[test]
    fn an_invalid_path_fails_during_preparation() {
        let missing = seed("missing.pdf");

        assert!(matches!(
            prepare_tabs([seed("hello.pdf"), missing.clone()]),
            Err(ShellError::Open { path, .. }) if path == missing
        ));
    }

    /// Starting with no path is the Home view, not a failure: Acrobat
    /// opens on Home, and File > Open is how a user leaves it.
    #[test]
    fn empty_startup_prepares_no_tabs_rather_than_failing() {
        assert!(prepare_tabs(Vec::<PathBuf>::new()).unwrap().is_empty());
    }

    #[test]
    fn closing_only_window_quits_but_closing_one_of_many_does_not() {
        assert!(should_quit_after_window_closed(0));
        assert!(!should_quit_after_window_closed(1));
    }

    fn seed(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(name)
    }
}
