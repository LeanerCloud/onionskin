//! The five tools driven by a text selection.
//!
//! Highlight, Underline and Strikethrough each write one annotation of their
//! own subtype over the selected glyphs. Insert Text writes a `/Caret` at the
//! selection's start rather than over it, because an insertion point is a
//! position and not a span. Replace Text writes a `/StrikeOut` over the
//! selection **and** a reply note linked to it by `/IRT`, which is how Acrobat
//! models a replacement and why it is one transaction rather than two
//! annotations that happen to overlap.
//!
//! All five share one gesture implementation, because they differ only in what
//! they write. A tool per file would be five copies of the same drag handling
//! with five chances for one of them to drift.
//!
//! **The quads come from `core`'s selection and are merged per line**, never
//! collapsed to a bounding rectangle. See `quads.rs` for why that distinction
//! is the one worth testing.

use onionskin_core::textselect::select_between;
use onionskin_core::{
    add_annotation, Annotation, Color, Document, PagePoint, PageQuad, Quad, Rect, Subtype,
    TextSelection, Viewport,
};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::place::{now, page_object};
use crate::quads::merge;

/// Below this, a drag is a click: it selects nothing and writes nothing.
/// Expressed in view pixels, so it means the same thing at every zoom.
const MIN_DRAG_PIXELS: f32 = 3.0;

/// What a markup tool writes when its gesture ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Writes {
    /// One annotation of this subtype over the selection's quads.
    Span(Subtype),
    /// A caret at the start of the selection, which marks a position.
    Caret,
    /// A strike-out over the selection plus a reply note linked to it.
    Replacement,
}

/// One text-markup tool.
pub struct MarkupTool {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    shortcut: Option<&'static str>,
    writes: Writes,
    color: Color,
    /// The fixed end of the drag. Kept past the drag so shift extends from it,
    /// which is what the select tool does and what a user expects.
    anchor: Option<PagePoint>,
    dragging: bool,
    /// The selection as it stands, drawn as an overlay until it is committed.
    pending: Option<TextSelection>,
    /// Whether the last gesture in this chain wrote a markup that a
    /// shift-extend should replace rather than overlap.
    committed: bool,
}

impl MarkupTool {
    pub fn highlight() -> Self {
        Self::new(
            "highlight",
            "Highlight",
            "highlight",
            Some("u"),
            Writes::Span(Subtype::Highlight),
            Color::new(1.0, 0.92, 0.23),
        )
    }

    pub fn underline() -> Self {
        Self::new(
            "underline",
            "Underline",
            "underline",
            None,
            Writes::Span(Subtype::Underline),
            Color::new(0.11, 0.51, 0.93),
        )
    }

    pub fn strikethrough() -> Self {
        Self::new(
            "strikethrough",
            "Strikethrough",
            "strikethrough",
            None,
            Writes::Span(Subtype::StrikeOut),
            Color::new(0.85, 0.16, 0.16),
        )
    }

    pub fn insert_text() -> Self {
        Self::new(
            "insert-text",
            "Insert Text At Cursor",
            "insert-text",
            None,
            Writes::Caret,
            Color::new(0.11, 0.51, 0.93),
        )
    }

    pub fn replace_text() -> Self {
        Self::new(
            "replace-text",
            "Replace Text",
            "replace-text",
            None,
            Writes::Replacement,
            Color::new(0.85, 0.16, 0.16),
        )
    }

    fn new(
        id: &'static str,
        name: &'static str,
        icon: &'static str,
        shortcut: Option<&'static str>,
        writes: Writes,
        color: Color,
    ) -> Self {
        MarkupTool {
            id,
            name,
            icon,
            shortcut,
            writes,
            color,
            anchor: None,
            dragging: false,
            pending: None,
            committed: false,
        }
    }

    /// Shift extends the markup rather than laying a second one over it.
    ///
    /// Without this, shift-dragging past the end of a highlight leaves two
    /// overlapping annotations: a doubly-dark band where they overlap and two
    /// rows in the comments pane for one thing the user did once. The previous
    /// markup is taken back first, and only if the top of the undo stack is
    /// still this tool's own entry, so a shift-extend after some other edit
    /// extends nothing and takes nothing back.
    fn retract_previous(&mut self, ctx: &mut ToolCtx) {
        if !self.committed {
            return;
        }
        self.committed = false;
        if ctx.doc.edit().history().undo_label() != Some(self.name) {
            return;
        }
        let (edit, base) = ctx.doc.edit_mut();
        let _ = edit.undo(base);
    }

    fn extend_to(&mut self, ctx: &mut ToolCtx, at: PagePoint) {
        let Some(anchor) = self.anchor else {
            return;
        };
        if !is_drag(anchor, at, ctx.viewport) {
            self.pending = None;
            return;
        }
        let Ok(page) = ctx.doc.page_text(anchor.page) else {
            return;
        };
        // The selection is taken by value: holding a borrow of `PageText`
        // across the commit would keep the document borrowed while the edit
        // needs it mutably.
        self.pending = select_between(page, anchor, at);
    }

