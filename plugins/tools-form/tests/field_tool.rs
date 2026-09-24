//! Prepare Form's field tools on a page: drawing, placing, selecting,
//! asking for properties, deleting, and radio buttons joining a group.

use onionskin_core::forms::{FieldKind, NewField};
use onionskin_core::{Document, FitMode, Modifiers, PagePoint, PageRect, ViewSize, Viewport};
use onionskin_plugin_api::{EditVerb, Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};
use onionskin_tools_form::field_tool::{FieldTool, GROUP};

/// One blank 612 by 792 page.
fn blank() -> Vec<u8> {
    b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n"
        .to_vec()
}

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

impl Fixture {
    fn new() -> Self {
        let mut doc = Document::open_bytes(blank()).expect("opens, repaired");
        let mut viewport = Viewport::new(
            doc.page_count(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
            12.0,
        )
        .expect("viewport");
        let geometry = doc.page_geometry(0).expect("measures").clone();
        viewport.measure_page(geometry).expect("measurable");
        viewport.fit(FitMode::Page).expect("fits");
        Fixture { doc, viewport }
    }

    fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }

    fn press(&mut self, tool: &mut FieldTool, from: (f64, f64), to: (f64, f64), clicks: u8) {
        let at = |(x, y), clicks| PointerInput {
            at: PagePoint { page: 0, x, y },
            pressure: 1.0,
            modifiers: Modifiers::default(),
            clicks,
        };
        tool.on_pointer_down(&mut self.ctx(), at(from, clicks));
        tool.on_pointer_move(&mut self.ctx(), at(to, 1));
        tool.on_pointer_up(&mut self.ctx(), at(to, 1));
    }

    fn click(&mut self, tool: &mut FieldTool, at: (f64, f64), clicks: u8) {
        self.press(tool, at, at, clicks);
    }

    fn names(&mut self) -> Vec<String> {
        let form = self.doc.form().expect("reads");
        form.fields.iter().map(|field| field.name.clone()).collect()
    }
}

fn tool(kind: NewField) -> FieldTool {
    FieldTool::new(kind)
}

#[test]
fn every_kind_has_a_tool_in_one_rail_slot() {
    let tools = FieldTool::all();
    let ids: Vec<&str> = tools.iter().map(|tool| tool.id()).collect();
    assert_eq!(
        ids,
        [
            "form-text",
            "form-check-box",
            "form-radio",
            "form-list-box",
            "form-dropdown",
            "form-button",
            "form-date",
            "form-signature"
        ]
    );
    let names: Vec<&str> = tools.iter().map(|tool| tool.name()).collect();
    assert_eq!(
        names,
        [
            "Text Field",
            "Check Box",
            "Radio Button",
            "List Box",
            "Dropdown",
            "Button",
            "Date Field",
            "Signature Field"
        ]
    );
    for tool in &tools {
        assert_eq!(tool.group(), GROUP);
        assert_eq!(tool.icon(), tool.id());
        assert_eq!(tool.capabilities(), [ToolCapability::PrepareForm]);
        assert!(tool.hint().is_some_and(|hint| hint.contains("properties")));
    }
    assert!(ToolCapability::PrepareForm.edits_document());
}

#[test]
fn a_drag_draws_a_field_and_a_click_places_one_at_its_usual_size() {
    let mut page = Fixture::new();
    let mut text = tool(NewField::Text);
    text.on_activate(&mut page.ctx());
    page.press(&mut text, (100.0, 700.0), (300.0, 670.0), 1);
    page.click(&mut text, (100.0, 600.0), 1);
    assert_eq!(page.names(), ["Text1", "Text2"]);
    let form = page.doc.form().expect("reads");
    assert_eq!(form.fields[0].widgets[0].rect, [100.0, 670.0, 300.0, 700.0]);
    assert_eq!(
        form.fields[1].widgets[0].rect,
        [100.0, 578.0, 244.0, 600.0],
        "144 by 22, hanging from the click"
    );
    let overlays = text.overlays(&page.doc);
    assert_eq!(
        overlays
            .iter()
            .filter(|shown| matches!(shown, Overlay::Rect(_)))
            .count(),
        2,
        "both outlined"
    );
    assert!(overlays.contains(&Overlay::AntsRect(PageRect {
        page: 0,
        x0: 100.0,
        y0: 578.0,
        x1: 244.0,
        y1: 600.0
    })));
    assert_eq!(
        page.doc.edit().history().undo_label(),
        Some("Add Text Field")
    );
}

