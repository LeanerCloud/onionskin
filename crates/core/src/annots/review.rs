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
use super::model::{Annotation, BaseFont, Color, Flags, Intent, Rect, Subtype, TextStyle};
use super::read::{as_number, color};
use crate::edit::Transaction;
use crate::structure::Structure;
use crate::{Error, Result};

/// The review model's states, as Acrobat writes them.
pub const REVIEW_STATES: [&str; 5] = ["Accepted", "Rejected", "Cancelled", "Completed", "None"];
/// The model a review status belongs to.
pub const REVIEW_MODEL: &str = "Review";
/// The model the checkmark belongs to.
pub const MARKED_MODEL: &str = "Marked";

fn dict_of(tx: &Transaction<'_>, annotation: ObjRef) -> Result<(Dict, u16)> {
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
        let model = free_text_model(&dict, text);
        let appearance = tx.reserve();
        tx.put_object(appearance, 0, Object::Stream(normal_appearance(&model)))?;
        let mut appearances = Dict::new();
        appearances.set(Name::new("N"), Object::Ref(ObjRef::new(appearance, 0)));
        dict.set(Name::new("AP"), Object::Dict(appearances));
    }
    tx.put_object(annotation.number, generation, Object::Dict(dict))
}

/// A free text's model, read back from its dictionary so its appearance can
/// be drawn again with the text changed and nothing else.
fn free_text_model(dict: &Dict, text: &str) -> Annotation {
    let numbers = |key: &[u8]| -> Vec<f64> {
        match dict.get(key) {
            Some(Object::Array(items)) => items.iter().filter_map(as_number).collect(),
            _ => Vec::new(),
        }
    };
    let rect = match numbers(b"Rect")[..] {
        [x0, y0, x1, y1] => Rect::new(x0, y0, x1, y1),
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    };
    let mut model = Annotation::new(Subtype::FreeText, rect);
    model.contents = (!text.is_empty()).then(|| text.to_owned());
    model.color = color(dict.get(b"C"));
    model.interior_color = color(dict.get(b"IC"));
    model.border_width = dict
        .get(b"BS")
        .and_then(Object::as_dict)
        .and_then(|border| border.get(b"W"))
        .and_then(as_number)
        .unwrap_or(1.0);
    model.intent = match dict.get(b"IT").and_then(Object::as_name) {
        Some(name) if name.as_bytes() == b"FreeTextTypewriter" => Some(Intent::FreeTextTypewriter),
        Some(name) if name.as_bytes() == b"FreeTextCallout" => Some(Intent::FreeTextCallout),
        _ => None,
    };
    model.callout = numbers(b"CL")
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect();
    model.text_style = Some(
        dict.get(b"DA")
            .and_then(|da| match da {
                Object::String(bytes) => parse_default_appearance(&String::from_utf8_lossy(bytes)),
                _ => None,
            })
            .unwrap_or(TextStyle::new(BaseFont::Helvetica, 12.0, Color::BLACK)),
    );
    model
}

/// A `/DA` string back into the style that wrote it: `/Font size Tf r g b rg`.
/// A font this crate does not name falls back to Helvetica; a missing colour
/// is black.
pub fn parse_default_appearance(da: &str) -> Option<TextStyle> {
    let tokens: Vec<&str> = da.split_whitespace().collect();
    let tf = tokens.iter().position(|token| *token == "Tf")?;
    let size: f64 = tokens.get(tf.checked_sub(1)?)?.parse().ok()?;
    let font_name = tokens.get(tf.checked_sub(2)?)?.trim_start_matches('/');
    let font = [
        BaseFont::Helvetica,
        BaseFont::HelveticaBold,
        BaseFont::TimesRoman,
        BaseFont::Courier,
    ]
    .into_iter()
    .find(|font| font.resource_name() == font_name)
    .unwrap_or(BaseFont::Helvetica);
    let colour = tokens
        .iter()
        .position(|token| *token == "rg")
        .and_then(|rg| {
            let part = |back: usize| tokens.get(rg.checked_sub(back)?)?.parse::<f64>().ok();
            Some(Color::new(part(3)?, part(2)?, part(1)?))
        })
        .unwrap_or(Color::BLACK);
    Some(TextStyle::new(font, size, colour))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_appearance_reads_back_as_the_style_that_wrote_it() {
        let style = TextStyle::new(BaseFont::Courier, 14.0, Color::new(0.5, 0.25, 1.0));
        assert_eq!(
            parse_default_appearance(&style.default_appearance()),
            Some(style)
        );
        assert_eq!(
            parse_default_appearance("/Unknown 9 Tf"),
            Some(TextStyle::new(BaseFont::Helvetica, 9.0, Color::BLACK))
        );
        assert_eq!(parse_default_appearance("0 g"), None);
    }
}
