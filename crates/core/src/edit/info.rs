//! Which object a document-information write may write through.
//!
//! The trailer's `/Info` selects the document-information dictionary, and a
//! selector is just bytes in a file that may be malformed. Two writers write
//! through it, and both used to follow whatever it named: a direct value was
//! treated as absent and silently replaced, and a reference to a stream or to
//! the document's own catalog was written over. This resolves the selector once,
//! for both, so what counts as a writable target is decided in one place.
//!
//! It lives under `crate::edit` because the generation authority is the
//! overlay's own entry, and the overlay is a private field of `Transaction`.

use onionskin_cos::{Dict, ObjRef, Object, XrefEntry};

use super::{Result, Transaction};
use crate::session::Error;

/// The `/Info` target a write may proceed against.
pub(crate) enum InfoTarget {
    /// `/Info` is absent, a literal null, or a direct dictionary. The
    /// dictionary is carried because a direct one supplies the starting
    /// entries, and the generic verb ignores it while the properties writer
    /// does not.
    Create { seed: Option<Dict> },
    /// `/Info` names this live object, and its dictionary is the one to edit.
    Write {
        number: u32,
        generation: u16,
        dict: Dict,
    },
}

/// Resolves the `/Info` selector, or refuses it. Never falls back to creating
/// an `/Info` for a target that exists and is unusable: that would replace
/// whatever the document actually wrote there.
pub(crate) fn info_target(tx: &Transaction<'_>) -> Result<InfoTarget> {
    match tx.trailer_value(b"Info") {
        None | Some(onionskin_cos::Object::Null) => Ok(InfoTarget::Create { seed: None }),
        Some(onionskin_cos::Object::Dict(dict)) => Ok(InfoTarget::Create { seed: Some(dict) }),
        Some(onionskin_cos::Object::Ref(selector)) => resolve_selector(tx, selector),
        Some(_) => Err(Error::MalformedInfoTarget),
    }
}

fn resolve_selector(tx: &Transaction<'_>, selector: ObjRef) -> Result<InfoTarget> {
    // The overlay first: an object this session created has no base row, and a
    // base-first lookup would refuse the `/Info` a properties write just made.
    let (generation, live) = match tx.overlay.object(selector.number) {
        Some(state) => (state.generation, true),
        None => match tx.base().xref().get(selector.number) {
            // An object-stream row has no generation of its own, and the spec
            // numbers those objects from zero.
            Some(XrefEntry::InObjectStream { .. }) => (0, true),
            Some(XrefEntry::InFile { generation, .. }) => (generation, true),
            Some(XrefEntry::Free { .. }) | None => (0, false),
        },
    };
    if !live {
        return Err(Error::MissingInfoTarget { selector });
    }

    // Structure before generation: the contract refuses a structural alias even
    // when the selected generation differs, so the more specific cause is
    // reported first.
    let structural = crate::pages::structural_numbers(tx)?;
    if structural.contains(&selector.number) {
        return Err(Error::StructuralInfoTarget {
            number: selector.number,
        });
    }

    let state = tx.object(selector.number)?;
    let Some(state) = state else {
        return Err(Error::MissingInfoTarget { selector });
    };
    let Object::Dict(dict) = state.object else {
        return Err(Error::NotADictionary {
            number: selector.number,
        });
    };

    if generation != selector.generation {
        return Err(Error::MismatchedInfoTarget {
            selector,
            current: generation,
        });
    }

    Ok(InfoTarget::Write {
        number: selector.number,
        generation,
        dict,
    })
}
