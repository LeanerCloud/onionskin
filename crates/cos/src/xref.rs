//! Cross-reference loading: classic tables, PDF 1.5 xref streams, hybrid
//! files, and the `/Prev` chain that links update sections together.
//!
//! Loading is eager and cheap: the header, the tail and the xref sections are
//! the only things read at open. Object bodies are not touched here.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{Error, Result};
use crate::filters;
use crate::object::{Dict, Object, RecoveredBoundary};
use crate::parse::{self, Lexer};
use crate::reader::Reader;
use crate::repair::RepairReason;

/// Guards against a `/Prev` chain that loops or fans out absurdly.
const MAX_SECTIONS: usize = 128;
const XREF_INITIAL_WINDOW: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrefEntry {
    /// The object number is not in use. Free entries form a linked list
    /// (ISO 32000-1 7.5.4) whose head is object 0 and whose tail links back to
    /// it, so the entry carries the number of the next free object. A new
    /// section splices its own free entries into that list rather than
    /// replacing it.
    Free { next: u32 },
    /// The object body lives at `offset` in the file.
    InFile { offset: u64, generation: u16 },
    /// The object body lives inside object stream `container`.
    InObjectStream { container: u32, index: u32 },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Xref {
    entries: BTreeMap<u32, XrefEntry>,
}

impl Xref {
    pub fn get(&self, number: u32) -> Option<XrefEntry> {
        self.entries.get(&number).copied()
    }

    /// Older sections never overwrite newer ones.
    pub(crate) fn insert_if_absent(&mut self, number: u32, entry: XrefEntry) {
        self.entries.entry(number).or_insert(entry);
    }

    pub(crate) fn insert(&mut self, number: u32, entry: XrefEntry) {
        self.entries.insert(number, entry);
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, XrefEntry)> + '_ {
        self.entries.iter().map(|(n, e)| (*n, *e))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn max_number(&self) -> u32 {
        self.entries.keys().next_back().copied().unwrap_or(0)
    }
}

pub(crate) struct Loaded {
    pub xref: Xref,
    pub trailer: Dict,
    /// Absolute offset of the newest section, which a new incremental section
    /// records as its `/Prev`.
    pub startxref: u64,
    /// Cross-reference streams whose own data boundary the parser had to
    /// recover. The document's map of objects then rests on a guessed
    /// boundary, which is exactly the kind of thing a caller must be able to
    /// find out about.
    pub recovered: Vec<(u32, RecoveredBoundary)>,
}

/// Reads `startxref` out of the file's tail.
pub(crate) fn find_startxref(tail: &[u8], tail_base: u64) -> Option<u64> {
    let at = parse::rfind(tail, b"startxref")?;
    let mut lex = Lexer::new(&tail[at + b"startxref".len()..], tail_base);
    lex.read_unsigned().ok()
}

/// Follows the whole `/Prev` chain from `startxref`, newest section first.
pub(crate) fn load_chain(
    reader: &Reader,
    startxref: u64,
    reasons: &mut Vec<RepairReason>,
) -> Result<Loaded> {
    let Some(first) = section_start(reader, startxref) else {
        return Err(Error::Unrecoverable {
            detail: format!("startxref {startxref} does not point at a cross-reference section"),
        });
    };

    let mut walk = Walk {
        reader,
        xref: Xref::default(),
        trailer: Dict::new(),
        visited: BTreeSet::new(),
        recovered: Vec::new(),
    };
    walk.section(first, reasons)?;
    let Walk {
        xref,
        trailer,
        recovered,
        ..
    } = walk;

    if xref.is_empty() {
        return Err(Error::Unrecoverable {
            detail: "cross-reference chain yielded no entries".into(),
        });
    }
    Ok(Loaded {
        xref,
        trailer,
        startxref: first,
        recovered,
    })
}

/// Walks the section chain newest section first, so `insert_if_absent` gives
/// the newest definition of each object. The order is `section`, then its
/// `/XRefStm` (a hybrid file's compressed entries belong to this generation),
/// then `/Prev`. That ordering has to be a recursive descent: a flat stack
/// interleaves an older section's `/Prev` ahead of a newer one.
struct Walk<'a> {
    reader: &'a Reader,
    xref: Xref,
    trailer: Dict,
    visited: BTreeSet<u64>,
    recovered: Vec<(u32, RecoveredBoundary)>,
}

