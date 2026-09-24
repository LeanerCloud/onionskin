//! Redaction's half in `content`: a page's content streams rewritten with
//! everything inside the redaction areas taken out.
//!
//! The same interpreter that extracts text walks the page, so a glyph is
//! removed exactly where extraction and search say it is. What goes:
//!
//! - **glyphs** at least a fifth covered by an area. The operator that drew
//!   them is rewritten to step over them, so the rest of the line stays put;
//! - **paths** wholly inside an area;
//! - **inline images** touching an area;
//! - **image XObjects** touching an area are renamed, so the caller can put
//!   a copy with the area painted out in their place ([`NewResource::Image`]);
//! - **form XObjects**, and soft-mask groups, whose own content changed are
//!   rewritten the same way and renamed ([`NewResource::Form`],
//!   [`NewResource::GState`]);
//! - an `/ActualText`, `/Alt` or `/E` on a marked-content sequence that lost
//!   a glyph, since it would otherwise still spell the text;
//! - everything drawn in a hidden layer, when the caller names the layers
//!   (removing hidden information).
//!
//! What is not done here is writing any of it: the caller owns the file.

mod geometry;
mod output;
mod paths;
pub(crate) mod text;

use onionskin_cos::{Dict, Name, ObjRef};

pub use geometry::{covers_glyph, touches, Area};
pub(crate) use output::{named, taken_names, Emit, Output};
pub(crate) use paths::{is_clip, is_construction, is_painting, PathBuffer};

use crate::error::Warning;
use crate::matrix::Matrix;
use crate::{PageIndex, PageQuad};

/// A content stream after redaction.
#[derive(Debug, Clone, PartialEq)]
pub struct Rewritten {
    pub bytes: Vec<u8>,
    /// Whether anything was removed or renamed. An unchanged stream need not
    /// be written.
    pub changed: bool,
    /// The resources the new bytes name that the original resources do not
    /// hold: the caller adds them to a copy of those resources.
    pub resources: Vec<NewResource>,
}

/// A resource a rewritten stream names in place of one it used to.
#[derive(Debug, Clone, PartialEq)]
pub enum NewResource {
    /// An `/XObject`: a form whose content was rewritten.
    Form {
        name: Name,
        original: ObjRef,
        content: Rewritten,
    },
    /// An `/XObject`: an image touching an area, drawn with `placement`
    /// taking its unit square onto the page. The caller writes a copy with
    /// the areas painted out, or an empty form when it cannot.
    Image {
        name: Name,
        original: ObjRef,
        placement: Matrix,
    },
    /// An `/ExtGState` whose soft mask's group was rewritten. `original` is
    /// the graphics state dictionary, resolved.
    GState {
        name: Name,
        original: Dict,
        group: ObjRef,
        content: Rewritten,
    },
}

/// One glyph taken out, for the verifier and the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
    /// What it spelled; empty for a glyph with no Unicode mapping.
    pub text: String,
    pub quad: PageQuad,
}

/// How much went.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub glyphs: usize,
    pub paths: usize,
    pub inline_images: usize,
    pub images: usize,
    pub forms: usize,
    /// Showing operators with no font, removed because they start inside an
    /// area: where their glyphs fall is unknown.
    pub fontless: usize,
    /// Glyphs and painting operators removed because they were in a hidden
    /// layer.
    pub hidden: usize,
}

/// A page's redaction.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRedaction {
    pub page: PageIndex,
    /// The page's content streams, as one.
    pub content: Rewritten,
    pub removed: Vec<Removed>,
    pub counts: Counts,
    /// What the interpreter could not honour exactly. A glyph limit here
    /// means some glyphs were never looked at, and the caller must refuse.
    pub warnings: Vec<Warning>,
}

/// What the interpreter keeps while it redacts.
pub(crate) struct Redacting {
    pub(crate) areas: Vec<Area>,
    /// Optional content groups whose content goes wherever it is.
    pub(crate) hidden: Vec<ObjRef>,
    /// How many open marked-content sequences are in a hidden layer.
    pub(crate) hiding: usize,
    pub(crate) removed: Vec<Removed>,
    pub(crate) counts: Counts,
    pub(crate) names: u32,
}

impl Redacting {
    pub(crate) fn new(areas: &[Area]) -> Redacting {
        Redacting {
            areas: areas.to_vec(),
            hidden: Vec::new(),
            hiding: 0,
            removed: Vec::new(),
            counts: Counts::default(),
            names: 0,
        }
    }
}

/// The keys a marked-content property list can spell text with.
pub(crate) const SPELLING_KEYS: [&[u8]; 3] = [b"ActualText", b"Alt", b"E"];

/// A `BDC` with its spelling keys taken out, or `None` when it has none.
pub(crate) fn unspelled(tag: &Name, properties: &Dict) -> Option<Vec<u8>> {
    if !SPELLING_KEYS.iter().any(|key| properties.contains(key)) {
        return None;
    }
    let mut kept = Dict::new();
    for (key, value) in properties.iter() {
        if !SPELLING_KEYS.contains(&key.as_bytes()) {
            kept.set(key.clone(), value.clone());
        }
    }
    let mut out = onionskin_cos::object_bytes(&onionskin_cos::Object::Name(tag.clone())).ok()?;
    out.push(b' ');
    out.extend(onionskin_cos::object_bytes(&onionskin_cos::Object::Dict(kept)).ok()?);
    out.extend_from_slice(b" BDC");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_cos::Object;

    #[test]
    fn a_property_list_loses_only_what_spells() {
        let mut properties = Dict::new();
        properties.set("MCID", Object::Integer(3));
        properties.set("ActualText", Object::String(b"secret".to_vec()));
        properties.set("Alt", Object::String(b"secret".to_vec()));
        let bytes = unspelled(&Name::new("Span"), &properties).expect("rewritten");
        assert_eq!(bytes, b"/Span <</MCID 3>> BDC");
        let mut plain = Dict::new();
        plain.set("MCID", Object::Integer(3));
        assert_eq!(unspelled(&Name::new("Span"), &plain), None);
    }
}
