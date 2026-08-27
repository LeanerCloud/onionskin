//! The lazy read layer: a `Source` plus the window-growing loop that parses
//! one indirect object without knowing in advance how long it is.

use crate::error::{Error, Result};
use crate::object::ObjRef;
use crate::parse::{self, Indirect, Lexer};
use crate::source::Source;

/// Starting window for parsing one object. Most objects are far smaller; the
/// window quadruples until the object parses or the file ends.
const INITIAL_WINDOW: usize = 1024;
const MAX_HEADER_SEARCH: usize = 4096;

pub(crate) struct Reader {
    source: Box<dyn Source>,
    len: u64,
    /// Byte offset of `%PDF-`. Non-zero when junk precedes the header, in
    /// which case xref offsets in the original file are relative to it.
    pub header_offset: u64,
}

impl Reader {
    pub fn new(source: Box<dyn Source>) -> Self {
        let len = source.len();
        Reader {
            source,
            len,
            header_offset: 0,
        }
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.source.read_at(offset, len)
    }

    pub fn read_all(&self) -> Result<Vec<u8>> {
        self.source.read_at(0, self.len as usize)
    }

    /// Reads the last `len` bytes of the file, with the absolute offset of the
    /// first byte returned.
    pub fn tail(&self, len: usize) -> Result<(u64, Vec<u8>)> {
        let start = self.len.saturating_sub(len as u64);
        Ok((start, self.source.read_at(start, len)?))
    }

    /// Finds the offset of `%PDF-`.
    pub fn find_header(&self) -> Result<u64> {
        let head = self.read(0, MAX_HEADER_SEARCH)?;
        if let Some(at) = parse::find(&head, b"%PDF-") {
            return Ok(at as u64);
        }
        // Beyond the window the spec allows, so this is already damage; the
        // whole-file scan is the only way to find it.
        let all = self.read_all()?;
        match parse::find(&all, b"%PDF-") {
            Some(at) => Ok(at as u64),
            None => Err(Error::NotAPdf),
        }
    }

    /// Reads the `N G obj` header at `offset` without parsing the body.
    pub fn object_header_at(&self, offset: u64) -> Option<(u32, u16)> {
        let probe = self.read(offset, 64).ok()?;
        let mut lex = Lexer::new(&probe, offset);
        let number = lex.read_unsigned().ok()?;
        let generation = lex.read_unsigned().ok()?;
        lex.expect_keyword(b"obj").ok()?;
        if number > u64::from(u32::MAX) || generation > u64::from(u16::MAX) {
            return None;
        }
        Some((number as u32, generation as u16))
    }

    /// Parses the indirect object at `offset`, growing the read window until
    /// the object is complete. This is the whole laziness bet: nothing outside
    /// the returned span is ever read.
    pub fn parse_indirect_at(
        &self,
        offset: u64,
        length_of: &dyn Fn(ObjRef) -> Option<i64>,
    ) -> Result<Indirect> {
        if offset >= self.len {
            return Err(Error::Syntax {
                offset,
                detail: "object offset is past the end of the file".into(),
            });
        }
        let available = (self.len - offset) as usize;
        let mut window = INITIAL_WINDOW.min(available);
        loop {
            let at_eof = window >= available;
            let buf = self.read(offset, window)?;
            match parse::parse_indirect(&buf, offset, at_eof, length_of) {
                Ok(indirect) => return Ok(indirect),
                Err(e) if e.is_eof() && !at_eof => {
                    window = window.saturating_mul(4).min(available);
                }
                Err(e) => {
                    return Err(Error::Syntax {
                        offset: e.offset,
                        detail: e.detail(),
                    })
                }
            }
        }
    }
}
