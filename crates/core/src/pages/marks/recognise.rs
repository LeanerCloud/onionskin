//! Which of a page's content streams are marks of a kind, ours or
//! Acrobat's, and dropping them.

use std::collections::BTreeSet;

use onionskin_content::Tokenizer;
use onionskin_cos::{Dict, Name, Object};

use super::super::ops::leaves;
use super::super::rewrite::resolve;
use super::contents::{self, resolved_stream};
use super::MarkKind;
use crate::edit::Transaction;
use crate::{Error, PageIndex, Result};

/// Operators a mark's own content stream may hold besides `Do`: state, and
/// the marked-content brackets.
const QUIET: [&[u8]; 6] = [b"q", b"Q", b"cm", b"gs", b"BDC", b"EMC"];

/// The kinds of mark `page` carries.
pub fn page_marks(tx: &Transaction<'_>, page: PageIndex) -> Result<Vec<MarkKind>> {
    let leaves = leaves(tx)?;
    let leaf = leaves.get(page).ok_or(Error::NoSuchPage {
        page,
        count: leaves.len(),
    })?;
    kinds_on(tx, leaf)
}

/// The settings the first page with a mark of `kind` keeps for it, if it
/// was made with any.
pub fn mark_settings(tx: &Transaction<'_>, kind: MarkKind) -> Result<Option<Vec<u8>>> {
    for leaf in leaves(tx)? {
        let dict = super::super::rewrite::dict_at(tx, leaf.objref)?;
        let resources = super::page_resources(tx, leaf.inherited.resources.as_ref())?;
        for part in contents::parts(tx, dict.get(b"Contents"))? {
            let Some(names) = is_mark(tx, kind, &part, &resources)? else {
                continue;
            };
            for name in names {
                if let Some(Object::Stream(form)) = form(tx, &resources, &name)? {
                    if let Some(Object::String(settings)) = form.dict.get(b"OnionskinSettings") {
                        return Ok(Some(settings.clone()));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Form `name` of `resources`, resolved.
fn form(tx: &Transaction<'_>, resources: &Dict, name: &str) -> Result<Option<Object>> {
    let Some(Object::Dict(forms)) = resolve(tx, resources.get(b"XObject"))? else {
        return Ok(None);
    };
    resolve(tx, forms.get(name.as_bytes()))
}

/// The pages that carry a mark of `kind`, in order.
pub fn marked_pages(tx: &Transaction<'_>, kind: MarkKind) -> Result<Vec<PageIndex>> {
    let mut marked = Vec::new();
    for (index, leaf) in leaves(tx)?.iter().enumerate() {
        if kinds_on(tx, leaf)?.contains(&kind) {
            marked.push(index);
        }
    }
    Ok(marked)
}

fn kinds_on(tx: &Transaction<'_>, leaf: &super::super::inherit::Leaf) -> Result<Vec<MarkKind>> {
    let dict = super::super::rewrite::dict_at(tx, leaf.objref)?;
    let resources = super::page_resources(tx, leaf.inherited.resources.as_ref())?;
    let parts = contents::parts(tx, dict.get(b"Contents"))?;
    let mut kinds = BTreeSet::new();
    for part in &parts {
        for kind in MarkKind::ALL {
            if is_mark(tx, kind, part, &resources)?.is_some() {
                kinds.insert(kind);
            }
        }
    }
    Ok(kinds.into_iter().collect())
}

/// Drop `kind`'s marks from `parts`, and the form names only they drew from
/// `resources`. Whether there were any.
pub(super) fn drop_marks(
    tx: &Transaction<'_>,
    kind: MarkKind,
    parts: &mut Vec<Object>,
    resources: &mut Dict,
) -> Result<bool> {
    let before = parts.len();
    let mut kept = Vec::with_capacity(before);
    let mut unused = BTreeSet::new();
    for part in parts.drain(..) {
        match is_mark(tx, kind, &part, resources)? {
            Some(names) => unused.extend(names),
            None => kept.push(part),
        }
    }
    let found = kept.len() < before;
    *parts = kept;
    let still_used = forms_drawn(tx, parts)?;
    if let Some(Object::Dict(forms)) = resolve(tx, resources.get(b"XObject"))? {
        let mut forms = forms;
        for name in unused.difference(&still_used) {
            forms.remove(name.as_bytes());
        }
        resources.set(Name::new("XObject"), Object::Dict(forms));
    }
    if !drawn_over(tx, parts, resources)? {
        contents::drop_guards(tx, parts)?;
    }
    Ok(found)
}

/// Whether anything but a background is still drawn over the page.
fn drawn_over(tx: &Transaction<'_>, parts: &[Object], resources: &Dict) -> Result<bool> {
    for part in parts {
        for kind in [MarkKind::Watermark, MarkKind::HeaderFooter, MarkKind::Bates] {
            if is_mark(tx, kind, part, resources)?.is_some() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The forms `parts` draw, by name.
fn forms_drawn(tx: &Transaction<'_>, parts: &[Object]) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for part in parts {
        if let Some(operations) = operations(tx, part)? {
            names.extend(
                operations
                    .into_iter()
                    .filter_map(|(operator, name)| (operator == b"Do").then_some(name).flatten()),
            );
        }
    }
    Ok(names)
}

/// If `part` is a mark of `kind`, the form names it draws.
///
/// Ours say so in `/OnionskinMark`. Acrobat's are a stream that only draws
/// forms whose `/PieceInfo` names the kind, inside marked content.
fn is_mark(
    tx: &Transaction<'_>,
    kind: MarkKind,
    part: &Object,
    resources: &Dict,
) -> Result<Option<Vec<String>>> {
    let marker = contents::marker(tx, part)?;
    let Some(operations) = operations(tx, part)? else {
        return Ok(None);
    };
    let drawn: Vec<String> = operations
        .iter()
        .filter(|(operator, _)| operator == b"Do")
        .filter_map(|(_, name)| name.clone())
        .collect();
    if let Some(marker) = marker {
        return Ok((marker == kind.marker()).then_some(drawn));
    }
    let quiet = operations
        .iter()
        .all(|(operator, _)| operator == b"Do" || QUIET.contains(&operator.as_slice()));
    if !quiet || drawn.is_empty() {
        return Ok(None);
    }
    for name in &drawn {
        if form_private(tx, resources, name)?.as_deref() != Some(kind.private()) {
            return Ok(None);
        }
    }
    // Acrobat writes Bates numbers as a header or footer; without our marker
    // they are that.
    Ok((kind != MarkKind::Bates).then_some(drawn))
}

/// One operator, with the form a `Do` names.
type Step = (Vec<u8>, Option<String>);

/// A content stream's operators, each with the name operand a `Do` takes.
/// `None` for something that is not a stream.
fn operations(tx: &Transaction<'_>, part: &Object) -> Result<Option<Vec<Step>>> {
    let Some(stream) = resolved_stream(tx, part)? else {
        return Ok(None);
    };
    let data = tx.base().decode_stream(&stream)?;
    let mut tokenizer = Tokenizer::new(&data);
    let mut out = Vec::new();
    while let Some(operation) = tokenizer.next_operation() {
        let name = operation
            .operator
            .is(b"Do")
            .then(|| operation.operands.last().and_then(Object::as_name))
            .flatten()
            .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned());
        out.push((operation.operator.as_bytes().to_vec(), name));
    }
    Ok(Some(out))
}

/// The `/PieceInfo /ADBE_CompoundType /Private` name of form `name`.
fn form_private(tx: &Transaction<'_>, resources: &Dict, name: &str) -> Result<Option<String>> {
    let Some(Object::Stream(form)) = form(tx, resources, name)? else {
        return Ok(None);
    };
    let piece = resolve(tx, form.dict.get(b"PieceInfo"))?;
    let compound = match piece {
        Some(Object::Dict(piece)) => resolve(tx, piece.get(b"ADBE_CompoundType"))?,
        _ => None,
    };
    Ok(match compound {
        Some(Object::Dict(compound)) => compound
            .get(b"Private")
            .and_then(Object::as_name)
            .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned()),
        _ => None,
    })
}
