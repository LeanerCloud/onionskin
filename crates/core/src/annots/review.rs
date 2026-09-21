//! What the Comments pane does to a comment: change its text, reply to it,
//! and set its review status.
//!
//! **Status is a reply, not a key on the comment.** Acrobat records "Accepted"
//! as a `/Text` annotation with `/IRT` naming the comment and `/State` and
//! `/StateModel` saying what, so a comment keeps the whole history of who set
//! what. A reader that wrote `/State` onto the comment itself would lose that,
//! and Acrobat would not see it. The comment's current status is its newest
//! such reply for the model.
//!
//! Replies and status annotations are hidden: they live in the Comments pane,
//! not as more note icons stacked on the page over the comment they answer.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::appearance::normal_appearance;
use super::author::{pdf_date, text_string};
use super::model::{Annotation, Flags, Rect, Subtype};
use super::read::as_number;
use super::rebuild::model_from_dict;
use crate::edit::Transaction;
use crate::structure::Structure;
use crate::{Error, Result};

/// The review model's states, as Acrobat writes them.
pub const REVIEW_STATES: [&str; 5] = ["Accepted", "Rejected", "Cancelled", "Completed", "None"];
/// The model a review status belongs to.
pub const REVIEW_MODEL: &str = "Review";
/// The model the checkmark belongs to.
pub const MARKED_MODEL: &str = "Marked";

pub(crate) fn dict_of(tx: &Transaction<'_>, annotation: ObjRef) -> Result<(Dict, u16)> {
    let not = || Error::NotADictionary {
        number: annotation.number,
    };
    let state = tx.object(annotation.number)?.ok_or_else(not)?;
    let dict = state.object.as_dict().cloned().ok_or_else(not)?;
    Ok((dict, state.generation))
}

/// Give a comment new text: `/Contents` and `/M`. A free text draws its text,
/// so its appearance is drawn again from the same style; every other kind
/// shows its text only in the pane and a popup, so its appearance stays.
pub fn set_contents(
    tx: &mut Transaction<'_>,
    annotation: ObjRef,
    text: &str,
    now: i64,
) -> Result<()> {
    let (mut dict, generation) = dict_of(tx, annotation)?;
    if text.is_empty() {
        dict.remove(b"Contents");
    } else {
        dict.set(Name::new("Contents"), text_string(text));
    }
    dict.set(Name::new("M"), Object::String(pdf_date(now).into_bytes()));
    let is_free_text = dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|name| name.as_bytes() == b"FreeText");
    if is_free_text {
        redraw(tx, &mut dict)?;
    }
    tx.put_object(annotation.number, generation, Object::Dict(dict))
}

/// Draw `dict`'s appearance again from what it now says, when this crate
/// draws its subtype. Nothing changes for one it does not.
pub(crate) fn redraw(tx: &mut Transaction<'_>, dict: &mut Dict) -> Result<()> {
    let Some(model) = model_from_dict(dict) else {
        return Ok(());
    };
    let appearance = tx.reserve();
    tx.put_object(appearance, 0, Object::Stream(normal_appearance(&model)))?;
    let mut appearances = Dict::new();
    appearances.set(Name::new("N"), Object::Ref(ObjRef::new(appearance, 0)));
    dict.set(Name::new("AP"), Object::Dict(appearances));
    Ok(())
}

pub use super::rebuild::parse_default_appearance;

/// A hidden `/Text` annotation answering `parent`, on the parent's page and
/// at its place.
fn answer(parent_rect: Rect, parent: ObjRef, author: Option<&str>) -> Annotation {
    let mut answer = Annotation::new(Subtype::Text, parent_rect);
    answer.in_reply_to = Some(parent);
    answer.author = author.map(str::to_owned);
    answer.flags = Flags(Flags::PRINT).with_hidden(true);
    answer
}

fn parent_rect(dict: &Dict) -> Rect {
    match dict.get(b"Rect") {
        Some(Object::Array(items)) => {
            let values: Vec<f64> = items.iter().filter_map(as_number).collect();
            match values[..] {
                [x0, y0, x1, y1] => Rect::new(x0, y0, x1, y1),
                _ => Rect::new(0.0, 0.0, 20.0, 20.0),
            }
        }
        _ => Rect::new(0.0, 0.0, 20.0, 20.0),
    }
}

/// Reply to `parent`, which is on `page`. Returns the reply.
pub fn add_reply(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
    parent: ObjRef,
    text: &str,
    author: Option<&str>,
    now: i64,
) -> Result<ObjRef> {
    let (dict, _) = dict_of(tx, parent)?;
    let mut reply = answer(parent_rect(&dict), parent, author);
    reply.contents = Some(text.to_owned());
    super::author::add(tx, structure, page, &reply, now)
}

/// Set `parent`'s status in `model` (`Review` or `Marked`) to `state`, by
/// adding the status annotation Acrobat would. Returns it.
#[allow(clippy::too_many_arguments)]
pub fn set_status(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
    parent: ObjRef,
    model: &str,
    state: &str,
    author: Option<&str>,
    now: i64,
) -> Result<ObjRef> {
    let (dict, _) = dict_of(tx, parent)?;
    let mut status = answer(parent_rect(&dict), parent, author);
    status.state = Some((state.to_owned(), model.to_owned()));
    status.contents = Some(format!("{} set by {}", state, author.unwrap_or("someone")));
    super::author::add(tx, structure, page, &status, now)
}
