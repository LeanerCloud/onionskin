//! The Distance, Perimeter and Area tools, through the real gesture
//! lifecycle, on a page with line art to snap to.

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_cos::{Dict, Object, PendingEdit};
use onionskin_plugin_api::{
    Overlay, PluginManifest, PluginRegistry, PointerInput, Reading, ToolCapability, ToolCtx,
    ToolEnvironment, ToolPlugin,
};
use onionskin_tools_measure::{MeasureTool, MeasureToolsPlugin, Shared};

const VIEWPORT: ViewSize = ViewSize {
    width: 800.0,
    height: 600.0,
};

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

impl Fixture {
    fn new() -> Self {
        let mut doc = Document::open_bytes(line_art()).expect("document opens");
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

    fn click(&mut self, tool: &mut dyn ToolPlugin, point: (f64, f64), clicks: u8) {
        let mut ctx = self.ctx();
        tool.on_pointer_move(&mut ctx, at(point));
        tool.on_pointer_down(
            &mut ctx,
            PointerInput {
                clicks,
                ..at(point)
            },
        );
        tool.on_pointer_up(&mut ctx, at(point));
    }

    fn annotations(&self) -> Vec<Dict> {
        self.doc
            .edit()
            .pending_edits()
            .values()
            .filter_map(|edit| match edit {
                PendingEdit::Set { object, .. } => object.as_dict().cloned(),
                PendingEdit::Delete { .. } => None,
            })
            .filter(|dict| name(dict, b"Type") == Some(b"Annot".to_vec()))
            .collect()
    }

    fn only_annotation(&self) -> Dict {
        let annotations = self.annotations();
        assert_eq!(annotations.len(), 1, "exactly one measurement was kept");
        annotations[0].clone()
    }
}

fn at((x, y): (f64, f64)) -> PointerInput {
    PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    }
}

fn name(dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    dict.get(key)
        .and_then(Object::as_name)
        .map(|name| name.as_bytes().to_vec())
}

fn text(dict: &Dict, key: &[u8]) -> Option<String> {
    match dict.get(key)? {
        Object::String(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

fn numbers(dict: &Dict, key: &[u8]) -> Vec<f64> {
    let Some(Object::Array(items)) = dict.get(key) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Object::Integer(value) => Some(*value as f64),
            Object::Real(value) => Some(*value),
            _ => None,
        })
        .collect()
}

fn reading(tool: &dyn ToolPlugin, label: &str) -> Option<String> {
    tool.readings()
        .into_iter()
        .find(|reading| reading.label == label)
        .map(|reading| reading.value)
}

/// A rule from (72, 100) to (300, 100), and a square with corners at
/// (100, 200) and (200, 300), on a 400-point page.
fn line_art() -> Vec<u8> {
    let content = "72 100 m 300 100 l S 100 200 100 100 re S";
    pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] >>".to_owned(),
    ])
}

