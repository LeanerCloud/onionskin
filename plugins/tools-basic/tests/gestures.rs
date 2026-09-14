//! Gesture semantics, driven headlessly against a real document: the tools
//! only ever see page-space pointer input and a viewport, so everything
//! they do can be asserted without a window.

use onionskin_core::{
    Document, FitMode, Modifiers, PageAlignment, PagePoint, PageQuad, PageRect, ViewPoint,
    ViewSize, Viewport, ZoomPolicy,
};
use onionskin_corpus_testing::seed;
use onionskin_plugin_api::{Overlay, PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_basic::{
    DynamicZoomTool, HandTool, SelectRegionTool, SelectTextTool, SnapshotTool, ZoomTool,
};

const VIEWPORT: ViewSize = ViewSize {
    width: 800.0,
    height: 600.0,
};

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

impl Fixture {
    fn open(name: &str) -> Self {
        let mut doc = Document::open_path(&seed(name)).expect("seed opens");
        let mut viewport =
            Viewport::new(doc.page_count(), VIEWPORT, 12.0).expect("viewport is valid");
        for page in 0..doc.page_count() {
            let geometry = doc.page_geometry(page).expect("page measures").clone();
            viewport.measure_page(geometry).expect("page is measurable");
        }
        viewport.fit(FitMode::Page).expect("the page fits");
        Self { doc, viewport }
    }

    fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }

    /// The centre of one glyph, in the page space the canvas hands tools.
    fn glyph(&mut self, page: usize, index: usize) -> PagePoint {
        let quad = self.glyph_quad(page, index);
        PagePoint {
            page,
            x: quad.corners.iter().map(|(x, _)| x).sum::<f64>() / 4.0,
            y: quad.corners.iter().map(|(_, y)| y).sum::<f64>() / 4.0,
        }
    }

    fn glyph_quad(&mut self, page: usize, index: usize) -> PageQuad {
        self.doc.page_text(page).expect("page text extracts").runs[0].glyphs[index].quad
    }

    fn run_text(&mut self, page: usize) -> String {
        self.doc.page_text(page).expect("page text extracts").runs[0]
            .text
            .clone()
    }

    /// A drag too short to be a drag: under the tools' viewport-pixel
    /// threshold at whatever zoom the fixture is at.
    fn nudge(&self, at: PagePoint) -> PagePoint {
        PagePoint {
            x: at.x + f64::from(1.0 / self.viewport.zoom()),
            ..at
        }
    }

    /// The page point currently under a screen position. Dynamic zoom is a
    /// screen gesture, so its tests are written in screen coordinates and
    /// converted here, against the viewport as it stands.
    fn page_point_at(&self, view: ViewPoint) -> PagePoint {
        self.viewport
            .page_point_at(view)
            .expect("the point is mappable")
            .expect("the point is on a page")
    }

    fn selected_text(&self) -> Option<String> {
        self.doc
            .selection()
            .text()
            .map(|selection| selection.text.clone())
    }
}

fn input(at: PagePoint, modifiers: Modifiers) -> PointerInput {
    PointerInput {
        at,
        pressure: 1.0,
        modifiers,
    }
}

fn drag(fixture: &mut Fixture, tool: &mut dyn ToolPlugin, from: PagePoint, to: PagePoint) {
    press(fixture, tool, from, Modifiers::default());
    tool.on_pointer_move(&mut fixture.ctx(), input(to, Modifiers::default()));
    tool.on_pointer_up(&mut fixture.ctx(), input(to, Modifiers::default()));
}

fn press(fixture: &mut Fixture, tool: &mut dyn ToolPlugin, at: PagePoint, modifiers: Modifiers) {
    tool.on_pointer_down(&mut fixture.ctx(), input(at, modifiers));
}

fn click(fixture: &mut Fixture, tool: &mut dyn ToolPlugin, at: PagePoint, modifiers: Modifiers) {
    press(fixture, tool, at, modifiers);
    tool.on_pointer_up(&mut fixture.ctx(), input(at, modifiers));
}

