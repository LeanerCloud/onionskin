//! The typed vocabulary of edits.
//!
//! [`DocumentEdit`] is a closed enum in `core` and that is deliberate: plugins
//! never write COS objects. It puts the structure-tree maintenance hook at one
//! choke point instead of in every tool, it makes the registry-exhaustive
//! "every edit is undoable" property meaningful rather than tautological, and
//! it is the structured verb set M4's MCP surface needs. The cost is that a
//! third-party plugin cannot invent an edit, which is not a cost until
//! `plugin-host-wasm` exists. There is no `Raw(Change)` escape hatch, because
//! one would delete the entire justification.
//!
//! P2 lands two variants and each names the M3 consumer that reaches it.
//! `SetInfoField` is what P13a's Description tab writes; `SetCatalogEntry` is
//! what its Initial View tab writes. `SetTrailerEntry` is deliberately not one
//! of them: `File > Properties` writes `/Info`, Initial View writes catalog
//! entries, and no M3 surface writes a trailer key, so it would be a verb with
//! no caller in the package whose argument for a closed enum is that every verb
//! has one. P5 and P6 grow the enum.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::{Result, Transaction};
use crate::session::Error;

const INFO: &[u8] = b"Info";
const ROOT: &[u8] = b"Root";

/// One structured edit. `None` as a value removes the key, which is how the
/// Description tab clears a field.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentEdit {
    SetInfoField { key: Name, value: Option<Object> },
    SetCatalogEntry { key: Name, value: Option<Object> },
}

impl DocumentEdit {
    /// The Edit menu's "Undo <label>".
    pub fn label(&self) -> &'static str {
        match self {
            DocumentEdit::SetInfoField { .. } => "Document Properties",
            DocumentEdit::SetCatalogEntry { .. } => "Initial View",
        }
    }

    pub(crate) fn apply(&self, tx: &mut Transaction<'_>) -> Result<()> {
        match self {
            DocumentEdit::SetInfoField { key, value } => set_info_field(tx, key, value.as_ref()),
            DocumentEdit::SetCatalogEntry { key, value } => {
                set_catalog_entry(tx, key, value.as_ref())
            }
        }
    }
}

/// Writing a Description field on a document with no `/Info` is one object
/// write **and** one trailer write, which is why `Change::TrailerKey` is not
/// speculative: without it an undo drops the object and leaves the trailer
/// naming it.
fn set_info_field(tx: &mut Transaction<'_>, key: &Name, value: Option<&Object>) -> Result<()> {
    match info_reference(tx)? {
        Some(objref) => {
            let dict = dict_at(tx, objref.number)?;
            let updated = with_entry(dict, key, value);
            tx.set_object(objref.number, objref.generation, Object::Dict(updated))
        }
        None => {
            let number = tx.reserve();
            let dict = with_entry(Dict::new(), key, value);
            tx.set_object(number, 0, Object::Dict(dict))?;
            tx.set_trailer(Name::new("Info"), Some(Object::Ref(ObjRef::new(number, 0))))
        }
    }
}

fn set_catalog_entry(tx: &mut Transaction<'_>, key: &Name, value: Option<&Object>) -> Result<()> {
    let objref = catalog_reference(tx)?;
    let dict = dict_at(tx, objref.number)?;
    let updated = with_entry(dict, key, value);
    tx.set_object(objref.number, objref.generation, Object::Dict(updated))
}

/// The live `/Info` reference: the overlay's trailer if it has one, else the
/// base's. Reading the base alone would miss an `/Info` this session created.
fn info_reference(tx: &Transaction<'_>) -> Result<Option<ObjRef>> {
    match tx.trailer_value(INFO) {
        Some(Object::Ref(objref)) => Ok(Some(objref)),
        Some(_) | None => Ok(None),
    }
}

fn catalog_reference(tx: &Transaction<'_>) -> Result<ObjRef> {
    match tx.trailer_value(ROOT) {
        Some(Object::Ref(objref)) => Ok(objref),
        Some(_) | None => Err(Error::NoCatalog),
    }
}

/// The current dictionary at a number, overlay first. An object that is not a
/// dictionary is a malformed document rather than a case to paper over: writing
/// a key into it would discard whatever it actually held.
fn dict_at(tx: &Transaction<'_>, number: u32) -> Result<Dict> {
    match tx.object(number)? {
        Some(state) => match state.object {
            Object::Dict(dict) => Ok(dict),
            _ => Err(Error::NotADictionary { number }),
        },
        None => Ok(Dict::new()),
    }
}

fn with_entry(mut dict: Dict, key: &Name, value: Option<&Object>) -> Dict {
    match value {
        Some(value) => dict.set(key.clone(), value.clone()),
        None => {
            dict.remove(key.as_bytes());
        }
    }
    dict
}
