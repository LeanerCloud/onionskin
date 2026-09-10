//! The document: an xref, a trailer, a byte source, and objects that parse
//! only when someone asks for them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};
use crate::filters;
use crate::object::{
    Dict, Holder, Name, ObjRef, Object, Origin, PageNode, Parsed, RecoveredBoundary, Span, Stream,
};
use crate::parse::Lexer;
use crate::reader::Reader;
use crate::repair::{self, Provenance, RepairReason, RepairReport};
use crate::source::{FileSource, Source};
use crate::writer::{self, RowEntry, XrefRow};
use crate::xref::{self, Xref, XrefEntry};

/// How far back from the end of the file `startxref` is looked for.
const TAIL_WINDOW: usize = 2048;
/// Depth limit for `/Length` chains and page-tree descent.
const MAX_INDIRECTION: usize = 64;
/// Visits a page-tree descent gets on top of one per object in the file.
///
/// The bound has to terminate a hostile tree without failing an honest one, and
/// the file's own object count separates them: a walk that never repeats a node
/// visits each of them once, so a flat 200,000-page tree needs 200,001 visits
/// out of the 200,002 objects it takes to write one. A tree that costs more
/// than the file has objects is revisiting, which is what the budget is for -
/// `seen` is a path set, so a node listed twice under one parent is descended
/// twice and a tree of those costs 2^depth visits out of a handful of objects.
///
/// The constant on top is room for the one honest shape that does revisit, a
/// `/Kids` array listing the same page object more than once, and it is what
/// caps the hostile case. It is a separate bound from `MAX_INDIRECTION`
/// because depth does not limit breadth: see the note in `descend_pages`.
const PAGE_TREE_VISIT_SLACK: usize = 200_000;
/// How much of the original a save copies at a time. The copy loop is what
/// this bounds, and it is the part that scales with the file: a 4 GB document
/// copies through in the same memory a 4 KB one does. The section a save
/// appends is assembled whole, and for a repaired document that section can
/// carry a table over every object.
const COPY_CHUNK: u64 = 64 * 1024;

/// A pending change to one object number. Both variants carry the generation
/// the appended section will record for it.
///
/// This is the vocabulary [`Document::section_for`] takes, so a caller holding
/// its own overlay says what a save would write without the document holding
/// any of it. `Document`'s own edit map speaks the same type, which is what
/// makes [`Document::incremental_section`] a caller of `section_for` rather
/// than a second serializer.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingEdit {
    Set {
        generation: u16,
        object: Object,
    },
    /// The object is to be marked free. `generation` is already the bumped
    /// one, which is what a free entry records (ISO 32000-1 7.5.4).
    Delete {
        generation: u16,
    },
}

/// One generation of a file: the byte range it occupies, from the first byte
/// after the previous generation to the end of its own `%%EOF`.
///
/// There is no `prev` field and no `startxref` field. The `Vec` a walk returns
/// *is* the chain, in file order, so a section's predecessor is the element
/// before it; storing that fact twice is storing two things that can disagree.
/// The offset of a table inside a section is likewise not handed out: a caller
/// showing generations wants ranges and sizes, and one rolling a generation
/// back truncates at a `start`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Section {
    pub start: u64,
    pub end: u64,
}

/// A reference whose target is free or absent: who names it, and what it
/// names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dangling {
    pub holder: Holder,
    pub target: ObjRef,
}

struct ObjectStream {
    data: Vec<u8>,
    /// `(object number, offset of its data within `data`)`, in stream order.
    entries: Vec<(u32, usize)>,
    container_span: Span,
    /// The container's own boundary recovery, if it had one. Every object
    /// decoded out of it inherits the doubt.
    recovered_boundary: Option<RecoveredBoundary>,
}

/// The inheritable page attributes carried down a page-tree descent, each
/// holding the nearest ancestor's value seen so far.
#[derive(Clone, Default)]
struct Inherited {
    resources: Option<Dict>,
    media_box: Option<[f64; 4]>,
    crop_box: Option<[f64; 4]>,
    rotate: Option<i64>,
}

/// The mutable state of one page-tree descent: which page is wanted, how many
/// leaves have gone by, the nodes on the current path, and how many nodes the
/// walk has visited against the budget it was given. `seen` is a path set
/// rather than a visited set - a node is removed on the way back out - so a
/// tree that legitimately shares a node between two branches still walks, while
/// a cycle terminates.
struct PageWalk {
    target: usize,
    found: usize,
    seen: BTreeSet<u32>,
    visits: usize,
    limit: usize,
}

pub struct Document {
    reader: Reader,
    xref: Xref,
    trailer: Dict,
    provenance: Provenance,
    /// Offset of the newest cross-reference section in the original bytes,
    /// recorded as `/Prev` by the next section. `None` when the xref was
    /// rebuilt and there is nothing trustworthy to chain to.
    prev_startxref: Option<u64>,
    original_len: u64,
    cache: RefCell<BTreeMap<u32, Parsed>>,
    object_streams: RefCell<BTreeMap<u32, Rc<ObjectStream>>>,
    /// Every recovered stream boundary seen since the document opened. Kept
    /// apart from `cache`, which an edit clears: a boundary the parser guessed
    /// is a fact about the file and does not stop being true.
    recovered_boundaries: RefCell<BTreeMap<u32, RecoveredBoundary>>,
    in_flight: RefCell<BTreeSet<u32>>,
    edits: BTreeMap<u32, PendingEdit>,
    trailer_edits: Dict,
    next_number: u32,
}

impl Document {
    /// Opens a file that is structurally sound. A file needing repair is
    /// refused with `Error::RepairRequired` rather than opened quietly.
    pub fn open(source: Box<dyn Source>) -> Result<Document> {
        let (document, provenance) = Document::open_repairing(source)?;
        match provenance {
            Provenance::Clean => Ok(document),
            Provenance::Repaired(report) => Err(Error::RepairRequired(Box::new(report))),
        }
    }

    pub fn open_path(path: &Path) -> Result<Document> {
        Document::open(Box::new(FileSource::open(path)?))
    }

    /// Opens a file, repairing it if it needs repair, and hands back what
    /// happened. The caller cannot receive a repaired document without also
    /// receiving its `Provenance`.
    pub fn open_repairing(source: Box<dyn Source>) -> Result<(Document, Provenance)> {
        let mut reader = Reader::new(source);
        if reader.len() == 0 {
            return Err(Error::NotAPdf);
        }
        let header_offset = reader.find_header()?;
        reader.header_offset = header_offset;

        let mut reasons = Vec::new();
        if header_offset != 0 {
            reasons.push(RepairReason::JunkBeforeHeader {
                offset: header_offset,
            });
        }

        let (tail_base, tail) = reader.tail(TAIL_WINDOW)?;
        if crate::parse::rfind(&tail, b"%%EOF").is_none() {
            reasons.push(RepairReason::MissingEof);
        }

        let mut loaded = None;
        match xref::find_startxref(&tail, tail_base) {
            None => reasons.push(RepairReason::MissingStartxref),
            Some(value) => match xref::load_chain(&reader, value, &mut reasons) {
                Ok(l) => loaded = Some(l),
                Err(e) => reasons.push(RepairReason::BrokenXrefSection {
                    offset: value,
                    detail: e.to_string(),
                }),
            },
        }

        if let Some(l) = &loaded {
            refuse_encrypted(&l.trailer)?;
        }

        let mut rebuilt_by_scan = false;
        // A cross-reference stream whose own boundary was guessed is recorded
        // before anything is read through the table it produced, and stays
        // recorded whether or not that table survives validation: what the
        // parser had to guess is a fact about the file either way.
        let recovered_boundaries: BTreeMap<u32, RecoveredBoundary> = loaded
            .as_ref()
            .map(|l| l.recovered.iter().copied().collect())
            .unwrap_or_default();
        let (xref_table, trailer, prev_startxref) = match loaded {
            Some(l) => match structure_ok(&reader, &l.xref, &l.trailer) {
                Ok(()) => (l.xref, l.trailer, Some(l.startxref)),
                Err(detail) => {
                    reasons.push(RepairReason::XrefOffsetsWrong { detail });
                    rebuilt_by_scan = true;
                    let scanned = repair::scan(&reader)?;
                    refuse_encrypted(&scanned.trailer)?;
                    reasons.extend(scanned.reasons);
                    (scanned.xref, scanned.trailer, None)
                }
            },
            None => {
                rebuilt_by_scan = true;
                let scanned = repair::scan(&reader)?;
                refuse_encrypted(&scanned.trailer)?;
                reasons.extend(scanned.reasons);
                (scanned.xref, scanned.trailer, None)
            }
        };

        let declared_size = trailer
            .get(b"Size")
            .and_then(Object::as_integer)
            .and_then(|size| u32::try_from(size).ok())
            .unwrap_or(0);
        let next_number = declared_size
            .max(xref_table.max_number().saturating_add(1))
            .max(1);
        let original_len = reader.len();

        let provenance = if reasons.is_empty() {
            Provenance::Clean
        } else {
            Provenance::Repaired(RepairReport {
                reasons,
                rebuilt_by_scan,
                recovered_objects: xref_table.len(),
            })
        };

        let document = Document {
            reader,
            xref: xref_table,
            trailer,
            provenance: provenance.clone(),
            prev_startxref,
            original_len,
            cache: RefCell::new(BTreeMap::new()),
            object_streams: RefCell::new(BTreeMap::new()),
            recovered_boundaries: RefCell::new(recovered_boundaries),
            in_flight: RefCell::new(BTreeSet::new()),
            edits: BTreeMap::new(),
            trailer_edits: Dict::new(),
            next_number,
        };
        Ok((document, provenance))
    }

