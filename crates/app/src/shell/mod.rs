//! GPUI adapters and the first windowed document shell.

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    canvas as gpui_canvas, div, fill, outline, point, px, size, App, AppContext as _, Application,
    BorderStyle, Bounds, ClipboardItem, Context, DispatchPhase, Image, ImageFormat,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, PinchEvent, Pixels, Point, Render,
    ScrollWheelEvent, Styled as _, Timer, TitlebarOptions, TouchPhase, Window, WindowBounds,
    WindowOptions,
};
use onionskin_core::{Document, ViewPoint, ViewRect, ViewSize};

use self::canvas::{CanvasError, CanvasModel, CanvasStatus, OverlayPaint, PaintList, ViewAction};
use self::chrome::{
    install_native_menus, install_search_keybindings, MenuState, ShellFrame, ShellViewState,
    ThemeTokens,
};

pub mod canvas;
mod chrome;
mod context_menu;
pub mod input;

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 860.0;
const SCROLL_LINE_HEIGHT: f32 = 30.0;
const POLL_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug)]
pub enum ShellError {
    NoDocuments,
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
            Self::NoDocuments => write!(f, "cannot open a window without a PDF path"),
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
            Self::NoDocuments | Self::Window(_) => None,
        }
    }
}

impl From<CanvasError> for ShellError {
    fn from(error: CanvasError) -> Self {
        Self::Canvas(error)
    }
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
                        let pending = canvas.model.has_pending_work();
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
                    move |bounds, paint: PaintList, window, _cx| {
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
                            window
                                .with_content_mask(
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
                                )
                                .expect("canvas tile image is valid");
                        }
                        paint_overlays(&paint.overlays, bounds.origin, theme, window);
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
    let launch_error = Rc::new(RefCell::new(None));
    let error_slot = Rc::clone(&launch_error);

    Application::new().run(move |cx: &mut App| {
        install_search_keybindings(cx);
        cx.on_window_closed(|cx| {
            if should_quit_after_window_closed(cx.windows().len()) {
                cx.quit();
            }
        })
        .detach();

        let shell_view_state = ShellViewState::new(cx.window_appearance());
        let theme = shell_view_state.tokens();
        let menu_state = MenuState::initial(
            prepared.len(),
            prepared.first().map(|(_, model)| model.view_state()),
            shell_view_state,
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
                cx.new(|cx| ShellFrame::new(tabs, shell_view_state, window, cx))
            },
        );
        match result {
            Ok(window) => {
                install_native_menus(cx, window, menu_state);
                cx.activate(true);
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
    if paths.is_empty() {
        return Err(ShellError::NoDocuments);
    }

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

    #[test]
    fn empty_startup_is_rejected_explicitly() {
        assert!(matches!(
            prepare_tabs(Vec::<PathBuf>::new()),
            Err(ShellError::NoDocuments)
        ));
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
