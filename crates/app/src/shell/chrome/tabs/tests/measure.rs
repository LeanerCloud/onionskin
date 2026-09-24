//! Measuring on a real window: the side panel's Measurement Info while a
//! distance is made, its settings, and the measurement kept as a comment.

use super::*;
use crate::shell::A11yElement;

/// A window on a blank letter page.
fn blank_window(cx: &mut TestAppContext) -> (tempfile::TempDir, gpui::WindowHandle<ShellFrame>) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("plan.pdf");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    std::fs::write(&path, bytes).expect("writes");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let window = bound_window_with_models(
        vec![(path, model)],
        crate::config::ConfigPaths::default(),
        cx,
    )
    .0;
    (dir, window)
}

/// Click page point `at` with whichever tool is active.
fn click(window: gpui::WindowHandle<ShellFrame>, at: (f64, f64), cx: &mut TestAppContext) {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| {
                let rect = [at.0, at.1, at.0, at.1];
                let (view, _, _) = canvas.model.view_rect(0, rect).expect("in view");
                let origin = canvas.model.canvas_origin();
                let point = gpui::point(gpui::px(origin.x + view.x), gpui::px(origin.y + view.y));
                let modifiers = gpui::Modifiers::default();
                canvas
                    .model
                    .pointer_move(point, 1.0, modifiers, false)
                    .unwrap();
                canvas.model.pointer_down(point, 1.0, modifiers).unwrap();
                canvas.model.pointer_up(point, 1.0, modifiers).unwrap();
                cx.notify();
            });
        })
        .unwrap();
}

fn find<'a>(node: &'a A11yElement, key: &str) -> Option<&'a A11yElement> {
    node.find(&gpui::SharedString::from(key.to_owned()).into())
}

#[gpui::test]
fn a_distance_is_read_in_the_side_panel_and_kept_on_the_page(cx: &mut TestAppContext) {
    let (_dir, window) = blank_window(cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                let index = canvas
                    .model
                    .registry()
                    .tools()
                    .position(|tool| tool.id() == "measure-distance")
                    .expect("installed");
                canvas.model.activate_tool(index).expect("activates");
            });
        })
        .unwrap();

    window
        .update(cx, |frame, window, cx| {
            let described = frame.accessible(window, cx);
            let scale = find(&described, "tool-setting-scale:1 in = 10 ft").expect("listed");
            assert_eq!(scale.state.toggled, Some(false));
            frame.run_activation(
                Activation::ToolSetting("scale:1 in = 10 ft".to_owned()),
                window,
                cx,
            );
            let described = frame.accessible(window, cx);
            let scale = find(&described, "tool-setting-scale:1 in = 10 ft").expect("listed");
            assert_eq!(scale.state.toggled, Some(true), "the scale chosen");
            let readings = find(&described, "side-panel-readings").expect("the info");
            assert_eq!(readings.label, "Measurement Info");
            assert_eq!(readings.children[0].label, "Scale: 1 in = 10 ft");
        })
        .unwrap();

    click(window, (72.0, 400.0), cx);
    click(window, (288.0, 400.0), cx);

    window
        .update(cx, |frame, window, cx| {
            let described = frame.accessible(window, cx);
            let readings = find(&described, "side-panel-readings").expect("the info");
            let labels: Vec<&str> = readings
                .children
                .iter()
                .map(|child| child.label.as_str())
                .collect();
            assert!(labels.contains(&"Distance: 30.00 ft"), "{labels:?}");
            assert!(labels.contains(&"Angle: 0.0°"), "{labels:?}");

            let canvas = frame.active_canvas().expect("a tab").read(cx);
            let document = canvas.model.document_mut();
            assert_eq!(document.edit().history().undo_label(), Some("Distance"));
        })
        .unwrap();
}