    pub fn open_path_repairing(path: &Path) -> Result<(Document, Provenance)> {
        Document::open_repairing(Box::new(FileSource::open(path)?))
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    pub fn xref(&self) -> &Xref {
        &self.xref
    }

    /// Length of the original bytes. Everything a save appends starts here, so
    /// truncating to this length recovers the file as it was opened.
    pub fn original_len(&self) -> u64 {
        self.original_len
    }

    /// The next object number [`Document::add_object`] would hand out: one
    /// above everything the file names and everything this session has written.
    ///
    /// Consumer: a caller that keeps its own overlay and allocates its own
    /// numbers, which is how `core` reserves one without calling `add_object`
    /// speculatively and leaving an edit it cannot withdraw.
    pub fn next_object_number(&self) -> u32 {
        self.next_number
    }

    /// Every generation in the file, oldest first, as byte ranges that
    /// partition it: the first starts at 0, each one starts where the previous
    /// ended, and the last ends at the end of the file.
    ///
    /// Consumers: the generations panel, which shows the ranges and their
    /// sizes, and a revert, which truncates at a `start`.
    ///
    /// This reports what is **in the file**, not what opening it produced. It
    /// walks the file's own `startxref` and `/Prev` chain again rather than
    /// reading the merged table, because the merged table is the union of every
    /// section and cannot say which section any of it came from. The cost is
    /// one re-read of each cross-reference section, which is what opening the
    /// document already paid.
    ///
    /// A chain that cannot be followed is [`Error::SectionChain`] rather than a
    /// shorter list: a hostile `/Prev` that loops, one that points at no
    /// section, or a section with no `%%EOF`. Reporting the prefix it managed
    /// to walk would be a list of generations that quietly omits some.
    ///
    /// A linearized file is one generation, not two. Its first-page
    /// cross-reference sits at the front of the file with an `%%EOF` of its
    /// own, and it names objects that live after that marker, which no
    /// generation's own table can do. Both halves were written in one pass, so
    /// that `%%EOF` is not a boundary a caller may truncate at: the bytes
    /// before it are a header and a table, not a document.
    pub fn sections(&self) -> Result<Vec<Section>> {
        // The opener rebuilt the table by scanning, which is what a `None`
        // here means, so the file's own chain is not what this document was
        // read through. Walking it anyway would report ranges derived from a
        // chain cos itself rejected, which is exactly the trap
        // [`Document::recovered_boundaries`] is written to avoid.
        if self.prev_startxref.is_none() {
            return Err(Error::SectionChain {
                offset: self.original_len,
                detail: "the cross-reference was rebuilt when the file opened, so the file's own \
                         chain is not the one this document was read through"
                    .into(),
            });
        }

        let (tail_base, tail) = self.reader.tail(TAIL_WINDOW)?;
        let Some(startxref) = xref::find_startxref(&tail, tail_base) else {
            return Err(Error::SectionChain {
                offset: self.original_len,
                detail: "the file's tail has no startxref".into(),
            });
        };

        let mut visited: BTreeSet<u64> = BTreeSet::new();
        // The end of each section that ends a generation, in chain order.
        let mut ends: Vec<u64> = Vec::new();
        let mut next = Some(startxref);
        while let Some(value) = next {
            let Some(offset) = xref::section_start(&self.reader, value) else {
                return Err(Error::SectionChain {
                    offset: value,
                    detail: "does not point at a cross-reference section".into(),
                });
            };
            if !visited.insert(offset) {
                return Err(Error::SectionChain {
                    offset,
                    detail: "the chain returns to a section it has already walked".into(),
                });
            }
            if visited.len() > xref::MAX_SECTIONS {
                return Err(Error::SectionChain {
                    offset,
                    detail: format!("the chain runs past {} sections", xref::MAX_SECTIONS),
                });
            }
            // The reasons a re-read collects are already in this document's
            // provenance from the open that produced it; a walk of the chain
            // does not get to add to them.
            let mut reasons = Vec::new();
            let section = xref::read_section(&self.reader, offset, &mut reasons)?;
            let end = self.end_of_section(offset, section.end)?;
            // A generation's own table can only name objects written before
            // the `%%EOF` that closes it. A table that names one *after* its
            // `%%EOF` is describing bytes that are not its own, which is what
            // a linearized file's first-page cross-reference does (ISO 32000-1
            // annex F): it sits at the front of the file, it belongs to the
            // same single pass that wrote the rest, and its `%%EOF` is not a
            // point a caller may truncate at. Truncating there leaves a file
            // with no catalog in it.
            let describes_later_bytes = section.entries.iter().any(|(_, entry)| match entry {
                XrefEntry::InFile { offset, .. } => {
                    // An offset in a file with junk before its header may be
                    // written short by that much or written absolute, and
                    // `locate` accepts either, so an entry counts only when
                    // both readings land past the end. And only when one of
                    // them lands inside the file at all: an offset past the
                    // end of the file is a corrupt entry, not a table
                    // describing bytes later on, and reading it as one would
                    // drop a real generation.
                    let candidates = [*offset, offset.saturating_add(self.reader.header_offset)];
                    candidates.iter().all(|candidate| *candidate >= end)
                        && candidates
                            .iter()
                            .any(|candidate| *candidate < self.original_len)
                }
                XrefEntry::Free { .. } | XrefEntry::InObjectStream { .. } => false,
            });
            if !describes_later_bytes {
                ends.push(end);
            }
            next = match section.trailer.get(b"Prev") {
                None => None,
                Some(Object::Integer(value)) if *value >= 0 => Some(*value as u64),
                Some(other) => {
                    return Err(Error::SectionChain {
                        offset,
                        detail: format!("/Prev is {other:?}, which names no offset"),
                    })
                }
            };
        }

        // A file has at least one generation, so an empty list is this walk
        // having talked itself out of every boundary it found. Saying so is
        // what keeps the rule above loud when it is wrong rather than handing
        // a caller a `Vec` with no first element.
        if ends.is_empty() {
            return Err(Error::SectionChain {
                offset: self.original_len,
                detail: "no section in the chain ends a generation".into(),
            });
        }

        ends.sort_unstable();
        let mut sections = Vec::with_capacity(ends.len());
        let mut start = 0u64;
        for (index, end) in ends.iter().enumerate() {
            // Whatever follows the last `%%EOF` belongs to the newest
            // generation: it is in the file, and truncating to a `start` has to
            // recover everything written before that start whether or not the
            // producer left bytes after its own end marker.
            let end = if index + 1 == ends.len() {
                self.original_len
            } else {
                *end
            };
            if end <= start {
                return Err(Error::SectionChain {
                    offset: start,
                    detail: format!("a section ending at {end} does not follow the one before it"),
                });
            }
            sections.push(Section { start, end });
            start = end;
        }
        Ok(sections)
    }

    /// The end of the section whose cross-reference starts at `table`: the
    /// first `%%EOF` at or after `parsed_end`, plus the end-of-line that
    /// follows it.
    ///
    /// The search starts past the table or cross-reference stream rather than
    /// at the table itself, because a stream's compressed data can hold those
    /// five bytes and a section's own body must not be able to end it.
    ///
    /// What follows a section's trailer is its `startxref`, its offset and its
    /// `%%EOF`, so the marker is a few dozen bytes away. The search is capped
    /// at `SEARCH` rather than running to the end of the file, which is what
    /// keeps a section with no `%%EOF` at all costing one read instead of a
    /// pass over the whole document, once per section in the chain.
    fn end_of_section(&self, table: u64, parsed_end: u64) -> Result<u64> {
        const WINDOW: usize = 4096;
        /// How far past a trailer a `%%EOF` may sit before the section counts
        /// as unterminated.
        const SEARCH: u64 = 4096;
        const EOF: &[u8] = b"%%EOF";
        let limit = self.original_len.min(parsed_end.saturating_add(SEARCH));
        let mut at = parsed_end;
        while at < limit {
            let want = ((limit - at) as usize).min(WINDOW);
            let buf = self.reader.read(at, want)?;
            if let Some(found) = crate::parse::find(&buf, EOF) {
                let end = at + (found + EOF.len()) as u64;
                return Ok(self.past_eol(end));
            }
            // Overlap by four bytes so a marker split across two reads is
            // still found, and stop when a window that small cannot hold one.
            if buf.len() < EOF.len() {
                break;
            }
            at += (buf.len() - (EOF.len() - 1)) as u64;
        }
        Err(Error::SectionChain {
            offset: table,
            detail: "no %%EOF closes this section".into(),
        })
    }

    /// `at`, plus the one end-of-line sequence that follows it, if any.
    fn past_eol(&self, at: u64) -> u64 {
        let Ok(next) = self.reader.read(at, 2) else {
            return at;
        };
        match next.first() {
            Some(b'\r') if next.get(1) == Some(&b'\n') => at + 2,
            Some(b'\r') | Some(b'\n') => at + 1,
            _ => at,
        }
    }

    /// Every stream whose data boundary the parser recovered, keyed by object
    /// number, with the container's entry standing for the objects inside it.
    ///
    /// Consumer: M5 redaction, which has to prove a byte range is gone and
    /// therefore may not certify one whose extent the parser guessed. This is
    /// not a `Provenance`: a wrong `/Length` is common enough that refusing
    /// the file would reject documents every reader opens, so the document
    /// stays `Clean` and the doubt is recorded per object instead.
    ///
    /// Parsing is lazy, so what is reported is what has been read: an object
    /// nobody fetched has no boundary to report yet. That is the same contract
    /// as [`Document::get`], which is where a caller learns about the one
    /// object it is holding, and redaction reads every stream it rewrites. A
    /// cross-reference stream is the exception that is always here, because
    /// opening the document is what parsed it.
    ///
    /// An entry for an object stored inside an object stream carries its
    /// container's byte counts, not its own: what was guessed is where the
    /// container ended. [`Parsed::origin`] says which case a note belongs to.
    pub fn recovered_boundaries(&self) -> BTreeMap<u32, RecoveredBoundary> {
        self.recovered_boundaries.borrow().clone()
    }

    // ---- lazy object access -------------------------------------------------

    /// Parses object `number` on demand, caching the result. Nothing outside
    /// the object's own byte span is read.
    pub fn get(&self, number: u32) -> Result<Parsed> {
        if let Some(hit) = self.cache.borrow().get(&number) {
            return Ok(hit.clone());
        }
        match self.edits.get(&number) {
            Some(PendingEdit::Set { generation, object }) => {
                return Ok(Parsed {
                    objref: ObjRef::new(number, *generation),
                    object: object.clone(),
                    origin: Origin::Pending,
                    recovered_boundary: None,
                })
            }
            // The object still has bytes in the file, and a caller that has
            // not saved yet could read them. Reporting it as present would
            // make the deletion invisible until the save.
            Some(PendingEdit::Delete { generation }) => {
                return Err(Error::MissingObject(ObjRef::new(number, *generation)))
            }
            None => {}
        }

        // A crafted file can name itself as its own object-stream container, or
        // as its own /Length. Refusing re-entry is what keeps that a typed
        // error instead of a stack overflow.
        {
            let mut in_flight = self.in_flight.borrow_mut();
            if !in_flight.insert(number) {
                return Err(Error::DepthExceeded {
                    detail: format!("object {number} is needed to parse itself"),
                });
            }
            // A chain of objects each needing the next (an indirect /Length
            // pointing at an object with an indirect /Length, and so on) would
            // otherwise recurse once per link until the stack ran out.
            if in_flight.len() > MAX_INDIRECTION {
                drop(in_flight);
                self.in_flight.borrow_mut().remove(&number);
                return Err(Error::DepthExceeded {
                    detail: format!("more than {MAX_INDIRECTION} objects are mid-parse"),
                });
            }
        }
        let parsed = self.parse_from_source(number);
        self.in_flight.borrow_mut().remove(&number);

        let parsed = parsed?;
        if let Some(boundary) = parsed.recovered_boundary {
            self.recovered_boundaries
                .borrow_mut()
                .insert(number, boundary);
        }
        self.cache.borrow_mut().insert(number, parsed.clone());
        Ok(parsed)
    }

    fn parse_from_source(&self, number: u32) -> Result<Parsed> {
        match self.xref.get(number) {
            Some(XrefEntry::InFile { generation, .. }) => {
                let offset = self
                    .locate(number)
                    .ok_or(Error::MissingObject(ObjRef::new(number, generation)))?;
                let indirect = self
                    .reader
                    .parse_indirect_at(offset, &|r| self.length_of(r))?;
                Ok(Parsed {
                    objref: indirect.objref,
                    object: indirect.object,
                    origin: Origin::File(indirect.span),
                    recovered_boundary: indirect.recovered,
                })
            }
            Some(XrefEntry::InObjectStream { container, index }) => {
                self.get_compressed(number, container, index)
            }
            Some(XrefEntry::Free { .. }) | None => {
                Err(Error::MissingObject(ObjRef::new(number, 0)))
            }
        }
    }

    /// The true file offset of object `number`, accepting an offset biased by
    /// the header position when junk precedes `%PDF-`.
    fn locate(&self, number: u32) -> Option<u64> {
        let Some(XrefEntry::InFile { offset, .. }) = self.xref.get(number) else {
            return None;
        };
        locate_at(&self.reader, number, offset)
    }

    fn get_compressed(&self, number: u32, container: u32, index: u32) -> Result<Parsed> {
        let stream = self.object_stream(container)?;
        // The xref names the slot, so the slot has to hold the object it named.
        // Searching the stream by number instead would paper over an xref that
        // disagrees with its own object streams on a document reported clean.
        let start = stream
            .entries
            .get(index as usize)
            .filter(|(n, _)| *n == number)
            .ok_or(Error::MissingObject(ObjRef::new(number, 0)))?
            .1;
        if start >= stream.data.len() {
            return Err(Error::Syntax {
                offset: start as u64,
                detail: format!("object stream {container} offset is past its data"),
            });
        }
        let mut lex = Lexer::new(&stream.data[start..], start as u64);
        let object = lex.parse_object().map_err(|e| Error::Syntax {
            offset: e.offset,
            detail: format!("in object stream {container}: {}", e.detail()),
        })?;
        Ok(Parsed {
            objref: ObjRef::new(number, 0),
            object,
            origin: Origin::ObjectStream {
                container,
                container_span: stream.container_span,
                within: Span::new(start as u64, (start + lex.position()) as u64),
            },
            // An object decoded out of a container whose own end was guessed
            // is only as trustworthy as that guess. `origin` says whose
            // boundary this is.
            recovered_boundary: stream.recovered_boundary,
        })
    }

    fn object_stream(&self, container: u32) -> Result<Rc<ObjectStream>> {
        if let Some(hit) = self.object_streams.borrow().get(&container) {
            return Ok(Rc::clone(hit));
        }
        let parsed = self.get(container)?;
        let Some(stream) = parsed.object.as_stream() else {
            return Err(Error::Unrecoverable {
                detail: format!(
                    "object {container} is referenced as an object stream but is not a stream"
                ),
            });
        };
        let container_span = parsed
            .origin
            .file_span()
            .ok_or_else(|| Error::Unrecoverable {
                detail: format!("object stream {container} has no bytes in the file"),
            })?;
        let data = filters::decode(
            &stream.dict,
            &stream.raw,
            &|o| self.resolve(o),
            filters::Damaged::Refuse,
        )?;
        let integer = |key: &[u8]| -> Result<usize> {
            Ok(self
                .resolve_key(&stream.dict, key)?
                .and_then(|o| o.as_integer())
                .unwrap_or(0)
                .max(0) as usize)
        };
        let entries =
            crate::parse::object_stream_entries(&data, integer(b"N")?, integer(b"First")?);

        let loaded = Rc::new(ObjectStream {
            data,
            entries,
            container_span,
            recovered_boundary: parsed.recovered_boundary,
        });
        self.object_streams
            .borrow_mut()
            .insert(container, Rc::clone(&loaded));
        Ok(loaded)
    }

    /// Resolves an indirect `/Length` while its own object is being parsed.
    /// A self-reference or a cycle yields `None` through the re-entry guard in
    /// `get`, which sends the parser down the `endstream`-scanning path.
    fn length_of(&self, r: ObjRef) -> Option<i64> {
        self.get(r.number).ok().and_then(|p| p.object.as_integer())
    }

    /// Follows indirect references until a direct object appears.
    pub fn resolve(&self, object: &Object) -> Result<Object> {
        let mut current = object.clone();
        for _ in 0..MAX_INDIRECTION {
            match current {
                Object::Ref(r) => current = self.get(r.number)?.object,
                other => return Ok(other),
            }
        }
        Err(Error::DepthExceeded {
            detail: "indirect reference chain".into(),
        })
    }

    fn resolve_key(&self, dict: &Dict, key: &[u8]) -> Result<Option<Object>> {
        match dict.get(key) {
            Some(value) => Ok(Some(self.resolve(value)?)),
            None => Ok(None),
        }
    }

    /// Decodes a stream's `/Filter` chain: Flate and LZW with the PNG and TIFF
    /// predictors, RunLength, and the two ASCII armours. An image codec is
    /// [`Error::UnsupportedFilter`], never a silently empty result.
    ///
    /// Consumers: `onionskin-content`, for page descriptions, embedded font
    /// programs and CMaps, and `codecs-common`'s exports.
    ///
    /// A payload the filter cannot finish decoding yields whatever did decode.
    /// A page description cut short by a wrong `/Length` is common and its
    /// first operators are still the page's real text. The structural layer's
    /// own decoding is stricter, because half a cross-reference stream is a
    /// fabricated cross-reference entry rather than a shorter page.
    pub fn decode_stream(&self, stream: &Stream) -> Result<Vec<u8>> {
        filters::decode(
            &stream.dict,
            &stream.raw,
            &|o| self.resolve(o),
            filters::Damaged::Salvage,
        )
    }

    pub fn catalog(&self) -> Result<Dict> {
        let root = self
            .trailer
            .get(b"Root")
            .ok_or_else(|| Error::Unrecoverable {
                detail: "trailer has no /Root".into(),
            })?;
        self.resolve(root)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::Unrecoverable {
                detail: "/Root does not resolve to a dictionary".into(),
            })
    }

