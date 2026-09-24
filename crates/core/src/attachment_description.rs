//! Edit Description: an attachment's `/Desc`, written into every file
//! specification that embeds its stream, whether the specification is its
//! own object or written inline in the name tree or a comment.

use onionskin_cos::{Dict, Name, Object};

use crate::annots::text_string;
use crate::edit::Transaction;
use crate::embedded::{embeds, read_entries, rewrite_attachments};
use crate::pages::{dict_at, page_ref, resolve};
use crate::Result;

/// Set the description of the attachment whose stream is object `stream`,
/// or remove it when `description` is blank. Returns how many
/// specifications were changed; zero changes nothing.
pub fn set_attachment_description(
    tx: &mut Transaction<'_>,
    stream: u32,
    description: &str,
) -> Result<usize> {
    let description = description.trim();
    let mut changed = in_name_tree(tx, stream, description)?;
    changed += in_comments(tx, stream, description)?;
    Ok(changed)
}

fn describe(spec: &mut Dict, description: &str) {
    if description.is_empty() {
        spec.remove(b"Desc");
    } else {
        spec.set(Name::new("Desc"), text_string(description));
    }
}

/// Change `spec` where it lives: in its own object, or, for one written
/// inline, return the changed dictionary for the caller to write back.
fn change_spec(
    tx: &mut Transaction<'_>,
    spec: &Object,
    description: &str,
) -> Result<Option<Object>> {
    match spec {
        Object::Ref(at) => {
            let mut dict = dict_at(tx, *at)?;
            describe(&mut dict, description);
            tx.put_object(at.number, at.generation, Object::Dict(dict))?;
            Ok(None)
        }
        Object::Dict(dict) => {
            let mut dict = dict.clone();
            describe(&mut dict, description);
            Ok(Some(Object::Dict(dict)))
        }
        _ => Ok(None),
    }
}

fn in_name_tree(tx: &mut Transaction<'_>, stream: u32, description: &str) -> Result<usize> {
    let mut entries = Vec::new();
    read_entries(tx, &mut entries)?;
    let mut changed = 0;
    let mut inline = false;
    for entry in &mut entries {
        if !embeds(tx, &entry.1, stream)? {
            continue;
        }
        changed += 1;
        if let Some(replaced) = change_spec(tx, &entry.1, description)? {
            entry.1 = replaced;
            inline = true;
        }
    }
    if inline {
        rewrite_attachments(tx, |current| {
            *current = entries;
            Ok(())
        })?;
    }
    Ok(changed)
}

fn in_comments(tx: &mut Transaction<'_>, stream: u32, description: &str) -> Result<usize> {
    let mut changed = 0;
    for index in 0..crate::pages::page_count(tx)? {
        let page = page_ref(tx, index)?;
        let annots = match resolve(tx, dict_at(tx, page)?.get(b"Annots"))? {
            Some(Object::Array(annots)) => annots,
            _ => continue,
        };
        for annotation in annots.iter().filter_map(Object::as_reference) {
            let Ok(mut dict) = dict_at(tx, annotation) else {
                continue;
            };
            let is_attachment = dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_some_and(|name| name.as_bytes() == b"FileAttachment");
            let Some(spec) = dict.get(b"FS").cloned().filter(|_| is_attachment) else {
                continue;
            };
            if !embeds(tx, &spec, stream)? {
                continue;
            }
            changed += 1;
            if let Some(replaced) = change_spec(tx, &spec, description)? {
                dict.set(Name::new("FS"), replaced);
                tx.put_object(annotation.number, annotation.generation, Object::Dict(dict))?;
            }
        }
    }
    Ok(changed)
}
