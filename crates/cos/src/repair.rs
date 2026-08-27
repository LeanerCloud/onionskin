//! Scan-and-rebuild recovery (decision 10).
//!
//! When the cross-reference cannot be trusted, the file is swept once for
//! `N G obj` headers and a fresh xref is built from what is actually there.
//! Nothing is rewritten: the recovered offsets point into the original bytes,
//! which is what lets a later save append the repaired structures while the
//! damaged original survives underneath.

use std::fmt;

use crate::error::{Error, Result};
use crate::object::{Dict, ObjRef, Object};
use crate::parse::{self, Lexer};
use crate::reader::Reader;
use crate::xref::{Xref, XrefEntry};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepairReason {
    /// Bytes precede `%PDF-`; every recorded offset is short by this much.
    JunkBeforeHeader {
        offset: u64,
    },
    MissingEof,
    MissingStartxref,
    BrokenXrefSection {
        offset: u64,
        detail: String,
    },
    /// A subsection header claimed more entries than it had.
    XrefCountMismatch {
        subsection_start: u32,
        declared: u64,
        found: u64,
    },
    /// The xref parsed but its offsets do not land on the objects it names.
    XrefOffsetsWrong {
        detail: String,
    },
    /// No `/Root` in any trailer; the catalog was found by scanning.
    TrailerRootRecovered,
    /// An object stream would not parse, so every object inside it is gone.
    ObjectStreamLost {
        container: u32,
    },
    /// The file ends mid-object.
    TruncatedTail {
        last_complete_object_end: u64,
    },
    /// A caller met an object the cross-reference could not produce and asked
    /// for a scan, after the document had already opened clean.
    MidSessionScan {
        /// The object whose absence prompted the escalation.
        unreachable: u32,
        /// How many cross-reference entries the scan replaced.
        entries_corrected: usize,
    },
}

impl fmt::Display for RepairReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepairReason::JunkBeforeHeader { offset } => {
                write!(f, "{offset} bytes of junk before %PDF-")
            }
            RepairReason::MissingEof => write!(f, "no %%EOF marker"),
            RepairReason::MissingStartxref => write!(f, "no startxref"),
            RepairReason::BrokenXrefSection { offset, detail } => {
                write!(f, "cross-reference section at {offset}: {detail}")
            }
            RepairReason::XrefCountMismatch {
                subsection_start,
                declared,
                found,
            } => write!(
                f,
                "subsection {subsection_start} declared {declared} entries, found {found}"
            ),
            RepairReason::XrefOffsetsWrong { detail } => {
                write!(f, "cross-reference offsets are wrong: {detail}")
            }
            RepairReason::TrailerRootRecovered => write!(f, "/Root recovered by scanning"),
            RepairReason::ObjectStreamLost { container } => {
                write!(f, "object stream {container} is unreadable")
            }
            RepairReason::TruncatedTail {
                last_complete_object_end,
            } => write!(
                f,
                "file ends mid-object; last complete object ends at {last_complete_object_end}"
            ),
            RepairReason::MidSessionScan {
                unreachable,
                entries_corrected,
            } => write!(
                f,
                "object {unreachable} was unreachable mid-session; a scan corrected \
                 {entries_corrected} cross-reference entries"
            ),
        }
    }
}

/// What `Document::open_repairing` reports when the file was not clean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairReport {
    pub reasons: Vec<RepairReason>,
    /// True when the cross-reference was rebuilt by scanning, at open or by a
    /// later escalation.
    pub rebuilt_by_scan: bool,
    /// How many objects the rebuild at open recovered. A mid-session
    /// escalation does not touch it: what that corrected is in its own reason,
    /// and moving this to mean two things would make neither checkable.
    pub recovered_objects: usize,
}

impl RepairReport {
    pub fn summary(&self) -> String {
        let reasons = self
            .reasons
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "{reasons} ({} objects recovered, rebuilt_by_scan={})",
            self.recovered_objects, self.rebuilt_by_scan
        )
    }
}

/// How a document was opened. `Document::open` refuses anything but `Clean`,
/// so a repaired open can never pass for a clean one by accident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    Clean,
    Repaired(RepairReport),
}

impl Provenance {
    pub fn is_clean(&self) -> bool {
        matches!(self, Provenance::Clean)
    }

    pub fn report(&self) -> Option<&RepairReport> {
        match self {
            Provenance::Clean => None,
            Provenance::Repaired(r) => Some(r),
        }
    }
}

/// How many objects the scan may re-parse against the whole rest of the file
/// after a bounded parse failed. Enough for the handful of streams whose data
/// happens to contain an object header, few enough to stay linear overall.
const UNBOUNDED_RETRIES: usize = 64;

