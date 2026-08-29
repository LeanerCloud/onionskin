use std::fmt;

use gpui::{Modifiers as GpuiModifiers, Pixels, Point};
use onionskin_core::{Modifiers, ViewPoint, Viewport, ViewportError};
use onionskin_plugin_api::PointerInput;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputError {
    InvalidPressure(f32),
    Viewport(ViewportError),
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPressure(pressure) => {
                write!(
                    f,
                    "pointer pressure must be between 0 and 1, got {pressure}"
                )
            }
            Self::Viewport(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for InputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPressure(_) => None,
            Self::Viewport(error) => Some(error),
        }
    }
}

impl From<ViewportError> for InputError {
    fn from(error: ViewportError) -> Self {
        Self::Viewport(error)
    }
}

pub fn pointer_input(
    viewport: &Viewport,
    window_point: Point<Pixels>,
    canvas_origin: Point<Pixels>,
    pressure: f32,
    modifiers: GpuiModifiers,
) -> Result<Option<PointerInput>, InputError> {
    if !pressure.is_finite() || !(0.0..=1.0).contains(&pressure) {
        return Err(InputError::InvalidPressure(pressure));
    }

    let point = ViewPoint {
        x: f32::from(window_point.x) - f32::from(canvas_origin.x),
        y: f32::from(window_point.y) - f32::from(canvas_origin.y),
    };
    Ok(viewport.page_point_at(point)?.map(|at| PointerInput {
        at,
        pressure,
        modifiers: Modifiers {
            shift: modifiers.shift,
            alt: modifiers.alt,
            ctrl_or_cmd: modifiers.control || modifiers.platform,
        },
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragKind {
    Pan,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragUpdate {
    PanBy(ViewPoint),
    ToolMove,
    CancelTool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Pan { last: ViewPoint },
    Tool,
}

#[derive(Debug, Default)]
pub struct InputState {
    drag: Option<Drag>,
}

impl InputState {
    pub fn begin_pan(&mut self, at: ViewPoint) {
        self.drag = Some(Drag::Pan { last: at });
    }

    pub fn begin_tool(&mut self) {
        self.drag = Some(Drag::Tool);
    }

    pub fn move_to(&mut self, at: ViewPoint, left_button_pressed: bool) -> Option<DragUpdate> {
        if !left_button_pressed {
            return self.cancel();
        }

        match self.drag.as_mut() {
            Some(Drag::Pan { last }) => {
                let delta = ViewPoint {
                    x: at.x - last.x,
                    y: at.y - last.y,
                };
                *last = at;
                Some(DragUpdate::PanBy(delta))
            }
            Some(Drag::Tool) => Some(DragUpdate::ToolMove),
            None => None,
        }
    }

    pub fn end(&mut self) -> Option<DragKind> {
        self.drag.take().map(|drag| match drag {
            Drag::Pan { .. } => DragKind::Pan,
            Drag::Tool => DragKind::Tool,
        })
    }

    pub fn cancel(&mut self) -> Option<DragUpdate> {
        match self.drag.take() {
            Some(Drag::Tool) => Some(DragUpdate::CancelTool),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{point, px};
    use onionskin_core::{Document, FitMode, PageAlignment, PageGeometry, ViewRotation, ViewSize};

    use super::*;

    fn measured_viewport() -> (Viewport, PageGeometry) {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
        let mut document = Document::open_path(&path).expect("seed opens");
        let first = document.page_geometry(0).expect("page is valid").clone();
        let geometry = document.page_geometry(1).expect("page is valid").clone();
        let mut viewport = Viewport::new(
            document.page_count(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
            12.0,
        )
        .expect("viewport is valid");
        viewport.measure_page(first).expect("page is measured");
        viewport
            .measure_page(geometry.clone())
            .expect("page is measured");
        viewport
            .go_to_page(1, PageAlignment::Start)
            .expect("page is visible");
        viewport
            .set_rotation(ViewRotation::Clockwise90)
            .expect("rotation is valid");
        viewport.fit(FitMode::Page).expect("page fits");
        (viewport, geometry)
    }

    #[test]
    fn gpui_pointer_input_uses_crop_pdf_rotation_and_view_rotation() {
        let (viewport, geometry) = measured_viewport();
        assert_eq!(geometry.crop_box, Some([10.0, 10.0, 190.0, 90.0]));
        assert_eq!(geometry.rotate, 90);
        let page = viewport
            .visible_pages()
            .unwrap()
            .into_iter()
            .find(|page| page.page == 1)
            .expect("second page is visible")
            .rect;
        let intrinsic = ViewPoint {
            x: geometry.render_size.0 as f32 * 0.37,
            y: geometry.render_size.1 as f32 * 0.61,
        };
        let rotated = ViewPoint {
            x: geometry.render_size.1 as f32 - intrinsic.y,
            y: intrinsic.x,
        };
        let local = ViewPoint {
            x: page.origin.x + rotated.x * viewport.zoom(),
            y: page.origin.y + rotated.y * viewport.zoom(),
        };
        let origin = point(px(50.0), px(40.0));
        let input = pointer_input(
            &viewport,
            point(px(local.x + 50.0), px(local.y + 40.0)),
            origin,
            0.42,
            GpuiModifiers {
                control: true,
                alt: true,
                shift: true,
                platform: true,
                function: false,
            },
        )
        .unwrap()
        .expect("point lands on the page");

        let expected = geometry
            .device_to_user(
                f64::from(intrinsic.x * viewport.zoom()),
                f64::from(intrinsic.y * viewport.zoom()),
                viewport.zoom(),
            )
            .unwrap();
        assert_eq!(input.at.page, 1);
        assert!((input.at.x - expected.x).abs() < 1e-5);
        assert!((input.at.y - expected.y).abs() < 1e-5);
        assert_eq!(input.at, viewport.page_point_at(local).unwrap().unwrap());
        assert_eq!(input.pressure, 0.42);
        assert_eq!(
            input.modifiers,
            Modifiers {
                shift: true,
                alt: true,
                ctrl_or_cmd: true,
            }
        );
    }

    #[test]
    fn a_background_pointer_does_not_create_tool_input() {
        let (viewport, _) = measured_viewport();
        assert!(pointer_input(
            &viewport,
            point(px(51.0), px(41.0)),
            point(px(50.0), px(40.0)),
            1.0,
            GpuiModifiers::default(),
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn invalid_pressure_is_rejected() {
        let (viewport, _) = measured_viewport();
        for pressure in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(matches!(
                pointer_input(
                    &viewport,
                    point(px(0.0), px(0.0)),
                    point(px(0.0), px(0.0)),
                    pressure,
                    GpuiModifiers::default(),
                ),
                Err(InputError::InvalidPressure(value)) if value.to_bits() == pressure.to_bits()
            ));
        }
    }

    #[test]
    fn pan_moves_are_incremental() {
        let mut state = InputState::default();
        state.begin_pan(ViewPoint { x: 10.0, y: 20.0 });

        assert_eq!(
            state.move_to(ViewPoint { x: 14.0, y: 27.0 }, true),
            Some(DragUpdate::PanBy(ViewPoint { x: 4.0, y: 7.0 }))
        );
        assert_eq!(
            state.move_to(ViewPoint { x: 13.0, y: 30.0 }, true),
            Some(DragUpdate::PanBy(ViewPoint { x: -1.0, y: 3.0 }))
        );
    }

    #[test]
    fn a_drag_released_outside_the_window_stops_panning() {
        let mut state = InputState::default();
        state.begin_pan(ViewPoint { x: 10.0, y: 20.0 });

        assert_eq!(state.end(), Some(DragKind::Pan));
        assert_eq!(state.move_to(ViewPoint { x: 30.0, y: 40.0 }, true), None);
    }

    #[test]
    fn a_move_without_the_left_button_clears_a_stale_pan() {
        let mut state = InputState::default();
        state.begin_pan(ViewPoint { x: 10.0, y: 20.0 });

        assert_eq!(state.move_to(ViewPoint { x: 30.0, y: 40.0 }, false), None);
        assert_eq!(state.end(), None);
    }

    #[test]
    fn a_move_without_the_left_button_cancels_a_stale_tool_gesture() {
        let mut state = InputState::default();
        state.begin_tool();

        assert_eq!(
            state.move_to(ViewPoint { x: 30.0, y: 40.0 }, false),
            Some(DragUpdate::CancelTool)
        );
        assert_eq!(state.end(), None);
    }

    #[test]
    fn a_tool_gesture_leaving_the_canvas_is_cancelled() {
        let mut state = InputState::default();
        state.begin_tool();

        assert_eq!(state.cancel(), Some(DragUpdate::CancelTool));
        assert_eq!(state.end(), None);
    }
}