    /// `/Count` from the page tree root, without walking the tree.
    pub fn page_count(&self) -> Result<i64> {
        let catalog = self.catalog()?;
        let pages = self
            .resolve_key(&catalog, b"Pages")?
            .ok_or_else(|| Error::Unrecoverable {
                detail: "catalog has no /Pages".into(),
            })?;
        let pages = pages.as_dict().ok_or_else(|| Error::Unrecoverable {
            detail: "/Pages does not resolve to a dictionary".into(),
        })?;
        let count = self
            .resolve_key(pages, b"Count")?
            .and_then(|o| o.as_integer())
            .ok_or_else(|| Error::Unrecoverable {
                detail: "page tree root has no /Count".into(),
            })?;
        if count < 0 {
            return Err(Error::InvalidPageCount { count });
        }
        Ok(count)
    }

    /// The page [`Document::page`] gives for index 0, as the object itself
    /// rather than with its inheritable attributes resolved. Reaching it
    /// touches only the nodes on that path, which is the shape the
    /// time-to-first-page budget (decision 11) measures.
    ///
    /// It is the indexed accessor rather than a descent of its own, because a
    /// second descent is a second set of rules about what counts as a page,
    /// and the two disagreeing is a bug that only shows up on the trees where
    /// it matters.
    pub fn first_page(&self) -> Result<Parsed> {
        self.get(self.page(0)?.objref.number)
    }

