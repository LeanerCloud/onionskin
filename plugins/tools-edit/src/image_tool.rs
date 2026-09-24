//! The image tools of Edit PDF.
//!
//! **Edit Image:** click an image to select it. Drag it to move it, or
//! drag one of its corners to resize it about the opposite one, keeping
//! its proportions. Edit > Delete takes it away, and the Edit menu turns,
//! flips and replaces it and saves it as a file.
//!
//! **Add Image:** the shell asks for the image when the tool is chosen and
//! hands it over as a one-page PDF. A click places it at its own size,
//! hanging from the click; a drag places it fitted in the rectangle.

use std::path::PathBuf;

use onionskin_core::image_edit::transforms;
use onionskin_core::{Document, PagePoint, PageRect, Viewport};
use onionskin_plugin_api::marquee::{is_drag, Marquee};
use onionskin_plugin_api::{EditVerb, Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::images::{
    add_image, delete_selected, picture_size, select_image_at, selected, transform_selected,
};

/// View pixels from a corner that grab it.
const GRAB_PIXELS: f32 = 8.0;
/// The smallest a resize makes an image, as a share of its size.
const MIN_SCALE: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    Move {
        anchor: PagePoint,
        at: PagePoint,
    },
    /// A corner dragged, the one opposite it `fixed`.
    Resize {
        fixed: (f64, f64),
        grabbed: (f64, f64),
        anchor: PagePoint,
        at: PagePoint,
    },
}

impl Gesture {
    fn anchor(&self) -> PagePoint {
        match self {
            Gesture::Move { anchor, .. } | Gesture::Resize { anchor, .. } => *anchor,
        }
    }

    fn follow(&mut self, to: PagePoint) {
        match self {
            Gesture::Move { at, .. } | Gesture::Resize { at, .. } => *at = to,
        }
    }

    /// The page-space change the gesture makes so far.
    fn change(&self) -> onionskin_content::Matrix {
        match *self {
            Gesture::Move { anchor, at } => transforms::translate(at.x - anchor.x, at.y - anchor.y),
            Gesture::Resize {
                fixed, grabbed, at, ..
            } => {
                let ratio = |now: f64, then: f64, from: f64| {
                    let span = then - from;
                    if span.abs() < f64::EPSILON {
                        1.0
                    } else {
                        (now - from) / span
                    }
                };
                let scale = ratio(at.x, grabbed.0, fixed.0)
                    .max(ratio(at.y, grabbed.1, fixed.1))
                    .max(MIN_SCALE);
                transforms::scale_about(scale, scale, fixed)
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct EditImageTool {
    gesture: Option<Gesture>,
}

impl EditImageTool {
    pub fn new() -> Self {
        Self::default()
    }
}

/// The selected image's corner within grabbing distance of `at`, and the
/// one opposite it.
fn corner_at(
    doc: &Document,
    viewport: &Viewport,
    at: PagePoint,
) -> Option<((f64, f64), (f64, f64))> {
    let chosen = selected(doc).filter(|chosen| chosen.page == at.page)?;
    let [x0, y0, x1, y1] = chosen.placement.bounds();
    let corners = [
        ((x0, y0), (x1, y1)),
        ((x1, y0), (x0, y1)),
        ((x1, y1), (x0, y0)),
        ((x0, y1), (x1, y0)),
    ];
    let seen = viewport.view_point_for(at).ok().flatten()?;
    corners.into_iter().find(|((x, y), _)| {
        let corner = PagePoint {
            page: at.page,
            x: *x,
            y: *y,
        };
        viewport
            .view_point_for(corner)
            .ok()
            .flatten()
            .is_some_and(|view| (view.x - seen.x).hypot(view.y - seen.y) <= GRAB_PIXELS)
    })
}

fn quad(page: usize, corners: [(f64, f64); 4]) -> Overlay {
    Overlay::Polyline {
        points: corners
            .into_iter()
            .map(|(x, y)| PagePoint { page, x, y })
            .collect(),
        closed: true,
    }
}

impl ToolPlugin for EditImageTool {
    fn id(&self) -> &'static str {
        "edit-image"
    }

    fn name(&self) -> &'static str {
        "Edit Image"
    }

    fn icon(&self) -> &'static str {
        "edit-image"
    }

    fn group(&self) -> &'static str {
        "images"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Click an image to select it, drag it to move it, drag a corner to resize it. The Edit menu turns, flips, replaces and saves it.")
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::EditImages]
    }

    fn claims(&self, verb: EditVerb) -> bool {
        verb == EditVerb::Delete
    }

    fn edit(&mut self, ctx: &mut ToolCtx, verb: EditVerb, _pasted: Option<&str>) -> Option<String> {
        if verb == EditVerb::Delete && selected(ctx.doc).is_some() {
            let _ = delete_selected(ctx.doc);
        }
        None
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let at = input.at;
        self.gesture = if let Some((grabbed, fixed)) = corner_at(ctx.doc, ctx.viewport, at) {
            Some(Gesture::Resize {
                fixed,
                grabbed,
                anchor: at,
                at,
            })
        } else {
            let on_selection = selected(ctx.doc).is_some_and(|chosen| {
                chosen.page == at.page && chosen.placement.contains(at.x, at.y)
            });
            (on_selection || select_image_at(ctx.doc, at.page, (at.x, at.y)))
                .then_some(Gesture::Move { anchor: at, at })
        };
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        if let Some(gesture) = self.gesture.as_mut() {
            if input.at.page == gesture.anchor().page {
                gesture.follow(input.at);
            }
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(mut gesture) = self.gesture.take() else {
            return;
        };
        if input.at.page == gesture.anchor().page {
            gesture.follow(input.at);
        }
        let to = match gesture {
            Gesture::Move { at, .. } | Gesture::Resize { at, .. } => at,
        };
        if !is_drag(gesture.anchor(), to, ctx.viewport) {
            return;
        }
        let label = match gesture {
            Gesture::Move { .. } => "Move Image",
            Gesture::Resize { .. } => "Resize Image",
        };
        let _ = transform_selected(ctx.doc, label, gesture.change());
    }

    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        if self.gesture.take().is_none() && selected(ctx.doc).is_some() {
            ctx.doc.selection_mut().clear();
        }
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.gesture = None;
        if selected(ctx.doc).is_some() {
            ctx.doc.selection_mut().clear();
        }
    }

    fn overlays(&self, doc: &Document) -> Vec<Overlay> {
        let Some(chosen) = selected(doc) else {
            return Vec::new();
        };
        let corners = chosen.placement.corners();
        let mut shown = vec![quad(chosen.page, corners)];
        if let Some(gesture) = self.gesture {
            let change = gesture.change();
            shown.push(quad(chosen.page, corners.map(|(x, y)| change.apply(x, y))));
        }
        shown
    }
}

