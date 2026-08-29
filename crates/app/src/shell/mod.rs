//! GPUI adapters and the first windowed document shell.

use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    canvas as gpui_canvas, div, fill, point, px, size, App, AppContext as _, Application, Bounds,
    Context, DispatchPhase, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PinchEvent, Pixels, Point,
    Render, ScrollWheelEvent, Styled as _, Timer, TitlebarOptions, TouchPhase, Window,
    WindowBounds, WindowOptions,
};
use onionskin_core::{Document, ViewPoint, ViewRect, ViewSize};

use self::canvas::{CanvasError, CanvasModel, CanvasStatus, PaintList};

pub mod canvas;
pub mod input;

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 860.0;
const SCROLL_LINE_HEIGHT: f32 = 30.0;
const POLL_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug)]
pub enum ShellError {
    Open(onionskin_core::Error),
    Canvas(CanvasError),
    Window(String),
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => write!(f, "cannot open PDF: {error}"),
            Self::Canvas(error) => write!(f, "cannot start canvas: {error}"),
            Self::Window(error) => write!(f, "cannot create window: {error}"),
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open(error) => Some(error),
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

pub struct Canvas {
    model: CanvasModel,
    polling: bool,
}

impl Canvas {
    fn new(model: CanvasModel) -> Self {
        Self {
            model,
            polling: false,
        }
    }

    fn prepare_paint(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) -> PaintList {
        let result = self
            .model
            .resize(
                ViewPoint {
                    x: f32::from(bounds.origin.x),
                    y: f32::from(bounds.origin.y),
                },
                ViewSize {
                    width: f32::from(bounds.size.width),
                    height: f32::from(bounds.size.height),
                },
            )
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
        let mut root = div()
            .id("canvas")
            .size_full()
            .overflow_hidden()
            .bg(gpui::rgb(0x2c2c30))
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
                        for tile in paint.tiles {
                            window
                                .paint_image(
                                    window_bounds(bounds.origin, tile.rect),
                                    gpui::Corners::default(),
                                    tile.image,
                                    0,
                                    false,
                                )
                                .expect("canvas tile image is valid");
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
                    .bg(gpui::rgb(0x7f1d1d))
                    .text_color(gpui::white())
                    .child(status),
            );
        }
        root
    }
}

pub fn run(path: impl AsRef<Path>) -> Result<(), ShellError> {
    let document = Document::open_path(path.as_ref()).map_err(ShellError::Open)?;
    let model = CanvasModel::new(
        document,
        crate::build_registry(),
        ViewSize {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
    )?;
    let launch_error = Rc::new(RefCell::new(None));
    let error_slot = Rc::clone(&launch_error);

    Application::new().run(move |cx: &mut App| {
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
            |_window, cx| cx.new(|_cx| Canvas::new(model)),
        );
        match result {
            Ok(_) => cx.activate(true),
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
