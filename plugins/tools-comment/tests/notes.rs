//! Sticky notes and the three `/FreeText` tools, through the real gesture
//! lifecycle.
//!
//! Three questions per tool, because the answers differ and getting any of
//! them wrong is a tool that looks like it works: what a **drag** produces,
//! what a **click** produces, and what a **degenerate** gesture produces. A
//! sticky note is a click and a text box is not; both have to be right.

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_cos::{Object, PendingEdit};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_comment::{FreeTextTool, NoteTool};

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

    fn gesture(&mut self, tool: &mut dyn ToolPlugin, from: (f64, f64), to: (f64, f64)) {
        let input = |(x, y): (f64, f64)| PointerInput {
            at: PagePoint { page: 0, x, y },
            pressure: 1.0,
            modifiers: Modifiers::default(),
        };
        let mut ctx = self.ctx();
        tool.on_pointer_down(&mut ctx, input(from));
        tool.on_pointer_move(&mut ctx, input(to));
        tool.on_pointer_up(&mut ctx, input(to));
    }

    /// A click: press and release at the same point, with a move in between,
    /// which is what a real pointer sends.
    fn click(&mut self, tool: &mut dyn ToolPlugin, at: (f64, f64)) {
        self.gesture(tool, at, at);
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

    fn edits(&self) -> usize {
        self.doc.edit().history().reach()
    }
}

// ---------------------------------------------------------------------------
// Sticky note
// ---------------------------------------------------------------------------

#[test]
fn a_click_places_a_sticky_note_at_the_pointer() {
    let mut fixture = Fixture::blank();
    fixture.click(&mut NoteTool::new(), (100.0, 700.0));

    let note = fixture.only_annotation();
    assert_eq!(subtype(&note), "Text");
    assert_eq!(
        note.get(b"Name").and_then(Object::as_name).map(as_string),
        Some("Note".to_owned()),
        "the icon a reader draws"
    );
    let rect = numbers(note.get(b"Rect"));
    assert_eq!(
        (rect[0], rect[3]),
        (100.0, 700.0),
        "the click is the icon's upper-left corner, so the icon lands under the pointer"
    );
    assert!(
        rect[2] > rect[0] && rect[3] > rect[1],
        "the rect is not degenerate: {rect:?}"
    );
}

/// A sticky note is a click, so a drag across the page is the user doing
/// something else, and leaving a note behind where they started would be a
/// note they did not ask for.
#[test]
fn a_sticky_note_dragged_across_the_page_writes_nothing() {
    let mut fixture = Fixture::blank();
    fixture.gesture(&mut NoteTool::new(), (100.0, 700.0), (300.0, 500.0));

    assert!(fixture.annotations().is_empty());
    assert_eq!(fixture.edits(), 0, "and no undo entry either");
}

#[test]
fn a_sticky_note_release_without_a_press_writes_nothing() {
    let mut fixture = Fixture::blank();
    let mut tool = NoteTool::new();
    let input = PointerInput {
        at: PagePoint {
            page: 0,
            x: 100.0,
            y: 700.0,
        },
        pressure: 1.0,
        modifiers: Modifiers::default(),
    };
    let mut ctx = fixture.ctx();
    tool.on_pointer_up(&mut ctx, input);

    assert!(fixture.annotations().is_empty());
}

// ---------------------------------------------------------------------------
// Free text
// ---------------------------------------------------------------------------

#[test]
fn a_drag_sizes_a_text_box_to_the_rectangle_dragged_out() {
    let mut fixture = Fixture::blank();
    fixture.gesture(
        &mut FreeTextTool::text_box(),
        (100.0, 700.0),
        (280.0, 640.0),
    );

    let annotation = fixture.only_annotation();
    assert_eq!(subtype(&annotation), "FreeText");
    assert_eq!(
        numbers(annotation.get(b"Rect")),
        vec![100.0, 640.0, 280.0, 700.0],
        "the rect is the dragged rectangle, normalized"
    );
    assert!(
        annotation.get(b"IT").is_none(),
        "a plain text box claims no intent"
    );
}

