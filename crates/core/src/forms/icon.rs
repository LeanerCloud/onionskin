//! A button's icon: what an image field shows once an image is chosen.
//!
//! The image comes as the first page of a document, as every image import
//! makes one, and goes in as a form XObject: the button's `/MK /I`, and its
//! normal appearance drawing it fitted inside the frame.

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use super::appearance::{icon_appearance, Frame};
use super::write::{dict_at, generation};
use super::{FieldKind, Form};
use crate::edit::Transaction;
use crate::pages::import_page_as_form;
use crate::{Error, Result};

/// Show page 1 of `source` on button `widget` of field `field`.
pub fn set_button_icon(
    tx: &mut Transaction<'_>,
    form: &Form,
    field: ObjRef,
    widget: ObjRef,
    source: &CosDocument,
) -> Result<()> {
    let is_button = form
        .field_by_ref(field)
        .filter(|target| target.kind == FieldKind::PushButton)
        .is_some_and(|target| target.widgets.iter().any(|each| each.objref == widget));
    if !is_button {
        return Err(Error::NotADictionary {
            number: widget.number,
        });
    }
    let (icon, bbox) = import_page_as_form(tx, source, 0)?;
    let mut dict = dict_at(tx, widget)?;
    let mut mk = match dict.get(b"MK") {
        Some(Object::Dict(mk)) => mk.clone(),
        _ => Dict::new(),
    };
    mk.set(Name::new("I"), Object::Ref(icon));
    mk.set(Name::new("TP"), Object::Integer(1));
    dict.set(Name::new("MK"), Object::Dict(mk));
    let frame = Frame::of(&dict, Clone::clone);
    let number = tx.reserve();
    tx.put_object(
        number,
        0,
        Object::Stream(icon_appearance(&frame, icon, bbox)),
    )?;
    let mut ap = Dict::new();
    ap.set(Name::new("N"), Object::Ref(ObjRef::new(number, 0)));
    dict.set(Name::new("AP"), Object::Dict(ap));
    tx.put_object(widget.number, generation(tx, widget)?, Object::Dict(dict))
}