pub(crate) struct Scanned {
    pub xref: Xref,
    pub trailer: Dict,
    pub reasons: Vec<RepairReason>,
}

/// Sweeps the whole file for object headers and rebuilds the xref from them.
/// This is the only path in `cos` that reads the entire file.
pub(crate) fn scan(reader: &Reader) -> Result<Scanned> {
    let bytes = reader.read_all()?;
    let mut reasons = Vec::new();
    let mut xref = Xref::default();
    let mut object_stream_containers = Vec::new();
    let mut catalog = None;
    let mut info = None;
    // A file with an xref stream has no `trailer` keyword, so the stream's own
    // dictionary is the only place /Root is written.
    let mut xref_stream_trailer = None;
    let mut last_complete_end = 0u64;
    let mut incomplete_tail = false;

    let offsets = object_header_offsets(&bytes);
    let last_offset = offsets.last().copied();
    // Parsing every header against the whole remainder of the file is
    // quadratic on a file full of `obj`-looking bytes, and this is the path
    // hostile files take. Each object is parsed against the run up to the next
    // header instead, which sums to one pass; an object whose data legitimately
    // contains a header (so the bounded parse fails) gets a full-remainder
    // retry, from a budget.
    let mut retries = UNBOUNDED_RETRIES;
    for (i, start) in offsets.iter().copied().enumerate() {
        let bounded = offsets.get(i + 1).copied().unwrap_or(bytes.len());
        let window = &bytes[start..];
        let parsed = parse::parse_indirect(&bytes[start..bounded], start as u64, false, &|_| None)
            .or_else(|e| {
                if retries == 0 {
                    return Err(e);
                }
                retries -= 1;
                parse::parse_indirect(window, start as u64, true, &|_| None)
            });
        let indirect = match parsed {
            Ok(indirect) => indirect,
            Err(_) => {
                if Some(start) == last_offset {
                    incomplete_tail = true;
                }
                continue;
            }
        };
        // A later definition of the same object number supersedes an earlier
        // one, exactly as an incremental update would.
        xref.insert(
            indirect.objref.number,
            XrefEntry::InFile {
                offset: start as u64,
                generation: indirect.objref.generation,
            },
        );

        if indirect.span.end >= reader.len() && !window_ends_cleanly(window) {
            incomplete_tail = true;
        } else {
            last_complete_end = last_complete_end.max(indirect.span.end);
        }

        if let Some(dict) = indirect.object.as_dict() {
            match dict.get(b"Type").and_then(Object::as_name) {
                Some(name) if name.as_bytes() == b"Catalog" => {
                    catalog = Some(indirect.objref);
                }
                Some(name) if name.as_bytes() == b"ObjStm" => {
                    object_stream_containers.push(indirect.objref.number);
                }
                Some(name) if name.as_bytes() == b"XRef" => {
                    xref_stream_trailer = Some(dict.clone());
                }
                _ => {}
            }
            if dict.contains(b"Producer") || dict.contains(b"CreationDate") {
                info.get_or_insert(indirect.objref);
            }
        }
    }

    if xref.is_empty() {
        return Err(Error::Unrecoverable {
            detail: "no indirect objects found while scanning".into(),
        });
    }
    if incomplete_tail {
        reasons.push(RepairReason::TruncatedTail {
            last_complete_object_end: last_complete_end,
        });
    }

    // Object streams recovered by the scan still hold objects the file needs;
    // register them so the document can reach them.
    for container in object_stream_containers {
        if let Some(found) = register_object_stream(reader, &mut xref, container, &mut reasons) {
            catalog.get_or_insert(found);
        }
    }

    let source = last_trailer_dict(&bytes)
        .filter(|d| d.contains(b"Root"))
        .or(xref_stream_trailer)
        .or_else(|| last_trailer_dict(&bytes))
        .unwrap_or_default();
    let mut trailer = crate::writer::trailer_for_new_section(&source);
    // A /Root the scan did not find an object for is as useless as no /Root at
    // all, and files that name a catalog they do not contain are real.
    let root_is_present = match trailer.get(b"Root") {
        Some(Object::Ref(r)) => !matches!(xref.get(r.number), None | Some(XrefEntry::Free { .. })),
        _ => false,
    };
    if !root_is_present {
        match catalog {
            Some(root) => {
                trailer.set("Root", Object::Ref(root));
                reasons.push(RepairReason::TrailerRootRecovered);
            }
            None => {
                return Err(Error::Unrecoverable {
                    detail: "no usable /Root and no /Type /Catalog object found".into(),
                })
            }
        }
    }
    if !trailer.contains(b"Info") {
        if let Some(info) = info {
            trailer.set("Info", Object::Ref(info));
        }
    }
    trailer.set("Size", Object::Integer(i64::from(xref.max_number()) + 1));

    Ok(Scanned {
        xref,
        trailer,
        reasons,
    })
}