/// The typewriter is the one that is not a box: a click is the whole gesture,
/// and a reader must not draw a frame around it.
#[test]
fn a_click_places_a_typewriter_with_no_border() {
    let mut fixture = Fixture::blank();
    fixture.click(&mut FreeTextTool::typewriter(), (100.0, 700.0));

    let annotation = fixture.only_annotation();
    assert_eq!(
        annotation
            .get(b"IT")
            .and_then(Object::as_name)
            .map(as_string),
        Some("FreeTextTypewriter".to_owned())
    );
    let border = annotation
        .get(b"BS")
        .and_then(Object::as_dict)
        .and_then(|bs| bs.get(b"W"))
        .cloned();
    assert!(
        matches!(border, Some(Object::Real(width)) if width == 0.0),
        "a typewriter has no border to draw, got {border:?}"
    );
    let rect = numbers(annotation.get(b"Rect"));
    assert!(
        rect[2] > rect[0] && rect[3] > rect[1],
        "a click still gets a usable box: {rect:?}"
    );
}

/// Every free-text tool names its font in `/DA`, and the appearance stream
/// draws with the same one. A `/DA` naming a font the stream does not resource
/// is what makes a text box render differently in Acrobat.
#[test]
fn the_default_appearance_and_the_stream_name_the_same_font() {
    let mut fixture = Fixture::blank();
    fixture.gesture(
        &mut FreeTextTool::text_box(),
        (100.0, 700.0),
        (280.0, 640.0),
    );

    let annotation = fixture.only_annotation();
    let Some(Object::String(da)) = annotation.get(b"DA") else {
        panic!("/DA is a string");
    };
    let da = String::from_utf8_lossy(da).into_owned();
    assert!(
        da.starts_with("/Helv "),
        "/DA names the font by its resource key, got {da:?}"
    );
    assert!(da.contains(" Tf "), "/DA selects a font: {da:?}");

    let form = appearance_of(&fixture, &annotation);
    let fonts = form
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .and_then(|resources| resources.get(b"Font"))
        .and_then(Object::as_dict)
        .cloned()
        .expect("the appearance resources a font");
    assert!(
        fonts.get(b"Helv").is_some(),
        "the stream resources the same key /DA names"
    );
    let font = fonts
        .get(b"Helv")
        .and_then(Object::as_dict)
        .expect("the font is a dictionary");
    assert_eq!(
        font.get(b"BaseFont")
            .and_then(Object::as_name)
            .map(as_string),
        Some("Helvetica".to_owned())
    );
    assert!(
        font.get(b"FontFile").is_none()
            && font.get(b"FontFile2").is_none()
            && font.get(b"FontFile3").is_none()
            && font.get(b"FontDescriptor").is_none(),
        "the font is named, never embedded"
    );
}

/// The mutation this package turns on: dropping `/CL` has to fail.
#[test]
fn a_callout_writes_a_leader_from_its_tail_to_its_box() {
    let mut fixture = Fixture::blank();
    // The drag starts at what the callout points at and ends where the box
    // goes, which is the order Acrobat asks for.
    fixture.gesture(&mut FreeTextTool::callout(), (120.0, 600.0), (300.0, 700.0));

    let annotation = fixture.only_annotation();
    assert_eq!(
        annotation
            .get(b"IT")
            .and_then(Object::as_name)
            .map(as_string),
        Some("FreeTextCallout".to_owned())
    );

    let leader = numbers(annotation.get(b"CL"));
    assert_eq!(leader.len(), 6, "three points: tail, knee, landing");
    assert_eq!(
        (leader[0], leader[1]),
        (120.0, 600.0),
        "the leader starts at what the callout points at"
    );

    let rect = numbers(annotation.get(b"Rect"));
    let (landing_x, landing_y) = (leader[4], leader[5]);
    assert!(
        (landing_x - rect[0]).abs() < 0.01
            || (landing_x - rect[2]).abs() < 0.01
            || (landing_y - rect[1]).abs() < 0.01
            || (landing_y - rect[3]).abs() < 0.01,
        "the leader lands on an edge of the rect: {landing_x},{landing_y} against {rect:?}"
    );
    assert!(
        landing_x >= rect[0] - 0.01
            && landing_x <= rect[2] + 0.01
            && landing_y >= rect[1] - 0.01
            && landing_y <= rect[3] + 0.01,
        "and on the rect rather than past it"
    );

    // A reader with an /AP draws what the stream says, so a leader that lives
    // only in /CL is a leader nobody sees.
    let form = appearance_of(&fixture, &annotation);
    let stream = String::from_utf8_lossy(&form.raw).into_owned();
    assert!(
        stream.contains(" m\n") && stream.contains(" l\n"),
        "the appearance draws the leader itself: {stream}"
    );
}