#[test]
fn a_drag_across_glyphs_selects_them_in_document_order() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SelectTextTool::new();
    let from = fixture.glyph(0, 2);
    let to = fixture.glyph(0, 8);
    let text = fixture.run_text(0);

    drag(&mut fixture, &mut tool, from, to);

    let selection = fixture
        .doc
        .selection()
        .text()
        .expect("the drag selected text");
    assert_eq!(selection.page, 0);
    assert_eq!(selection.quads.len(), 7);
    assert_eq!(selection.text, text[2..9]);
    // Dragging the other way selects the same glyphs: the range runs in
    // document order whichever end the gesture started from.
    drag(&mut fixture, &mut tool, to, from);
    assert_eq!(fixture.selected_text().as_deref(), Some(&text[2..9]));
}

#[test]
fn a_tiny_drag_selects_nothing() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SelectTextTool::new();
    let from = fixture.glyph(0, 4);
    let to = fixture.nudge(from);

    drag(&mut fixture, &mut tool, from, to);

    assert!(fixture.doc.selection().text().is_none());
    assert!(fixture.doc.selection().text_quads().is_empty());
    assert!(tool.overlays(&fixture.doc).is_empty());
}

#[test]
fn shift_extends_an_existing_text_selection() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SelectTextTool::new();
    let from = fixture.glyph(0, 2);
    let short = fixture.glyph(0, 4);
    let far = fixture.glyph(0, 10);
    let text = fixture.run_text(0);

    drag(&mut fixture, &mut tool, from, short);
    assert_eq!(fixture.selected_text().as_deref(), Some(&text[2..5]));

    click(
        &mut fixture,
        &mut tool,
        far,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );

    assert_eq!(fixture.selected_text().as_deref(), Some(&text[2..11]));
}

#[test]
fn a_selection_draws_one_quad_overlay_per_glyph() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SelectTextTool::new();
    let from = fixture.glyph(0, 0);
    let to = fixture.glyph(0, 3);
    let first = fixture.glyph_quad(0, 0);

    drag(&mut fixture, &mut tool, from, to);

    let overlays = tool.overlays(&fixture.doc);
    let [Overlay::Quads(quads)] = overlays.as_slice() else {
        panic!("a text selection draws one quads overlay, got {overlays:?}");
    };
    assert_eq!(quads.len(), 4);
    assert_eq!(quads[0], first);
}

#[test]
fn cancelling_a_text_gesture_drops_the_selection() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SelectTextTool::new();
    let from = fixture.glyph(0, 1);
    let to = fixture.glyph(0, 6);

    press(&mut fixture, &mut tool, from, Modifiers::default());
    tool.on_pointer_move(&mut fixture.ctx(), input(to, Modifiers::default()));
    assert!(fixture.doc.selection().text().is_some());

    tool.on_cancel(&mut fixture.ctx());

    assert!(fixture.doc.selection().text().is_none());
    assert!(tool.overlays(&fixture.doc).is_empty());
}

#[test]
fn a_region_drag_produces_one_ants_rect() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SelectRegionTool::new();
    let from = PagePoint {
        page: 0,
        x: 20.0,
        y: 20.0,
    };
    let to = PagePoint {
        page: 0,
        x: 120.0,
        y: 70.0,
    };
    let expected = PageRect {
        page: 0,
        x0: 20.0,
        y0: 20.0,
        x1: 120.0,
        y1: 70.0,
    };

    press(&mut fixture, &mut tool, from, Modifiers::default());
    tool.on_pointer_move(&mut fixture.ctx(), input(to, Modifiers::default()));

    assert_eq!(
        tool.overlays(&fixture.doc),
        vec![Overlay::AntsRect(expected)]
    );

    tool.on_pointer_up(&mut fixture.ctx(), input(to, Modifiers::default()));

    assert_eq!(fixture.doc.selection().region(), Some(expected));
}