    /// Loads page `index` in document order, parsing only the page-tree nodes
    /// on the path to it.
    ///
    /// Consumers: `onionskin-content`'s page loader, the viewer's page model,
    /// and the navigation-pane readers above it.
    ///
    /// `/Count` lets a whole subtree be skipped after parsing only its root, so
    /// a thousand-page document costs a handful of nodes rather than a
    /// thousand. It is trusted only when it is a plausible non-negative number
    /// on a node that has `/Kids`: a lie there would silently renumber every
    /// later page, so the walk falls through to a real descent whenever
    /// `/Count` is absent or nonsensical.
    pub fn page(&self, index: usize) -> Result<PageNode> {
        let catalog = self.catalog()?;
        let root = catalog
            .get(b"Pages")
            .and_then(Object::as_reference)
            .ok_or_else(|| Error::Unrecoverable {
                detail: "catalog has no indirect /Pages".into(),
            })?;

        let mut walk = PageWalk {
            target: index,
            found: 0,
            seen: BTreeSet::new(),
            visits: 0,
            limit: PAGE_TREE_VISIT_SLACK.saturating_add(self.xref.len()),
        };
        match self.descend_pages(root, &Inherited::default(), &mut walk, 0)? {
            Some(page) => Ok(page),
            None => Err(Error::NoSuchPage {
                index,
                count: walk.found,
            }),
        }
    }

    fn descend_pages(
        &self,
        node: ObjRef,
        inherited: &Inherited,
        walk: &mut PageWalk,
        depth: usize,
    ) -> Result<Option<PageNode>> {
        // The depth cap alone does not bound the work. `seen` is a path set, so
        // a node listed twice under the same parent is descended twice, and a
        // tree of such nodes costs 2^depth visits while never repeating a node
        // on any one path. The budget is what makes that terminate.
        walk.visits += 1;
        if walk.visits > walk.limit {
            return Err(Error::DepthExceeded {
                detail: format!("page tree visits more than {} nodes", walk.limit),
            });
        }
        if depth >= MAX_INDIRECTION || !walk.seen.insert(node.number) {
            return Ok(None);
        }
        let parsed = self.get(node.number)?;
        let Some(dict) = parsed.object.as_dict().cloned() else {
            return Ok(None);
        };

        let mut inherited = inherited.clone();
        if let Some(Object::Dict(d)) = self.resolved(&dict, b"Resources") {
            inherited.resources = Some(d);
        }
        if let Some(rect) = self.rectangle(&dict, b"MediaBox") {
            inherited.media_box = Some(rect);
        }
        if let Some(rect) = self.rectangle(&dict, b"CropBox") {
            inherited.crop_box = Some(rect);
        }
        if let Some(Object::Integer(r)) = self.resolved(&dict, b"Rotate") {
            inherited.rotate = Some(r);
        }

        // A node with /Kids is internal even when it also claims /Type /Page,
        // which some producers do. A node without them is a leaf whatever it
        // claims.
        let kids = match self.resolved(&dict, b"Kids") {
            Some(Object::Array(kids)) => kids,
            _ => {
                let index = walk.found;
                walk.found += 1;
                if index != walk.target {
                    walk.seen.remove(&node.number);
                    return Ok(None);
                }
                return Ok(Some(PageNode {
                    objref: parsed.objref,
                    dict,
                    resources: inherited.resources,
                    media_box: inherited.media_box,
                    crop_box: inherited.crop_box,
                    rotate: inherited.rotate,
                }));
            }
        };

        for kid in kids {
            let Some(kid) = kid.as_reference() else {
                continue;
            };
            if let Some(count) = self.subtree_count(kid) {
                if walk.found + count <= walk.target {
                    walk.found += count;
                    continue;
                }
            }
            if let Some(page) = self.descend_pages(kid, &inherited, walk, depth + 1)? {
                return Ok(Some(page));
            }
        }
        walk.seen.remove(&node.number);
        Ok(None)
    }

    /// How many pages a subtree claims, when the claim is usable at all.
    fn subtree_count(&self, node: ObjRef) -> Option<usize> {
        let parsed = self.get(node.number).ok()?;
        let dict = parsed.object.as_dict()?;
        if !dict.contains(b"Kids") {
            return None;
        }
        match self.resolved(dict, b"Count") {
            Some(Object::Integer(c)) if c >= 0 => usize::try_from(c).ok(),
            _ => None,
        }
    }

    fn resolved(&self, dict: &Dict, key: &[u8]) -> Option<Object> {
        self.resolve_key(dict, key).ok().flatten()
    }

    /// A rectangle entry, if it is four finite numbers enclosing a positive
    /// area. Returned in the file's own coordinate order; only the area test
    /// needs the ordered copy. A box that fails is not a box, and must not
    /// shadow the one an ancestor gave.
    fn rectangle(&self, dict: &Dict, key: &[u8]) -> Option<[f64; 4]> {
        let Some(Object::Array(items)) = self.resolved(dict, key) else {
            return None;
        };
        if items.len() < 4 {
            return None;
        }
        let mut v = [0.0f64; 4];
        for (slot, item) in v.iter_mut().zip(items.iter()) {
            *slot = as_number(&self.resolve(item).ok()?)?;
        }
        // Finiteness is tested on the values themselves: `min` and `max` drop
        // NaN, so an ordered copy would hide one.
        if !v.iter().all(|n| n.is_finite()) {
            return None;
        }
        if v[0].max(v[2]) <= v[0].min(v[2]) || v[1].max(v[3]) <= v[1].min(v[3]) {
            return None;
        }
        Some(v)
    }

    // ---- escalation ---------------------------------------------------------

