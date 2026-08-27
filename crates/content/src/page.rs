//! Page lookup and content assembly.
//!
//! `cos` descends to the first page but has no indexed page accessor, so the
//! tree walk lives here. It is the same shape: only the nodes on the path to
//! the requested page are parsed, and `/Count` is used to skip whole subtrees
//! when it is consistent with what the walk finds.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Document, ObjRef, Object, Origin, Span};
use onionskin_plugin_api::PageIndex;

use crate::error::{Error, Result, Warning};
use crate::filter;

/// Depth cap for the page tree, matching the one `cos` applies to its own
/// descent. Deeper than this and the tree is either hostile or broken.
const MAX_DEPTH: usize = 64;

/// A page, with the four attributes ISO 32000-2 7.7.3.4 says are inheritable
/// already resolved against its ancestors.
#[derive(Clone, Debug)]
pub struct Page {
    pub index: PageIndex,
    pub objref: ObjRef,
    pub dict: Dict,
    pub resources: Dict,
    /// `[llx lly urx ury]`, normalised so the first pair is the lower left.
    pub media_box: [f64; 4],
    /// `/Rotate`, normalised to 0, 90, 180 or 270. Quads are in default user
    /// space, which is before rotation, so this is carried for the renderer
    /// rather than applied here.
    pub rotate: i32,
}

impl Page {
    /// The transform from the page's own coordinates into the default user
    /// space `plugin_api::PagePoint` names: origin at the media box's lower
    /// left corner, y up.
    pub fn base_ctm(&self) -> crate::Matrix {
        crate::Matrix::translate(-self.media_box[0], -self.media_box[1])
    }

    pub fn width(&self) -> f64 {
        self.media_box[2] - self.media_box[0]
    }

    pub fn height(&self) -> f64 {
        self.media_box[3] - self.media_box[1]
    }
}

/// Loads page `index`, parsing only the tree nodes on the path to it.
pub fn page(doc: &Document, index: PageIndex) -> Result<Page> {
    let catalog = doc.catalog()?;
    let root = catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .ok_or_else(|| Error::Structure("catalog has no indirect /Pages".into()))?;

    let mut seen = BTreeSet::new();
    let mut found = 0usize;
    let inherited = Inherited::default();
    match descend(doc, root, &inherited, index, &mut found, &mut seen, 0)? {
        Some(page) => Ok(page),
        None => Err(Error::NoSuchPage {
            index,
            count: found,
        }),
    }
}

/// Number of pages, taken from the page tree root's `/Count`.
pub fn page_count(doc: &Document) -> Result<usize> {
    let count = doc.page_count()?;
    Ok(count.max(0) as usize)
}

#[derive(Clone, Default)]
struct Inherited {
    resources: Option<Dict>,
    media_box: Option<[f64; 4]>,
    rotate: Option<i32>,
}

#[allow(clippy::too_many_arguments)]
fn descend(
    doc: &Document,
    node: ObjRef,
    inherited: &Inherited,
    target: PageIndex,
    found: &mut usize,
    seen: &mut BTreeSet<u32>,
    depth: usize,
) -> Result<Option<Page>> {
    if depth >= MAX_DEPTH || !seen.insert(node.number) {
        return Ok(None);
    }
    let parsed = doc.get(node.number)?;
    let Some(dict) = parsed.object.as_dict().cloned() else {
        return Ok(None);
    };

    let mut inherited = inherited.clone();
    if let Some(Object::Dict(d)) = resolved(doc, &dict, b"Resources") {
        inherited.resources = Some(d);
    }
    if let Some(rect) = rectangle(doc, &dict, b"MediaBox") {
        inherited.media_box = Some(rect);
    }
    if let Some(Object::Integer(r)) = resolved(doc, &dict, b"Rotate") {
        inherited.rotate = Some(r as i32);
    }

    let kids = match resolved(doc, &dict, b"Kids") {
        Some(Object::Array(kids)) => Some(kids),
        _ => None,
    };
    // A node with /Kids is internal even when it also claims /Type /Page, which
    // some producers do. A node without them is a leaf whatever it claims.
    let Some(kids) = kids else {
        let index = *found;
        *found += 1;
        if index != target {
            seen.remove(&node.number);
            return Ok(None);
        }
        return Ok(Some(Page {
            index,
            objref: parsed.objref,
            resources: inherited.resources.unwrap_or_default(),
            media_box: inherited.media_box.unwrap_or(US_LETTER),
            rotate: normalise_rotation(inherited.rotate.unwrap_or(0)),
            dict,
        }));
    };

    for kid in kids {
        let Some(kid) = kid.as_reference() else {
            continue;
        };
        // /Count lets a whole subtree be skipped without parsing it. Trusted
        // only when it is a plausible non-negative number; a lie here would
        // silently renumber every later page, so the walk falls through to a
        // real descent whenever it is absent or nonsensical.
        if let Some(count) = subtree_count(doc, kid) {
            if *found + count <= target {
                *found += count;
                continue;
            }
        }
        if let Some(page) = descend(doc, kid, &inherited, target, found, seen, depth + 1)? {
            return Ok(Some(page));
        }
    }
    seen.remove(&node.number);
    Ok(None)
}