    /// Write the annotation, or nothing when the gesture selected nothing.
    fn commit(&mut self, ctx: &mut ToolCtx) {
        let Some(selection) = self.pending.take() else {
            return;
        };
        let merged = merge(&selection.quads);
        if merged.is_empty() {
            return;
        }
        let Some(page) = page_object(ctx.doc, selection.page) else {
            return;
        };
        let quads: Vec<Quad> = merged.iter().map(to_quad).collect();

        let writes = self.writes;
        let color = self.color;
        let label = self.name;
        let text = selection.text.clone();
        self.committed = true;
        let _ = ctx.doc.edit_annotations(label, |tx, structure| {
            match writes {
                Writes::Span(subtype) => {
                    let mut annotation = span(subtype, &quads, color);
                    annotation.contents = None;
                    add_annotation(tx, structure, page, &annotation, now())?;
                }
                Writes::Caret => {
                    let mut annotation = Annotation::new(Subtype::Text, caret_rect(&quads));
                    annotation.icon = Some("Comment".into());
                    annotation.color = Some(color);
                    add_annotation(tx, structure, page, &annotation, now())?;
                }
                Writes::Replacement => {
                    // One transaction: the strike-out and the note that
                    // replaces it are one edit, and undoing half of a
                    // replacement is not a state anyone wants.
                    let struck = add_annotation(
                        tx,
                        structure,
                        page,
                        &span(Subtype::StrikeOut, &quads, color),
                        now(),
                    )?;
                    let mut reply = Annotation::new(Subtype::Text, caret_rect(&quads));
                    reply.icon = Some("Comment".into());
                    reply.color = Some(color);
                    reply.in_reply_to = Some(struck);
                    reply.subject = Some("Replacement".into());
                    reply.contents = Some(text);
                    add_annotation(tx, structure, page, &reply, now())?;
                }
            }
            Ok(())
        });
    }
}

fn span(subtype: Subtype, quads: &[Quad], color: Color) -> Annotation {
    let mut annotation =
        Annotation::markup(subtype, quads.to_vec()).expect("quads are non-empty here");
    annotation.color = Some(color);
    annotation
}

/// A caret sits at the leading edge of the first quad, a thin box rather than a
/// span, because an insertion point has no width.
fn caret_rect(quads: &[Quad]) -> Rect {
    let bounds = quads[0].bounds();
    let width = (bounds.height() * 0.6).max(4.0);
    Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y1)
}

fn to_quad(quad: &PageQuad) -> Quad {
    let [upper_left, upper_right, lower_left, lower_right] = quad.corners;
    Quad {
        upper_left,
        upper_right,
        lower_left,
        lower_right,
    }
}

fn is_drag(from: PagePoint, to: PagePoint, viewport: &Viewport) -> bool {
    let (Ok(Some(from)), Ok(Some(to))) =
        (viewport.view_point_for(from), viewport.view_point_for(to))
    else {
        return false;
    };
    (to.x - from.x).hypot(to.y - from.y) >= MIN_DRAG_PIXELS
}

impl ToolPlugin for MarkupTool {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn takes_text(&self) -> bool {
        matches!(self.writes, Writes::Caret | Writes::Replacement)
    }

    fn hint(&self) -> Option<&'static str> {
        Some(match self.id {
            "insert-text" => "Click in the text where words should go, then type them in the pop-up. Enter finishes.",
            "replace-text" => "Drag across the text to replace, then type the replacement in the pop-up. Enter finishes.",
            _ => "Drag across text on the page. Works only on real text, not on scanned images.",
        })
    }

    fn icon(&self) -> &'static str {
        self.icon
    }

    fn shortcut(&self) -> Option<&'static str> {
        self.shortcut
    }

    /// All five share one rail slot, the way Acrobat groups a toolset's
    /// variants: the slot shows whichever was last used.
    fn group(&self) -> &'static str {
        "markup"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Highlight, ToolCapability::Comment]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.dragging = true;
        let extends = input.modifiers.shift
            && self
                .anchor
                .is_some_and(|anchor| anchor.page == input.at.page);
        if extends {
            self.retract_previous(ctx);
            self.extend_to(ctx, input.at);
            return;
        }
        self.committed = false;
        self.anchor = Some(input.at);
        self.pending = None;
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.dragging {
            self.extend_to(ctx, input.at);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if !self.dragging {
            return;
        }
        self.extend_to(ctx, input.at);
        self.dragging = false;
        self.commit(ctx);
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        self.commit(ctx);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.dragging = false;
        self.anchor = None;
        self.pending = None;
        self.committed = false;
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    /// The selection is drawn while the drag is live, so the user sees what
    /// the markup will cover before it is written.
    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        match &self.pending {
            Some(selection) if self.dragging => vec![Overlay::Quads(merge(&selection.quads))],
            _ => Vec::new(),
        }
    }
}
