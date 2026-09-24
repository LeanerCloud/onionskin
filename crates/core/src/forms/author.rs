//! Preparing a form: new fields, and fields taken away.
//!
//! A new field is written as Acrobat's Prepare Form writes one: a field
//! dictionary merged with its one widget, on the page's `/Annots` and in
//! `/AcroForm /Fields`, with an appearance, and on a tagged document a
//! `/Form` structure element. A radio button joins its group as another
//! widget of the group's field. The form dictionary is made when the
//! document has none, with Helvetica and ZapfDingbats in its `/DR`.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::appearance::{button_states, Frame};
use super::write::{dict_at, generation, write_field_value};
use super::{Field, FieldFlags, FieldKind, FieldScripts, FieldValue, Form, Widget};
use crate::annots::author::{annots_array, append_to_page_annots, text_string, write_annots};
use crate::edit::Transaction;
use crate::pages::page_ref;
use crate::structure::{attach_form_field, Structure};
use crate::{Error, PageIndex, Result};

/// `/Ff` bits a new field is made with.
const RADIO_FLAGS: i64 = (1 << 14) | (1 << 15);
const PUSH_BUTTON: i64 = 1 << 16;
const COMBO: i64 = 1 << 17;

/// The `/DA` every new field is set in: Helvetica, sized to fit, black.
const DEFAULT_DA: &str = "/Helv 0 Tf 0 g";

/// The kinds of field Prepare Form's tools make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewField {
    Text,
    /// A text field formatted and checked as a date, `mm/dd/yyyy`.
    Date,
    CheckBox,
    /// A radio button, in the group named, or a new group.
    Radio {
        group: Option<String>,
    },
    ListBox,
    Dropdown,
    Button,
    Signature,
}

impl NewField {
    /// The name a new field of this kind is numbered after, as Acrobat
    /// numbers them: `Text1`, `Check Box1`, `Group1`.
    pub fn base_name(&self) -> &'static str {
        match self {
            NewField::Text => "Text",
            NewField::Date => "Date",
            NewField::CheckBox => "Check Box",
            NewField::Radio { .. } => "Group",
            NewField::ListBox => "List Box",
            NewField::Dropdown => "Dropdown",
            NewField::Button => "Button",
            NewField::Signature => "Signature",
        }
    }

    /// The size a click makes, in points, as Acrobat sizes them.
    pub fn default_size(&self) -> (f64, f64) {
        match self {
            NewField::Text | NewField::Date | NewField::Dropdown => (144.0, 22.0),
            NewField::CheckBox | NewField::Radio { .. } => (14.0, 14.0),
            NewField::ListBox => (144.0, 72.0),
            NewField::Button => (72.0, 22.0),
            NewField::Signature => (180.0, 36.0),
        }
    }
}

/// What [`add_field`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    /// The field: for a radio button, its group.
    pub field: ObjRef,
    pub widget: ObjRef,
    pub name: String,
}

/// The first of `base1`, `base2`, ... that no field of `form` is called.
pub fn unique_name(form: &Form, base: &str) -> String {
    (1..)
        .map(|number| format!("{base}{number}"))
        .find(|name| form.field(name).is_none())
        .expect("a name is free")
}

