//! A field's properties, as Prepare Form's Properties dialog shows and
//! changes them: its name and tooltip, whether it is hidden, read-only or
//! required, its colours and text, where it is, what its kind offers, and
//! its scripts.
//!
//! Read from the field and the widget it was chosen by; written back to
//! both, with the widget's appearance drawn again and the field put into or
//! taken out of the calculation order as it gains or loses a calculation.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::appearance::{button_states, parse_da, Frame};
use super::author::javascript;
use super::write::{dict_at, generation, write_field_value};
use super::{ChoiceOption, Field, FieldKind, FieldScripts, FieldValue, Form, Widget};
use crate::annots::author::text_string;
use crate::edit::Transaction;
use crate::{Error, Result};

// `/Ff` bits this module sets; the rest are kept as they are.
const READ_ONLY: i64 = 1 << 0;
const REQUIRED: i64 = 1 << 1;
const MULTILINE: i64 = 1 << 12;
const PASSWORD: i64 = 1 << 13;
const NO_TOGGLE_TO_OFF: i64 = 1 << 14;
const EDIT: i64 = 1 << 18;
const MULTI_SELECT: i64 = 1 << 21;
const COMB: i64 = 1 << 24;
// Annotation `/F`.
const HIDDEN: i64 = 1 << 1;
const PRINT: i64 = 1 << 2;

/// What the Properties dialog edits.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldProperties {
    /// The field's own part of its name: `last` of `person.last`.
    pub name: String,
    pub tooltip: String,
    pub hidden: bool,
    pub read_only: bool,
    pub required: bool,
    pub border: Option<[f64; 3]>,
    pub fill: Option<[f64; 3]>,
    /// Points; 0 fits the text to the field.
    pub font_size: f64,
    pub text_color: [f64; 3],
    /// The widget's rectangle.
    pub rect: [f64; 4],
    pub options: KindOptions,
    pub scripts: FieldScripts,
}

/// What a field's kind offers on the Options tab.
#[derive(Debug, Clone, PartialEq)]
pub enum KindOptions {
    Text {
        /// 0 left, 1 centred, 2 right.
        align: u8,
        default: String,
        multiline: bool,
        password: bool,
        comb: bool,
        max_len: Option<usize>,
    },
    /// A check box, or one radio button: the value it exports when on.
    Button {
        export: String,
        on_by_default: bool,
        /// Radio buttons only: clicking the chosen one leaves it chosen.
        no_toggle_to_off: Option<bool>,
    },
    Choice {
        options: Vec<ChoiceOption>,
        editable: bool,
        multi_select: bool,
        /// The export value chosen by default.
        default: Option<String>,
    },
    PushButton {
        caption: String,
    },
    Signature,
}

impl FieldProperties {
    /// `field`'s properties as widget `widget` shows them.
    pub fn of(field: &Field, widget: &Widget) -> FieldProperties {
        let da = parse_da(field.appearance.as_deref());
        let options = match &field.kind {
            FieldKind::Text {
                multiline,
                password,
                comb,
                max_len,
            } => KindOptions::Text {
                align: field.align,
                default: field.default.as_text(),
                multiline: *multiline,
                password: *password,
                comb: *comb,
                max_len: *max_len,
            },
            FieldKind::CheckBox | FieldKind::Radio { .. } => KindOptions::Button {
                export: widget.on_state.clone().unwrap_or_else(|| "Yes".to_owned()),
                on_by_default: matches!(&field.default, FieldValue::State(Some(on)) if Some(on) == widget.on_state.as_ref()),
                no_toggle_to_off: match field.kind {
                    FieldKind::Radio { no_toggle_to_off } => Some(no_toggle_to_off),
                    _ => None,
                },
            },
            FieldKind::Choice {
                options,
                editable,
                multi_select,
                ..
            } => KindOptions::Choice {
                options: options.clone(),
                editable: *editable,
                multi_select: *multi_select,
                default: match &field.default {
                    FieldValue::Chosen(chosen) => chosen.first().cloned(),
                    _ => None,
                },
            },
            FieldKind::PushButton => KindOptions::PushButton {
                caption: widget.caption.clone().unwrap_or_default(),
            },
            FieldKind::Signature => KindOptions::Signature,
        };
        FieldProperties {
            name: field.name.rsplit('.').next().unwrap_or_default().to_owned(),
            tooltip: field.tooltip.clone().unwrap_or_default(),
            hidden: widget.hidden,
            read_only: field.flags.read_only,
            required: field.flags.required,
            border: widget.border,
            fill: widget.fill,
            font_size: da.size,
            text_color: match da.color.as_slice() {
                [grey] => [*grey; 3],
                [r, g, b] => [*r, *g, *b],
                [c, m, y, k] => [c, m, y].map(|each| (1.0 - each) * (1.0 - k)),
                _ => [0.0; 3],
            },
            rect: widget.rect,
            options,
            scripts: field.scripts.clone(),
        }
    }
}

