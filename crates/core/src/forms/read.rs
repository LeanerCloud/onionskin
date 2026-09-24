//! Reading the field tree: ISO 32000-2 12.7.4, with the inheritable keys
//! (`/FT`, `/Ff`, `/V`, `/DV`, `/DA`, `/Q`, `/Opt`, `/MaxLen`) carried down
//! from parent to terminal field.

use std::collections::BTreeMap;

use onionskin_content::pdf_text_string;
use onionskin_cos::{Dict, Document as CosDocument, ObjRef, Object};

use super::{ChoiceOption, Field, FieldFlags, FieldKind, FieldScripts, FieldValue, Form, Widget};
use crate::Result;

/// A field tree can nest; a hostile one can nest forever.
const MAX_DEPTH: usize = 32;
const MAX_FIELDS: usize = 20_000;

// `/Ff` bits, numbered from 1 as ISO 32000-2 table 227 onwards numbers them.
const READ_ONLY: i64 = 1 << 0;
const REQUIRED: i64 = 1 << 1;
const NO_EXPORT: i64 = 1 << 2;
const MULTILINE: i64 = 1 << 12;
const PASSWORD: i64 = 1 << 13;
const NO_TOGGLE_TO_OFF: i64 = 1 << 14;
const RADIO: i64 = 1 << 15;
const PUSH_BUTTON: i64 = 1 << 16;
const COMBO: i64 = 1 << 17;
const EDIT: i64 = 1 << 18;
const MULTI_SELECT: i64 = 1 << 21;
const COMB: i64 = 1 << 24;

// Annotation `/F` bits.
const HIDDEN: i64 = 1 << 1;
const NO_VIEW: i64 = 1 << 5;

/// What a field inherits from its parents.
#[derive(Clone, Default)]
struct Inherited {
    kind: Option<Vec<u8>>,
    flags: i64,
    value: Option<Object>,
    default: Option<Object>,
    appearance: Option<String>,
    align: Option<u8>,
    options: Option<Object>,
    max_len: Option<usize>,
}

impl Inherited {
    fn with(&self, doc: &CosDocument, dict: &Dict) -> Inherited {
        let mut next = self.clone();
        if let Some(kind) = dict.get(b"FT").and_then(Object::as_name) {
            next.kind = Some(kind.as_bytes().to_vec());
        }
        if let Some(flags) = dict.get(b"Ff").and_then(Object::as_integer) {
            next.flags = flags;
        }
        if let Some(value) = dict.get(b"V") {
            next.value = doc.resolve(value).ok();
        }
        if let Some(value) = dict.get(b"DV") {
            next.default = doc.resolve(value).ok();
        }
        if let Some(Ok(Object::String(bytes))) = dict.get(b"DA").map(|da| doc.resolve(da)) {
            next.appearance = Some(String::from_utf8_lossy(&bytes).into_owned());
        }
        if let Some(align) = dict.get(b"Q").and_then(Object::as_integer) {
            next.align = u8::try_from(align.clamp(0, 2)).ok();
        }
        if let Some(options) = dict.get(b"Opt") {
            next.options = doc.resolve(options).ok();
        }
        if let Some(max) = dict.get(b"MaxLen").and_then(Object::as_integer) {
            next.max_len = usize::try_from(max).ok();
        }
        next
    }
}

/// The document's form. A document with no `/AcroForm` has an empty one.
pub fn read_form(doc: &CosDocument) -> Result<Form> {
    let catalog = doc.catalog()?;
    let acroform = match catalog.get(b"AcroForm").map(|form| doc.resolve(form)) {
        Some(Ok(Object::Dict(form))) => form,
        _ => return Ok(Form::default()),
    };
    let pages = widget_pages(doc)?;
    let mut form = Form {
        need_appearances: matches!(acroform.get(b"NeedAppearances"), Some(Object::Bool(true))),
        xfa: acroform.contains(b"XFA"),
        resources: acroform
            .get(b"DR")
            .and_then(|dr| doc.resolve(dr).ok())
            .and_then(|dr| dr.as_dict().cloned()),
        calculation_order: refs(doc, acroform.get(b"CO")),
        ..Form::default()
    };
    let mut inherited = Inherited::default();
    if let Some(Ok(Object::String(bytes))) = acroform.get(b"DA").map(|da| doc.resolve(da)) {
        inherited.appearance = Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    if let Some(align) = acroform.get(b"Q").and_then(Object::as_integer) {
        inherited.align = u8::try_from(align.clamp(0, 2)).ok();
    }
    let mut walk = Walk {
        doc,
        pages: &pages,
        fields: Vec::new(),
    };
    for field in refs(doc, acroform.get(b"Fields")) {
        walk.node(field, &inherited, "", 0)?;
    }
    form.fields = walk.fields;
    Ok(form)
}

/// Which page each annotation is on, by object number.
fn widget_pages(doc: &CosDocument) -> Result<BTreeMap<u32, usize>> {
    let mut pages = BTreeMap::new();
    for index in 0..doc.page_count()? as usize {
        let page = doc.page(index)?;
        for annotation in refs(doc, page.dict.get(b"Annots")) {
            pages.entry(annotation.number).or_insert(index);
        }
    }
    Ok(pages)
}

fn refs(doc: &CosDocument, value: Option<&Object>) -> Vec<ObjRef> {
    match value.map(|value| doc.resolve(value)) {
        Some(Ok(Object::Array(items))) => items.iter().filter_map(Object::as_reference).collect(),
        _ => Vec::new(),
    }
}

struct Walk<'a> {
    doc: &'a CosDocument,
    pages: &'a BTreeMap<u32, usize>,
    fields: Vec<Field>,
}