/// Offsets of every `N G obj` header, found by locating the keyword and
/// walking back over the two numbers in front of it.
fn object_header_offsets(bytes: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(found) = parse::find(&bytes[cursor..], b"obj") {
        let at = cursor + found;
        cursor = at + 3;
        // `obj` must be a token of its own, not the tail of `endobj`.
        if at > 0 && !parse::is_whitespace(bytes[at - 1]) {
            continue;
        }
        if let Some(after) = bytes.get(at + 3) {
            if !parse::is_whitespace(*after) && !parse::is_delimiter(*after) {
                continue;
            }
        }
        if let Some(start) = header_start(bytes, at) {
            out.push(start);
        }
    }
    out
}

fn header_start(bytes: &[u8], obj_at: usize) -> Option<usize> {
    let mut i = obj_at;
    let skip_ws = |i: &mut usize| {
        while *i > 0 && parse::is_whitespace(bytes[*i - 1]) {
            *i -= 1;
        }
    };
    let take_digits = |i: &mut usize| -> bool {
        let end = *i;
        while *i > 0 && bytes[*i - 1].is_ascii_digit() {
            *i -= 1;
        }
        *i < end
    };
    skip_ws(&mut i);
    if !take_digits(&mut i) {
        return None;
    }
    skip_ws(&mut i);
    if !take_digits(&mut i) {
        return None;
    }
    Some(i)
}

fn window_ends_cleanly(window: &[u8]) -> bool {
    let tail_start = window.len().saturating_sub(32);
    parse::rfind(&window[tail_start..], b"endobj").is_some()
        || parse::rfind(&window[tail_start..], b"endstream").is_some()
}

/// The last `trailer <<...>>` in the file, whatever the xref around it says.
/// Each retry rescans from the start, so the number of retries is capped: a
/// file stuffed with the word `trailer` must not turn this quadratic.
fn last_trailer_dict(bytes: &[u8]) -> Option<Dict> {
    const MAX_ATTEMPTS: usize = 16;
    let mut search_end = bytes.len();
    for _ in 0..MAX_ATTEMPTS {
        let at = parse::rfind(&bytes[..search_end], b"trailer")?;
        let mut lex = Lexer::new(&bytes[at + b"trailer".len()..], (at + 7) as u64);
        if let Ok(Object::Dict(d)) = lex.parse_object() {
            return Some(d);
        }
        search_end = at;
    }
    None
}

/// Registers the objects an object stream carries, and reports the number of a
/// `/Type /Catalog` found inside it: in a file with an xref stream, that is
/// often the only place the catalog exists.
///
/// A container that will not parse takes every object inside it with it, so
/// that is recorded rather than passed over.
fn register_object_stream(
    reader: &Reader,
    xref: &mut Xref,
    container: u32,
    reasons: &mut Vec<RepairReason>,
) -> Option<ObjRef> {
    let lost = |reasons: &mut Vec<RepairReason>| {
        reasons.push(RepairReason::ObjectStreamLost { container });
        None
    };
    let Some(XrefEntry::InFile { offset, .. }) = xref.get(container) else {
        return lost(reasons);
    };
    let Ok(indirect) = reader.parse_indirect_at(offset, &|_| None) else {
        return lost(reasons);
    };
    let Some(stream) = indirect.object.as_stream() else {
        return lost(reasons);
    };
    let Ok(data) = crate::filters::decode(&stream.dict, &stream.raw, &|o: &Object| Ok(o.clone()))
    else {
        return lost(reasons);
    };
    let integer = |key: &[u8]| {
        stream
            .dict
            .get(key)
            .and_then(Object::as_integer)
            .unwrap_or(0)
            .max(0) as usize
    };
    let entries = parse::object_stream_entries(&data, integer(b"N"), integer(b"First"));

    let mut catalog = None;
    for (index, (number, start)) in entries.into_iter().enumerate() {
        // A body found directly in the file wins over a compressed copy.
        xref.insert_if_absent(
            number,
            XrefEntry::InObjectStream {
                container,
                index: index as u32,
            },
        );
        if catalog.is_none() && start < data.len() && is_catalog(&data[start..], start) {
            catalog = Some(ObjRef::new(number, 0));
        }
    }
    catalog
}

fn is_catalog(data: &[u8], base: usize) -> bool {
    let Ok(object) = Lexer::new(data, base as u64).parse_object() else {
        return false;
    };
    object
        .as_dict()
        .and_then(|d| d.get(b"Type"))
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Catalog")
}