#[test]
fn a_region_click_selects_nothing() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SelectRegionTool::new();
    let at = PagePoint {
        page: 0,
        x: 40.0,
        y: 40.0,
    };

    let nudged = fixture.nudge(at);
    click(&mut fixture, &mut tool, nudged, Modifiers::default());

    assert_eq!(fixture.doc.selection().region(), None);
    assert!(tool.overlays(&fixture.doc).is_empty());
}

#[test]
fn a_snapshot_drag_selects_the_region_and_raises_one_request() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SnapshotTool::new();
    let from = PagePoint {
        page: 0,
        x: 10.0,
        y: 15.0,
    };
    let to = PagePoint {
        page: 0,
        x: 90.0,
        y: 65.0,
    };
    let expected = PageRect {
        page: 0,
        x0: 10.0,
        y0: 15.0,
        x1: 90.0,
        y1: 65.0,
    };

    drag(&mut fixture, &mut tool, from, to);

    assert_eq!(fixture.doc.selection().region(), Some(expected));
    assert_eq!(
        fixture.doc.take_snapshot_request().map(|it| it.region),
        Some(expected)
    );
    assert_eq!(fixture.doc.take_snapshot_request(), None);
}

#[test]
fn a_snapshot_click_raises_nothing() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = SnapshotTool::new();
    let at = PagePoint {
        page: 0,
        x: 30.0,
        y: 30.0,
    };

    let nudged = fixture.nudge(at);
    click(&mut fixture, &mut tool, nudged, Modifiers::default());

    assert_eq!(fixture.doc.take_snapshot_request(), None);
}

#[test]
fn marquee_zoom_fits_the_dragged_rectangle() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = ZoomTool::new();
    let before = fixture.viewport.zoom();
    let from = PagePoint {
        page: 0,
        x: 30.0,
        y: 30.0,
    };
    let to = PagePoint {
        page: 0,
        x: 80.0,
        y: 60.0,
    };

    drag(&mut fixture, &mut tool, from, to);

    assert!(matches!(
        fixture.viewport.zoom_policy(),
        ZoomPolicy::Fit(FitMode::Visible(_))
    ));
    assert!(
        fixture.viewport.zoom() > before,
        "fitting a part of the page zooms in from fit-page"
    );
    for corner in [from, to] {
        let at = fixture
            .viewport
            .view_point_for(corner)
            .expect("the page is measured")
            .expect("the page is laid out");
        assert!(
            (-1.0..=VIEWPORT.width + 1.0).contains(&at.x)
                && (-1.0..=VIEWPORT.height + 1.0).contains(&at.y),
            "the dragged rectangle should be on screen, got {at:?}"
        );
    }
}

#[test]
fn a_zoom_click_zooms_in_and_acrobats_modifiers_zoom_out() {
    let at = PagePoint {
        page: 0,
        x: 50.0,
        y: 50.0,
    };

    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = ZoomTool::new();
    let start = fixture.viewport.zoom();
    click(&mut fixture, &mut tool, at, Modifiers::default());
    let zoomed_in = fixture.viewport.zoom();
    assert!(zoomed_in > start);

    for modifiers in [
        Modifiers {
            alt: true,
            ..Modifiers::default()
        },
        Modifiers {
            ctrl_or_cmd: true,
            ..Modifiers::default()
        },
    ] {
        let mut fixture = Fixture::open("hello.pdf");
        let mut tool = ZoomTool::new();
        click(&mut fixture, &mut tool, at, modifiers);
        assert!(
            fixture.viewport.zoom() < start,
            "{modifiers:?} should zoom out"
        );
    }
}

