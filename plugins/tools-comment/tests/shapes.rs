//! The seven shape tools, through the real gesture lifecycle.
//!
//! The assertion this file exists for is **the in-progress overlay matches the
//! committed result**. An oval previewed as a circle looks almost right and
//! ships; the only thing that catches it is rendering the preview and the
//! annotation and comparing, which is what `an_ovals_preview_is_the_ellipse_it_-
//! commits` does. The same for a polygon's closing edge.

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, PageRect, ViewSize, Viewport};
use onionskin_cos::{Object, PendingEdit};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_comment::ShapeTool;

const VIEWPORT: ViewSize = ViewSize {
    width: 800.0,
    height: 600.0,
};

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

impl Fixture {
    fn blank() -> Self {
        let mut doc = Document::open_bytes(blank_page()).expect("document opens");
        let mut viewport =
            Viewport::new(doc.page_count(), VIEWPORT, 12.0).expect("viewport is valid");
        for page in 0..doc.page_count() {
            let geometry = doc.page_geometry(page).expect("page measures").clone();
            viewport.measure_page(geometry).expect("page is measurable");
        }
        viewport.fit(FitMode::Page).expect("the page fits");
        Fixture { doc, viewport }
    }

    fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }

    fn drag(&mut self, tool: &mut dyn ToolPlugin, from: (f64, f64), to: (f64, f64)) {
        let mut ctx = self.ctx();
        tool.on_pointer_down(&mut ctx, at(from));
        tool.on_pointer_move(&mut ctx, at(to));
        tool.on_pointer_up(&mut ctx, at(to));
    }

    /// Click each point, then Enter, which is how the three vertex tools end.
    fn click_through(&mut self, tool: &mut dyn ToolPlugin, points: &[(f64, f64)]) {
        let mut ctx = self.ctx();
        for point in points {
            tool.on_pointer_move(&mut ctx, at(*point));
            tool.on_pointer_down(&mut ctx, at(*point));
            tool.on_pointer_up(&mut ctx, at(*point));
        }
        tool.on_commit(&mut ctx);
    }

    fn annotations(&self) -> Vec<onionskin_cos::Dict> {
        self.doc
            .edit()
            .pending_edits()
            .values()
            .filter_map(|edit| match edit {
                PendingEdit::Set { object, .. } => object.as_dict().cloned(),
                PendingEdit::Delete { .. } => None,
            })
            .filter(|dict| {
                dict.get(b"Type")
                    .and_then(Object::as_name)
                    .is_some_and(|name| name.as_bytes() == b"Annot")
            })
            .collect()
    }

    fn only_annotation(&self) -> onionskin_cos::Dict {
        let annotations = self.annotations();
        assert_eq!(annotations.len(), 1, "exactly one annotation was written");
        annotations[0].clone()
    }

    fn appearance(&self) -> String {
        let annotation = self.only_annotation();
        let Some(Object::Ref(objref)) = annotation
            .get(b"AP")
            .and_then(Object::as_dict)
            .and_then(|ap| ap.get(b"N"))
        else {
            panic!("/AP /N is an indirect reference");
        };
        let number = objref.number;
        let stream = self
            .doc
            .edit()
            .pending_edits()
            .get(&number)
            .and_then(|edit| match edit {
                PendingEdit::Set { object, .. } => object.as_stream().cloned(),
                PendingEdit::Delete { .. } => None,
            })
            .expect("the appearance stream was written");
        String::from_utf8_lossy(&stream.raw).into_owned()
    }

    fn edits(&self) -> usize {
        self.doc.edit().history().reach()
    }

    /// Page 0 as a reader would draw it now, pending annotation and all.
    fn pixels(&mut self) -> Vec<[u8; 4]> {
        self.doc
            .render_page_now(0, 2.0)
            .expect("the page renders")
            .raster
            .rgba()
            .chunks_exact(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
            .collect()
    }
}

fn at((x, y): (f64, f64)) -> PointerInput {
    PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
    }
}

