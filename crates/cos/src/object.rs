//! The COS object model, plus the byte-span provenance every parsed object
//! carries (decision 7).

use std::fmt;

/// An indirect reference, `number generation R`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ObjRef {
    pub number: u32,
    pub generation: u16,
}

impl ObjRef {
    pub fn new(number: u32, generation: u16) -> Self {
        ObjRef { number, generation }
    }
}

/// A half-open byte range `[start, end)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    pub fn new(start: u64, end: u64) -> Self {
        Span { start, end }
    }
}

/// Where a parsed object's bytes live. An object inside an object stream has
/// no span of its own in the file, so it reports both the container's span and
/// its own span inside the container's decoded data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    File(Span),
    ObjectStream {
        container: u32,
        container_span: Span,
        within: Span,
    },
    /// An edit that has not been saved, so it has no bytes yet.
    Pending,
}

impl Origin {
    /// The byte range in the file that must be preserved for this object to
    /// survive: its own bytes, or its container's. `None` for an edit that has
    /// not been saved, which is not the same thing as a zero-length span at
    /// offset zero.
    pub fn file_span(&self) -> Option<Span> {
        match self {
            Origin::File(s) => Some(*s),
            Origin::ObjectStream { container_span, .. } => Some(*container_span),
            Origin::Pending => None,
        }
    }
}

/// A PDF name with `#xx` escapes already decoded.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Name(pub Vec<u8>);

impl Name {
    pub fn new(text: &str) -> Self {
        Name(text.as_bytes().to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Name::new(s)
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}", String::from_utf8_lossy(&self.0))
    }
}

/// A dictionary. Insertion order is kept so a re-serialized dictionary reads
/// the way its source did.
#[derive(Clone, Default, PartialEq)]
pub struct Dict(Vec<(Name, Object)>);

impl Dict {
    pub fn new() -> Self {
        Dict(Vec::new())
    }

    pub fn get(&self, key: &[u8]) -> Option<&Object> {
        self.0.iter().find(|(k, _)| k.0 == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: impl Into<Name>, value: Object) {
        let key = key.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<Object> {
        let idx = self.0.iter().position(|(k, _)| k.0 == key)?;
        Some(self.0.remove(idx).1)
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        self.get(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(Name, Object)> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Dict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.0.iter().map(|(k, v)| (k, v)))
            .finish()
    }
}

/// A stream: its dictionary plus its raw, still-encoded bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub dict: Dict,
    pub raw: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Null,
    Bool(bool),
    Integer(i64),
    Real(f64),
    /// Literal or hex string, decoded to raw bytes.
    String(Vec<u8>),
    Name(Name),
    Array(Vec<Object>),
    Dict(Dict),
    Stream(Stream),
    Ref(ObjRef),
}

impl Object {
    pub fn name(text: &str) -> Object {
        Object::Name(Name::new(text))
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Object::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Object::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&Name> {
        match self {
            Object::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_stream(&self) -> Option<&Stream> {
        match self {
            Object::Stream(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_reference(&self) -> Option<ObjRef> {
        match self {
            Object::Ref(r) => Some(*r),
            _ => None,
        }
    }
}

/// A parsed indirect object with its provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub objref: ObjRef,
    pub object: Object,
    pub origin: Origin,
}
