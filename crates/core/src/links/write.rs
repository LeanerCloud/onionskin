//! Writing links: a new one, a new target or look for one, and removing.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::read::is_link;
use super::{LinkLook, LinkTarget};
use crate::annots::author::{annots_array, append_to_page_annots, write_annots};
use crate::edit::Transaction;
use crate::pages::page_ref;
use crate::structure::{attach_link, Structure};
use crate::{Error, PageIndex, Result};

/// Put a link on `page` over `rect`, going to `target`. The new link's
/// reference.
pub fn add_link(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: PageIndex,
    rect: [f64; 4],
    target: &LinkTarget,
    look: LinkLook,
) -> Result<ObjRef> {
    let page_object = page_ref(tx, page)?;
    let number = tx.reserve();
    let link = ObjRef::new(number, 0);
    let (_, parent_key) = attach_link(tx, structure, page_object, link)?;
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("Annot"));
    dict.set(Name::new("Subtype"), Object::name("Link"));
    let [x0, y0, x1, y1] = rect;
    dict.set(
        Name::new("Rect"),
        numbers(&[x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]),
    );
    dict.set(Name::new("P"), Object::Ref(page_object));
    // Printed, as a link's rectangle is when it is visible at all.
    dict.set(Name::new("F"), Object::Integer(4));
    if let Some(key) = parent_key {
        dict.set(Name::new("StructParent"), Object::Integer(key));
    }
    set_target(tx, &mut dict, target)?;
    set_look(&mut dict, look);
    tx.put_object(number, 0, Object::Dict(dict))?;
    append_to_page_annots(tx, page_object, link)?;
    Ok(link)
}

/// Give link `link` a new target and look. A target of
/// [`LinkTarget::Other`] keeps the action the link has.
pub fn set_link(
    tx: &mut Transaction<'_>,
    link: ObjRef,
    target: &LinkTarget,
    look: LinkLook,
) -> Result<()> {
    let Some(state) = tx.object(link.number)? else {
        return Err(Error::NotADictionary {
            number: link.number,
        });
    };
    let Some(dict) = state.object.as_dict().filter(|dict| is_link(dict)) else {
        return Err(Error::NotADictionary {
            number: link.number,
        });
    };
    let mut dict = dict.clone();
    if !matches!(target, LinkTarget::Other(_)) {
        set_target(tx, &mut dict, target)?;
    }
    set_look(&mut dict, look);
    tx.put_object(link.number, state.generation, Object::Dict(dict))
}

/// Take link `link` off `page`. Whether it was there.
pub fn remove_link(tx: &mut Transaction<'_>, page: PageIndex, link: ObjRef) -> Result<bool> {
    let page_object = page_ref(tx, page)?;
    crate::annots::author::remove(tx, page_object, link)
}

/// Remove every link to a web page from `pages`: Acrobat's Remove All
/// Links, for web links. How many went.
pub fn remove_web_links(tx: &mut Transaction<'_>, pages: &[PageIndex]) -> Result<usize> {
    let mut removed = 0;
    for &page in pages {
        let page_object = page_ref(tx, page)?;
        let Some((holder, items)) = annots_array(tx, page_object)? else {
            continue;
        };
        let mut kept = Vec::with_capacity(items.len());
        for item in &items {
            if is_web_link(tx, item)? {
                removed += 1;
            } else {
                kept.push(item.clone());
            }
        }
        if kept.len() != items.len() {
            write_annots(tx, page_object, holder, kept)?;
        }
    }
    Ok(removed)
}

fn is_web_link(tx: &Transaction<'_>, item: &Object) -> Result<bool> {
    let Object::Ref(objref) = item else {
        return Ok(false);
    };
    let Some(state) = tx.object(objref.number)? else {
        return Ok(false);
    };
    let Some(dict) = state.object.as_dict().filter(|dict| is_link(dict)) else {
        return Ok(false);
    };
    let action = match dict.get(b"A") {
        Some(Object::Ref(action)) => tx.object(action.number)?.map(|state| state.object),
        Some(other) => Some(other.clone()),
        None => None,
    };
    Ok(action
        .as_ref()
        .and_then(Object::as_dict)
        .and_then(|action| action.get(b"S"))
        .and_then(Object::as_name)
        .is_some_and(|name| name.as_bytes() == b"URI"))
}

fn numbers(values: &[f64]) -> Object {
    Object::Array(values.iter().map(|value| Object::Real(*value)).collect())
}

/// `/A` for `target`, replacing any `/Dest`. A page is a `/GoTo` to it
/// fitted in the window; a web page a `/URI`; a file a `/Launch`.
fn set_target(tx: &Transaction<'_>, dict: &mut Dict, target: &LinkTarget) -> Result<()> {
    let mut action = Dict::new();
    action.set(Name::new("Type"), Object::name("Action"));
    match target {
        LinkTarget::Page(page) => {
            action.set(Name::new("S"), Object::name("GoTo"));
            action.set(
                Name::new("D"),
                Object::Array(vec![Object::Ref(page_ref(tx, *page)?), Object::name("Fit")]),
            );
        }
        LinkTarget::Web(url) => {
            action.set(Name::new("S"), Object::name("URI"));
            action.set(Name::new("URI"), Object::String(url.as_bytes().to_vec()));
        }
        LinkTarget::File(path) => {
            action.set(Name::new("S"), Object::name("Launch"));
            action.set(Name::new("F"), crate::annots::author::text_string(path));
        }
        LinkTarget::Other(_) => return Ok(()),
    }
    dict.remove(b"Dest");
    dict.set(Name::new("A"), Object::Dict(action));
    Ok(())
}

/// `/Border`, `/BS`, `/C` and `/H` for `look`.
fn set_look(dict: &mut Dict, look: LinkLook) {
    let width = if look.visible { look.width } else { 0.0 };
    dict.set(Name::new("Border"), numbers(&[0.0, 0.0, width]));
    let mut border = Dict::new();
    border.set(Name::new("W"), Object::Real(width));
    border.set(Name::new("S"), Object::name(look.style.key()));
    if look.style == super::LineStyle::Dashed {
        border.set(Name::new("D"), numbers(&[3.0]));
    }
    dict.set(Name::new("BS"), Object::Dict(border));
    dict.set(Name::new("C"), numbers(&look.color));
    dict.set(Name::new("H"), Object::name(look.highlight.key()));
}