#[test]
fn the_hand_keeps_the_grabbed_point_under_the_cursor() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = HandTool::new();
    fixture
        .viewport
        .zoom_to(
            8.0,
            ViewPoint {
                x: VIEWPORT.width / 2.0,
                y: VIEWPORT.height / 2.0,
            },
        )
        .expect("zoom is valid");
    // A zoom may leave the offset outside its scroll range; the first pan
    // would then clamp it, which is the viewport's business and not the
    // gesture's. Settle on a scroll position inside the range first.
    fixture
        .viewport
        .go_to_page(0, PageAlignment::Center)
        .expect("the page exists");
    let grabbed = fixture.glyph(0, 5);
    let start = fixture
        .viewport
        .view_point_for(grabbed)
        .unwrap()
        .expect("the glyph is on screen");
    let moved = ViewPoint {
        x: start.x - 40.0,
        y: start.y - 30.0,
    };
    let under_cursor = fixture
        .viewport
        .page_point_at(moved)
        .unwrap()
        .expect("the cursor is still over the page");

    press(&mut fixture, &mut tool, grabbed, Modifiers::default());
    tool.on_pointer_move(
        &mut fixture.ctx(),
        input(under_cursor, Modifiers::default()),
    );

    let now = fixture
        .viewport
        .view_point_for(grabbed)
        .unwrap()
        .expect("the glyph is still laid out");
    assert!((now.x - moved.x).abs() < 0.5, "{now:?} != {moved:?}");
    assert!((now.y - moved.y).abs() < 0.5, "{now:?} != {moved:?}");
}

#[test]
fn every_tool_survives_a_page_with_no_text() {
    let mut fixture = Fixture::open("minimal.pdf");
    let at = PagePoint {
        page: 0,
        x: 40.0,
        y: 40.0,
    };
    let to = PagePoint {
        page: 0,
        x: 90.0,
        y: 70.0,
    };
    let mut tools: Vec<Box<dyn ToolPlugin>> = vec![
        Box::new(HandTool::new()),
        Box::new(SelectTextTool::new()),
        Box::new(SelectRegionTool::new()),
        Box::new(ZoomTool::new()),
        Box::new(SnapshotTool::new()),
    ];

    for tool in &mut tools {
        tool.on_activate(&mut fixture.ctx());
        drag(&mut fixture, tool.as_mut(), at, to);
        tool.on_cancel(&mut fixture.ctx());
        tool.on_deactivate(&mut fixture.ctx());
    }

    assert!(fixture.doc.selection().text().is_none());
}

/// Move the pointer to a screen position mid-gesture. Screen coordinates,
/// because that is what the gesture is measured in.
fn move_to(fixture: &mut Fixture, tool: &mut dyn ToolPlugin, view: ViewPoint) {
    let to = fixture.page_point_at(view);
    tool.on_pointer_move(&mut fixture.ctx(), input(to, Modifiers::default()));
}

/// A screen point `dy` pixels below `from`. Negative is up the screen.
fn below(from: ViewPoint, dy: f32) -> ViewPoint {
    ViewPoint {
        x: from.x,
        y: from.y + dy,
    }
}

/// Acrobat's dynamic zoom: drag up to zoom in, down to zoom out, and the
/// page point under the press stays under the press for the whole gesture.
///
/// 240 screen pixels is one doubling, so 120 up is a factor of root two and
/// coming back to where the press started undoes it.
#[test]
fn dragging_up_zooms_in_and_dragging_down_zooms_out_about_the_press_point() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = DynamicZoomTool::new();
    let anchor_view = ViewPoint { x: 400.0, y: 300.0 };
    let anchor_page = fixture.page_point_at(anchor_view);
    let before = fixture.viewport.zoom();

    press(&mut fixture, &mut tool, anchor_page, Modifiers::default());
    move_to(&mut fixture, &mut tool, below(anchor_view, -120.0));

    let zoomed_in = fixture.viewport.zoom();
    assert!(
        (zoomed_in / before - 2.0_f32.sqrt()).abs() < 1e-3,
        "120 pixels up should be a factor of root two: {before} to {zoomed_in}"
    );
    assert_eq!(fixture.viewport.zoom_policy(), ZoomPolicy::Fixed);

    move_to(&mut fixture, &mut tool, anchor_view);
    let back = fixture.viewport.zoom();
    assert!(
        (back / before - 1.0).abs() < 1e-3,
        "dragging back down should undo the zoom: {before} to {back}"
    );

    let held = fixture.page_point_at(anchor_view);
    assert_eq!(held.page, anchor_page.page);
    assert!(
        (held.x - anchor_page.x).abs() < 0.5 && (held.y - anchor_page.y).abs() < 0.5,
        "the anchor drifted over two zooms: {anchor_page:?} to {held:?}"
    );

    tool.on_pointer_up(&mut fixture.ctx(), input(anchor_page, Modifiers::default()));
}