impl Walk<'_> {
    fn section(&mut self, offset: u64, reasons: &mut Vec<RepairReason>) -> Result<()> {
        if !self.visited.insert(offset) {
            return Ok(());
        }
        if self.visited.len() > MAX_SECTIONS {
            reasons.push(RepairReason::BrokenXrefSection {
                offset,
                detail: format!("the /Prev chain runs past {MAX_SECTIONS} sections"),
            });
            return Ok(());
        }

        // An unreadable section fails the whole chain rather than being noted
        // and skipped: the objects it defined are then missing from a partial
        // chain that `structure_ok` might still accept, whereas failing here
        // sends the document down the scan, which finds them.
        let section = read_section(self.reader, offset, reasons)?;
        self.recovered.extend(section.recovered);
        for (number, entry) in section.entries {
            self.xref.insert_if_absent(number, entry);
        }
        for (key, value) in section.trailer.iter() {
            if !self.trailer.contains(key.as_bytes()) {
                self.trailer.set(key.clone(), value.clone());
            }
        }

        for key in [b"XRefStm".as_slice(), b"Prev".as_slice()] {
            let Some(Object::Integer(value)) = section.trailer.get(key) else {
                continue;
            };
            if *value < 0 {
                continue;
            }
            match section_start(self.reader, *value as u64) {
                Some(next) => self.section(next, reasons)?,
                None => reasons.push(RepairReason::BrokenXrefSection {
                    offset: *value as u64,
                    detail: format!(
                        "/{} does not point at a cross-reference section",
                        String::from_utf8_lossy(key)
                    ),
                }),
            }
        }
        Ok(())
    }
}

/// Accepts an xref offset as written, or biased by the header offset when junk
/// precedes `%PDF-` and every recorded offset is short by that much.
fn section_start(reader: &Reader, value: u64) -> Option<u64> {
    // Both operands come out of the file, so the biased candidate is a checked
    // add rather than a wrap.
    [Some(value), value.checked_add(reader.header_offset)]
        .into_iter()
        .flatten()
        .find(|&candidate| looks_like_section(reader, candidate))
}

fn looks_like_section(reader: &Reader, offset: u64) -> bool {
    if offset >= reader.len() {
        return false;
    }
    let Ok(probe) = reader.read(offset, 64) else {
        return false;
    };
    let mut lex = Lexer::new(&probe, offset);
    lex.skip_whitespace();
    if lex.remaining().starts_with(b"xref") {
        return true;
    }
    reader.object_header_at(offset).is_some()
}

struct Section {
    entries: Vec<(u32, XrefEntry)>,
    trailer: Dict,
    /// Set when this section is a cross-reference stream whose data boundary
    /// the parser recovered rather than read from `/Length`.
    recovered: Option<(u32, RecoveredBoundary)>,
}

fn read_section(reader: &Reader, offset: u64, reasons: &mut Vec<RepairReason>) -> Result<Section> {
    let probe = reader.read(offset, 64)?;
    let mut lex = Lexer::new(&probe, offset);
    lex.skip_whitespace();
    if lex.remaining().starts_with(b"xref") {
        read_table(reader, offset, reasons)
    } else {
        read_stream(reader, offset)
    }
}

fn read_table(reader: &Reader, offset: u64, reasons: &mut Vec<RepairReason>) -> Result<Section> {
    let Some(available) = reader.len().checked_sub(offset) else {
        return Err(Error::Syntax {
            offset,
            detail: "cross-reference table starts past the end of the file".into(),
        });
    };
    let available = available as usize;
    let mut window = XREF_INITIAL_WINDOW.min(available);
    loop {
        let at_eof = window >= available;
        let buf = reader.read(offset, window)?;
        match parse_table(&buf, offset, at_eof) {
            TableParse::Done(section, new_reasons) => {
                reasons.extend(new_reasons);
                return Ok(section);
            }
            TableParse::NeedMore if !at_eof => window = window.saturating_mul(4).min(available),
            TableParse::NeedMore => {
                return Err(Error::Unrecoverable {
                    detail: format!("cross-reference table at {offset} has no trailer"),
                })
            }
            TableParse::Failed(detail) => return Err(Error::Syntax { offset, detail }),
        }
    }
}

