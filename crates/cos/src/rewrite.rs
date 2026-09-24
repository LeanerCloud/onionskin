//! A whole document written afresh, which is how its security changes.

use std::collections::BTreeMap;

use onionskin_crypto::Protection;

use crate::encrypt::Random;
use crate::error::Result;
use crate::object::{Dict, ObjRef, Object};

/// Every object `trailer`'s `/Root` and `/Info` reach through `lookup`, and
/// the trailer that names them: encrypted under `protection`, with its
/// `/Encrypt` dictionary as a new object, when there is one.
pub(crate) fn whole(
    trailer: &Dict,
    lookup: &dyn Fn(u32) -> Option<(ObjRef, Object)>,
    protection: Option<&Protection>,
    random: Random<'_>,
) -> Result<(Vec<(ObjRef, Object)>, Dict)> {
    let mut out = Dict::new();
    for key in ["Root", "Info"] {
        if let Some(value) = trailer.get(key.as_bytes()) {
            out.set(key, value.clone());
        }
    }
    let mut objects = reached(&out, lookup);
    let present: BTreeMap<u32, u16> = objects
        .iter()
        .map(|(number, (objref, _))| (*number, objref.generation))
        .collect();
    for (_, object) in objects.values_mut() {
        null_dangling(object, &present);
    }
    null_dangling_in_dict(&mut out, &present);
    let mut objects: Vec<(ObjRef, Object)> = objects.into_values().collect();

    let Some(protection) = protection else {
        if let Some(id) = trailer.get(b"ID") {
            out.set("ID", id.clone());
        }
        return Ok((objects, out));
    };
    let first = match trailer.get(b"ID") {
        Some(Object::Array(ids)) => match ids.first() {
            Some(Object::String(first)) => first.clone(),
            _ => fresh_id(random),
        },
        _ => fresh_id(random),
    };
    let (dict, handler) =
        onionskin_crypto::protect(protection, &first, random).map_err(|error| {
            crate::error::Error::Unrecoverable {
                detail: format!("the document could not be encrypted: {error}"),
            }
        })?;
    for (objref, object) in &mut objects {
        crate::encrypt::object(&handler, objref.number, objref.generation, object, random);
    }
    let number = objects.last().map_or(1, |(objref, _)| objref.number + 1);
    let encrypt = ObjRef::new(number, 0);
    objects.push((encrypt, Object::Dict(crate::encrypt::write_dict(&dict))));
    out.set("Encrypt", Object::Ref(encrypt));
    out.set(
        "ID",
        Object::Array(vec![
            Object::String(first),
            Object::String(fresh_id(random)),
        ]),
    );
    Ok((objects, out))
}

fn fresh_id(random: Random<'_>) -> Vec<u8> {
    let mut id = vec![0u8; 16];
    random(&mut id);
    id
}

/// The objects `roots` reach, by number.
fn reached(
    roots: &Dict,
    lookup: &dyn Fn(u32) -> Option<(ObjRef, Object)>,
) -> BTreeMap<u32, (ObjRef, Object)> {
    let mut found = BTreeMap::new();
    let mut queue: Vec<Object> = roots.iter().map(|(_, value)| value.clone()).collect();
    while let Some(object) = queue.pop() {
        match object {
            Object::Ref(objref) if !found.contains_key(&objref.number) => {
                if let Some((objref, object)) = lookup(objref.number) {
                    queue.push(object.clone());
                    found.insert(objref.number, (objref, object));
                }
            }
            Object::Array(items) => queue.extend(items),
            Object::Dict(dict) => queue.extend(dict.iter().map(|(_, value)| value.clone())),
            Object::Stream(stream) => {
                queue.extend(stream.dict.iter().map(|(_, value)| value.clone()))
            }
            _ => {}
        }
    }
    found
}

/// A reference to an object that is not being written made null, as ISO
/// 32000-1 7.3.10 reads it anyway, and one naming the wrong generation of
/// an object that is, named right: readers go by the number.
fn null_dangling(object: &mut Object, present: &BTreeMap<u32, u16>) {
    match object {
        Object::Ref(objref) => match present.get(&objref.number) {
            Some(generation) => objref.generation = *generation,
            None => *object = Object::Null,
        },
        Object::Array(items) => {
            for item in items {
                null_dangling(item, present);
            }
        }
        Object::Dict(dict) => null_dangling_in_dict(dict, present),
        Object::Stream(stream) => null_dangling_in_dict(&mut stream.dict, present),
        _ => {}
    }
}

fn null_dangling_in_dict(dict: &mut Dict, present: &BTreeMap<u32, u16>) {
    for (_, value) in dict.values_mut() {
        null_dangling(value, present);
    }
}