/// Put a new field of kind `kind` over `rect` on `page`.
pub fn add_field(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    form: &Form,
    kind: &NewField,
    page: PageIndex,
    rect: [f64; 4],
) -> Result<Added> {
    let page_object = page_ref(tx, page)?;
    let rect = normalized(rect);
    let (acroform_holder, mut acroform) = acroform(tx)?;
    ensure_resources(&mut acroform);
    let widget = ObjRef::new(tx.reserve(), 0);
    let (_, parent_key) = attach_form_field(tx, structure, page_object, widget)?;

    let group = match kind {
        NewField::Radio { group: Some(name) } => form
            .field(name)
            .filter(|field| matches!(field.kind, FieldKind::Radio { .. })),
        _ => None,
    };
    let name = match (kind, group) {
        (_, Some(group)) => group.name.clone(),
        _ => unique_name(form, kind.base_name()),
    };
    let mut dict = widget_dict(page_object, rect, kind, parent_key);
    let (field, on_state) = match (kind, group) {
        (NewField::Radio { .. }, Some(group)) => {
            let on = format!("Choice{}", group.widgets.len() + 1);
            dict.set(Name::new("Parent"), Object::Ref(group.objref));
            add_kid(tx, group.objref, widget)?;
            (group.objref, Some(on))
        }
        (NewField::Radio { .. }, None) => {
            let parent = ObjRef::new(tx.reserve(), 0);
            let mut group_dict = Dict::new();
            group_dict.set(Name::new("FT"), Object::name("Btn"));
            group_dict.set(Name::new("Ff"), Object::Integer(RADIO_FLAGS));
            group_dict.set(Name::new("T"), text_string(&name));
            group_dict.set(Name::new("V"), Object::name("Off"));
            group_dict.set(Name::new("Kids"), Object::Array(vec![Object::Ref(widget)]));
            tx.put_object(parent.number, 0, Object::Dict(group_dict))?;
            dict.set(Name::new("Parent"), Object::Ref(parent));
            push_field(&mut acroform, parent);
            (parent, Some("Choice1".to_owned()))
        }
        _ => {
            field_keys(&mut dict, kind, &name);
            push_field(&mut acroform, widget);
            let on = matches!(kind, NewField::CheckBox).then(|| "Yes".to_owned());
            (widget, on)
        }
    };
    if let Some(on) = &on_state {
        let mark = if matches!(kind, NewField::CheckBox) {
            '4'
        } else {
            'l'
        };
        states_appearance(tx, &mut dict, on, mark)?;
    }
    tx.put_object(widget.number, 0, Object::Dict(dict))?;
    append_to_page_annots(tx, page_object, widget)?;
    write_acroform(tx, acroform_holder, acroform)?;

    let model = model(field, widget, &name, kind, page, rect, on_state);
    if matches!(
        model.kind,
        FieldKind::Text { .. } | FieldKind::Choice { .. } | FieldKind::PushButton
    ) {
        let resources = default_resources();
        write_field_value(tx, Some(&resources), &model, &model.value, None)?;
    }
    Ok(Added {
        field,
        widget,
        name,
    })
}

/// Take field `field` of `form` away: every widget off its page, the field
/// out of `/Fields` or its parent's `/Kids`, and out of the calculation
/// order.
pub fn remove_field(tx: &mut Transaction<'_>, form: &Form, field: ObjRef) -> Result<()> {
    let target = form.field_by_ref(field).ok_or(Error::NotADictionary {
        number: field.number,
    })?;
    for widget in &target.widgets {
        let Some(page) = widget.page else { continue };
        let page_object = page_ref(tx, page)?;
        if let Some((holder, mut items)) = annots_array(tx, page_object)? {
            items.retain(|item| !matches!(item, Object::Ref(objref) if objref.number == widget.objref.number));
            write_annots(tx, page_object, holder, items)?;
        }
    }
    let dict = dict_at(tx, field)?;
    if let Some(parent) = dict.get(b"Parent").and_then(Object::as_reference) {
        let mut parent_dict = dict_at(tx, parent)?;
        if let Some(Object::Array(kids)) = parent_dict.get(b"Kids").cloned() {
            let kept: Vec<Object> = kids
                .into_iter()
                .filter(|kid| kid.as_reference() != Some(field))
                .collect();
            parent_dict.set(Name::new("Kids"), Object::Array(kept));
            tx.put_object(
                parent.number,
                generation(tx, parent)?,
                Object::Dict(parent_dict),
            )?;
        }
    }
    let (holder, mut acroform) = acroform(tx)?;
    for key in ["Fields", "CO"] {
        let items = match acroform.get(key.as_bytes()) {
            Some(Object::Ref(objref)) => dict_or_array(tx, *objref)?,
            Some(Object::Array(items)) => items.clone(),
            _ => continue,
        };
        let kept: Vec<Object> = items
            .into_iter()
            .filter(|item| item.as_reference() != Some(field))
            .collect();
        acroform.set(Name::new(key), Object::Array(kept));
    }
    write_acroform(tx, holder, acroform)
}

