//! Prepare Form's field tools: one per kind of field, sharing a rail slot.
//!
//! Drag to draw a field, or click to place one at its usual size with its
//! top left corner where the click was. Clicking a field selects it;
//! double-clicking it, or Enter with it selected, asks the shell for its
//! Properties, and Edit > Delete takes it away. A radio button placed while
//! another radio button is selected joins its group, as Acrobat's "Add
//! another button" does. Every field is outlined while a tool is chosen, so
//! one with no border can be found.

use onionskin_core::forms::{add_field, remove_field, Form, NewField};
use onionskin_core::{Document, FieldRequest, ObjRef, PagePoint, PageRect};
use onionskin_plugin_api::marquee::Marquee;
use onionskin_plugin_api::{EditVerb, Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

/// The group the field tools share on the rail.
pub const GROUP: &str = "form-fields";

/// The widget a click chose.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Selected {
    field: ObjRef,
    widget: ObjRef,
    rect: PageRect,
}

#[derive(Debug)]
pub struct FieldTool {
    kind: NewField,
    marquee: Marquee,
    selected: Option<Selected>,
    /// The press's click count: a release always reports one.
    clicks: u8,
    /// Every widget as of the tool's last look, to outline.
    outlined: Vec<PageRect>,
}

impl FieldTool {
    pub fn new(kind: NewField) -> Self {
        FieldTool {
            kind,
            marquee: Marquee::default(),
            selected: None,
            clicks: 1,
            outlined: Vec::new(),
        }
    }

    /// One tool for each kind Prepare Form places.
    pub fn all() -> Vec<FieldTool> {
        [
            NewField::Text,
            NewField::CheckBox,
            NewField::Radio { group: None },
            NewField::ListBox,
            NewField::Dropdown,
            NewField::Button,
            NewField::Date,
            NewField::Signature,
        ]
        .into_iter()
        .map(FieldTool::new)
        .collect()
    }

    fn refresh(&mut self, doc: &mut Document) {
        self.outlined = doc
            .form()
            .map(|form| {
                form.fields
                    .iter()
                    .flat_map(|field| &field.widgets)
                    .filter_map(|widget| Some(page_rect(widget.page?, widget.rect)))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(selected) = self.selected {
            let still_there = doc
                .form()
                .ok()
                .and_then(|form| form.field_by_ref(selected.field).cloned())
                .is_some_and(|field| field.widgets.iter().any(|w| w.objref == selected.widget));
            if !still_there {
                self.selected = None;
            }
        }
    }

    /// The field's widget under `at`, hidden ones included: preparing a
    /// form reaches every field. A point's worth of slack round each, since
    /// a rectangle read back from the file is rounded, and a double click
    /// where a field was just placed lands on its corner.
    fn hit(form: &Form, at: PagePoint) -> Option<Selected> {
        const SLACK: f64 = 1.0;
        form.fields.iter().find_map(|field| {
            field.widgets.iter().find_map(|widget| {
                let [x0, y0, x1, y1] = widget.rect;
                let inside = (x0 - SLACK..=x1 + SLACK).contains(&at.x)
                    && (y0 - SLACK..=y1 + SLACK).contains(&at.y);
                (widget.page == Some(at.page) && inside).then(|| Selected {
                    field: field.objref,
                    widget: widget.objref,
                    rect: page_rect(at.page, widget.rect),
                })
            })
        })
    }

    /// Place a field over `rect`, and select it.
    fn place(&mut self, doc: &mut Document, rect: PageRect) {
        let kind = match (&self.kind, self.selected) {
            (NewField::Radio { .. }, Some(selected)) => NewField::Radio {
                group: doc.form().ok().and_then(|form| {
                    let field = form.field_by_ref(selected.field)?;
                    matches!(field.kind, onionskin_core::forms::FieldKind::Radio { .. })
                        .then(|| field.name.clone())
                }),
            },
            (kind, _) => kind.clone(),
        };
        let Ok(form) = doc.form() else { return };
        let corners = [rect.x0, rect.y0, rect.x1, rect.y1];
        let added = doc.edit_annotations(label(&self.kind), |tx, structure| {
            add_field(tx, structure, &form, &kind, rect.page, corners)
        });
        if let Ok(added) = added {
            self.selected = Some(Selected {
                field: added.field,
                widget: added.widget,
                rect,
            });
        }
    }

    fn ask_for_properties(&self, doc: &mut Document) {
        if let Some(selected) = self.selected {
            doc.request_field_properties(FieldRequest {
                field: selected.field,
                widget: selected.widget,
                page: selected.rect.page,
                point: (selected.rect.x0, selected.rect.y1),
            });
        }
    }
}

fn page_rect(page: usize, [x0, y0, x1, y1]: [f64; 4]) -> PageRect {
    PageRect {
        page,
        x0,
        y0,
        x1,
        y1,
    }
}

/// The undo label a new field of `kind` is added under.
fn label(kind: &NewField) -> &'static str {
    match kind {
        NewField::Text => "Add Text Field",
        NewField::Date => "Add Date Field",
        NewField::CheckBox => "Add Check Box",
        NewField::Radio { .. } => "Add Radio Button",
        NewField::ListBox => "Add List Box",
        NewField::Dropdown => "Add Dropdown",
        NewField::Button => "Add Button",
        NewField::Signature => "Add Signature Field",
    }
}

impl ToolPlugin for FieldTool {
    fn id(&self) -> &'static str {
        match self.kind {
            NewField::Text => "form-text",
            NewField::Date => "form-date",
            NewField::CheckBox => "form-check-box",
            NewField::Radio { .. } => "form-radio",
            NewField::ListBox => "form-list-box",
            NewField::Dropdown => "form-dropdown",
            NewField::Button => "form-button",
            NewField::Signature => "form-signature",
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            NewField::Text => "Text Field",
            NewField::Date => "Date Field",
            NewField::CheckBox => "Check Box",
            NewField::Radio { .. } => "Radio Button",
            NewField::ListBox => "List Box",
            NewField::Dropdown => "Dropdown",
            NewField::Button => "Button",
            NewField::Signature => "Signature Field",
        }
    }

