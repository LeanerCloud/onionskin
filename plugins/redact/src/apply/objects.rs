//! The new file's objects: every object the document reaches, changed in
//! place, added to, and written with nothing it no longer reaches.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use crate::RedactError;

pub(crate) struct Objects {
    map: BTreeMap<u32, (u16, Object)>,
    next: u32,
    trailer: Dict,
}

impl Objects {
    /// Every object `source` reaches from its trailer.
    pub(crate) fn from_source(source: &CosDocument) -> Result<Objects, RedactError> {
        let mut map = BTreeMap::new();
        for number in source.reachable_from_trailer() {
            let parsed = source.get(number)?;
            map.insert(number, (parsed.objref.generation, parsed.object));
        }
        let next = map.keys().next_back().copied().unwrap_or(0) + 1;
        let mut trailer = Dict::new();
        for key in ["Root", "Info", "ID"] {
            if let Some(value) = source.trailer().get(key.as_bytes()) {
                trailer.set(Name::new(key), value.clone());
            }
        }
        Ok(Objects { map, next, trailer })
    }

    pub(crate) fn get(&self, number: u32) -> Option<&Object> {
        self.map.get(&number).map(|(_, object)| object)
    }

    /// `object`, followed through references.
    pub(crate) fn resolve(&self, object: &Object) -> Object {
        let mut current = object.clone();
        for _ in 0..32 {
            match current {
                Object::Ref(objref) => {
                    current = self.get(objref.number).cloned().unwrap_or(Object::Null);
                }
                other => return other,
            }
        }
        Object::Null
    }

    /// The dictionary `object` is or refers to, or an empty one.
    pub(crate) fn dict(&self, object: Option<&Object>) -> Dict {
        object
            .map(|object| self.resolve(object))
            .and_then(|object| object.as_dict().cloned())
            .unwrap_or_default()
    }

    /// Replaces object `number`, keeping its generation.
    pub(crate) fn set(&mut self, number: u32, object: Object) {
        let generation = self
            .map
            .get(&number)
            .map_or(0, |(generation, _)| *generation);
        self.map.insert(number, (generation, object));
    }

    pub(crate) fn add(&mut self, object: Object) -> ObjRef {
        let number = self.next;
        self.next += 1;
        self.map.insert(number, (0, object));
        ObjRef::new(number, 0)
    }

    pub(crate) fn trailer_mut(&mut self) -> &mut Dict {
        &mut self.trailer
    }

    pub(crate) fn root(&self) -> Dict {
        self.dict(self.trailer.get(b"Root"))
    }

    /// Every object number the trailer reaches now.
    pub(crate) fn reachable(&self) -> BTreeSet<u32> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<Object> = self
            .trailer
            .iter()
            .map(|(_, value)| value.clone())
            .collect();
        while let Some(object) = stack.pop() {
            match object {
                Object::Ref(objref) if seen.insert(objref.number) => {
                    if let Some(target) = self.get(objref.number) {
                        stack.push(target.clone());
                    }
                }
                Object::Array(items) => stack.extend(items),
                Object::Dict(dict) => stack.extend(dict.iter().map(|(_, value)| value.clone())),
                Object::Stream(stream) => {
                    stack.extend(stream.dict.iter().map(|(_, value)| value.clone()))
                }
                _ => {}
            }
        }
        seen
    }

    /// The new file: what the trailer reaches, with a reference to an object
    /// the document does not have written as `null`.
    pub(crate) fn write(self) -> Result<Vec<u8>, RedactError> {
        let reached = self.reachable();
        let known: BTreeMap<u32, u16> = reached
            .iter()
            .filter_map(|number| {
                self.map
                    .get(number)
                    .map(|(generation, _)| (*number, *generation))
            })
            .collect();
        let objects: Vec<(ObjRef, Object)> = known
            .iter()
            .map(|(number, generation)| {
                let object = &self.map[number].1;
                (
                    ObjRef::new(*number, *generation),
                    without_dangling(object, &known),
                )
            })
            .collect();
        let trailer = match without_dangling(&Object::Dict(self.trailer), &known) {
            Object::Dict(trailer) => trailer,
            _ => Dict::new(),
        };
        Ok(CosDocument::write_new(&objects, trailer)?)
    }
}

fn without_dangling(object: &Object, known: &BTreeMap<u32, u16>) -> Object {
    match object {
        Object::Ref(objref) => match known.get(&objref.number) {
            Some(generation) => Object::Ref(ObjRef::new(objref.number, *generation)),
            None => Object::Null,
        },
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|item| without_dangling(item, known))
                .collect(),
        ),
        Object::Dict(dict) => Object::Dict(dict_without_dangling(dict, known)),
        Object::Stream(stream) => {
            let mut stream = stream.clone();
            stream.dict = dict_without_dangling(&stream.dict, known);
            Object::Stream(stream)
        }
        other => other.clone(),
    }
}

fn dict_without_dangling(dict: &Dict, known: &BTreeMap<u32, u16>) -> Dict {
    let mut out = Dict::new();
    for (key, value) in dict.iter() {
        out.set(key.clone(), without_dangling(value, known));
    }
    out
}