    /// Rebuilds what the cross-reference cannot serve by scanning the file,
    /// merges the result, and records the escalation in the provenance. The
    /// document that comes back is `Repaired`, never `Clean`.
    ///
    /// Repair is otherwise decided once, at open, and an object whose recorded
    /// offset is wrong stays `Error::MissingObject` for the session. That is
    /// the right default: a scan that fired by itself would turn a file that
    /// lies about one object into a document reporting itself clean. So the
    /// escalation is the caller's call, and it costs what it costs, a full
    /// pass over the file.
    ///
    /// Consumer: the M2 viewer, which meets a page it cannot resolve and can
    /// then offer to go looking for it rather than only saying no.
    ///
    /// `unreachable` is the object whose absence prompted the escalation. It
    /// is checked, not taken on trust: an escalation for an object that
    /// resolves changes nothing and reports nothing, and one that cannot be
    /// made to resolve is `Err(MissingObject)` with the document exactly as it
    /// was. Only an escalation that actually recovers the object leaves a
    /// `Repaired` provenance behind, so a repair section is never written for
    /// a repair that did not happen.
    ///
    /// Entries the current table can still reach are kept, and a free entry is
    /// kept free however plainly its bytes are still in the file: a deliberate
    /// deletion is not damage.
    pub fn escalate_to_scan(&mut self, unreachable: u32) -> Result<&Provenance> {
        // Whether the object can be reached is a question the document can
        // answer exactly, by trying. Nothing structural is as reliable: the
        // container of a compressed object can be exactly where the table says
        // while the slot it names holds something else.
        if self.get(unreachable).is_ok() {
            return Ok(&self.provenance);
        }

        let scanned = repair::scan(&self.reader)?;
        refuse_encrypted(&scanned.trailer)?;

        let mut merged = self.xref.clone();
        let mut entries_corrected = 0usize;
        for (number, entry) in scanned.xref.iter() {
            if matches!(self.xref.get(number), Some(XrefEntry::Free { .. })) {
                continue;
            }
            // The object the caller could not reach takes the scan's entry
            // whatever the table says about it, because the table saying
            // something reachable is precisely what has already proved wrong.
            if number != unreachable && reaches(&self.reader, &self.xref, number) {
                continue;
            }
            if merged.get(number) == Some(entry) {
                continue;
            }
            merged.insert(number, entry);
            entries_corrected += 1;
        }
        if entries_corrected == 0 {
            return Err(Error::MissingObject(ObjRef::new(unreachable, 0)));
        }

        let previous = std::mem::replace(&mut self.xref, merged);
        self.forget_parsed_objects();
        if self.get(unreachable).is_err() {
            self.xref = previous;
            self.forget_parsed_objects();
            return Err(Error::MissingObject(ObjRef::new(unreachable, 0)));
        }

        // The scan reaches objects the table never named, so the next number
        // to hand out has to move past them or `add_object` would write over
        // one of them.
        self.next_number = self
            .next_number
            .max(self.xref.max_number().saturating_add(1));

        let mut report = match std::mem::replace(&mut self.provenance, Provenance::Clean) {
            Provenance::Clean => RepairReport {
                reasons: Vec::new(),
                rebuilt_by_scan: false,
                recovered_objects: 0,
            },
            Provenance::Repaired(report) => report,
        };
        report.reasons.push(RepairReason::MidSessionScan {
            unreachable,
            entries_corrected,
        });
        report.rebuilt_by_scan = true;
        self.provenance = Provenance::Repaired(report);
        Ok(&self.provenance)
    }

    // ---- edits --------------------------------------------------------------

    /// Replaces object `number`, superseding a deletion of it that has not
    /// been saved yet.
    ///
    /// Refuses a number the file itself has already marked free. Taking one
    /// back means re-linking a free list that lives in a section already
    /// written, which an append-only save cannot do: the result is a chain
    /// pointing at an object that is in use. `add_object` hands out a number
    /// with no such history.
    ///
    /// Refuses object 0 for the same reason `delete_object` does: it is the
    /// head of that list, not a document object.
    pub fn set_object(&mut self, number: u32, generation: u16, object: Object) -> Result<()> {
        if number == 0 {
            return Err(Error::Unrecoverable {
                detail: "object 0 is the head of the free list, not a document object".into(),
            });
        }
        if matches!(self.xref.get(number), Some(XrefEntry::Free { .. })) {
            return Err(Error::FreedObject(ObjRef::new(number, generation)));
        }
        self.forget_parsed_objects();
        self.next_number = self.next_number.max(number.saturating_add(1));
        self.edits
            .insert(number, PendingEdit::Set { generation, object });
        Ok(())
    }

    /// Appends a new object. Fails when the document already uses the whole
    /// object-number space rather than colliding with an existing object.
    pub fn add_object(&mut self, object: Object) -> Result<ObjRef> {
        let number = self.next_number;
        self.next_number = number.checked_add(1).ok_or_else(|| Error::Unrecoverable {
            detail: "the document has no free object numbers left".into(),
        })?;
        self.edits.insert(
            number,
            PendingEdit::Set {
                generation: 0,
                object,
            },
        );
        Ok(ObjRef::new(number, 0))
    }

    /// Marks an object free. The appended section records it as a free entry
    /// spliced into the file's free list (ISO 32000-1 7.5.4) with its
    /// generation bumped, so the number cannot be reused at the generation the
    /// deleted object had. The object's bytes stay where they are, under the
    /// section, as the core invariant requires.
    ///
    /// Consumer: M3 `tools-organize`, whose page deletion has to remove the
    /// page object and its content streams.
    ///
    /// Deleting a number the document does not have, or one already deleted in
    /// this session, is an error rather than a quiet no-op: a caller working
    /// from a stale object number has to hear about it.
    pub fn delete_object(&mut self, number: u32) -> Result<()> {
        if number == 0 {
            return Err(Error::Unrecoverable {
                detail: "object 0 is the head of the free list, not a document object".into(),
            });
        }
        // Deleting the catalog produces a file that will not open. Nothing a
        // caller can do afterwards recovers from it, so it is refused here
        // rather than discovered later.
        if self.root_number() == Some(number) {
            return Err(Error::Unrecoverable {
                detail: format!("object {number} is the document catalog"),
            });
        }
        let live = match self.edits.get(&number) {
            Some(PendingEdit::Delete { generation }) => {
                return Err(Error::MissingObject(ObjRef::new(number, *generation)))
            }
            Some(PendingEdit::Set { generation, .. }) => *generation,
            None => match self.xref.get(number) {
                Some(XrefEntry::InFile { generation, .. }) => generation,
                // A compressed object carries no generation of its own; the
                // spec gives it 0.
                Some(XrefEntry::InObjectStream { .. }) => 0,
                Some(XrefEntry::Free { .. }) | None => {
                    return Err(Error::MissingObject(ObjRef::new(number, 0)))
                }
            },
        };

        // 65535 is the spec's "never reuse this number" and is where the bump
        // stops, rather than wrapping back to a generation that is in use.
        let generation = live.saturating_add(1);
        self.forget_parsed_objects();
        self.edits
            .insert(number, PendingEdit::Delete { generation });
        Ok(())
    }

    /// The object the trailer names as `/Root`, an unsaved change to that
    /// entry included.
    fn root_number(&self) -> Option<u32> {
        self.trailer_edits
            .get(b"Root")
            .or_else(|| self.trailer.get(b"Root"))
            .and_then(Object::as_reference)
            .map(|r| r.number)
    }

    /// Drops both object caches. An edited or deleted object may be an object
    /// stream, and the objects decoded out of it would otherwise keep serving
    /// bytes from a container the document no longer has.
    fn forget_parsed_objects(&self) {
        self.cache.borrow_mut().clear();
        self.object_streams.borrow_mut().clear();
    }

    pub fn set_trailer_entry(&mut self, key: &str, value: Object) {
        self.trailer_edits.set(Name::new(key), value);
    }

    /// Sets one field of the document information dictionary, creating the
    /// dictionary when the file has none.
    pub fn set_info_field(&mut self, key: &str, value: Object) -> Result<ObjRef> {
        match self.trailer.get(b"Info").and_then(Object::as_reference) {
            Some(r) => {
                let mut dict = self
                    .get(r.number)?
                    .object
                    .as_dict()
                    .cloned()
                    .unwrap_or_default();
                dict.set(Name::new(key), value);
                self.set_object(r.number, r.generation, Object::Dict(dict))?;
                Ok(r)
            }
            None => {
                let mut dict = Dict::new();
                dict.set(Name::new(key), value);
                let r = self.add_object(Object::Dict(dict))?;
                self.set_trailer_entry("Info", Object::Ref(r));
                Ok(r)
            }
        }
    }

    /// True when a save would append anything: a pending edit, or repaired
    /// structures that are not yet recorded in the file.
    pub fn has_pending_changes(&self) -> bool {
        !self.edits.is_empty() || !self.trailer_edits.is_empty() || !self.provenance.is_clean()
    }

    // ---- reference checking -------------------------------------------------

    /// Every reference in the document that resolves to no object: the
    /// complete check, as a query rather than a gate.
    ///
    /// It walks every in-use object and the trailer, and reports each
    /// `(holder, target)` pair whose target is free or absent from the
    /// cross-reference. It is O(file), which is why it is a query the
    /// verification suites run over a fixture rather than something a save
    /// pays for.
    ///
    /// This is the check that sees what the gate in [`Document::section_for`]
    /// cannot: an object already in the file, not rewritten by a section,
    /// pointing at a number that section freed. No walk bounded by the edit
    /// can find that one, and pretending otherwise is what makes a cheap gate
    /// look complete.
    ///
    /// An object the file cannot produce at all is an error rather than a
    /// dangling entry: the two are different findings and a damaged file is
    /// not this walk's answer to give.
    ///
    /// `0 0 R` is never reported. ISO 32000-1 7.3.10 makes a reference to a
    /// nonexistent object the null object, and object 0 is the head of the
    /// free list, so naming it is how a file writes a null reference.
    pub fn audit_references(&self) -> Result<Vec<Dangling>> {
        let mut numbers: BTreeSet<u32> = self
            .xref
            .iter()
            .filter(|(number, entry)| {
                *number != 0
                    && matches!(
                        entry,
                        XrefEntry::InFile { .. } | XrefEntry::InObjectStream { .. }
                    )
            })
            .map(|(number, _)| number)
            .collect();
        for (number, edit) in &self.edits {
            match edit {
                PendingEdit::Set { .. } => numbers.insert(*number),
                PendingEdit::Delete { .. } => numbers.remove(number),
            };
        }

        let resolves = |number: u32| self.in_use(&self.edits, number);
        let mut found = Vec::new();
        let mut trailer = self.trailer.clone();
        for (key, value) in self.trailer_edits.iter() {
            trailer.set(key.clone(), value.clone());
        }
        collect_dangling(
            Holder::Trailer,
            &Object::Dict(trailer),
            &resolves,
            &mut found,
        );
        for number in numbers {
            let parsed = self.get(number)?;
            collect_dangling(
                Holder::Object(number),
                &parsed.object,
                &resolves,
                &mut found,
            );
        }
        Ok(found)
    }