enum TableParse {
    Done(Section, Vec<RepairReason>),
    NeedMore,
    Failed(String),
}

/// Room for `trailer` or a subsection header. Below it, a token that looks
/// wrong may simply be cut in half by the read window.
const TABLE_TOKEN_SLACK: usize = 32;

fn parse_table(buf: &[u8], base: u64, at_eof: bool) -> TableParse {
    let mut lex = Lexer::new(buf, base);
    lex.skip_whitespace();
    if !lex.eat_keyword(b"xref") {
        return TableParse::Failed("expected the keyword `xref`".into());
    }

    let mut entries = Vec::new();
    let mut reasons = Vec::new();
    loop {
        lex.skip_whitespace();
        if !at_eof && lex.remaining().len() < TABLE_TOKEN_SLACK {
            return TableParse::NeedMore;
        }
        if lex.remaining().is_empty() {
            return TableParse::NeedMore;
        }
        if lex.eat_keyword(b"trailer") {
            break;
        }
        let save = lex.position();
        let Ok(start) = lex.read_unsigned() else {
            return TableParse::Failed("expected a subsection header".into());
        };
        let Ok(count) = lex.read_unsigned() else {
            lex.seek(save);
            return TableParse::Failed("subsection header has no entry count".into());
        };
        if start > u64::from(u32::MAX) {
            return TableParse::Failed("subsection start is out of range".into());
        }

        let mut read = 0u64;
        while read < count {
            lex.skip_whitespace();
            if !at_eof && lex.remaining().len() < TABLE_TOKEN_SLACK {
                return TableParse::NeedMore;
            }
            if lex.remaining().is_empty() {
                return TableParse::NeedMore;
            }
            let entry_at = lex.position();
            match read_entry(&mut lex) {
                Some(entry) => {
                    let number = start + read;
                    if number <= u64::from(u32::MAX) {
                        entries.push((number as u32, entry));
                    }
                    read += 1;
                }
                None => {
                    // The subsection header overstates its count: the entries
                    // ran out and the trailer starts here.
                    lex.seek(entry_at);
                    reasons.push(RepairReason::XrefCountMismatch {
                        subsection_start: start as u32,
                        declared: count,
                        found: read,
                    });
                    break;
                }
            }
        }
    }

    lex.skip_whitespace();
    let trailer = match lex.parse_object() {
        Ok(Object::Dict(d)) => d,
        Ok(_) => return TableParse::Failed("trailer is not a dictionary".into()),
        Err(e) if e.is_eof() => return TableParse::NeedMore,
        Err(e) => return TableParse::Failed(e.detail()),
    };

    TableParse::Done(
        Section {
            entries,
            trailer,
            recovered: None,
        },
        reasons,
    )
}

fn read_entry(lex: &mut Lexer) -> Option<XrefEntry> {
    let save = lex.position();
    let offset = lex.read_unsigned().ok();
    let generation = lex.read_unsigned().ok();
    lex.skip_whitespace();
    let kind = lex.read_regular();
    match (offset, generation, kind) {
        (Some(offset), Some(generation), b"n") if generation <= u64::from(u16::MAX) => {
            Some(XrefEntry::InFile {
                offset,
                generation: generation as u16,
            })
        }
        // The first field of a free entry is the next free object number. One
        // too big to be an object number cannot be followed, so the list ends
        // here rather than pointing somewhere invented.
        (Some(next), Some(_), b"f") => Some(XrefEntry::Free {
            next: u32::try_from(next).unwrap_or(0),
        }),
        _ => {
            lex.seek(save);
            None
        }
    }
}