/// Why properties cannot be written as they are, when they cannot.
pub fn refusal(form: &Form, field: &Field, properties: &FieldProperties) -> Option<String> {
    let name = properties.name.trim();
    if name.is_empty() {
        return Some("A field needs a name".to_owned());
    }
    if name.contains('.') {
        return Some("A field's name cannot have a full stop in it".to_owned());
    }
    let full = match field.name.rsplit_once('.') {
        Some((parent, _)) => format!("{parent}.{name}"),
        None => name.to_owned(),
    };
    if full != field.name && form.field(&full).is_some() {
        return Some(format!("There is already a field called {full}"));
    }
    let [x0, y0, x1, y1] = properties.rect;
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return Some("A field has to be at least a point wide and high".to_owned());
    }
    if let KindOptions::Button { export, .. } = &properties.options {
        if export.trim().is_empty() || export == "Off" {
            return Some("The export value cannot be empty or Off".to_owned());
        }
    }
    None
}

/// Write `properties` to `field` of `form`, as widget `widget` shows it.
pub fn set_field_properties(
    tx: &mut Transaction<'_>,
    form: &Form,
    field: ObjRef,
    widget: ObjRef,
    properties: &FieldProperties,
) -> Result<()> {
    let target = form.field_by_ref(field).ok_or(Error::NotADictionary {
        number: field.number,
    })?;
    let shown = target
        .widgets
        .iter()
        .find(|each| each.objref == widget)
        .ok_or(Error::NotADictionary {
            number: widget.number,
        })?;
    let mut field_dict = dict_at(tx, field)?;
    write_field_keys(&mut field_dict, target, shown, properties);
    let merged = field == widget;
    let mut widget_dict = if merged {
        field_dict.clone()
    } else {
        tx.put_object(
            field.number,
            generation(tx, field)?,
            Object::Dict(field_dict.clone()),
        )?;
        dict_at(tx, widget)?
    };
    write_widget_keys(&mut widget_dict, properties);
    let renamed_state = rename_state(tx, &mut widget_dict, shown, properties)?;
    tx.put_object(
        widget.number,
        generation(tx, widget)?,
        Object::Dict(widget_dict),
    )?;
    set_calculated(tx, form, field, properties.scripts.calculate.is_some())?;

    let model = updated(target, shown, properties, renamed_state.as_deref());
    let value = match (&model.kind, &target.value) {
        (FieldKind::CheckBox | FieldKind::Radio { .. }, FieldValue::State(Some(on)))
            if shown.on_state.as_ref() == Some(on) =>
        {
            FieldValue::State(renamed_state.clone())
        }
        _ => model.value.clone(),
    };
    write_field_value(tx, form.resources.as_ref(), &model, &value, None)
}

