//! A page's `/Contents` as a list of streams, and the guard streams that
//! keep a drawn-over mark out of the page's leftover graphics state.

use onionskin_cos::{Dict, Name, ObjRef, Object, Stream};

use super::super::rewrite::resolve;
use crate::edit::Transaction;
use crate::Result;

/// The `/OnionskinMark` value of the two guard streams.
const GUARD: &str = "Guard";

/// The page's content streams, each as the reference (or value) the page
/// holds: `/Contents` may be one stream, an array, or missing.
pub(in crate::pages) fn parts(
    tx: &Transaction<'_>,
    contents: Option<&Object>,
) -> Result<Vec<Object>> {
    Ok(match contents {
        None => Vec::new(),
        Some(Object::Array(items)) => items.clone(),
        Some(reference @ Object::Ref(_)) => match resolve(tx, Some(reference))? {
            Some(Object::Array(items)) => items,
            Some(_) => vec![reference.clone()],
            None => Vec::new(),
        },
        Some(other) => vec![other.clone()],
    })
}

/// A new content stream object marked `marker`, holding `content`.
pub(super) fn stream(tx: &mut Transaction<'_>, marker: &str, content: Vec<u8>) -> Result<Object> {
    let mut dict = Dict::new();
    dict.set(Name::new("Length"), Object::Integer(content.len() as i64));
    dict.set(Name::new("OnionskinMark"), Object::name(marker));
    let number = tx.reserve();
    tx.put_object(number, 0, Object::Stream(Stream { dict, raw: content }))?;
    Ok(Object::Ref(ObjRef::new(number, 0)))
}

/// Put `draw` behind everything, or after the page's content, guarding
/// that content first if it is not guarded yet.
pub(in crate::pages) fn insert(
    tx: &mut Transaction<'_>,
    parts: &mut Vec<Object>,
    draw: Object,
    behind: bool,
) -> Result<()> {
    if behind {
        parts.insert(0, draw);
        return Ok(());
    }
    if !is_guarded(tx, parts)? {
        let (open, close) = (
            stream(tx, GUARD, b"q\n".to_vec())?,
            stream(tx, GUARD, b"\nQ\n".to_vec())?,
        );
        let first_own = leading_backgrounds(tx, parts)?;
        parts.insert(first_own, open);
        parts.push(close);
    }
    parts.push(draw);
    Ok(())
}

/// Whether the page already has its guard pair.
fn is_guarded(tx: &Transaction<'_>, parts: &[Object]) -> Result<bool> {
    for part in parts {
        if marker(tx, part)?.as_deref() == Some(GUARD) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// How many backgrounds lead the list: the page's own content, and so the
/// opening guard, starts after them.
fn leading_backgrounds(tx: &Transaction<'_>, parts: &[Object]) -> Result<usize> {
    let mut count = 0;
    for part in parts {
        if marker(tx, part)?.as_deref() != Some("Background") {
            break;
        }
        count += 1;
    }
    Ok(count)
}

/// Remove the guards, once nothing is drawn over the page any more.
pub(super) fn drop_guards(tx: &Transaction<'_>, parts: &mut Vec<Object>) -> Result<()> {
    let mut kept = Vec::with_capacity(parts.len());
    for part in parts.drain(..) {
        if marker(tx, &part)?.as_deref() != Some(GUARD) {
            kept.push(part);
        }
    }
    *parts = kept;
    Ok(())
}

/// The `/OnionskinMark` a content stream carries, if it is one of ours.
pub(super) fn marker(tx: &Transaction<'_>, part: &Object) -> Result<Option<String>> {
    Ok(match resolve(tx, Some(part))? {
        Some(Object::Stream(stream)) => stream
            .dict
            .get(b"OnionskinMark")
            .and_then(Object::as_name)
            .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned()),
        _ => None,
    })
}

/// The stream a part names, if it is one.
pub(super) fn resolved_stream(tx: &Transaction<'_>, part: &Object) -> Result<Option<Stream>> {
    Ok(match resolve(tx, Some(part))? {
        Some(Object::Stream(stream)) => Some(stream),
        _ => None,
    })
}
