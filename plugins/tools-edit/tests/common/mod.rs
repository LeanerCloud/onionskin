//! What the tool suites share: a file from numbered objects, and a page
//! in a viewport that a tool can be pressed and dragged on.

#![allow(dead_code)]

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};

/// A PDF of `objects`, numbered from 1, the first the catalog.
pub fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
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

/// A content stream object holding `text`.
pub fn content(text: &str) -> Vec<u8> {
    format!("<< /Length {} >>\nstream\n{text}\nendstream", text.len()).into_bytes()
}

/// A document's first page, fitted in an 800 by 600 view.
pub struct Page {
    pub doc: Document,
    pub viewport: Viewport,
}

impl Page {
    pub fn new(mut doc: Document) -> Self {
        let mut viewport = Viewport::new(
            1,
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
            12.0,
        )
        .expect("viewport");
        viewport
            .measure_page(doc.page_geometry(0).expect("measures").clone())
            .expect("measurable");
        viewport.fit(FitMode::Page).expect("fits");
        Page { doc, viewport }
    }

    pub fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }

    /// Press at `from`, move to `to` and let go there, on page 1.
    pub fn drag(&mut self, tool: &mut dyn ToolPlugin, from: (f64, f64), to: (f64, f64)) {
        tool.on_pointer_down(&mut self.ctx(), at(from));
        tool.on_pointer_move(&mut self.ctx(), at(to));
        tool.on_pointer_up(&mut self.ctx(), at(to));
    }
}

/// A single press at `(x, y)` on page 1.
pub fn at((x, y): (f64, f64)) -> PointerInput {
    PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    }
}