// ---------------------------------------------------------------------------
// What a drag produces
// ---------------------------------------------------------------------------

#[test]
fn each_dragged_shape_writes_its_own_subtype() {
    for (mut tool, expected) in [
        (ShapeTool::line(), "Line"),
        (ShapeTool::arrow(), "Line"),
        (ShapeTool::rectangle(), "Square"),
        (ShapeTool::oval(), "Circle"),
    ] {
        let mut fixture = Fixture::blank();
        let name = tool.name();
        fixture.drag(&mut tool, (100.0, 700.0), (300.0, 560.0));

        let annotation = fixture.only_annotation();
        assert_eq!(
            annotation
                .get(b"Subtype")
                .and_then(Object::as_name)
                .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
            Some(expected.to_owned()),
            "{name}"
        );
        assert_eq!(
            numbers(annotation.get(b"Rect")),
            vec![100.0, 560.0, 300.0, 700.0],
            "{name}: the rect is the dragged rectangle, normalized"
        );
    }
}

/// The review risk this package names. Acrobat has no arrow subtype, so an
/// arrow is a `/Line` with `/LE`; written as anything else it renders in
/// Acrobat as a line with no head.
#[test]
fn an_arrow_is_a_line_with_endings_and_a_plain_line_has_none() {
    let mut fixture = Fixture::blank();
    fixture.drag(&mut ShapeTool::arrow(), (100.0, 700.0), (300.0, 560.0));

    let arrow = fixture.only_annotation();
    let Some(Object::Array(endings)) = arrow.get(b"LE") else {
        panic!("/LE is an array");
    };
    let names: Vec<String> = endings
        .iter()
        .filter_map(Object::as_name)
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
        .collect();
    assert_eq!(names, vec!["None", "ClosedArrow"], "the head is on the end");
    assert_eq!(
        numbers(arrow.get(b"L")),
        vec![100.0, 700.0, 300.0, 560.0],
        "and it is a real /Line, with /L"
    );

    // The head has to be in the appearance too: a reader with an /AP draws the
    // stream, so a head that lives only in /LE is a line.
    assert!(
        fixture.appearance().contains(" f\n"),
        "the closed head is filled in the stream: {}",
        fixture.appearance()
    );

    let mut plain = Fixture::blank();
    plain.drag(&mut ShapeTool::line(), (100.0, 700.0), (300.0, 560.0));
    assert!(
        plain.only_annotation().get(b"LE").is_none(),
        "a plain line claims no endings"
    );
}

#[test]
fn a_click_makes_no_shape() {
    for mut tool in [
        ShapeTool::line(),
        ShapeTool::arrow(),
        ShapeTool::rectangle(),
        ShapeTool::oval(),
    ] {
        let mut fixture = Fixture::blank();
        let name = tool.name();
        fixture.drag(&mut tool, (100.0, 700.0), (100.0, 700.0));

        assert!(
            fixture.annotations().is_empty(),
            "{name} made a shape out of a click"
        );
        assert_eq!(fixture.edits(), 0, "{name}: and no undo entry");
    }
}

// ---------------------------------------------------------------------------
// Vertices
// ---------------------------------------------------------------------------

#[test]
fn a_polygon_is_closed_and_connected_lines_are_not() {
    let points = [(100.0, 700.0), (300.0, 700.0), (200.0, 560.0)];

    let mut polygon = Fixture::blank();
    polygon.click_through(&mut ShapeTool::polygon(), &points);
    let written = polygon.only_annotation();
    assert_eq!(
        written
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
        Some("Polygon".to_owned())
    );
    assert_eq!(
        numbers(written.get(b"Vertices")),
        vec![100.0, 700.0, 300.0, 700.0, 200.0, 560.0],
        "the closing edge is implied by the subtype, not a repeated vertex"
    );

    let mut open = Fixture::blank();
    open.click_through(&mut ShapeTool::connected_lines(), &points);
    assert_eq!(
        open.only_annotation()
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
        Some("PolyLine".to_owned())
    );
}