fn pdf(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

#[test]
fn a_distance_is_two_clicks_snapped_to_the_rules_ends() {
    let mut fixture = Fixture::new();
    let mut tool = MeasureTool::distance(Shared::default());
    tool.configure(&ToolEnvironment {
        author: Some("Ana".to_owned()),
        ..ToolEnvironment::default()
    });
    tool.on_activate(&mut fixture.ctx());

    fixture.click(&mut tool, (73.0, 101.5), 1);
    assert_eq!(reading(&tool, "Snapped to").as_deref(), Some("Endpoint"));
    // Halfway: the pointer is over the rule's middle.
    tool.on_pointer_move(&mut fixture.ctx(), at((187.0, 98.0)));
    assert_eq!(reading(&tool, "Snapped to").as_deref(), Some("Midpoint"));
    assert_eq!(reading(&tool, "Distance").as_deref(), Some("1.58 in"));
    let overlays = tool.overlays(&fixture.doc);
    assert!(
        matches!(overlays[0], Overlay::Line { from, to } if from.x == 72.0 && to.x == 186.0),
        "{overlays:?}"
    );
    assert!(
        matches!(overlays[1], Overlay::Rect(_)),
        "the snap is marked"
    );

    // Off the rule by two points: snapped onto it.
    fixture.click(&mut tool, (288.0, 102.0), 1);
    let annotation = fixture.only_annotation();
    assert_eq!(name(&annotation, b"Subtype"), Some(b"Line".to_vec()));
    assert_eq!(name(&annotation, b"IT"), Some(b"LineDimension".to_vec()));
    assert_eq!(numbers(&annotation, b"L"), [72.0, 100.0, 288.0, 100.0]);
    assert_eq!(text(&annotation, b"Contents").as_deref(), Some("3.00 in"));
    assert_eq!(text(&annotation, b"T").as_deref(), Some("Ana"), "signed");
    assert_eq!(fixture.doc.edit().history().undo_label(), Some("Distance"));

    let readings = tool.readings();
    assert_eq!(readings[0], Reading::new("Scale", "1 in = 1 in"));
    assert_eq!(
        reading(&tool, "Distance").as_deref(),
        Some("3.00 in"),
        "kept after"
    );
    assert_eq!(reading(&tool, "ΔX").as_deref(), Some("3.00 in"));
    assert_eq!(reading(&tool, "ΔY").as_deref(), Some("0.00 in"));
    assert_eq!(reading(&tool, "Angle").as_deref(), Some("0.0°"));
    assert!(tool
        .overlays(&fixture.doc)
        .iter()
        .all(|overlay| !matches!(overlay, Overlay::Line { .. })));
}

#[test]
fn a_distance_can_be_dragged_and_is_read_at_the_scale_the_tools_share() {
    let mut registry = PluginRegistry::new();
    MeasureToolsPlugin.register(&mut registry);
    let ids: Vec<_> = registry.tools().map(|tool| tool.id()).collect();
    assert_eq!(
        ids,
        ["measure-distance", "measure-perimeter", "measure-area"]
    );
    assert!(registry
        .tool_mut(2)
        .expect("area")
        .choose("scale:1 in = 10 ft"));

    let mut fixture = Fixture::new();
    let tool = registry.tool_mut(0).expect("distance");
    assert_eq!(tool.chosen().as_deref(), Some("scale:1 in = 10 ft"));
    let mut ctx = fixture.ctx();
    tool.on_pointer_down(&mut ctx, at((20.0, 20.0)));
    tool.on_pointer_move(&mut ctx, at((20.0, 164.0)));
    assert_eq!(
        tool.overlays(ctx.doc).len(),
        1,
        "the line, with nothing to snap to"
    );
    tool.on_pointer_up(&mut ctx, at((20.0, 164.0)));
    let annotation = fixture.only_annotation();
    assert_eq!(text(&annotation, b"Contents").as_deref(), Some("20.00 ft"));
    let measure = annotation
        .get(b"Measure")
        .and_then(Object::as_dict)
        .expect("/Measure");
    assert_eq!(text(measure, b"R").as_deref(), Some("1 in = 10 ft"));
    let tool = registry.tool(0).expect("distance");
    assert_eq!(reading(tool, "Angle").as_deref(), Some("90.0°"));
}

#[test]
fn an_area_closes_on_its_first_corner_snapped_to_the_square() {
    let mut fixture = Fixture::new();
    let mut tool = MeasureTool::area(Shared::default());
    for corner in [(101.0, 201.0), (199.0, 202.0), (198.0, 299.0)] {
        fixture.click(&mut tool, corner, 1);
    }
    tool.on_pointer_move(&mut fixture.ctx(), at((102.0, 298.0)));
    let overlays = tool.overlays(&fixture.doc);
    assert!(
        matches!(&overlays[0], Overlay::Polyline { points, closed: true } if points.len() == 4),
        "{overlays:?}"
    );
    fixture.click(&mut tool, (102.0, 298.0), 1);
    assert!(fixture.annotations().is_empty(), "not closed yet");
    fixture.click(&mut tool, (100.5, 200.5), 1);
    let annotation = fixture.only_annotation();
    assert_eq!(name(&annotation, b"Subtype"), Some(b"Polygon".to_vec()));
    assert_eq!(name(&annotation, b"IT"), Some(b"PolygonDimension".to_vec()));
    assert_eq!(
        numbers(&annotation, b"Vertices"),
        [100.0, 200.0, 200.0, 200.0, 200.0, 300.0, 100.0, 300.0]
    );
    // 100 points square: 1.93 sq in.
    assert_eq!(
        text(&annotation, b"Contents").as_deref(),
        Some("1.93 sq in")
    );
    assert_eq!(reading(&tool, "Area").as_deref(), Some("1.93 sq in"));
}

#[test]
fn a_perimeter_ends_on_a_double_click() {
    let mut fixture = Fixture::new();
    let mut tool = MeasureTool::perimeter(Shared::default());
    fixture.click(&mut tool, (20.0, 20.0), 1);
    fixture.click(&mut tool, (20.0, 92.0), 1);
    let overlays = tool.overlays(&fixture.doc);
    assert!(matches!(
        &overlays[0],
        Overlay::Polyline { closed: false, .. }
    ));
    fixture.click(&mut tool, (56.0, 92.0), 1);
    fixture.click(&mut tool, (56.0, 92.0), 2);
    let annotation = fixture.only_annotation();
    assert_eq!(name(&annotation, b"Subtype"), Some(b"PolyLine".to_vec()));
    assert_eq!(
        name(&annotation, b"IT"),
        Some(b"PolyLineDimension".to_vec())
    );
    assert_eq!(text(&annotation, b"Contents").as_deref(), Some("1.50 in"));
    assert_eq!(fixture.doc.edit().history().undo_label(), Some("Perimeter"));
}

#[test]
fn without_markup_a_measurement_is_read_and_not_kept() {
    let mut fixture = Fixture::new();
    let settings = Shared::default();
    let mut tool = MeasureTool::distance(settings.clone());
    assert!(tool.picked("markup"));
    assert!(tool.choose("markup"));
    assert!(!settings.get().markup && !tool.picked("markup"));
    fixture.click(&mut tool, (20.0, 20.0), 1);
    fixture.click(&mut tool, (20.0, 92.0), 1);
    assert!(fixture.annotations().is_empty());
    assert_eq!(reading(&tool, "Distance").as_deref(), Some("1.00 in"));
}

#[test]
fn escape_abandons_and_another_page_or_too_few_points_make_nothing() {
    let mut fixture = Fixture::new();
    let mut tool = MeasureTool::area(Shared::default());
    fixture.click(&mut tool, (20.0, 20.0), 1);
    fixture.click(&mut tool, (40.0, 20.0), 1);
    let mut ctx = fixture.ctx();
    let elsewhere = PointerInput {
        at: PagePoint {
            page: 1,
            x: 40.0,
            y: 40.0,
        },
        ..at((0.0, 0.0))
    };
    tool.on_pointer_down(&mut ctx, elsewhere);
    tool.on_pointer_move(&mut ctx, elsewhere);
    tool.on_commit(&mut ctx);
    assert!(fixture.annotations().is_empty(), "two corners are no area");
    assert_eq!(reading(&tool, "Area"), None);

    fixture.click(&mut tool, (20.0, 20.0), 1);
    fixture.click(&mut tool, (40.0, 20.0), 1);
    tool.on_cancel(&mut fixture.ctx());
    assert!(tool.overlays(&fixture.doc).is_empty());
    tool.on_deactivate(&mut fixture.ctx());
    tool.on_commit(&mut fixture.ctx());
    assert!(fixture.annotations().is_empty());

    let mut distance = MeasureTool::distance(Shared::default());
    let mut ctx = fixture.ctx();
    distance.on_pointer_up(&mut ctx, at((5.0, 5.0)));
    distance.on_pointer_down(&mut ctx, at((20.0, 20.0)));
    distance.on_pointer_up(&mut ctx, at((20.5, 20.0)));
    assert!(
        matches!(distance.overlays(ctx.doc)[..], [Overlay::Line { .. }]),
        "a click, not a drag: the line follows the pointer to the second"
    );
    assert!(fixture.annotations().is_empty());
    fixture.click(&mut distance, (20.0, 56.0), 1);
    assert_eq!(fixture.annotations().len(), 1);
}

#[test]
fn the_tools_describe_themselves_and_their_settings() {
    let settings = Shared::default();
    let tools = [
        MeasureTool::distance(settings.clone()),
        MeasureTool::perimeter(settings.clone()),
        MeasureTool::area(settings),
    ];
    let names: Vec<_> = tools.iter().map(|tool| tool.name()).collect();
    assert_eq!(names, ["Distance", "Perimeter", "Area"]);
    for tool in &tools {
        assert_eq!(tool.group(), "measure");
        assert_eq!(tool.icon(), tool.id());
        assert!(tool.hint().is_some_and(|hint| hint.contains("Click")));
        assert_eq!(tool.capabilities(), [ToolCapability::Measure]);
        let choices = tool.settings();
        assert!(tool.choices().is_empty(), "settings, not things to place");
        assert!(choices
            .iter()
            .any(|choice| choice.id == "snap:intersections"));
        assert!(tool.picked("scale:1 in = 1 in"));
    }
    assert_eq!(MeasureToolsPlugin.name(), "Measure");
    assert_eq!(MeasureToolsPlugin.id(), "onionskin.tools-measure");
}