    /// Whether object `number` exists, with `overlay` laid over the file's own
    /// table. The gate asks it of the overlay it is about to write; the audit
    /// asks it of the document's own edit map, which is the same question
    /// about the document as it stands.
    fn in_use(&self, overlay: &BTreeMap<u32, PendingEdit>, number: u32) -> bool {
        match overlay.get(&number) {
            Some(PendingEdit::Set { .. }) => true,
            Some(PendingEdit::Delete { .. }) => false,
            None => matches!(
                self.xref.get(number),
                Some(XrefEntry::InFile { .. }) | Some(XrefEntry::InObjectStream { .. })
            ),
        }
    }

    /// The gate: refuses to emit a section that would leave a reference in its
    /// own bytes pointing at nothing. It is the same walk
    /// [`Document::audit_references`] uses, over the objects this section
    /// writes and the trailer it emits rather than over the whole file.
    ///
    /// One rule, in two halves, because a section is answerable for what it
    /// writes and not for what it carries forward:
    ///
    /// - **What the section introduces** - an object the overlay writes, a
    ///   trailer key the section sets - must resolve, after this section, to
    ///   an object that exists.
    /// - **What it carries forward** - a trailer key it inherits, a copy of a
    ///   base object a repaired document's full table has to re-serialize -
    ///   must at least not name a number this section frees. Refusing those
    ///   for absence instead would make a document whose trailer already names
    ///   an `/Info` nothing defines uneditable, and files like that are real:
    ///   a section that never touched `/Info` did not make that true.
    ///
    /// The carried-forward copies are walked only when the section frees
    /// something, since freeing is the only way it can make one of them
    /// dangle. Under the free-nothing rule that is never, so the gate stays
    /// proportional to the edit rather than O(file) on every preview of a
    /// repaired document.
    ///
    /// **What it still cannot see**: an object already in the file that this
    /// section does not write, pointing at a number this section frees. No
    /// walk bounded by the edit can. `audit_references` over the result is
    /// what finds that one.
    fn refuse_dangling_references(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        objects: &[(ObjRef, Object)],
        trailer: &Dict,
        trailer_edits: &BTreeMap<Name, Option<Object>>,
    ) -> Result<()> {
        // `set_object` and `delete_object` refuse five things outright. An
        // overlay comes straight from a caller and reaches the same writer, so
        // the same five are refused here rather than written into a file where
        // nothing would notice: a self-linked free entry is a cycle in the
        // free list, and a file whose catalog is free does not open at all.
        let root = trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .map(|objref| objref.number);
        for (number, edit) in overlay {
            let already_free = matches!(self.xref.get(*number), Some(XrefEntry::Free { .. }));
            match edit {
                // Taking a freed number back means re-linking a free list that
                // lives in a section already on disk.
                PendingEdit::Set { generation, .. } if already_free => {
                    return Err(Error::FreedObject(ObjRef::new(*number, *generation)))
                }
                PendingEdit::Set { .. } => {}
                PendingEdit::Delete { generation } => {
                    if *number == 0 || root == Some(*number) {
                        return Err(Error::Unrecoverable {
                            detail: format!(
                                "object {number} is {}, not something a section may free",
                                if *number == 0 {
                                    "the head of the free list"
                                } else {
                                    "the document catalog"
                                }
                            ),
                        });
                    }
                    // Freeing a number the file does not have in use writes a
                    // free entry for an object that was never there, and a
                    // free entry for one that is already free links the list
                    // to itself.
                    if !matches!(
                        self.xref.get(*number),
                        Some(XrefEntry::InFile { .. }) | Some(XrefEntry::InObjectStream { .. })
                    ) {
                        return Err(Error::MissingObject(ObjRef::new(*number, *generation)));
                    }
                }
            }
        }

        let resolves = |number: u32| self.in_use(overlay, number);
        let not_freed_here =
            |number: u32| !matches!(overlay.get(&number), Some(PendingEdit::Delete { .. }));
        // The copies of base objects a repaired document's full table carries
        // are worth walking only when the section frees something, because
        // freeing is the only way this section can make one of them dangle.
        // Under the free-nothing rule that is every M3 section, so the gate
        // stays proportional to the edit.
        let frees = overlay
            .values()
            .any(|edit| matches!(edit, PendingEdit::Delete { .. }));

        let mut found = Vec::new();
        for (objref, object) in objects {
            let introduced = overlay.contains_key(&objref.number);
            if !introduced && !frees {
                continue;
            }
            let test: &dyn Fn(u32) -> bool = if introduced {
                &resolves
            } else {
                &not_freed_here
            };
            collect_dangling(Holder::Object(objref.number), object, test, &mut found);
        }
        for (key, value) in trailer.iter() {
            let test: &dyn Fn(u32) -> bool = if trailer_edits.contains_key(key) {
                &resolves
            } else {
                &not_freed_here
            };
            collect_dangling(Holder::Trailer, value, test, &mut found);
        }

        match found.first() {
            Some(dangling) => Err(Error::DanglingReference {
                holder: dangling.holder,
                target: dangling.target,
            }),
            None => Ok(()),
        }
    }

    // ---- save ---------------------------------------------------------------

    /// Whether the appended section must carry a table covering every object,
    /// rather than a delta chained to the file's own xref with `/Prev`.
    ///
    /// A missing `%%EOF` is the one kind of damage that leaves the existing
    /// chain worth pointing at, so it alone keeps the cheap delta. Everything
    /// else means a reader following `/Prev` would land back in the damage.
    fn needs_full_table(&self) -> bool {
        match &self.provenance {
            Provenance::Clean => false,
            Provenance::Repaired(report) => !report
                .reasons
                .iter()
                .all(|r| matches!(r, RepairReason::MissingEof)),
        }
    }

    /// The free entries the appended section writes: object 0, the head of the
    /// list, followed by everything deleted in this session.
    ///
    /// ISO 32000-1 7.5.4 makes the free entries a linked list, each one naming
    /// the next and the last naming 0. New entries go in at the head, so
    /// whatever was already on the list stays on it: the tail of the new run
    /// points at whatever object 0 used to point at. A rebuilt full table is
    /// the exception, because it does not carry the older free entries and a
    /// link into them would lead nowhere.
    ///
    /// Returns an empty vector when there is nothing to say: a delta section
    /// with no deletions has no business rewriting the head of the list.
    fn free_list_rows(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        full_table: bool,
    ) -> Vec<XrefRow> {
        let deleted: Vec<(u32, u16)> = overlay
            .iter()
            .filter_map(|(number, edit)| match edit {
                PendingEdit::Delete { generation } => Some((*number, *generation)),
                PendingEdit::Set { .. } => None,
            })
            .collect();
        if deleted.is_empty() && !full_table {
            return Vec::new();
        }

        let tail = match (full_table, self.xref.get(0)) {
            (false, Some(XrefEntry::Free { next })) => next,
            _ => 0,
        };
        let mut rows = vec![XrefRow {
            number: 0,
            generation: 65535,
            entry: RowEntry::Free(deleted.first().map_or(tail, |(number, _)| *number)),
        }];
        for (index, (number, generation)) in deleted.iter().enumerate() {
            let next = deleted.get(index + 1).map_or(tail, |(next, _)| *next);
            rows.push(XrefRow {
                number: *number,
                generation: *generation,
                entry: RowEntry::Free(next),
            });
        }
        rows
    }

    /// The bytes a save would append, or `None` when there is nothing to say.
    ///
    /// This is [`Document::section_for`] over the document's own edit map. The
    /// trailer edits are adapted entry by entry into the argument form, every
    /// one of them as `Some`: this path has no verb that removes a trailer key
    /// and never needed one, so the adaptation is total and loses nothing.
    pub fn incremental_section(&self) -> Result<Option<Vec<u8>>> {
        let trailer_edits: BTreeMap<Name, Option<Object>> = self
            .trailer_edits
            .iter()
            .map(|(key, value)| (key.clone(), Some(value.clone())))
            .collect();
        self.section_for(&self.edits, &trailer_edits)
    }