/// Every one of them: a gesture that selects nothing leaves nothing behind.
#[test]
fn a_gesture_on_no_page_writes_nothing() {
    for mut tool in [
        Box::new(FreeTextTool::typewriter()) as Box<dyn ToolPlugin>,
        Box::new(FreeTextTool::text_box()),
        Box::new(FreeTextTool::callout()),
        Box::new(NoteTool::new()),
    ] {
        let mut fixture = Fixture::blank();
        let off = PointerInput {
            at: PagePoint {
                page: 7,
                x: 10.0,
                y: 10.0,
            },
            pressure: 1.0,
            modifiers: Modifiers::default(),
        };
        let mut ctx = fixture.ctx();
        tool.on_pointer_down(&mut ctx, off);
        tool.on_pointer_up(&mut ctx, off);

        assert!(
            fixture.annotations().is_empty(),
            "{} wrote an annotation onto a page that is not there",
            tool.name()
        );
        assert_eq!(fixture.edits(), 0, "{}: and no undo entry", tool.name());
    }
}

/// Cancelling mid-drag is the user changing their mind, and it has to leave
/// nothing behind - including on the next gesture.
#[test]
fn escape_mid_drag_writes_nothing_and_does_not_leak_into_the_next_gesture() {
    let mut fixture = Fixture::blank();
    let mut tool = FreeTextTool::text_box();
    let input = |x: f64, y: f64| PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
    };
    {
        let mut ctx = fixture.ctx();
        tool.on_pointer_down(&mut ctx, input(100.0, 700.0));
        tool.on_pointer_move(&mut ctx, input(280.0, 640.0));
        tool.on_cancel(&mut ctx);
    }
    assert!(fixture.annotations().is_empty());

    fixture.gesture(&mut tool, (400.0, 500.0), (500.0, 460.0));
    assert_eq!(
        numbers(fixture.only_annotation().get(b"Rect")),
        vec![400.0, 460.0, 500.0, 500.0],
        "the second box is the second drag, not the union of both"
    );
}

/// The in-progress preview is the shape that will be written, which is the
/// whole reason a preview exists.
#[test]
fn the_preview_shows_the_box_and_a_callouts_leader() {
    let mut fixture = Fixture::blank();
    let mut tool = FreeTextTool::callout();
    let input = |x: f64, y: f64| PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
    };
    {
        let mut ctx = fixture.ctx();
        tool.on_pointer_down(&mut ctx, input(120.0, 600.0));
        tool.on_pointer_move(&mut ctx, input(300.0, 700.0));
    }

    let overlays = tool.overlays(&fixture.doc);
    assert!(
        overlays
            .iter()
            .any(|overlay| matches!(overlay, Overlay::Rect(_))),
        "the box is previewed: {overlays:?}"
    );
    let Some(Overlay::Polyline { points, closed }) = overlays
        .iter()
        .find(|overlay| matches!(overlay, Overlay::Polyline { .. }))
    else {
        panic!("the leader is previewed: {overlays:?}");
    };
    assert!(!closed, "a leader is an open path");
    assert_eq!(points.len(), 3);
    assert_eq!((points[0].x, points[0].y), (120.0, 600.0));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn appearance_of(fixture: &Fixture, annotation: &onionskin_cos::Dict) -> onionskin_cos::Stream {
    let Some(Object::Ref(objref)) = annotation
        .get(b"AP")
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
    else {
        panic!("/AP /N is an indirect reference");
    };
    let number = objref.number;
    fixture
        .doc
        .edit()
        .pending_edits()
        .get(&number)
        .and_then(|edit| match edit {
            PendingEdit::Set { object, .. } => object.as_stream().cloned(),
            PendingEdit::Delete { .. } => None,
        })
        .expect("the appearance stream was written")
}

fn subtype(dict: &onionskin_cos::Dict) -> String {
    dict.get(b"Subtype")
        .and_then(Object::as_name)
        .map(as_string)
        .expect("/Subtype")
}

fn as_string(name: &onionskin_cos::Name) -> String {
    String::from_utf8_lossy(name.as_bytes()).into_owned()
}

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

/// A blank page, hand-built: none of these tools reads the page's contents, so
/// a fixture with text on it would only add ways for the test to be wrong.
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