impl Walk<'_> {
    fn node(
        &mut self,
        objref: ObjRef,
        inherited: &Inherited,
        parent: &str,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_DEPTH || self.fields.len() >= MAX_FIELDS {
            return Ok(());
        }
        let Some(dict) = self.doc.get(objref.number)?.object.as_dict().cloned() else {
            return Ok(());
        };
        let name = match dict.get(b"T").map(|t| self.doc.resolve(t)) {
            Some(Ok(Object::String(bytes))) => {
                let part = pdf_text_string(&bytes);
                if parent.is_empty() {
                    part
                } else {
                    format!("{parent}.{part}")
                }
            }
            _ => parent.to_owned(),
        };
        let inherited = inherited.with(self.doc, &dict);
        let kids = refs(self.doc, dict.get(b"Kids"));
        let mut widgets = Vec::new();
        let mut children = Vec::new();
        for kid in kids {
            let Some(kid_dict) = self.doc.get(kid.number)?.object.as_dict().cloned() else {
                continue;
            };
            if kid_dict.contains(b"T") || (!is_widget(&kid_dict) && kid_dict.contains(b"Kids")) {
                children.push(kid);
            } else {
                widgets.push((kid, kid_dict));
            }
        }
        let branch = !children.is_empty();
        for child in children {
            self.node(child, &inherited, &name, depth + 1)?;
        }
        if widgets.is_empty() && is_widget(&dict) {
            widgets.push((objref, dict.clone()));
        }
        // A node with fields under it and no widget of its own only names
        // them.
        if branch && widgets.is_empty() {
            return Ok(());
        }
        if inherited.kind.is_none() {
            return Ok(());
        }
        let kind = kind(self.doc, &inherited);
        let value = value_of(&kind, inherited.value.as_ref());
        let default = value_of(&kind, inherited.default.as_ref());
        self.fields.push(Field {
            objref,
            name,
            flags: FieldFlags {
                read_only: inherited.flags & READ_ONLY != 0,
                required: inherited.flags & REQUIRED != 0,
                no_export: inherited.flags & NO_EXPORT != 0,
            },
            value,
            default,
            widgets: widgets
                .iter()
                .map(|(widget, dict)| self.widget(*widget, dict))
                .collect(),
            tooltip: match dict.get(b"TU").map(|tu| self.doc.resolve(tu)) {
                Some(Ok(Object::String(bytes))) => Some(pdf_text_string(&bytes)),
                _ => None,
            },
            scripts: scripts(self.doc, &dict),
            align: inherited.align.unwrap_or(0),
            appearance: inherited.appearance.clone(),
            kind,
        });
        Ok(())
    }

    fn widget(&self, objref: ObjRef, dict: &Dict) -> Widget {
        let numbers: Vec<f64> = match dict.get(b"Rect").map(|rect| self.doc.resolve(rect)) {
            Some(Ok(Object::Array(items))) => items.iter().filter_map(number).collect(),
            _ => Vec::new(),
        };
        let rect = match numbers.as_slice() {
            [x0, y0, x1, y1] => [x0.min(*x1), y0.min(*y1), x0.max(*x1), y0.max(*y1)],
            _ => [0.0; 4],
        };
        let flags = dict.get(b"F").and_then(Object::as_integer).unwrap_or(0);
        let page = self.pages.get(&objref.number).copied();
        let on_state = self
            .doc
            .resolve(dict.get(b"AP").unwrap_or(&Object::Null))
            .ok()
            .and_then(|ap| ap.as_dict().cloned())
            .and_then(|ap| ap.get(b"N").cloned())
            .and_then(|normal| self.doc.resolve(&normal).ok())
            .and_then(|normal| normal.as_dict().cloned())
            .and_then(|states| {
                states
                    .iter()
                    .map(|(name, _)| String::from_utf8_lossy(name.as_bytes()).into_owned())
                    .find(|name| name != "Off")
            });
        Widget {
            objref,
            page,
            rect,
            on_state,
            state: dict
                .get(b"AS")
                .and_then(Object::as_name)
                .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned()),
            hidden: flags & (HIDDEN | NO_VIEW) != 0,
            border: self.colour(dict, b"BC"),
            fill: self.colour(dict, b"BG"),
            caption: self
                .mk(dict)
                .and_then(|mk| match self.doc.resolve(mk.get(b"CA")?).ok()? {
                    Object::String(bytes) => Some(pdf_text_string(&bytes)),
                    _ => None,
                }),
        }
    }

    fn mk(&self, widget: &Dict) -> Option<Dict> {
        self.doc
            .resolve(widget.get(b"MK")?)
            .ok()?
            .as_dict()
            .cloned()
    }

    /// A `/MK` colour as RGB: grey and CMYK converted, as a viewer shows
    /// them.
    fn colour(&self, widget: &Dict, key: &[u8]) -> Option<[f64; 3]> {
        let mk = self.mk(widget)?;
        let components: Vec<f64> = match self.doc.resolve(mk.get(key)?).ok()? {
            Object::Array(items) => items.iter().filter_map(number).collect(),
            _ => return None,
        };
        match components.as_slice() {
            [grey] => Some([*grey; 3]),
            [r, g, b] => Some([*r, *g, *b]),
            [c, m, y, k] => Some([c, m, y].map(|each| (1.0 - each) * (1.0 - k))),
            _ => None,
        }
    }
}

