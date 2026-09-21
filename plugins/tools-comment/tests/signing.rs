//! Every comment tool signs what it places with the author the shell hands
//! it, the name the user chose in Commenting preferences, and signs nothing
//! when there is no name.

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_plugin_api::{CommentDefault, PointerInput, ToolCtx, ToolEnvironment, ToolPlugin};
use onionskin_tools_comment::{FreeTextTool, InkTool, NoteTool, ShapeTool};

fn document() -> (Document, Viewport) {
    let mut doc = Document::open_bytes(blank_page()).expect("document opens");
    let mut viewport = Viewport::new(
        doc.page_count(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
        12.0,
    )
    .expect("viewport is valid");
    let geometry = doc.page_geometry(0).expect("page measures").clone();
    viewport.measure_page(geometry).expect("page is measurable");
    viewport.fit(FitMode::Page).expect("the page fits");
    (doc, viewport)
}

/// Drag from `from` to `to` with `tool`, configured to sign as `author`,
/// and return the author of every annotation now in the document.
fn authors_after(
    tool: &mut dyn ToolPlugin,
    author: Option<&str>,
    from: (f64, f64),
    to: (f64, f64),
) -> Vec<Option<String>> {
    tool.configure(&ToolEnvironment {
        author: author.map(str::to_owned),
        ..ToolEnvironment::default()
    });
    let (mut doc, mut viewport) = document();
    let input = |(x, y): (f64, f64)| PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    };
    let mut ctx = ToolCtx {
        doc: &mut doc,
        viewport: &mut viewport,
    };
    tool.on_pointer_down(&mut ctx, input(from));
    tool.on_pointer_move(&mut ctx, input(to));
    tool.on_pointer_up(&mut ctx, input(to));
    doc.annotations()
        .expect("reads")
        .into_iter()
        .map(|annotation| annotation.author)
        .collect()
}

/// A tool under test: its name, the tool, and the drag that places one.
type Case = (&'static str, Box<dyn ToolPlugin>, (f64, f64), (f64, f64));

fn tools() -> Vec<Case> {
    vec![
        (
            "sticky note",
            Box::new(NoteTool::new()),
            (100.0, 700.0),
            (100.0, 700.0),
        ),
        (
            "text box",
            Box::new(FreeTextTool::text_box()),
            (100.0, 700.0),
            (300.0, 600.0),
        ),
        (
            "rectangle",
            Box::new(ShapeTool::rectangle()),
            (100.0, 700.0),
            (300.0, 600.0),
        ),
        (
            "ink",
            Box::new(InkTool::new()),
            (100.0, 700.0),
            (300.0, 600.0),
        ),
    ]
}

#[test]
fn each_tool_signs_its_comment_with_the_configured_author() {
    for (name, mut tool, from, to) in tools() {
        let authors = authors_after(tool.as_mut(), Some("Ana Pop"), from, to);
        assert_eq!(authors, [Some("Ana Pop".to_owned())], "{name}");
    }
}

/// "Make Current Properties Default": a kind with a default takes its colour
/// and opacity; a kind without one keeps the tool's own look.
#[test]
fn a_default_look_applies_to_its_own_kind_only() {
    let environment = ToolEnvironment {
        comment_defaults: [(
            "Square".to_owned(),
            CommentDefault {
                color: Some([255, 0, 0]),
                opacity_percent: 50,
            },
        )]
        .into(),
        ..ToolEnvironment::default()
    };
    let place = |tool: &mut dyn ToolPlugin| {
        tool.configure(&environment);
        let (mut doc, mut viewport) = document();
        let input = |(x, y): (f64, f64)| PointerInput {
            at: PagePoint { page: 0, x, y },
            pressure: 1.0,
            modifiers: Modifiers::default(),
            clicks: 1,
        };
        let mut ctx = ToolCtx {
            doc: &mut doc,
            viewport: &mut viewport,
        };
        tool.on_pointer_down(&mut ctx, input((100.0, 700.0)));
        tool.on_pointer_move(&mut ctx, input((300.0, 600.0)));
        tool.on_pointer_up(&mut ctx, input((300.0, 600.0)));
        doc.annotations().expect("reads").remove(0)
    };
    let rectangle = place(&mut ShapeTool::rectangle());
    assert_eq!(
        rectangle.color,
        Some(onionskin_core::Color::new(1.0, 0.0, 0.0))
    );
    assert_eq!(rectangle.opacity, Some(0.5));
    let oval = place(&mut ShapeTool::oval());
    assert_ne!(oval.color, Some(onionskin_core::Color::new(1.0, 0.0, 0.0)));
    assert_eq!(oval.opacity, None);
}

#[test]
fn with_no_author_configured_a_comment_is_unsigned() {
    for (name, mut tool, from, to) in tools() {
        let authors = authors_after(tool.as_mut(), None, from, to);
        assert_eq!(authors, [None], "{name}");
    }
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