    fn icon(&self) -> &'static str {
        self.id()
    }

    fn group(&self) -> &'static str {
        GROUP
    }

    fn hint(&self) -> Option<&'static str> {
        Some(match self.kind {
            NewField::Radio { .. } => {
                "Drag or click to place a radio button; with one selected, the next joins its group. Double-click a field for its properties."
            }
            _ => {
                "Drag to draw the field, or click to place it. Click a field to select it, double-click for its properties, Delete to remove it."
            }
        })
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::PrepareForm]
    }

    fn claims(&self, verb: EditVerb) -> bool {
        verb == EditVerb::Delete && self.selected.is_some()
    }

    fn edit(&mut self, ctx: &mut ToolCtx, verb: EditVerb, _pasted: Option<&str>) -> Option<String> {
        if verb != EditVerb::Delete {
            return None;
        }
        let selected = self.selected.take()?;
        if let Ok(form) = ctx.doc.form() {
            let _ = ctx.doc.edit_annotations("Delete Field", |tx, _| {
                remove_field(tx, &form, selected.field)
            });
        }
        self.refresh(ctx.doc);
        None
    }

    fn on_activate(&mut self, ctx: &mut ToolCtx) {
        self.refresh(ctx.doc);
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.clicks = input.clicks;
        self.marquee.begin(input.at);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
        match self.marquee.finish() {
            Some(rect) => self.place(ctx.doc, rect),
            None => {
                let hit = ctx
                    .doc
                    .form()
                    .ok()
                    .and_then(|form| Self::hit(&form, input.at));
                match hit {
                    Some(hit) => {
                        self.selected = Some(hit);
                        if self.clicks >= 2 {
                            self.ask_for_properties(ctx.doc);
                        }
                    }
                    None => {
                        let (width, height) = self.kind.default_size();
                        let at = input.at;
                        let rect = PageRect {
                            page: at.page,
                            x0: at.x,
                            y0: at.y - height,
                            x1: at.x + width,
                            y1: at.y,
                        };
                        self.place(ctx.doc, rect);
                    }
                }
            }
        }
        self.refresh(ctx.doc);
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        self.ask_for_properties(ctx.doc);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        if self.marquee.anchor().is_some() {
            self.marquee.cancel();
        } else {
            self.selected = None;
        }
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.marquee.cancel();
        self.selected = None;
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        let mut shown: Vec<Overlay> = self.outlined.iter().copied().map(Overlay::Rect).collect();
        shown.extend(
            self.selected
                .map(|selected| Overlay::AntsRect(selected.rect)),
        );
        shown.extend(self.marquee.overlays());
        shown
    }
}
