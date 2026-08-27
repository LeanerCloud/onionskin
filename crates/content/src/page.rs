//! Page lookup and content assembly.
//!
//! The page-tree walk lives in `cos`, which reaches a page by index with the
//! inheritable attributes already resolved. What is left here is the layer
//! above it: turning those raw attributes into the geometry the interpreter
//! and the viewer use, and concatenating the page's content streams.

use onionskin_cos::{Dict, Document, ObjRef, Object, Origin, PageNode, Span};
use onionskin_plugin_api::PageIndex;

use crate::error::{Result, Warning};

/// A page, with the four attributes ISO 32000-2 7.7.3.4 says are inheritable
/// resolved against its ancestors and normalised.
#[derive(Clone, Debug)]
pub struct Page {
    pub index: PageIndex,
    pub objref: ObjRef,
    pub dict: Dict,
    pub resources: Dict,
    /// `[llx lly urx ury]`, normalised so the first pair is the lower left.
    pub media_box: [f64; 4],
    /// `/CropBox`, normalised the same way. `None` when neither the page nor
    /// an ancestor gave one, which is not the same as one equal to the media
    /// box: intersecting and defaulting is the renderer's job.
    pub crop_box: Option<[f64; 4]>,
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
    Ok(from_node(doc.page(index)?))
}

/// Number of pages, taken from the page tree root's `/Count`.
pub fn page_count(doc: &Document) -> Result<usize> {
    let count = doc.page_count()?;
    Ok(count.max(0) as usize)
}

/// A page with no `/MediaBox` anywhere above it. ISO 32000-2 leaves the size
/// undefined; every reader picks a default and this is the one they pick.
const US_LETTER: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

fn from_node(node: PageNode) -> Page {
    Page {
        index: node.index,
        objref: node.objref,
        resources: node.resources.unwrap_or_default(),
        media_box: node.media_box.map_or(US_LETTER, lower_left_first),
        crop_box: node.crop_box.map(lower_left_first),
        rotate: normalise_rotation(node.rotate.unwrap_or(0) as i32),
        dict: node.dict,
    }
}

/// `cos` hands rectangles back in the file's own coordinate order, which
/// producers write either way round.
fn lower_left_first(v: [f64; 4]) -> [f64; 4] {
    [
        v[0].min(v[2]),
        v[1].min(v[3]),
        v[0].max(v[2]),
        v[1].max(v[3]),
    ]
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
            Object::Array(items) => items
                .iter()
                .filter_map(|item| match item {
                    Object::Ref(r) => Some(*r),
                    // A null is padding a producer left behind. Anything else
                    // is a part of the page description that will not be
                    // interpreted, which the caller has to be told about.
                    Object::Null => None,
                    _ => {
                        warnings.push(Warning::ContentPartFailed {
                            stream: page.objref,
                            detail: "/Contents array holds something that is not a stream".into(),
                        });
                        None
                    }
                })
                .collect(),
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
        let decoded = match doc.decode_stream(raw) {
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
