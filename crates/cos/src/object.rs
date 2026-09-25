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

/// Where a reference was found. The trailer is a holder of its own because it
/// is the one thing a section emits that is not one of its objects, and it can
/// dangle on its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Holder {
    Trailer,
    Object(u32),
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Holder::Trailer => write!(f, "the trailer"),
            Holder::Object(number) => write!(f, "object {number}"),
        }
    }
}

/// A half-open byte range `[start, end)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
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

/// A stream whose data boundary the parser had to find for itself, because
/// `/Length` did not say where it was.
///
/// Recovering is the right behaviour: a wrong `/Length` is common, every real
/// reader scans for `endstream` instead, and refusing would reject files that
/// open everywhere else. But the boundary that comes out is a guess, and a
/// caller that has to be able to prove where a stream ended has to be able to
/// see that it is one. `Parsed::recovered_boundary` is where it sees that.
/// The three ways it fails are kept apart, because they are not equally bad:
/// a value that disagrees with the bytes is a producer bug, an unresolvable
/// reference means the recovery also depended on the cross-reference being
/// wrong, and no value at all is a stream nobody described.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoveredBoundary {
    /// `/Length` gave a number and it was wrong: negative, or `endstream` was
    /// not at the offset it pointed to. The keyword was searched for instead.
    LengthWrong {
        declared: i64,
        /// The length the search found.
        actual: u64,
    },
    /// `/Length` was an indirect reference that did not resolve, so there was
    /// no declared value to check against.
    LengthUnresolved { reference: ObjRef, actual: u64 },
    /// The dictionary had no `/Length`, or one that was neither an integer nor
    /// a reference.
    LengthMissing { actual: u64 },
}

impl RecoveredBoundary {
    /// How many bytes of stream data the parser settled on. For an object
    /// inside an object stream this is the container's data, not the object's:
    /// the note describes whose boundary was guessed, and `Origin` says who
    /// that is.
    pub fn actual(&self) -> u64 {
        match self {
            RecoveredBoundary::LengthWrong { actual, .. } => *actual,
            RecoveredBoundary::LengthUnresolved { actual, .. } => *actual,
            RecoveredBoundary::LengthMissing { actual } => *actual,
        }
    }
}

impl fmt::Display for RecoveredBoundary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveredBoundary::LengthWrong { declared, actual } => write!(
                f,
                "/Length said {declared} bytes; endstream was {actual} bytes in"
            ),
            RecoveredBoundary::LengthUnresolved { reference, actual } => write!(
                f,
                "/Length {} {} R did not resolve; endstream was {actual} bytes in",
                reference.number, reference.generation
            ),
            RecoveredBoundary::LengthMissing { actual } => {
                write!(f, "no /Length; endstream was {actual} bytes in")
            }
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

    /// Every entry, with its value open for change. The keys are not: a
    /// dictionary whose keys could be renamed in place could grow a duplicate,
    /// which `set` exists to prevent.
    pub fn values_mut(&mut self) -> impl Iterator<Item = (&Name, &mut Object)> {
        self.0.iter_mut().map(|(key, value)| (&*key, value))
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

/// A page reached through [`crate::Document::page`], with the four attributes
/// ISO 32000-2 7.7.3.4 makes inheritable already resolved against the page's
/// ancestors.
///
/// Structural only, with one deliberate exception. Nothing here is normalised
/// or defaulted: an attribute the page and its ancestors never gave is `None`
/// rather than a substituted value, so a caller can tell "the file says
/// nothing" from "the file says US Letter", and the rectangles keep the
/// coordinate order the file wrote, which is not necessarily lower-left first.
/// Turning any of that into a page's geometry is `onionskin-content`'s job.
///
/// The exception is the one policy call the inheritance rule cannot avoid: a
/// rectangle entry counts only when it resolves to four finite numbers
/// enclosing a positive area. A `/MediaBox [0 0 0 0]` is not a smaller page,
/// it is a producer bug, and treating it as a value would shadow the usable
/// one an ancestor gave. The cost is that a caller cannot see that the page
/// declared a degenerate box at all; only that the effective one came from
/// higher up.
#[derive(Clone, Debug, PartialEq)]
pub struct PageNode {
    pub objref: ObjRef,
    pub dict: Dict,
    pub resources: Option<Dict>,
    pub media_box: Option<[f64; 4]>,
    pub crop_box: Option<[f64; 4]>,
    /// `/Rotate` exactly as the file gave it, in degrees. Not reduced to the
    /// four right angles, and not clamped.
    pub rotate: Option<i64>,
}

/// A parsed indirect object with its provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub objref: ObjRef,
    pub object: Object,
    pub origin: Origin,
    /// Set when this object is a stream whose data boundary was recovered
    /// rather than read from `/Length`, and for an object inside an object
    /// stream, when its container's was. `None` means the boundary is the
    /// file's own word, not the parser's.
    pub recovered_boundary: Option<RecoveredBoundary>,
}