/// Two clicks cannot be a polygon: a closed shape of two points is a line
/// drawn twice.
#[test]
fn a_polygon_of_fewer_than_three_vertices_writes_nothing() {
    let mut fixture = Fixture::blank();
    fixture.click_through(&mut ShapeTool::polygon(), &[(100.0, 700.0), (300.0, 700.0)]);

    assert!(fixture.annotations().is_empty());
    assert_eq!(fixture.edits(), 0);
}

#[test]
fn a_shape_built_by_clicking_and_then_cancelled_writes_nothing() {
    let mut fixture = Fixture::blank();
    let mut tool = ShapeTool::polygon();
    {
        let mut ctx = fixture.ctx();
        for point in [(100.0, 700.0), (300.0, 700.0), (200.0, 560.0)] {
            tool.on_pointer_down(&mut ctx, at(point));
        }
        tool.on_cancel(&mut ctx);
        tool.on_commit(&mut ctx);
    }

    assert!(fixture.annotations().is_empty());
}

/// The mutation this package turns on: dropping `/BE` has to fail.
#[test]
fn a_cloud_is_a_polygon_with_a_cloudy_border_effect() {
    let points = [(100.0, 700.0), (300.0, 700.0), (200.0, 560.0)];

    let mut cloud = Fixture::blank();
    cloud.click_through(&mut ShapeTool::cloud(), &points);
    let written = cloud.only_annotation();
    assert_eq!(
        written
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
        Some("Polygon".to_owned()),
        "a cloud is a polygon with an effect, not a subtype of its own"
    );
    let effect = written
        .get(b"BE")
        .and_then(Object::as_dict)
        .cloned()
        .expect("/BE is a dictionary");
    assert_eq!(
        effect
            .get(b"S")
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
        Some("C".to_owned()),
        "/S /C is what makes it cloudy"
    );

    // The scallops have to be in the appearance as well, for the same reason
    // the arrow head does. Compared against a plain polygon rather than against
    // an exact pixel pattern.
    let mut plain = Fixture::blank();
    plain.click_through(&mut ShapeTool::polygon(), &points);
    assert_ne!(
        cloud.appearance(),
        plain.appearance(),
        "a cloud that draws the same path as a polygon is a polygon"
    );
    assert!(
        cloud.appearance().contains(" c\n"),
        "the scallops are curves: {}",
        cloud.appearance()
    );
    assert!(
        !plain.appearance().contains(" c\n"),
        "and a plain polygon has none"
    );

    // And it has to reach the page. Compared as rendered pixels against a
    // plain polygon over the same vertices rather than against an exact
    // pattern: what is being claimed is that the edge is not the straight one.
    let cloudy = cloud.pixels();
    let straight = plain.pixels();
    assert_eq!(
        cloudy.len(),
        straight.len(),
        "the same page at the same zoom"
    );
    let differing = cloudy
        .iter()
        .zip(&straight)
        .filter(|(left, right)| left != right)
        .count();
    assert!(
        differing > 200,
        "the cloud renders the same as a plain polygon: only {differing} pixels differ"
    );
}