fn subtree_count(doc: &Document, node: ObjRef) -> Option<usize> {
    let parsed = doc.get(node.number).ok()?;
    let dict = parsed.object.as_dict()?;
    if !dict.contains(b"Kids") {
        return None;
    }
    match resolved(doc, dict, b"Count") {
        Some(Object::Integer(c)) if c >= 0 => usize::try_from(c).ok(),
        _ => None,
    }
}

const US_LETTER: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

fn resolved(doc: &Document, dict: &Dict, key: &[u8]) -> Option<Object> {
    doc.resolve(dict.get(key)?).ok()
}

fn rectangle(doc: &Document, dict: &Dict, key: &[u8]) -> Option<[f64; 4]> {
    let Some(Object::Array(items)) = resolved(doc, dict, key) else {
        return None;
    };
    if items.len() < 4 {
        return None;
    }
    let mut v = [0.0f64; 4];
    for (slot, item) in v.iter_mut().zip(items.iter()) {
        *slot = crate::tokenizer::number(&doc.resolve(item).ok()?)?;
    }
    let rect = [
        v[0].min(v[2]),
        v[1].min(v[3]),
        v[0].max(v[2]),
        v[1].max(v[3]),
    ];
    // A zero-area box would put every glyph at the same point.
    if !rect.iter().all(|n| n.is_finite()) || rect[2] <= rect[0] || rect[3] <= rect[1] {
        return None;
    }
    Some(rect)
}

fn normalise_rotation(degrees: i32) -> i32 {
    let r = degrees % 360;
    let r = if r < 0 { r + 360 } else { r };
    // Only the four right angles are legal; anything else is a producer bug
    // and every reader treats it as no rotation.
    if r % 90 == 0 {
        r
    } else {
        0
    }
}

// ---- content assembly -------------------------------------------------------

/// One `/Contents` entry's contribution to the concatenated stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentPart {
    pub stream: ObjRef,
    /// Where this stream's bytes live in the file, straight from `cos`.
    pub origin: Origin,
    /// Half-open range of [`Content::bytes`] this part occupies.
    pub range: Span,
}

/// The page's content streams, concatenated the way ISO 32000-2 7.8.2 requires
/// (one whitespace byte between parts, so a token cannot straddle a join),
/// with each part's provenance kept alongside.
#[derive(Clone, Debug, Default)]
pub struct Content {
    pub bytes: Vec<u8>,
    pub parts: Vec<ContentPart>,
}

impl Content {
    /// The part covering `offset`, and that offset expressed relative to the
    /// part's own decoded data. `None` for an offset in a join byte or past
    /// the end.
    pub fn locate(&self, offset: usize) -> Option<(&ContentPart, u64)> {
        let part = self
            .parts
            .iter()
            .find(|p| (p.range.start..p.range.end).contains(&(offset as u64)))?;
        Some((part, offset as u64 - part.range.start))
    }
}

/// Decodes and concatenates a page's `/Contents`. A part that fails to decode
/// is reported and skipped; the rest of the page still has text.
pub fn content(doc: &Document, page: &Page, warnings: &mut Vec<Warning>) -> Result<Content> {
    // Streams are indirect by construction (ISO 32000-2 7.3.8), so every part
    // is a reference; anything else in /Contents is a producer bug worth
    // saying out loud rather than rendering as a blank page.
    let refs: Vec<ObjRef> = match page.dict.get(b"Contents") {
        None => Vec::new(),
        Some(entry) => match doc.resolve(entry)? {
            Object::Array(items) => items.iter().filter_map(Object::as_reference).collect(),
            Object::Null => Vec::new(),
            _ => match entry.as_reference() {
                Some(r) => vec![r],
                None => {
                    warnings.push(Warning::ContentPartFailed {
                        stream: page.objref,
                        detail: "/Contents is neither a reference nor an array".into(),
                    });
                    Vec::new()
                }
            },
        },
    };

    let mut out = Content::default();
    for stream in refs {
        let parsed = match doc.get(stream.number) {
            Ok(p) => p,
            Err(e) => {
                warnings.push(Warning::ContentPartFailed {
                    stream,
                    detail: e.to_string(),
                });
                continue;
            }
        };
        let Some(raw) = parsed.object.as_stream() else {
            warnings.push(Warning::ContentPartFailed {
                stream,
                detail: "/Contents entry is not a stream".into(),
            });
            continue;
        };
        let decoded = match filter::decode(&raw.dict, &raw.raw, &|o| doc.resolve(o)) {
            Ok(d) => d,
            Err(e) => {
                warnings.push(Warning::ContentPartFailed {
                    stream,
                    detail: e.to_string(),
                });
                continue;
            }
        };
        if !out.bytes.is_empty() {
            out.bytes.push(b'\n');
        }
        let start = out.bytes.len();
        out.bytes.extend_from_slice(&decoded);
        out.parts.push(ContentPart {
            stream,
            origin: parsed.origin,
            range: Span::new(start as u64, out.bytes.len() as u64),
        });
    }
    Ok(out)
}
