//! The Fill & Sign tools, driven headlessly: page-space pointer input and a
//! viewport, as the canvas hands them over, read back as the annotations a
//! reader finds and the pixels the renderer draws.

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, Subtype, ViewSize, Viewport};
use onionskin_corpus_testing::encrypted_fixture;
use onionskin_plugin_api::{
    Overlay, PluginManifest, PluginRegistry, PointerInput, ToolCapability, ToolCtx,
    ToolEnvironment, ToolPlugin,
};
use onionskin_tools_fill_sign::signature::{drawn, typed};
use onionskin_tools_fill_sign::{
    FillSignToolsPlugin, FillTextTool, ShapeTool, SignTool, SignatureKind, SignatureLibrary,
    Symbol, SymbolTool,
};

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

fn input(x: f64, y: f64) -> PointerInput {
    PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    }
}

impl Fixture {
    fn open(mut doc: Document) -> Self {
        let mut viewport = Viewport::new(
            doc.page_count(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
            12.0,
        )
        .expect("viewport");
        for page in 0..doc.page_count() {
            let geometry = doc.page_geometry(page).expect("measures").clone();
            viewport.measure_page(geometry).expect("measurable");
        }
        viewport.fit(FitMode::Page).expect("fits");
        Self { doc, viewport }
    }

    /// A blank Letter page.
    fn seeded() -> Self {
        let objects: [&[u8]; 3] = [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>",
            b"<< /Type /Page /Parent 2 0 R >>",
        ];
        let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(b"trailer\n<< /Size 4 /Root 1 0 R >>\n");
        out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
        Self::open(Document::open_bytes(out).expect("opens"))
    }

    fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }

    fn gesture(&mut self, tool: &mut dyn ToolPlugin, from: (f64, f64), to: (f64, f64)) {
        let mut ctx = self.ctx();
        tool.on_pointer_down(&mut ctx, input(from.0, from.1));
        tool.on_pointer_move(&mut ctx, input(to.0, to.1));
        tool.on_pointer_up(&mut ctx, input(to.0, to.1));
    }

    fn annotations(&mut self) -> Vec<onionskin_core::ReadAnnotation> {
        self.doc.annotations().expect("reads")
    }

    /// Whether any pixel within `radius` points of page point `(x, y)` is
    /// dark, at one pixel a point.
    fn inked_near(&mut self, (x, y): (f64, f64), radius: f64) -> bool {
        let height = self.doc.page_geometry(0).expect("geometry").render_size.1;
        let raster = self.doc.render_page_now(0, 1.0).expect("renders").raster;
        let (cx, cy) = (x, height - y);
        let span = |c: f64| (c - radius).max(0.0) as u32..(c + radius) as u32;
        span(cy).any(|py| {
            span(cx).any(|px| {
                let at = ((py * raster.width() + px) * 4) as usize;
                raster.rgba()[at] < 160
            })
        })
    }
}

#[test]
fn add_text_places_a_borderless_typewriter_box_at_the_click() {
    let mut fixture = Fixture::seeded();
    let mut tool = FillTextTool::new();
    assert!(tool.takes_text(), "the shell opens a field to type in");
    fixture.gesture(&mut tool, (300.0, 400.0), (300.0, 400.0));
    let placed = fixture.annotations();
    let text = placed.last().expect("placed");
    assert_eq!(text.subtype, Some(Subtype::FreeText));
    assert_eq!(text.subject.as_deref(), Some("Fill & Sign Text"));
    assert_eq!(text.border_width, 0.0);
    assert!(text.rect.x0 == 300.0 && text.rect.y0 < 400.0 && text.rect.y1 > 400.0);
    assert!(fixture.doc.undo().expect("undoes"), "one undo step");
    tool.on_deactivate(&mut fixture.ctx());
}

#[test]
fn each_mark_draws_at_the_click() {
    for symbol in Symbol::ALL {
        let mut fixture = Fixture::seeded();
        let mut tool = SymbolTool::new(symbol);
        fixture.gesture(&mut tool, (400.0, 300.0), (400.5, 300.5));
        let placed = fixture.annotations();
        let mark = placed.last().expect("placed");
        assert_eq!(mark.subtype, Some(Subtype::Stamp), "{symbol:?}");
        assert_eq!(
            (mark.rect.x0, mark.rect.y0, mark.rect.x1, mark.rect.y1),
            (394.0, 294.0, 406.0, 306.0),
            "{symbol:?}: centred on the press"
        );
        assert!(fixture.inked_near((400.0, 300.0), 5.0), "{symbol:?} draws");
        assert!(!fixture.inked_near((450.0, 300.0), 5.0));
        tool.on_deactivate(&mut fixture.ctx());
    }
}