/// `/T`, `/TU`, `/Ff`, `/DA`, `/Q`, `/MaxLen`, `/Opt`, `/DV` and `/AA`.
fn write_field_keys(dict: &mut Dict, field: &Field, widget: &Widget, properties: &FieldProperties) {
    dict.set(Name::new("T"), text_string(properties.name.trim()));
    if properties.tooltip.trim().is_empty() {
        dict.remove(b"TU");
    } else {
        dict.set(Name::new("TU"), text_string(properties.tooltip.trim()));
    }
    let mut flags = dict.get(b"Ff").and_then(Object::as_integer).unwrap_or(0);
    let mut set = |bit: i64, on: bool| {
        if on {
            flags |= bit;
        } else {
            flags &= !bit;
        }
    };
    set(READ_ONLY, properties.read_only);
    set(REQUIRED, properties.required);
    match &properties.options {
        KindOptions::Text {
            align,
            default,
            multiline,
            password,
            comb,
            max_len,
        } => {
            set(MULTILINE, *multiline);
            set(PASSWORD, *password);
            set(COMB, *comb && max_len.is_some());
            dict.set(Name::new("Q"), Object::Integer(i64::from(*align)));
            match max_len {
                Some(most) => dict.set(Name::new("MaxLen"), Object::Integer(*most as i64)),
                None => {
                    dict.remove(b"MaxLen");
                }
            }
            if default.is_empty() {
                dict.remove(b"DV");
            } else {
                dict.set(Name::new("DV"), text_string(default));
            }
        }
        KindOptions::Button {
            export,
            on_by_default,
            no_toggle_to_off,
        } => {
            if let Some(hold) = no_toggle_to_off {
                set(NO_TOGGLE_TO_OFF, *hold);
            }
            let was_default = matches!(&field.default, FieldValue::State(Some(on)) if Some(on) == widget.on_state.as_ref());
            if *on_by_default {
                dict.set(Name::new("DV"), Object::name(export.trim()));
            } else if was_default {
                dict.remove(b"DV");
            }
        }
        KindOptions::PushButton { .. } | KindOptions::Signature => {}
        KindOptions::Choice {
            options,
            editable,
            multi_select,
            default,
        } => {
            set(EDIT, *editable);
            set(MULTI_SELECT, *multi_select);
            dict.set(
                Name::new("Opt"),
                Object::Array(
                    options
                        .iter()
                        .map(|option| {
                            Object::Array(vec![
                                text_string(&option.export),
                                text_string(&option.display),
                            ])
                        })
                        .collect(),
                ),
            );
            match default {
                Some(chosen) => dict.set(Name::new("DV"), text_string(chosen)),
                None => {
                    dict.remove(b"DV");
                }
            }
        }
    }
    if flags == 0 {
        dict.remove(b"Ff");
    } else {
        dict.set(Name::new("Ff"), Object::Integer(flags));
    }
    let da = parse_da(field.appearance.as_deref());
    let [r, g, b] = properties.text_color;
    dict.set(
        Name::new("DA"),
        text_string(&format!(
            "/{} {} Tf {r} {g} {b} rg",
            da.font, properties.font_size
        )),
    );
    write_scripts(dict, &properties.scripts);
}

/// `/AA` with the four field events, keeping any other action it has.
fn write_scripts(dict: &mut Dict, scripts: &FieldScripts) {
    let mut actions = match dict.get(b"AA") {
        Some(Object::Dict(actions)) => actions.clone(),
        _ => Dict::new(),
    };
    for (key, script) in [
        ("K", &scripts.keystroke),
        ("F", &scripts.format),
        ("V", &scripts.validate),
        ("C", &scripts.calculate),
    ] {
        match script
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            Some(text) => actions.set(Name::new(key), javascript(text)),
            None => {
                actions.remove(key.as_bytes());
            }
        }
    }
    if actions.is_empty() {
        dict.remove(b"AA");
    } else {
        dict.set(Name::new("AA"), Object::Dict(actions));
    }
}

fn rgb(colour: [f64; 3]) -> Object {
    Object::Array(colour.iter().map(|each| Object::Real(*each)).collect())
}

/// `/Rect`, `/F`, and `/MK`'s colours and caption.
fn write_widget_keys(dict: &mut Dict, properties: &FieldProperties) {
    dict.set(Name::new("Rect"), rect_object(properties.rect));
    let mut flags = dict.get(b"F").and_then(Object::as_integer).unwrap_or(PRINT);
    if properties.hidden {
        flags = (flags | HIDDEN) & !PRINT;
    } else {
        flags = (flags & !HIDDEN) | PRINT;
    }
    dict.set(Name::new("F"), Object::Integer(flags));
    let mut mk = match dict.get(b"MK") {
        Some(Object::Dict(mk)) => mk.clone(),
        _ => Dict::new(),
    };
    for (key, colour) in [("BC", properties.border), ("BG", properties.fill)] {
        match colour {
            Some(colour) => mk.set(Name::new(key), rgb(colour)),
            None => {
                mk.remove(key.as_bytes());
            }
        }
    }
    if let KindOptions::PushButton { caption } = &properties.options {
        mk.set(Name::new("CA"), text_string(caption));
    }
    dict.set(Name::new("MK"), Object::Dict(mk));
}