#[test]
fn a_field_is_selected_by_a_click_and_its_properties_asked_for() {
    let mut page = Fixture::new();
    let mut check = tool(NewField::CheckBox);
    page.click(&mut check, (100.0, 700.0), 1);
    check.on_cancel(&mut page.ctx());
    assert!(!check.claims(EditVerb::Delete), "Escape let it go");
    page.click(&mut check, (105.0, 695.0), 1);
    assert!(check.claims(EditVerb::Delete));
    assert_eq!(page.doc.take_field_properties_request(), None);
    page.click(&mut check, (105.0, 695.0), 2);
    let request = page
        .doc
        .take_field_properties_request()
        .expect("a double click asks");
    let form = page.doc.form().expect("reads");
    assert_eq!(request.field, form.fields[0].objref);
    assert_eq!(request.widget, form.fields[0].widgets[0].objref);
    check.on_commit(&mut page.ctx());
    assert!(
        page.doc.take_field_properties_request().is_some(),
        "Enter asks"
    );
    assert_eq!(page.names(), ["Check Box1"], "no field placed over one");
}

#[test]
fn delete_takes_the_selected_field_away() {
    let mut page = Fixture::new();
    let mut list = tool(NewField::ListBox);
    page.click(&mut list, (100.0, 700.0), 1);
    page.click(&mut list, (100.0, 700.0), 2);
    assert!(
        page.doc.take_field_properties_request().is_some(),
        "a double click on the corner it was placed from is on the field"
    );
    assert_eq!(list.edit(&mut page.ctx(), EditVerb::Copy, None), None);
    assert_eq!(page.names(), ["List Box1"], "only Delete deletes");
    assert_eq!(list.edit(&mut page.ctx(), EditVerb::Delete, None), None);
    assert!(page.names().is_empty());
    assert_eq!(page.doc.edit().history().undo_label(), Some("Delete Field"));
    assert!(!list.claims(EditVerb::Delete));
    assert_eq!(list.edit(&mut page.ctx(), EditVerb::Delete, None), None);
}

#[test]
fn a_radio_button_placed_with_one_selected_joins_its_group() {
    let mut page = Fixture::new();
    let mut radio = tool(NewField::Radio { group: None });
    page.click(&mut radio, (100.0, 700.0), 1);
    page.click(&mut radio, (130.0, 700.0), 1);
    radio.on_cancel(&mut page.ctx());
    page.click(&mut radio, (200.0, 700.0), 1);
    let form = page.doc.form().expect("reads");
    assert_eq!(page.names(), ["Group1", "Group2"]);
    assert_eq!(form.fields[0].widgets.len(), 2);
    assert!(matches!(form.fields[0].kind, FieldKind::Radio { .. }));

    let mut button = tool(NewField::Button);
    page.click(&mut button, (300.0, 400.0), 1);
    let mut another = tool(NewField::Radio { group: None });
    page.click(&mut another, (300.0, 400.0), 1);
    page.click(&mut another, (400.0, 300.0), 1);
    assert_eq!(
        page.names().last().map(String::as_str),
        Some("Group3"),
        "a button selected is not a group to join"
    );
}

#[test]
fn a_selection_whose_field_went_away_is_dropped() {
    let mut page = Fixture::new();
    let mut text = tool(NewField::Text);
    page.click(&mut text, (100.0, 700.0), 1);
    let (edit, base) = page.doc.edit_mut();
    edit.undo(base).expect("undoes");
    text.on_activate(&mut page.ctx());
    assert!(!text.claims(EditVerb::Delete));
    text.on_deactivate(&mut page.ctx());
    assert!(text.overlays(&page.doc).is_empty());
}
