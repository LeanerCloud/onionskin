//! Links through the plugin: the Link tool's requests, the functions the
//! Link dialog and the Edit menu call, and Create Links from URLs on real
//! text.

use onionskin_core::links::{Highlight, LineStyle, LinkLook, LinkTarget};
use onionskin_core::{
    Document, FitMode, LinkRequest, Modifiers, PagePoint, PageRect, ViewSize, Viewport,
};
use onionskin_plugin_api::{
    CommandError, Overlay, PluginManifest, PluginRegistry, PointerInput, ToolCtx, ToolPlugin,
};
use onionskin_tools_edit::links::{
    create_link, create_links_from_urls, delete_link, edit_link, find_link, remove_web_links,
};
use onionskin_tools_edit::{EditToolsPlugin, LinkTool};

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// Two Letter pages; the first says `text` in Helvetica at (72, 700).
fn document(text: &str) -> Document {
    let content = format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET");
    Document::open_bytes(pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 6 0 R >> >> >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ]))
    .expect("opens")
}

const RECT: [f64; 4] = [100.0, 100.0, 200.0, 150.0];

#[test]
fn a_link_is_made_changed_and_deleted_each_as_one_step() {
    let mut doc = document("hello");
    let link =
        create_link(&mut doc, 0, RECT, &LinkTarget::Page(1), LinkLook::default()).expect("creates");
    let found = find_link(&mut doc, link).expect("reads").expect("there");
    assert_eq!(found.target, LinkTarget::Page(1));

    let look = LinkLook {
        visible: true,
        width: 3.0,
        color: [0.0, 1.0, 0.0],
        style: LineStyle::Underline,
        highlight: Highlight::None,
    };
    let web = LinkTarget::Web("https://example.com".into());
    edit_link(&mut doc, link, &web, look).expect("edits");
    let found = find_link(&mut doc, link).expect("reads").expect("there");
    assert_eq!((found.target, found.look), (web, look));

    delete_link(&mut doc, 0, link).expect("deletes");
    assert_eq!(find_link(&mut doc, link).expect("reads"), None);
    assert!(doc.undo().expect("undoes the delete"));
    assert!(doc.undo().expect("undoes the edit"));
    assert!(doc.undo().expect("undoes the create"));
    assert!(doc.links().expect("reads").is_empty());
}

#[test]
fn a_refused_link_says_which_edit() {
    let mut doc = document("hello");
    let refused = create_link(&mut doc, 9, RECT, &LinkTarget::Page(0), LinkLook::default());
    assert!(matches!(
        refused,
        Err(CommandError::Edit {
            label: "Create Link",
            ..
        })
    ));
    let not_a_link = onionskin_core::ObjRef::new(3, 0);
    assert!(matches!(
        edit_link(
            &mut doc,
            not_a_link,
            &LinkTarget::Page(0),
            LinkLook::default()
        ),
        Err(CommandError::Edit {
            label: "Edit Link",
            ..
        })
    ));
    assert!(matches!(
        delete_link(&mut doc, 5, not_a_link),
        Err(CommandError::Edit {
            label: "Delete Link",
            ..
        })
    ));
}

#[test]
fn web_addresses_in_the_text_become_links_once() {
    let mut doc = document("Read https://example.com/guide or www.example.org now");
    assert_eq!(
        create_links_from_urls(&mut doc, &[0, 1]).expect("creates"),
        2
    );
    let links = doc.links().expect("reads");
    let targets: Vec<_> = links.iter().map(|link| link.target.clone()).collect();
    assert_eq!(
        targets,
        [
            LinkTarget::Web("https://example.com/guide".into()),
            LinkTarget::Web("http://www.example.org".into()),
        ]
    );
    // The first address starts after "Read " (about 29 points of Helvetica
    // 12) and sits on the line at 700.
    let [x0, y0, x1, y1] = links[0].rect;
    assert!(x0 > 72.0 + 25.0 && x0 < 72.0 + 35.0, "{x0}");
    assert!(x1 > x0 + 100.0, "{x1}");
    assert!(y0 < 702.0 && y1 > 705.0, "{y0} {y1}");
    assert!(!links[0].look.visible);

    assert_eq!(
        create_links_from_urls(&mut doc, &[0]).expect("runs"),
        0,
        "already linked"
    );
    assert_eq!(remove_web_links(&mut doc).expect("removes"), 2);
    assert!(doc.links().expect("reads").is_empty());
    assert!(matches!(
        create_links_from_urls(&mut doc, &[4]),
        Err(CommandError::Page { page: 4, .. })
    ));
}

struct Fixture {
    doc: Document,
    viewport: Viewport,
    tool: LinkTool,
}

impl Fixture {
    fn new() -> Self {
        let mut doc = document("hello");
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
        Self {
            doc,
            viewport,
            tool: LinkTool::new(),
        }
    }

    fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }

    fn gesture(&mut self, from: (f64, f64), to: (f64, f64)) {
        let at = |(x, y)| PointerInput {
            at: PagePoint { page: 0, x, y },
            pressure: 1.0,
            modifiers: Modifiers::default(),
            clicks: 1,
        };
        let mut tool = std::mem::take(&mut self.tool);
        tool.on_pointer_down(&mut self.ctx(), at(from));
        tool.on_pointer_move(&mut self.ctx(), at(to));
        tool.on_pointer_up(&mut self.ctx(), at(to));
        self.tool = tool;
    }
}

#[test]
fn the_link_tool_asks_for_a_new_link_or_to_change_one() {
    let mut fixture = Fixture::new();
    fixture.gesture((100.0, 100.0), (200.0, 150.0));
    assert_eq!(
        fixture.doc.take_link_request(),
        Some(LinkRequest::Create(PageRect {
            page: 0,
            x0: 100.0,
            y0: 100.0,
            x1: 200.0,
            y1: 150.0,
        }))
    );

    let link = create_link(
        &mut fixture.doc,
        0,
        RECT,
        &LinkTarget::Page(1),
        LinkLook::default(),
    )
    .expect("creates");
    fixture.gesture((150.0, 120.0), (150.0, 120.0));
    assert_eq!(
        fixture.doc.take_link_request(),
        Some(LinkRequest::Edit(link))
    );
    // Once it has looked again, the tool outlines the link it found.
    assert!(fixture
        .tool
        .overlays(&fixture.doc)
        .iter()
        .any(|overlay| matches!(overlay, Overlay::Rect(rect) if rect.x0 == 100.0)));

    fixture.gesture((400.0, 400.0), (400.0, 400.0));
    assert_eq!(fixture.doc.take_link_request(), None, "a click on nothing");
    fixture.tool.on_deactivate(&mut ToolCtx {
        doc: &mut fixture.doc,
        viewport: &mut fixture.viewport,
    });
}

#[test]
fn the_link_tool_is_registered_and_edits_pages() {
    let mut registry = PluginRegistry::new();
    EditToolsPlugin.register(&mut registry);
    let tool = registry
        .tools()
        .find(|tool| tool.id() == "link")
        .expect("registered");
    assert_eq!(tool.name(), "Link");
    assert_eq!(tool.icon(), "link");
    assert!(tool.hint().expect("a hint").contains("click a link"));
    assert_eq!(
        tool.capabilities(),
        [onionskin_plugin_api::ToolCapability::Link]
    );
}