/// The Add Image tool, holding the picture the shell handed it.
#[derive(Debug, Default)]
pub struct AddImageTool {
    picture: Option<PathBuf>,
    marquee: Marquee,
}

impl AddImageTool {
    pub fn new() -> Self {
        Self::default()
    }

    fn place(&self, doc: &mut Document, rect: Option<PageRect>, at: PagePoint) {
        let Some(bytes) = self
            .picture
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
        else {
            return;
        };
        let rect = rect.unwrap_or_else(|| {
            let (width, height) = picture_size(&bytes).unwrap_or((144.0, 144.0));
            PageRect {
                page: at.page,
                x0: at.x,
                y0: at.y - height,
                x1: at.x + width,
                y1: at.y,
            }
        });
        let _ = add_image(doc, rect, bytes);
    }
}

impl ToolPlugin for AddImageTool {
    fn id(&self) -> &'static str {
        "add-image"
    }

    fn name(&self) -> &'static str {
        "Add Image"
    }

    fn icon(&self) -> &'static str {
        "add-image"
    }

    fn group(&self) -> &'static str {
        "images"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Click to place the image at its own size, or drag to fit it in a rectangle.")
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::ChoosesFile, ToolCapability::PlacesImage]
    }

    fn choose(&mut self, id: &str) -> bool {
        let path = PathBuf::from(id);
        let readable = std::fs::read(&path)
            .ok()
            .and_then(|bytes| picture_size(&bytes))
            .is_some();
        if readable {
            self.picture = Some(path);
        }
        readable
    }

    fn chosen(&self) -> Option<String> {
        self.picture
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.begin(input.at);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
        let anchor = self.marquee.anchor();
        let rect = self.marquee.finish();
        self.place(ctx.doc, rect, anchor.unwrap_or(input.at));
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.marquee.cancel();
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        self.marquee.overlays()
    }
}