#[test]
fn circle_and_line_are_dragged_or_clicked_to_their_usual_size() {
    let mut fixture = Fixture::seeded();
    let mut circle = ShapeTool::circle();
    fixture.gesture(&mut circle, (100.0, 100.0), (200.0, 150.0));
    let mut line = ShapeTool::line();
    fixture.gesture(&mut line, (300.0, 100.0), (300.0, 100.0));
    let placed = fixture.annotations();
    let (drawn_circle, clicked_line) = (&placed[placed.len() - 2], &placed[placed.len() - 1]);
    assert_eq!(drawn_circle.subtype, Some(Subtype::Circle));
    assert_eq!((drawn_circle.rect.x0, drawn_circle.rect.y1), (100.0, 150.0));
    assert_eq!(clicked_line.subtype, Some(Subtype::Line));
    assert!(
        fixture.inked_near((330.0, 100.0), 3.0),
        "a line from the click"
    );
    assert!(
        fixture.inked_near((100.0, 125.0), 3.0),
        "the circle's left edge"
    );

    // The drag is previewed while it is under way.
    let mut preview = ShapeTool::circle();
    let mut ctx = fixture.ctx();
    preview.on_pointer_down(&mut ctx, input(10.0, 10.0));
    assert!(preview.overlays(ctx.doc).is_empty(), "nothing yet");
    preview.on_pointer_move(&mut ctx, input(50.0, 40.0));
    assert!(matches!(
        preview.overlays(ctx.doc)[..],
        [Overlay::Ellipse { .. }]
    ));
    preview.on_cancel(&mut ctx);
    assert!(preview.overlays(ctx.doc).is_empty());
    let mut line_preview = ShapeTool::line();
    line_preview.on_pointer_down(&mut ctx, input(10.0, 10.0));
    line_preview.on_pointer_move(&mut ctx, input(50.0, 40.0));
    assert!(matches!(
        line_preview.overlays(ctx.doc)[..],
        [Overlay::Line { .. }]
    ));
    line_preview.on_deactivate(&mut ctx);
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fill-sign-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn the_library_keeps_a_signature_and_initials_until_forgotten() {
    let dir = temp_dir("library");
    let library = SignatureLibrary::in_data_dir(&dir);
    assert_eq!(library.get(SignatureKind::Signature), None);
    library
        .save(SignatureKind::Signature, &typed("Ana Pop").expect("typed"))
        .expect("saves");
    library
        .save(
            SignatureKind::Initials,
            &drawn(&[vec![(0.0, 0.0), (10.0, 10.0)]]).expect("drawn"),
        )
        .expect("saves");
    assert!(library.get(SignatureKind::Signature).is_some());
    assert!(
        library
            .save(SignatureKind::Signature, b"not a pdf")
            .is_err(),
        "refused"
    );
    assert!(
        library.get(SignatureKind::Signature).is_some(),
        "and the old one kept"
    );
    library.clear(SignatureKind::Initials).expect("forgets");
    library
        .clear(SignatureKind::Initials)
        .expect("forgetting twice is fine");
    assert_eq!(library.get(SignatureKind::Initials), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sign_places_the_chosen_saved_page_scaled_to_fit() {
    let dir = temp_dir("sign");
    let library = SignatureLibrary::in_data_dir(&dir);
    let mut tool = SignTool::new();
    assert!(tool.choices().is_empty(), "no library yet");
    tool.configure(&ToolEnvironment {
        data_dir: Some(dir.clone()),
        ..ToolEnvironment::default()
    });
    assert!(tool.choices().is_empty(), "nothing saved yet");

    let mut fixture = Fixture::seeded();
    let before = fixture.annotations().len();
    fixture.gesture(&mut tool, (300.0, 300.0), (300.0, 300.0));
    assert_eq!(before, fixture.annotations().len(), "nothing to place");

    library
        .save(
            SignatureKind::Signature,
            &typed("Ana Maria Popescu-Ionescu").expect("typed"),
        )
        .expect("saves");
    library
        .save(SignatureKind::Initials, &typed("AP").expect("typed"))
        .expect("saves");
    let ids: Vec<_> = tool.choices().into_iter().map(|choice| choice.id).collect();
    assert_eq!(ids, ["signature", "initials"]);
    assert!(!tool.choose("nobody"));
    assert_eq!(tool.chosen().as_deref(), Some("signature"));

    fixture.gesture(&mut tool, (300.0, 300.0), (300.0, 300.0));
    let placed = fixture.annotations();
    let signature = placed.last().expect("placed");
    assert_eq!(signature.subtype, Some(Subtype::Stamp));
    let width = signature.rect.x1 - signature.rect.x0;
    assert!(
        (width - 150.0).abs() < 0.01,
        "a long name is scaled down: {width}"
    );
    assert!(
        fixture.inked_near((300.0, 300.0), 20.0),
        "the name is drawn"
    );

    assert!(tool.choose("initials"));
    fixture.gesture(&mut tool, (100.0, 200.0), (100.0, 200.0));
    let placed = fixture.annotations();
    let initials = placed.last().expect("placed");
    assert!(initials.rect.x1 - initials.rect.x0 <= 60.0 + 1e-9);
    tool.on_cancel(&mut fixture.ctx());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_plugin_registers_every_tool_in_one_group() {
    let mut registry = PluginRegistry::new();
    FillSignToolsPlugin.register(&mut registry);
    let tools: Vec<_> = registry.tools().map(|tool| tool.id()).collect();
    assert_eq!(
        tools,
        [
            "fill-sign.text",
            "fill-sign.check",
            "fill-sign.cross",
            "fill-sign.dot",
            "fill-sign.circle",
            "fill-sign.line",
            "fill-sign.sign"
        ]
    );
    for tool in registry.tools() {
        assert_eq!(tool.group(), "fill-sign");
        assert!(!tool.name().is_empty() && !tool.icon().is_empty());
        assert!(tool.hint().is_some());
        assert!(tool
            .capabilities()
            .iter()
            .all(|capability| capability.edits_document()));
    }
    assert!(registry
        .tools()
        .any(|tool| tool.capabilities() == [ToolCapability::AddSignature]));
    assert_eq!(FillSignToolsPlugin.name(), "Fill & Sign");
    assert_eq!(FillSignToolsPlugin.id(), "onionskin.tools-fill-sign");
}

#[test]
fn a_protected_document_is_left_as_it_was() {
    let doc = Document::open_path(&encrypted_fixture("r4-aes-128.pdf")).expect("opens");
    let mut fixture = Fixture::open(doc);
    let before = fixture.annotations().len();
    let mut tool = SymbolTool::new(Symbol::Check);
    fixture.gesture(&mut tool, (50.0, 50.0), (50.0, 50.0));
    assert_eq!(fixture.annotations().len(), before);
}