    /// The bytes a save of `overlay` would append, or `None` when it would
    /// append nothing.
    ///
    /// **Nothing appended is exactly when `overlay` and `trailer_edits` are
    /// both empty *and* the provenance is [`Provenance::Clean`]**: a repaired
    /// document with an empty overlay still owes its repair, and dropping that
    /// third clause would stop the repair ever being written.
    ///
    /// `trailer_edits` maps a key to `Some(value)` to set it and to `None` to
    /// **clear** it. A cleared key is emitted as `Object::Null`, which
    /// ISO 32000-1 7.3.7 makes equivalent to the entry being absent. That is
    /// the whole of what a set-only dictionary of edits could not say, and
    /// without it an undo that took back the creation of a trailer key could
    /// not be saved: nothing above the trailer can stop naming it.
    ///
    /// Takes `&self` and the overlay by reference: no cache is dropped, the
    /// document's own edit map is neither read nor written, and the bytes a
    /// preview renders are the bytes a save writes, because they are one call.
    ///
    /// It grows no ability to resurrect a freed number. Removal is expressed by
    /// rewriting the referrer, so nothing an overlay names is a number the file
    /// has already marked free, and [`Document::set_object`] keeps its refusal.
    pub fn section_for(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        trailer_edits: &BTreeMap<Name, Option<Object>>,
    ) -> Result<Option<Vec<u8>>> {
        if overlay.is_empty() && trailer_edits.is_empty() && self.provenance.is_clean() {
            return Ok(None);
        }
        // Ordered after the early-out on purpose. An unconditional refusal
        // would make every encrypted document unrenderable, because the canvas
        // draws from the original plus this section: refusing on an empty
        // overlay would leave nothing to draw.
        if self.trailer.contains(b"Encrypt") {
            return Err(Error::EncryptedWrite);
        }

        // The section must start on its own line.
        let last = self.reader.read(self.original_len.saturating_sub(1), 1)?;
        let lead: &[u8] = match last.first() {
            Some(b'\n') | Some(b'\r') | None => b"",
            Some(_) => b"\n",
        };
        let section_start = self.original_len + lead.len() as u64;

        let mut objects: Vec<(ObjRef, Object)> = overlay
            .iter()
            .filter_map(|(number, edit)| match edit {
                PendingEdit::Set { generation, object } => {
                    Some((ObjRef::new(*number, *generation), object.clone()))
                }
                PendingEdit::Delete { .. } => None,
            })
            .collect();

        let full_table = self.needs_full_table();
        let mut rows = self.free_list_rows(overlay, full_table);
        if full_table {
            for (number, entry) in self.xref.iter() {
                // The overlay, not the document's edit map, which is the
                // fourth of the four places that distinction has to be made.
                // Reading the edit map here would push an overlaid compressed
                // object twice - the overlay's copy and the base's - and the
                // table's last row wins, so the edit would be written into the
                // file and then indexed away.
                if number == 0 || overlay.contains_key(&number) {
                    continue;
                }
                match entry {
                    // Objects that were already free are not carried into a
                    // rebuilt table: no subsection covers them, which is how a
                    // classic table says a number is not in use.
                    XrefEntry::Free { .. } => {}
                    XrefEntry::InFile { generation, .. } => {
                        // Dropping a row here would delete the object from the
                        // only table the saved file can be read through.
                        let offset = self
                            .locate(number)
                            .ok_or(Error::MissingObject(ObjRef::new(number, generation)))?;
                        rows.push(XrefRow {
                            number,
                            generation,
                            entry: RowEntry::InUse(offset),
                        });
                    }
                    // A compressed object has no offset of its own to point
                    // at, so the repaired section carries a copy. The original
                    // container is left untouched underneath.
                    XrefEntry::InObjectStream { .. } => {
                        let parsed = self.get(number)?;
                        objects.push((parsed.objref, parsed.object));
                    }
                }
            }
        }
        objects.sort_by_key(|(r, _)| r.number);

        let mut trailer = writer::trailer_for_new_section(&self.trailer);
        // A rebuilt table is self-sufficient and the chain it would point at is
        // the damaged one, so /Prev is written only for a delta section.
        if !full_table {
            if let Some(prev) = self.prev_startxref {
                trailer.set("Prev", Object::Integer(prev as i64));
            }
        }
        for (key, value) in trailer_edits {
            match value {
                Some(value) => trailer.set(key.clone(), value.clone()),
                // ISO 32000-1 7.3.7: an entry whose value is null is
                // equivalent to the entry being absent. That is how a section
                // removes a trailer key without rewriting the file underneath
                // it, which an append-only save cannot do.
                None => trailer.set(key.clone(), Object::Null),
            }
        }
        let highest = objects
            .iter()
            .map(|(r, _)| r.number)
            .chain(rows.iter().map(|r| r.number))
            .max()
            .unwrap_or(0)
            .max(self.xref.max_number());
        trailer.set("Size", Object::Integer(i64::from(highest) + 1));

        // Before a byte is serialized: a section that would index a reference
        // into nothing is refused rather than written and discovered later.
        self.refuse_dangling_references(overlay, &objects, &trailer, trailer_edits)?;

        let mut section = lead.to_vec();
        section.extend_from_slice(&writer::incremental_section(
            section_start,
            &objects,
            &rows,
            trailer,
        )?);
        Ok(Some(section))
    }

    /// Writes the original bytes followed by the incremental section, copying
    /// the original through in `COPY_CHUNK` pieces rather than materializing
    /// it. The bytes that come out are the same bytes that went in; only the
    /// memory to produce them changes.
    ///
    /// `save_to_path` is the consumer that matters; this is the seam it and
    /// the guarantee tests share, and what a later milestone will hand a
    /// non-file sink.
    pub fn save_to_writer(&self, out: &mut dyn Write) -> Result<()> {
        // Assembled before a byte is written, so a section that cannot be
        // built leaves the writer untouched rather than holding a document
        // that is all original and no update.
        let section = self.incremental_section()?;
        self.write_original_then(section, out)
    }

    fn write_original_then(&self, section: Option<Vec<u8>>, out: &mut dyn Write) -> Result<()> {
        let mut offset = 0u64;
        while offset < self.original_len {
            let want = (self.original_len - offset).min(COPY_CHUNK);
            let chunk = self.reader.read(offset, want as usize)?;
            // A source may hand back less than the whole request, but nothing
            // and more-than-asked are both it failing its contract. Writing
            // what came back would produce a plausible, wrong document.
            if chunk.is_empty() || chunk.len() as u64 > want {
                return Err(Error::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "the source returned {} bytes for a {want} byte read at {offset}",
                        chunk.len()
                    ),
                )));
            }
            out.write_all(&chunk)?;
            offset += chunk.len() as u64;
        }

        if let Some(section) = section {
            out.write_all(&section)?;
        }
        Ok(())
    }

    /// Saves to `path` through a temporary file in the same directory, renamed
    /// into place once every byte is on disk.
    ///
    /// Saving over the file the document was opened from is the ordinary case,
    /// and opening that path for writing would truncate the bytes this save is
    /// still reading. The rename also means a crash mid-save leaves the
    /// previous file whole rather than a half-written one.
    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        self.write_section_to_path(self.incremental_section()?, path)
    }

    /// Saves `overlay` the way [`Document::save_to_path`] saves the document's
    /// own edits, through the same section builder and the same temporary
    /// file. The bytes on disk are the bytes
    /// [`Document::section_for`] returned, so a preview built from them cannot
    /// disagree with what a save wrote.
    pub fn save_overlay_to_path(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        trailer_edits: &BTreeMap<Name, Option<Object>>,
        path: &Path,
    ) -> Result<()> {
        self.write_section_to_path(self.section_for(overlay, trailer_edits)?, path)
    }

    fn write_section_to_path(&self, section: Option<Vec<u8>>, path: &Path) -> Result<()> {
        let Some(directory) = path.parent() else {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} names no file to save to", path.display()),
            )));
        };
        let temporary = directory.join(temporary_name(path));
        let written = self.write_through(section, &temporary, path);
        if written.is_err() {
            // The save already failed; a failure to clean up after it is not
            // the error worth reporting in its place.
            let _ = std::fs::remove_file(&temporary);
        }
        written
    }

    fn write_through(&self, section: Option<Vec<u8>>, temporary: &Path, path: &Path) -> Result<()> {
        let mut out = BufWriter::new(File::create(temporary)?);
        self.write_original_then(section, &mut out)?;
        let file = out.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        file.sync_all()?;
        // A file created here gets the process's default mode, so replacing a
        // document that only its owner could read with one the world can read
        // is the default outcome unless the mode is carried across.
        match std::fs::metadata(path) {
            Ok(existing) => std::fs::set_permissions(temporary, existing.permissions())?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io(e)),
        }
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    /// The original bytes, plus one incremental section when there is anything
    /// to append. A no-op save on a clean document appends nothing.
    pub fn save_to_vec(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.save_to_writer(&mut out)?;
        Ok(out)
    }

    // ---- write from scratch -------------------------------------------------

    /// Serializes a complete document: the header, `objects` in ascending
    /// number order, a classic cross-reference table covering all of them, and
    /// `trailer`.
    ///
    /// Consumers: the operations that produce a **new** file rather than
    /// editing one - combine, split, extract, create-from-image, compress, and
    /// the printed sheets a print-to-file backend composes. The core invariant
    /// applies to those files from their first save onwards, because there is
    /// nothing underneath them to preserve.
    ///
    /// It is the section writer with an empty file in front of it, not a second
    /// serializer: the same object writer, the same table builder, the same
    /// trailer stripping. It emits a classic table, never a cross-reference
    /// stream, and it never writes object streams.
    ///
    /// Four refusals, because each of them produces a file no reader opens: a
    /// trailer with no `/Root`, an object numbered 0 (the free-list head is
    /// not a document object), two objects sharing a number, and a reference
    /// to a number the document does not contain. The last one is the same
    /// check [`Document::section_for`]'s gate runs, and here it is complete
    /// rather than bounded: a document written from scratch is the whole of
    /// its own object graph, so every reference in it either resolves inside
    /// that graph or resolves to nothing at all.
    pub fn write_new(objects: &[(ObjRef, Object)], trailer: Dict) -> Result<Vec<u8>> {
        // The binary comment (ISO 32000-1 7.5.2) is what makes a transfer that
        // sniffs content treat the file as binary rather than as text.
        const HEADER: &[u8] = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n";

        if !trailer.contains(b"Root") {
            return Err(Error::Unrecoverable {
                detail: "a new document's trailer must name a /Root".into(),
            });
        }
        let mut objects: Vec<(ObjRef, Object)> = objects.to_vec();
        objects.sort_by_key(|(r, _)| r.number);
        if let Some((r, _)) = objects.first() {
            if r.number == 0 {
                return Err(Error::Unrecoverable {
                    detail: "object 0 is the head of the free list, not a document object".into(),
                });
            }
        }
        if let Some(pair) = objects.windows(2).find(|p| p[0].0.number == p[1].0.number) {
            return Err(Error::Unrecoverable {
                detail: format!("object {} was given twice", pair[0].0.number),
            });
        }

        // The free list of a file with nothing freed is its head alone,
        // linking back to itself (ISO 32000-1 7.5.4). Writing it is what makes
        // the table's first subsection cover object 0, which every conforming
        // reader expects to be there.
        let rows = [XrefRow {
            number: 0,
            generation: 65535,
            entry: RowEntry::Free(0),
        }];
        let highest = objects.last().map_or(0, |(r, _)| r.number);
        let mut trailer = writer::trailer_for_new_section(&trailer);
        trailer.set("Size", Object::Integer(i64::from(highest) + 1));

        let numbers: BTreeSet<u32> = objects.iter().map(|(objref, _)| objref.number).collect();
        let resolves = |number: u32| numbers.contains(&number);
        let mut found = Vec::new();
        for (objref, object) in &objects {
            collect_dangling(Holder::Object(objref.number), object, &resolves, &mut found);
        }
        collect_dangling(
            Holder::Trailer,
            &Object::Dict(trailer.clone()),
            &resolves,
            &mut found,
        );
        if let Some(dangling) = found.first() {
            return Err(Error::DanglingReference {
                holder: dangling.holder,
                target: dangling.target,
            });
        }

        let mut out = HEADER.to_vec();
        out.extend_from_slice(&writer::incremental_section(
            HEADER.len() as u64,
            &objects,
            &rows,
            trailer,
        )?);
        Ok(out)
    }
}