/// The gesture ends where the canvas says it ends. A move outside one is a
/// no-op rather than a zoom against a stale anchor.
#[test]
fn dynamic_zoom_only_zooms_between_a_press_and_the_end_of_its_gesture() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = DynamicZoomTool::new();
    let anchor_view = ViewPoint { x: 400.0, y: 300.0 };
    let anchor_page = fixture.page_point_at(anchor_view);
    let before = fixture.viewport.zoom();

    move_to(&mut fixture, &mut tool, below(anchor_view, -120.0));
    assert_eq!(
        fixture.viewport.zoom(),
        before,
        "a move with no press zoomed"
    );

    let endings: [fn(&mut DynamicZoomTool, &mut Fixture); 3] = [
        |tool, fixture| tool.on_cancel(&mut fixture.ctx()),
        |tool, fixture| tool.on_deactivate(&mut fixture.ctx()),
        |tool, fixture| {
            let at = fixture.page_point_at(ViewPoint { x: 400.0, y: 300.0 });
            tool.on_pointer_up(&mut fixture.ctx(), input(at, Modifiers::default()));
        },
    ];
    for end in endings {
        press(&mut fixture, &mut tool, anchor_page, Modifiers::default());
        end(&mut tool, &mut fixture);
        let ended = fixture.viewport.zoom();

        move_to(&mut fixture, &mut tool, below(anchor_view, -120.0));

        assert_eq!(
            fixture.viewport.zoom(),
            ended,
            "a move after the gesture ended zoomed"
        );
    }
}

/// A press the window edge cuts off still zooms.
///
/// `Viewport::dynamic_zoom` validates its anchor and answers
/// `AnchorOutsideViewport` for one outside the view, which this tool
/// discards, so an unclamped anchor makes the whole gesture a silent no-op
/// rather than a wrong one. The measurement is the other half of that split
/// and must not be clamped: the drag is counted from where the pointer
/// really was, so the part of the press outside the window still counts.
#[test]
fn a_press_below_the_window_still_anchors_and_still_measures_from_there() {
    let mut fixture = Fixture::open("hello.pdf");
    let mut tool = DynamicZoomTool::new();
    // Zoomed in far enough that the page runs off the bottom of the window,
    // so a press below the window is still a press on the page.
    fixture
        .viewport
        .zoom_to(12.0, ViewPoint { x: 400.0, y: 300.0 })
        .expect("the zoom applies");
    fixture
        .viewport
        .go_to_page(0, PageAlignment::Start)
        .expect("the page is reachable");
    let below = ViewPoint { x: 400.0, y: 650.0 };
    assert!(
        below.y > VIEWPORT.height,
        "the press has to be outside the window for this to test anything"
    );
    let pressed = fixture.page_point_at(below);
    let before = fixture.viewport.zoom();

    press(&mut fixture, &mut tool, pressed, Modifiers::default());
    move_to(&mut fixture, &mut tool, ViewPoint { x: 400.0, y: 500.0 });

    let after = fixture.viewport.zoom();
    assert!(
        after > before,
        "the gesture did nothing at all, which is what an anchor outside the \
         viewport makes it: {before} to {after}"
    );
    // 150 pixels up, measured from the press at 650 rather than from the
    // window edge the anchor was pulled back to.
    let expected = before * 2.0_f32.powf(150.0 / 240.0);
    assert!(
        (after / expected - 1.0).abs() < 1e-3,
        "{after} is not 2^(150/240) of {before}, so the drag was measured \
         from the clamped anchor rather than from the press"
    );
}
