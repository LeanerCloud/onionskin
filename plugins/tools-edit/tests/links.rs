//! Links through the plugin: the Link tool's requests, the functions the
//! Link dialog and the Edit menu call, and Create Links from URLs on real
//! text.

mod common;

use common::pdf;
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
use std::ops::Range;

/// Two Letter pages; the first says `text` in Helvetica at (72, 700).
fn document(text: &str) -> Document {
    let content = format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET");
    document_with_content(&content)
}

fn document_with_content(content: &str) -> Document {
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

fn bounds(page: &onionskin_core::PageText, runs: Range<usize>) -> [f64; 4] {
    let mut rect = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for run in &page.runs[runs] {
        for glyph in &run.glyphs {
            for &(x, y) in &glyph.quad.corners {
                rect[0] = rect[0].min(x);
                rect[1] = rect[1].min(y);
                rect[2] = rect[2].max(x);
                rect[3] = rect[3].max(y);
            }
        }
    }
    rect
}

fn assert_rect_rounded(actual: [f64; 4], expected: [f64; 4]) {
    // Link rectangles are serialized by the PDF writer to six decimals.
    for (index, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= 1e-6,
            "rect coordinate {index}: actual {actual} expected {expected}"
        );
    }
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

#[test]
fn a_replacement_url_spanning_operators_links_all_members_only_once() {
    let mut doc = document_with_content(
        "BT /F1 12 Tf 72 700 Td (before ) Tj \
         /Span << /ActualText (https://example.com) >> BDC (AB) Tj (CD) Tj EMC \
         ( after) Tj ET",
    );
    let page = doc.page_text(0).expect("extracts").clone();
    assert_eq!(
        page.flatten().text,
        "before https://example.com after",
        "the semantic replacement is the URL"
    );
    assert_eq!(
        page.runs.len(),
        4,
        "ordinary neighbors and both members remain"
    );
    assert_eq!(page.runs[1].decoded_text, "AB");
    assert_eq!(page.runs[2].decoded_text, "CD");
    let expected = bounds(&page, 1..3);
    let neighbor = page.runs[0].glyphs[0].quad.corners;
    let neighbor_center = (
        neighbor.iter().map(|(x, _)| *x).sum::<f64>() / 4.0,
        neighbor.iter().map(|(_, y)| *y).sum::<f64>() / 4.0,
    );

    assert_eq!(create_links_from_urls(&mut doc, &[0]).expect("creates"), 1);
    let links = doc.links().expect("reads");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].page, 0);
    assert_eq!(
        links[0].target,
        LinkTarget::Web("https://example.com".into())
    );
    assert_rect_rounded(links[0].rect, expected);
    assert!(
        !links[0].contains(neighbor_center),
        "ordinary neighbor is excluded"
    );
}

#[test]
fn repeated_replacement_urls_get_separate_links_and_bounds() {
    let mut doc = document_with_content(
        "BT /F1 12 Tf 72 700 Td (left ) Tj \
         /Span << /ActualText (https://example.com) >> BDC (AB) Tj (CD) Tj EMC \
         ( middle ) Tj /Span << /ActualText (https://example.com) >> BDC (EF) Tj (GH) Tj EMC \
         ( right) Tj ET",
    );
    let page = doc.page_text(0).expect("extracts").clone();
    assert_eq!(
        page.flatten().text,
        "left https://example.com middle https://example.com right"
    );
    assert_eq!(create_links_from_urls(&mut doc, &[0]).expect("creates"), 2);
    let links = doc.links().expect("reads");
    assert_eq!(links.len(), 2);
    assert_eq!(
        links[0].target,
        LinkTarget::Web("https://example.com".into())
    );
    assert_eq!(
        links[1].target,
        LinkTarget::Web("https://example.com".into())
    );
    assert_eq!(links[0].page, 0);
    assert_eq!(links[1].page, 0);
    assert_rect_rounded(links[0].rect, bounds(&page, 1..3));
    assert_rect_rounded(links[1].rect, bounds(&page, 4..6));
    assert_ne!(
        links[0].rect, links[1].rect,
        "occurrences keep separate geometry"
    );
}

#[test]
fn a_decoded_url_replaced_by_non_url_does_not_get_a_link() {
    let mut doc = document_with_content(
        "BT /F1 12 Tf 72 700 Td /Span << /ActualText (not a URL) >> BDC \
         (https://example.com) Tj EMC ET",
    );
    assert_eq!(
        doc.page_text(0).expect("extracts").flatten().text,
        "not a URL"
    );
    assert_eq!(create_links_from_urls(&mut doc, &[0]).expect("scans"), 0);
    assert!(doc.links().expect("reads").is_empty());
}

#[test]
fn created_web_links_survive_undo_redo_and_saved_readback() {
    let mut doc = document_with_content(
        "BT /F1 12 Tf 72 700 Td (before ) Tj \
         /Span << /ActualText (https://example.com) >> BDC (AB) Tj (CD) Tj EMC \
         ( after) Tj ET",
    );
    let page = doc.page_text(0).expect("extracts").clone();
    let expected_rect = bounds(&page, 1..3);
    let expected_target = LinkTarget::Web("https://example.com".into());
    assert_eq!(create_links_from_urls(&mut doc, &[0]).expect("creates"), 1);
    let links = doc.links().expect("reads");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, expected_target);
    assert_eq!(links[0].page, 0);
    assert_rect_rounded(links[0].rect, expected_rect);
    assert!(doc.undo().expect("undoes link creation"));
    assert!(doc.links().expect("reads").is_empty());
    assert!(doc.redo().expect("redoes link creation"));
    let links = doc.links().expect("reads after redo");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, expected_target);
    assert_eq!(links[0].page, 0);
    assert_rect_rounded(links[0].rect, expected_rect);
    let saved = doc
        .preview_bytes(onionskin_core::AnnotationFilter::DocumentAndMarkups)
        .expect("serializes")
        .as_ref()
        .clone();
    let mut reopened = Document::open_bytes(saved).expect("reopens");
    let links = reopened.links().expect("reads saved links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, expected_target);
    assert_eq!(links[0].page, 0);
    assert_rect_rounded(links[0].rect, expected_rect);
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