/// The scallops bulge out of the shape whichever way the user clicked.
///
/// Which side is outward depends on the winding, and the user picks that by
/// the order of their clicks. A cloud that assumes one winding scallops inward
/// for the other and looks like a gear - and every other assertion here stays
/// green while it does, because an inward scallop differs from a straight edge
/// and covers about as many pixels as an outward one. What separates them is
/// **where** the ink is: outside the straight triangle, or not.
#[test]
fn a_cloud_scallops_outward_whichever_way_its_vertices_were_clicked() {
    let counterclockwise = [(100.0, 560.0), (300.0, 560.0), (200.0, 700.0)];
    let mut clockwise = counterclockwise;
    clockwise.reverse();

    for (order, vertices) in [
        ("counterclockwise", counterclockwise),
        ("clockwise", clockwise),
    ] {
        let mut fixture = Fixture::blank();
        fixture.click_through(&mut ShapeTool::cloud(), &vertices);

        let geometry = fixture.doc.page_geometry(0).expect("page measures").clone();
        let corners: Vec<(f64, f64)> = counterclockwise
            .iter()
            .map(|(x, y)| {
                geometry
                    .user_to_device_point(
                        PagePoint {
                            page: 0,
                            x: *x,
                            y: *y,
                        },
                        2.0,
                    )
                    .expect("the vertex maps")
            })
            .collect();

        let width = fixture
            .doc
            .render_page_now(0, 2.0)
            .expect("the page renders")
            .raster
            .width() as usize;
        let outside = fixture
            .pixels()
            .into_iter()
            .enumerate()
            .filter(|(_, pixel)| *pixel != [255, 255, 255, 255])
            .filter(|(index, _)| {
                let (x, y) = ((index % width) as f64 + 0.5, (index / width) as f64 + 0.5);
                // A margin, so the stroke drawn along the edge itself does not
                // count as ink outside the shape.
                !inside_triangle(&corners, x, y, 3.0)
            })
            .count();
        assert!(
            outside > 300,
            "{order}: only {outside} pixels of ink land outside the straight triangle, \
             so the scallops turned inward"
        );
    }
}

/// Whether `(x, y)` is inside the triangle, grown by `margin` on every side.
fn inside_triangle(corners: &[(f64, f64)], x: f64, y: f64, margin: f64) -> bool {
    let centre = (
        corners.iter().map(|(x, _)| x).sum::<f64>() / corners.len() as f64,
        corners.iter().map(|(_, y)| y).sum::<f64>() / corners.len() as f64,
    );
    let grown: Vec<(f64, f64)> = corners
        .iter()
        .map(|(cx, cy)| {
            let (dx, dy) = (cx - centre.0, cy - centre.1);
            let length = dx.hypot(dy).max(f64::MIN_POSITIVE);
            (cx + dx / length * margin, cy + dy / length * margin)
        })
        .collect();
    let side = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
    let signs = [
        side(grown[0], grown[1]),
        side(grown[1], grown[2]),
        side(grown[2], grown[0]),
    ];
    signs.iter().all(|s| *s >= 0.0) || signs.iter().all(|s| *s <= 0.0)
}

// ---------------------------------------------------------------------------
// The preview is the committed shape
// ---------------------------------------------------------------------------

/// The assertion the `Overlay` correction exists for. A circle preview for an
/// oval looks almost right at the moment of drawing and wrong the instant the
/// annotation appears; comparing the two is the only thing that catches it.
#[test]
fn an_ovals_preview_is_the_ellipse_it_commits() {
    let mut fixture = Fixture::blank();
    let mut tool = ShapeTool::oval();
    // A rectangle that is not square, so a circle preview cannot match it by
    // accident.
    let (from, to) = ((100.0, 700.0), (340.0, 620.0));
    {
        let mut ctx = fixture.ctx();
        tool.on_pointer_down(&mut ctx, at(from));
        tool.on_pointer_move(&mut ctx, at(to));
    }

    let overlays = tool.overlays(&fixture.doc);
    let [Overlay::Ellipse { bounds }] = overlays.as_slice() else {
        panic!("an oval previews as an ellipse, got {overlays:?}");
    };
    assert_eq!(
        *bounds,
        PageRect {
            page: 0,
            x0: 100.0,
            y0: 620.0,
            x1: 340.0,
            y1: 700.0
        }
    );
    assert_ne!(
        bounds.x1 - bounds.x0,
        bounds.y1 - bounds.y0,
        "the fixture has to be a real ellipse, or a circle preview would pass"
    );

    {
        let mut ctx = fixture.ctx();
        tool.on_pointer_up(&mut ctx, at(to));
    }
    let committed = fixture.only_annotation();
    assert_eq!(
        numbers(committed.get(b"Rect")),
        vec![bounds.x0, bounds.y0, bounds.x1, bounds.y1],
        "the committed ellipse occupies the previewed bounds"
    );
}