fn rect_object([x0, y0, x1, y1]: [f64; 4]) -> Object {
    Object::Array(
        [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
            .iter()
            .map(|each| Object::Real(*each))
            .collect(),
    )
}

/// A check box's or radio button's on state renamed to its new export
/// value, with its on and off appearances drawn again for the new look.
/// The on state the widget has afterwards.
fn rename_state(
    tx: &mut Transaction<'_>,
    dict: &mut Dict,
    widget: &Widget,
    properties: &FieldProperties,
) -> Result<Option<String>> {
    let KindOptions::Button { export, .. } = &properties.options else {
        return Ok(widget.on_state.clone());
    };
    let export = export.trim().to_owned();
    let frame = Frame::of(dict, Clone::clone);
    let mark = widget
        .caption
        .as_deref()
        .and_then(|caption| caption.chars().next())
        .unwrap_or('4');
    let [r, g, b] = properties.text_color;
    let (on, off) = button_states(&frame, mark, &[r, g, b]);
    let mut states = Dict::new();
    for (state, stream) in [(export.as_str(), on), ("Off", off)] {
        let number = tx.reserve();
        tx.put_object(number, 0, Object::Stream(stream))?;
        states.set(Name::new(state), Object::Ref(ObjRef::new(number, 0)));
    }
    let mut ap = Dict::new();
    ap.set(Name::new("N"), Object::Dict(states));
    dict.set(Name::new("AP"), Object::Dict(ap));
    let was_on = widget.state.is_some() && widget.state == widget.on_state;
    dict.set(
        Name::new("AS"),
        Object::name(if was_on { &export } else { "Off" }),
    );
    Ok(Some(export))
}

/// Put `field` into the form's `/CO`, or take it out.
fn set_calculated(
    tx: &mut Transaction<'_>,
    form: &Form,
    field: ObjRef,
    calculated: bool,
) -> Result<()> {
    let listed = form.calculation_order.contains(&field);
    if listed == calculated {
        return Ok(());
    }
    let mut order = form.calculation_order.clone();
    if calculated {
        order.push(field);
    } else {
        order.retain(|each| *each != field);
    }
    super::author::set_calculation_order(tx, &order)
}

/// The field as it reads back after `properties`, for its appearance.
fn updated(
    field: &Field,
    widget: &Widget,
    properties: &FieldProperties,
    on_state: Option<&str>,
) -> Field {
    let mut model = field.clone();
    let [r, g, b] = properties.text_color;
    let da = parse_da(field.appearance.as_deref());
    model.appearance = Some(format!(
        "/{} {} Tf {r} {g} {b} rg",
        da.font, properties.font_size
    ));
    model.flags.read_only = properties.read_only;
    model.flags.required = properties.required;
    match (&mut model.kind, &properties.options) {
        (
            FieldKind::Text {
                multiline,
                password,
                comb,
                max_len,
            },
            KindOptions::Text {
                align,
                multiline: new_multiline,
                password: new_password,
                comb: new_comb,
                max_len: new_max,
                ..
            },
        ) => {
            *multiline = *new_multiline;
            *password = *new_password;
            *comb = *new_comb && new_max.is_some();
            *max_len = *new_max;
            model.align = *align;
        }
        (
            FieldKind::Choice {
                options,
                editable,
                multi_select,
                ..
            },
            KindOptions::Choice {
                options: new_options,
                editable: new_editable,
                multi_select: new_multi,
                ..
            },
        ) => {
            options.clone_from(new_options);
            *editable = *new_editable;
            *multi_select = *new_multi;
        }
        _ => {}
    }
    // Only the widget the properties were chosen by is drawn again; the
    // field's other widgets keep theirs.
    model.widgets = vec![Widget {
        rect: properties.rect,
        on_state: on_state.map(str::to_owned),
        ..widget.clone()
    }];
    model
}
