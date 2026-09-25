//! The Window menu (P22, rows 8 and 24): New Window, Minimize, Zoom and
//! Bring All to Front.
//!
//! New Window opens a second window on the document in front. It is the same
//! session, not the file opened twice: an edit in either window is an edit
//! to the one document, one Undo in either takes it back once, and a save
//! saves both. Each window has its own view (scroll, zoom, layout) and draws
//! through a render queue of its own.

use gpui::{
    px, size, AppContext as _, Bounds, Context, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use onionskin_core::ViewSize;
use std::path::Path;

use super::ShellFrame;
use crate::shell::Canvas;

/// A second window's size: the first window's, which is the one it opens
/// beside.
const NEW_WINDOW_OFFSET: f32 = 32.0;

impl ShellFrame {
    /// View > New Window, or Window > New Window.
    pub(super) fn open_new_window(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let title = tab.title().to_owned();
        let canvas = tab.canvas.clone();
        let model = canvas.update(cx, |canvas, _| {
            let size = canvas.model.viewport().size();
            canvas.model.new_window(
                crate::build_registry(),
                ViewSize {
                    width: size.width,
                    height: size.height,
                },
            )
        });
        let model = match model {
            Ok(model) => model,
            Err(error) => {
                self.notices
                    .push(format!("A new window could not be opened: {error}"));
                cx.notify();
                return;
            }
        };
        let settings = self.settings.for_new_window();
        let shell_view_state = self.shell_view_state;
        let theme = shell_view_state.tokens();
        let bounds = Bounds::centered(
            None,
            size(
                px(crate::shell::WINDOW_WIDTH),
                px(crate::shell::WINDOW_HEIGHT),
            ),
            cx,
        );
        let bounds = Bounds {
            origin: bounds.origin + gpui::point(px(NEW_WINDOW_OFFSET), px(NEW_WINDOW_OFFSET)),
            ..bounds
        };
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(window_title(&title).into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let mut model = model;
                settings.configure(&mut model);
                let canvas = cx.new(|_| Canvas::new(model, theme));
                cx.new(|cx| {
                    ShellFrame::new(
                        vec![(title, canvas)],
                        shell_view_state,
                        settings,
                        window,
                        cx,
                    )
                })
            },
        );
        if let Err(error) = opened {
            self.notices
                .push(format!("A new window could not be opened: {error}"));
        }
        cx.notify();
    }
}

/// "report.pdf", as a second window on it is titled.
fn window_title(title: &str) -> String {
    Path::new(title).file_name().map_or_else(
        || "Onionskin".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Every Onionskin window forward, this one last so it stays in front.
pub(super) fn bring_all_to_front(window: &mut Window, cx: &mut Context<ShellFrame>) {
    let this = window.window_handle();
    for handle in cx.windows() {
        if handle != this {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        }
    }
    window.activate_window();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_titled_with_its_documents_name() {
        assert_eq!(window_title("/tmp/report.pdf"), "report.pdf");
        assert_eq!(window_title("/"), "Onionskin");
    }
}