#[test]
fn a_polygons_preview_is_closed_and_a_polylines_is_not() {
    for (mut tool, closed) in [
        (ShapeTool::polygon(), true),
        (ShapeTool::cloud(), true),
        (ShapeTool::connected_lines(), false),
    ] {
        let mut fixture = Fixture::blank();
        let name = tool.name();
        {
            let mut ctx = fixture.ctx();
            for point in [(100.0, 700.0), (300.0, 700.0)] {
                tool.on_pointer_down(&mut ctx, at(point));
            }
            tool.on_pointer_move(&mut ctx, at((200.0, 560.0)));
        }

        let overlays = tool.overlays(&fixture.doc);
        let [Overlay::Polyline {
            points,
            closed: previewed,
        }] = overlays.as_slice()
        else {
            panic!("{name} previews as a polyline, got {overlays:?}");
        };
        assert_eq!(*previewed, closed, "{name}");
        assert_eq!(
            points.len(),
            3,
            "{name}: the vertex under the pointer is part of what the user sees"
        );
    }
}

/// `Overlay::Ellipse` with equal axes is the circle the removed
/// `Overlay::Circle` described, so the replacement lost nothing.
#[test]
fn an_ellipse_with_equal_axes_is_a_circle() {
    let mut fixture = Fixture::blank();
    let mut tool = ShapeTool::oval();
    {
        let mut ctx = fixture.ctx();
        tool.on_pointer_down(&mut ctx, at((100.0, 700.0)));
        tool.on_pointer_move(&mut ctx, at((180.0, 620.0)));
    }

    let overlays = tool.overlays(&fixture.doc);
    let [Overlay::Ellipse { bounds }] = overlays.as_slice() else {
        panic!("got {overlays:?}");
    };
    assert_eq!(bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
    assert_eq!(
        ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0),
        (140.0, 660.0),
        "centred where a centre-and-radius circle would have been"
    );
}

#[test]
fn a_line_previews_the_segment_it_commits() {
    let mut fixture = Fixture::blank();
    let mut tool = ShapeTool::line();
    {
        let mut ctx = fixture.ctx();
        tool.on_pointer_down(&mut ctx, at((100.0, 700.0)));
        tool.on_pointer_move(&mut ctx, at((300.0, 560.0)));
    }

    let overlays = tool.overlays(&fixture.doc);
    let [Overlay::Line { from, to }] = overlays.as_slice() else {
        panic!("got {overlays:?}");
    };
    assert_eq!(
        ((from.x, from.y), (to.x, to.y)),
        ((100.0, 700.0), (300.0, 560.0))
    );

    {
        let mut ctx = fixture.ctx();
        tool.on_pointer_up(&mut ctx, at((300.0, 560.0)));
    }
    assert_eq!(
        numbers(fixture.only_annotation().get(b"L")),
        vec![100.0, 700.0, 300.0, 560.0]
    );
}

/// Nothing drawn, nothing previewed: an overlay for a gesture that is not
/// happening is a shape on screen the user cannot get rid of.
#[test]
fn no_gesture_previews_nothing() {
    let fixture = Fixture::blank();
    for tool in [
        ShapeTool::line(),
        ShapeTool::arrow(),
        ShapeTool::rectangle(),
        ShapeTool::oval(),
        ShapeTool::polygon(),
        ShapeTool::connected_lines(),
        ShapeTool::cloud(),
    ] {
        assert!(
            tool.overlays(&fixture.doc).is_empty(),
            "{} previewed something before the gesture started",
            tool.name()
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn numbers(object: Option<&Object>) -> Vec<f64> {
    let Some(Object::Array(items)) = object else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Object::Integer(v) => Some(*v as f64),
            Object::Real(v) => Some(*v),
            _ => None,
        })
        .collect()
}

fn blank_page() -> Vec<u8> {
    let objects: &[&[u8]] = &[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
    ];
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}