/// Names the temporary file a `save_to_path` writes through. Two saves of the
/// same document, from one process or several, must not pick the same name.
fn temporary_name(path: &Path) -> String {
    static SAVES: AtomicU64 = AtomicU64::new(0);
    let serial = SAVES.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    format!(".{name}.onionskin-save.{}.{serial}", std::process::id())
}

/// Calls `found` for every indirect reference inside `object`: dictionary
/// values, array elements, and a stream's dictionary, at any depth. A walk
/// that only looked at the top level would pass on exactly the nesting a page
/// tree and an annotation list are made of.
fn each_reference(object: &Object, found: &mut dyn FnMut(ObjRef)) {
    match object {
        Object::Ref(r) => found(*r),
        Object::Array(items) => {
            for item in items {
                each_reference(item, found);
            }
        }
        Object::Dict(dict) => {
            for (_, value) in dict.iter() {
                each_reference(value, found);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                each_reference(value, found);
            }
        }
        Object::Null
        | Object::Bool(_)
        | Object::Integer(_)
        | Object::Real(_)
        | Object::String(_)
        | Object::Name(_) => {}
    }
}

/// The one reference check. The gate and the audit differ in the set of
/// objects they hand it, not in how a reference is found.
///
/// `0 0 R` is skipped: object 0 is the head of the free list and never an
/// object, so a file names it to write a reference that resolves to null
/// (ISO 32000-1 7.3.10).
fn collect_dangling(
    holder: Holder,
    object: &Object,
    resolves: &dyn Fn(u32) -> bool,
    out: &mut Vec<Dangling>,
) {
    each_reference(object, &mut |target| {
        if target.number != 0 && !resolves(target.number) {
            out.push(Dangling { holder, target });
        }
    });
}

/// A numeric object as an `f64`. Rectangles are the only place `cos` needs
/// one, and integers and reals are equally legal there.
fn as_number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn refuse_encrypted(trailer: &Dict) -> Result<()> {
    if trailer.contains(b"Encrypt") {
        return Err(Error::Encrypted);
    }
    Ok(())
}

/// Matches on the object number only. The generation an xref entry records is
/// advisory: producers get it wrong, every reader ignores it, and refusing here
/// would reject files that open everywhere else. Nothing is fabricated by the
/// leniency, because `Parsed.objref` carries the generation the object's own
/// bytes declare rather than the one the xref claimed.
fn locate_at(reader: &Reader, number: u32, offset: u64) -> Option<u64> {
    if reader.object_header_at(offset).map(|(n, _)| n) == Some(number) {
        return Some(offset);
    }
    // Both operands come out of the file, so the biased offset is a checked
    // add rather than a wrap.
    let biased = offset.checked_add(reader.header_offset)?;
    if reader.object_header_at(biased).map(|(n, _)| n) == Some(number) {
        return Some(biased);
    }
    None
}

/// Whether `table` leads to object `number` in the file: the bytes at the
/// offset it records really are that object, or, for a compressed one, its
/// container's are.
///
/// Structural and cheap. It says the table is consistent with the bytes, not
/// that the object parses, so it is a reason to leave an entry alone rather
/// than proof that the entry works.
fn reaches(reader: &Reader, table: &Xref, number: u32) -> bool {
    match table.get(number) {
        Some(XrefEntry::InFile { offset, .. }) => locate_at(reader, number, offset).is_some(),
        Some(XrefEntry::InObjectStream { container, .. }) => match table.get(container) {
            Some(XrefEntry::InFile { offset, .. }) => {
                locate_at(reader, container, offset).is_some()
            }
            _ => false,
        },
        Some(XrefEntry::Free { .. }) | None => false,
    }
}

/// Bounded check that a loaded xref is worth trusting: the trailer names a
/// `/Root`, the bytes at its offset really are that object, and it parses to a
/// dictionary. Three objects, not the whole file - this is the gate that keeps
/// open lazy while still catching a globally wrong xref.
fn structure_ok(reader: &Reader, table: &Xref, trailer: &Dict) -> std::result::Result<(), String> {
    let Some(Object::Ref(root)) = trailer.get(b"Root") else {
        return Err("trailer has no indirect /Root".to_string());
    };
    match reachable(reader, table, root.number)? {
        // A catalog stored in the file itself is cheap to check properly.
        Some(object) => {
            let dict = object
                .as_dict()
                .ok_or_else(|| format!("/Root object {} is not a dictionary", root.number))?;
            if let Some(Object::Ref(pages)) = dict.get(b"Pages") {
                reachable(reader, table, pages.number)?;
            }
            Ok(())
        }
        // A catalog inside an object stream: reaching the container is the
        // check. Decoding it here would cost a whole object stream at open.
        None => Ok(()),
    }
}

/// `Ok(Some(object))` for an object read from the file, `Ok(None)` for one
/// whose container was reached inside an object stream, `Err` when the xref
/// does not lead to it at all.
fn reachable(
    reader: &Reader,
    table: &Xref,
    number: u32,
) -> std::result::Result<Option<Object>, String> {
    let missing = || format!("object {number} is not readable through the xref");
    match table.get(number) {
        Some(XrefEntry::InFile { offset, .. }) => {
            let real = locate_at(reader, number, offset).ok_or_else(missing)?;
            let indirect = reader
                .parse_indirect_at(real, &|_| None)
                .map_err(|e| format!("object {number}: {e}"))?;
            Ok(Some(indirect.object))
        }
        Some(XrefEntry::InObjectStream { container, .. }) => match table.get(container) {
            Some(XrefEntry::InFile { offset, .. }) => {
                locate_at(reader, container, offset).ok_or_else(|| {
                    format!("object stream {container} is not readable through the xref")
                })?;
                Ok(None)
            }
            _ => Err(format!("object stream {container} has no xref entry")),
        },
        Some(XrefEntry::Free { .. }) | None => Err(missing()),
    }
}