fn is_widget(dict: &Dict) -> bool {
    dict.get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|subtype| subtype.as_bytes() == b"Widget")
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

fn kind(doc: &CosDocument, inherited: &Inherited) -> FieldKind {
    let flags = inherited.flags;
    match inherited.kind.as_deref() {
        Some(b"Btn") if flags & PUSH_BUTTON != 0 => FieldKind::PushButton,
        Some(b"Btn") if flags & RADIO != 0 => FieldKind::Radio {
            no_toggle_to_off: flags & NO_TOGGLE_TO_OFF != 0,
        },
        Some(b"Btn") => FieldKind::CheckBox,
        Some(b"Ch") => FieldKind::Choice {
            combo: flags & COMBO != 0,
            editable: flags & EDIT != 0,
            multi_select: flags & MULTI_SELECT != 0,
            options: options(doc, inherited.options.as_ref()),
        },
        Some(b"Sig") => FieldKind::Signature,
        _ => FieldKind::Text {
            multiline: flags & MULTILINE != 0,
            password: flags & PASSWORD != 0,
            comb: flags & COMB != 0,
            max_len: inherited.max_len,
        },
    }
}

fn options(doc: &CosDocument, value: Option<&Object>) -> Vec<ChoiceOption> {
    let Some(Object::Array(items)) = value else {
        return Vec::new();
    };
    let text = |object: &Object| match doc.resolve(object) {
        Ok(Object::String(bytes)) => Some(pdf_text_string(&bytes)),
        _ => None,
    };
    items
        .iter()
        .filter_map(|item| match doc.resolve(item).ok()? {
            Object::Array(pair) if pair.len() == 2 => Some(ChoiceOption {
                export: text(&pair[0])?,
                display: text(&pair[1])?,
            }),
            other => text(&other).map(|display| ChoiceOption {
                export: display.clone(),
                display,
            }),
        })
        .collect()
}

fn value_of(kind: &FieldKind, value: Option<&Object>) -> FieldValue {
    let Some(value) = value else {
        return match kind {
            FieldKind::CheckBox | FieldKind::Radio { .. } => FieldValue::State(None),
            _ => FieldValue::None,
        };
    };
    match (kind, value) {
        (FieldKind::CheckBox | FieldKind::Radio { .. }, Object::Name(name)) => {
            let name = String::from_utf8_lossy(name.as_bytes()).into_owned();
            FieldValue::State((name != "Off").then_some(name))
        }
        (FieldKind::CheckBox | FieldKind::Radio { .. }, _) => FieldValue::State(None),
        (FieldKind::Choice { .. }, Object::String(bytes)) => {
            FieldValue::Chosen(vec![pdf_text_string(bytes)])
        }
        (FieldKind::Choice { .. }, Object::Array(items)) => FieldValue::Chosen(
            items
                .iter()
                .filter_map(|item| match item {
                    Object::String(bytes) => Some(pdf_text_string(bytes)),
                    _ => None,
                })
                .collect(),
        ),
        (FieldKind::Text { .. }, Object::String(bytes)) => FieldValue::Text(pdf_text_string(bytes)),
        _ => FieldValue::None,
    }
}

/// The JavaScript of `/AA /K`, `/F`, `/V` and `/C`.
fn scripts(doc: &CosDocument, dict: &Dict) -> FieldScripts {
    let Some(Ok(Object::Dict(actions))) = dict.get(b"AA").map(|aa| doc.resolve(aa)) else {
        return FieldScripts::default();
    };
    let script = |key: &[u8]| -> Option<String> {
        let action = doc.resolve(actions.get(key)?).ok()?;
        let action = action.as_dict()?;
        let is_javascript = action
            .get(b"S")
            .and_then(Object::as_name)
            .is_some_and(|kind| kind.as_bytes() == b"JavaScript");
        if !is_javascript {
            return None;
        }
        match doc.resolve(action.get(b"JS")?).ok()? {
            Object::String(bytes) => Some(pdf_text_string(&bytes)),
            Object::Stream(stream) => doc
                .decode_stream(&stream)
                .ok()
                .map(|bytes| pdf_text_string(&bytes)),
            _ => None,
        }
    };
    FieldScripts {
        keystroke: script(b"K"),
        format: script(b"F"),
        validate: script(b"V"),
        calculate: script(b"C"),
    }
}