fn dict_or_array(tx: &Transaction<'_>, objref: ObjRef) -> Result<Vec<Object>> {
    Ok(tx
        .object(objref.number)?
        .and_then(|state| state.object.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default())
}

fn normalized([x0, y0, x1, y1]: [f64; 4]) -> [f64; 4] {
    [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
}

fn numbers(values: &[f64]) -> Object {
    Object::Array(values.iter().map(|value| Object::Real(*value)).collect())
}

/// The widget's own keys: what it is, where, and how it is drawn.
fn widget_dict(page: ObjRef, rect: [f64; 4], kind: &NewField, parent_key: Option<i64>) -> Dict {
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("Annot"));
    dict.set(Name::new("Subtype"), Object::name("Widget"));
    dict.set(Name::new("Rect"), numbers(&rect));
    dict.set(Name::new("P"), Object::Ref(page));
    dict.set(Name::new("F"), Object::Integer(4));
    if let Some(key) = parent_key {
        dict.set(Name::new("StructParent"), Object::Integer(key));
    }
    let mut mk = Dict::new();
    mk.set(Name::new("BC"), numbers(&[0.0, 0.0, 0.0]));
    match kind {
        NewField::CheckBox => mk.set(Name::new("CA"), text_string("4")),
        NewField::Radio { .. } => mk.set(Name::new("CA"), text_string("l")),
        NewField::Button => {
            mk.set(Name::new("BG"), numbers(&[0.75, 0.75, 0.75]));
            mk.set(Name::new("CA"), text_string("Button"));
        }
        _ => {}
    }
    dict.set(Name::new("MK"), Object::Dict(mk));
    let mut bs = Dict::new();
    bs.set(Name::new("W"), Object::Integer(1));
    bs.set(Name::new("S"), Object::name("S"));
    dict.set(Name::new("BS"), Object::Dict(bs));
    dict.set(Name::new("DA"), text_string(DEFAULT_DA));
    dict
}

/// A field's own keys, for a field that is its own widget.
fn field_keys(dict: &mut Dict, kind: &NewField, name: &str) {
    dict.set(Name::new("T"), text_string(name));
    let (field_type, flags) = match kind {
        NewField::Text | NewField::Date => ("Tx", 0),
        NewField::CheckBox => ("Btn", 0),
        NewField::Radio { .. } => ("Btn", RADIO_FLAGS),
        NewField::ListBox => ("Ch", 0),
        NewField::Dropdown => ("Ch", COMBO),
        NewField::Button => ("Btn", PUSH_BUTTON),
        NewField::Signature => ("Sig", 0),
    };
    dict.set(Name::new("FT"), Object::name(field_type));
    if flags != 0 {
        dict.set(Name::new("Ff"), Object::Integer(flags));
    }
    if matches!(kind, NewField::ListBox | NewField::Dropdown) {
        dict.set(Name::new("Opt"), Object::Array(Vec::new()));
    }
    if matches!(kind, NewField::CheckBox) {
        dict.set(Name::new("V"), Object::name("Off"));
    }
    if matches!(kind, NewField::Date) {
        let mut aa = Dict::new();
        for (key, script) in [
            ("F", "AFDate_FormatEx(\"mm/dd/yyyy\");"),
            ("K", "AFDate_KeystrokeEx(\"mm/dd/yyyy\");"),
        ] {
            aa.set(Name::new(key), javascript(script));
        }
        dict.set(Name::new("AA"), Object::Dict(aa));
    }
}

/// A JavaScript action running `script`.
pub(super) fn javascript(script: &str) -> Object {
    let mut action = Dict::new();
    action.set(Name::new("S"), Object::name("JavaScript"));
    action.set(Name::new("JS"), text_string(script));
    Object::Dict(action)
}

/// A button's on and off appearances, and `/AS` off.
fn states_appearance(
    tx: &mut Transaction<'_>,
    dict: &mut Dict,
    on: &str,
    mark: char,
) -> Result<()> {
    let frame = Frame::of(dict, Clone::clone);
    let (on_stream, off_stream) = button_states(&frame, mark, &[0.0]);
    let mut states = Dict::new();
    for (state, stream) in [(on, on_stream), ("Off", off_stream)] {
        let number = tx.reserve();
        tx.put_object(number, 0, Object::Stream(stream))?;
        states.set(Name::new(state), Object::Ref(ObjRef::new(number, 0)));
    }
    let mut ap = Dict::new();
    ap.set(Name::new("N"), Object::Dict(states));
    dict.set(Name::new("AP"), Object::Dict(ap));
    dict.set(Name::new("AS"), Object::name("Off"));
    Ok(())
}

/// The new field as the reader would read it back.
fn model(
    field: ObjRef,
    widget: ObjRef,
    name: &str,
    kind: &NewField,
    page: PageIndex,
    rect: [f64; 4],
    on_state: Option<String>,
) -> Field {
    let field_kind = match kind {
        NewField::Text | NewField::Date => FieldKind::Text {
            multiline: false,
            password: false,
            comb: false,
            max_len: None,
        },
        NewField::CheckBox => FieldKind::CheckBox,
        NewField::Radio { .. } => FieldKind::Radio {
            no_toggle_to_off: true,
        },
        NewField::ListBox | NewField::Dropdown => FieldKind::Choice {
            combo: matches!(kind, NewField::Dropdown),
            editable: false,
            multi_select: false,
            options: Vec::new(),
        },
        NewField::Button => FieldKind::PushButton,
        NewField::Signature => FieldKind::Signature,
    };
    let value = match field_kind {
        FieldKind::CheckBox | FieldKind::Radio { .. } => FieldValue::State(None),
        _ => FieldValue::None,
    };
    Field {
        objref: field,
        name: name.to_owned(),
        kind: field_kind,
        flags: FieldFlags::default(),
        value: value.clone(),
        default: value,
        widgets: vec![Widget {
            objref: widget,
            page: Some(page),
            rect,
            on_state,
            state: Some("Off".to_owned()),
            hidden: false,
        }],
        tooltip: None,
        scripts: FieldScripts::default(),
        align: u8::from(matches!(kind, NewField::Button)),
        appearance: Some(DEFAULT_DA.to_owned()),
    }
}

/// Where `/AcroForm` is, and what it holds; an empty one when there is
/// none.
enum Holder {
    Catalog(ObjRef),
    Object(ObjRef),
}

fn acroform(tx: &Transaction<'_>) -> Result<(Holder, Dict)> {
    let catalog = crate::pages::catalog_ref(tx)?;
    let catalog_dict = dict_at(tx, catalog)?;
    match catalog_dict.get(b"AcroForm") {
        Some(Object::Ref(objref)) => Ok((Holder::Object(*objref), dict_at(tx, *objref)?)),
        Some(Object::Dict(dict)) => Ok((Holder::Catalog(catalog), dict.clone())),
        _ => {
            let mut dict = Dict::new();
            dict.set(Name::new("Fields"), Object::Array(Vec::new()));
            dict.set(Name::new("DA"), text_string(DEFAULT_DA));
            Ok((Holder::Catalog(catalog), dict))
        }
    }
}

fn write_acroform(tx: &mut Transaction<'_>, holder: Holder, acroform: Dict) -> Result<()> {
    match holder {
        Holder::Object(objref) => tx.put_object(
            objref.number,
            generation(tx, objref)?,
            Object::Dict(acroform),
        ),
        Holder::Catalog(catalog) => {
            let mut dict = dict_at(tx, catalog)?;
            dict.set(Name::new("AcroForm"), Object::Dict(acroform));
            tx.put_object(catalog.number, generation(tx, catalog)?, Object::Dict(dict))
        }
    }
}

/// `/Fields` with `field` added. A `/Fields` held in an object of its own is
/// brought inline, which is where a new one is written.
fn push_field(acroform: &mut Dict, field: ObjRef) {
    let mut fields = match acroform.get(b"Fields") {
        Some(Object::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    fields.push(Object::Ref(field));
    acroform.set(Name::new("Fields"), Object::Array(fields));
}

/// The fonts new fields are drawn in, in `/DR` so a viewer that draws its
/// own appearances has them.
fn default_resources() -> Dict {
    let font = |base: &str, encoding: Option<&str>| {
        let mut dict = Dict::new();
        dict.set(Name::new("Type"), Object::name("Font"));
        dict.set(Name::new("Subtype"), Object::name("Type1"));
        dict.set(Name::new("BaseFont"), Object::name(base));
        if let Some(encoding) = encoding {
            dict.set(Name::new("Encoding"), Object::name(encoding));
        }
        Object::Dict(dict)
    };
    let mut fonts = Dict::new();
    fonts.set(
        Name::new("Helv"),
        font("Helvetica", Some("WinAnsiEncoding")),
    );
    fonts.set(Name::new("ZaDb"), font("ZapfDingbats", None));
    let mut resources = Dict::new();
    resources.set(Name::new("Font"), Object::Dict(fonts));
    resources
}

/// `/DR` with Helvetica and ZapfDingbats, keeping what it has.
fn ensure_resources(acroform: &mut Dict) {
    let wanted = default_resources();
    let mut resources = match acroform.get(b"DR") {
        Some(Object::Dict(dict)) => dict.clone(),
        // A `/DR` in an object of its own is left as it is.
        Some(_) => return,
        None => Dict::new(),
    };
    let mut fonts = match resources.get(b"Font") {
        Some(Object::Dict(dict)) => dict.clone(),
        Some(_) => return,
        None => Dict::new(),
    };
    if let Some(Object::Dict(wanted_fonts)) = wanted.get(b"Font") {
        for (name, font) in wanted_fonts.iter() {
            if !fonts.contains(name.as_bytes()) {
                fonts.set(name.clone(), font.clone());
            }
        }
    }
    resources.set(Name::new("Font"), Object::Dict(fonts));
    acroform.set(Name::new("DR"), Object::Dict(resources));
}

/// Add `kid` to `parent`'s `/Kids`.
fn add_kid(tx: &mut Transaction<'_>, parent: ObjRef, kid: ObjRef) -> Result<()> {
    let mut dict = dict_at(tx, parent)?;
    let mut kids = match dict.get(b"Kids") {
        Some(Object::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    kids.push(Object::Ref(kid));
    dict.set(Name::new("Kids"), Object::Array(kids));
    tx.put_object(parent.number, generation(tx, parent)?, Object::Dict(dict))
}