fn read_stream(reader: &Reader, offset: u64) -> Result<Section> {
    // An xref stream cannot depend on the xref to find its own /Length, so no
    // indirect length resolution is available here; the parser falls back to
    // scanning for `endstream`.
    let indirect = reader.parse_indirect_at(offset, &|_| None)?;
    let recovered = indirect
        .recovered
        .map(|boundary| (indirect.objref.number, boundary));
    let Object::Stream(stream) = indirect.object else {
        return Err(Error::Syntax {
            offset,
            detail: "cross-reference section is not a stream".into(),
        });
    };
    let dict = stream.dict.clone();
    let data = filters::decode(
        &dict,
        &stream.raw,
        &|o: &Object| Ok(o.clone()),
        filters::Damaged::Refuse,
    )?;

    let bad = |detail: String| Error::Syntax { offset, detail };
    let Some(items) = dict.get(b"W").and_then(Object::as_array) else {
        return Err(bad("cross-reference stream has no /W".into()));
    };
    if items.len() < 3 {
        return Err(bad(format!("/W has {} fields, expected 3", items.len())));
    }
    // A width that is not a small non-negative integer makes every row a
    // guess, so it is refused rather than defaulted to zero.
    let widths: Vec<usize> = items
        .iter()
        .map(|o| match o.as_integer() {
            Some(w) if (0..=8).contains(&w) => Ok(w as usize),
            _ => Err(bad(format!("/W holds {o:?}, expected 0 to 8"))),
        })
        .collect::<Result<_>>()?;
    let row = widths.iter().sum::<usize>();
    if row == 0 {
        return Err(bad("/W declares zero-width rows".into()));
    }

    let index: Vec<i64> = match dict.get(b"Index").and_then(Object::as_array) {
        Some(items) => items.iter().filter_map(Object::as_integer).collect(),
        None => match dict.get(b"Size").and_then(Object::as_integer) {
            Some(size) if size >= 0 => vec![0, size],
            // Without /Index, /Size is the only thing saying how many rows the
            // stream has. Defaulting it to zero would drop the whole section.
            _ => {
                return Err(bad(
                    "cross-reference stream has neither /Index nor /Size".into()
                ))
            }
        },
    };

    let mut entries = Vec::new();
    let mut cursor = 0usize;
    for pair in index.chunks(2) {
        let (&start, &count) = match pair {
            [start, count] => (start, count),
            _ => break,
        };
        if count <= 0 {
            continue;
        }
        for i in 0..count {
            if cursor + row > data.len() {
                break;
            }
            let fields = read_fields(&data[cursor..cursor + row], &widths);
            cursor += row;
            // `start` is an arbitrary integer from /Index, so the object
            // number is computed rather than assumed. The row is consumed
            // either way: skipping one without advancing the cursor would
            // misalign every subsection after it.
            let Some(number) = start.checked_add(i).and_then(|n| u32::try_from(n).ok()) else {
                continue;
            };
            let entry = match fields[0] {
                0 => XrefEntry::Free {
                    next: fields[1].min(u64::from(u32::MAX)) as u32,
                },
                1 => XrefEntry::InFile {
                    offset: fields[1],
                    generation: fields[2].min(u64::from(u16::MAX)) as u16,
                },
                2 => XrefEntry::InObjectStream {
                    container: fields[1].min(u64::from(u32::MAX)) as u32,
                    index: fields[2].min(u64::from(u32::MAX)) as u32,
                },
                // Types beyond 2 are reserved; the spec says treat as null,
                // which is a free entry that leads nowhere.
                _ => XrefEntry::Free { next: 0 },
            };
            entries.push((number, entry));
        }
    }

    Ok(Section {
        entries,
        trailer: dict,
        recovered,
    })
}

fn read_fields(row: &[u8], widths: &[usize]) -> [u64; 3] {
    let mut out = [0u64; 3];
    // A zero-width type field means type 1; a zero-width third field means 0.
    out[0] = 1;
    let mut cursor = 0usize;
    for (i, &width) in widths.iter().take(3).enumerate() {
        if width == 0 {
            continue;
        }
        let mut value = 0u64;
        for &byte in &row[cursor..cursor + width] {
            value = (value << 8) | u64::from(byte);
        }
        out[i] = value;
        cursor += width;
    }
    out
}
