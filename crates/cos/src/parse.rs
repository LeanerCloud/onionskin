//! Lexer and object parser. Everything works over a byte window plus the
//! absolute file offset of that window's first byte, so spans come out
//! absolute and a caller can parse an object without holding the file.
//!
//! Running off the end of the window is reported as `LexErrorKind::Eof`, which
//! is what lets the reader grow its window instead of guessing how long an
//! object is.

use crate::object::{Dict, Name, ObjRef, Object, Span, Stream};

const MAX_DEPTH: u32 = 96;
/// Room for `endstream` plus the whitespace that may precede it. A search
/// window smaller than this cannot tell "not there" from "just past the edge".
const ENDSTREAM_SLACK: usize = 32;
/// Same idea for `endobj`: a window ending just short of one would record a
/// span that stops before the object's last byte.
const ENDOBJ_SLACK: usize = 16;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LexErrorKind {
    /// The window ended mid-token. Grow it and retry.
    Eof,
    Syntax(String),
}

#[derive(Debug)]
pub(crate) struct LexError {
    pub offset: u64,
    pub kind: LexErrorKind,
}

impl LexError {
    pub fn is_eof(&self) -> bool {
        self.kind == LexErrorKind::Eof
    }

    pub fn detail(&self) -> String {
        match &self.kind {
            LexErrorKind::Eof => "unexpected end of input".to_string(),
            LexErrorKind::Syntax(d) => d.clone(),
        }
    }
}

pub(crate) type LexResult<T> = std::result::Result<T, LexError>;

pub(crate) fn is_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}

pub(crate) fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

pub(crate) struct Lexer<'a> {
    buf: &'a [u8],
    pos: usize,
    base: u64,
}

impl<'a> Lexer<'a> {
    pub fn new(buf: &'a [u8], base: u64) -> Self {
        Lexer { buf, pos: 0, base }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.buf.len());
    }

    pub fn offset(&self) -> u64 {
        self.base + self.pos as u64
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    fn eof(&self) -> LexError {
        LexError {
            offset: self.offset(),
            kind: LexErrorKind::Eof,
        }
    }

    fn syntax(&self, detail: impl Into<String>) -> LexError {
        LexError {
            offset: self.offset(),
            kind: LexErrorKind::Syntax(detail.into()),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    pub fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if is_whitespace(b) {
                self.pos += 1;
            } else if b == b'%' {
                while let Some(c) = self.peek() {
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Reads a run of regular characters: a keyword, a number, anything that
    /// is not a delimiter-introduced token.
    pub fn read_regular(&mut self) -> &'a [u8] {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_regular(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        &self.buf[start..self.pos]
    }

    /// Consumes `word` if it is the next token. Returns false and stays put
    /// otherwise.
    pub fn eat_keyword(&mut self, word: &[u8]) -> bool {
        let save = self.pos;
        self.skip_whitespace();
        if self.read_regular() == word {
            true
        } else {
            self.pos = save;
            false
        }
    }

    pub fn expect_keyword(&mut self, word: &[u8]) -> LexResult<()> {
        self.skip_whitespace();
        if self.peek().is_none() {
            return Err(self.eof());
        }
        let at = self.offset();
        let got = self.read_regular();
        if got == word {
            Ok(())
        } else if got.is_empty() && self.pos >= self.buf.len() {
            Err(self.eof())
        } else {
            Err(LexError {
                offset: at,
                kind: LexErrorKind::Syntax(format!(
                    "expected keyword {}, found {}",
                    String::from_utf8_lossy(word),
                    String::from_utf8_lossy(got)
                )),
            })
        }
    }

    pub fn read_unsigned(&mut self) -> LexResult<u64> {
        self.skip_whitespace();
        if self.peek().is_none() {
            return Err(self.eof());
        }
        let at = self.offset();
        let token = self.read_regular();
        if token.is_empty() {
            return Err(self.syntax("expected a number"));
        }
        parse_unsigned(token).ok_or(LexError {
            offset: at,
            kind: LexErrorKind::Syntax(format!(
                "expected a non-negative integer, found {}",
                String::from_utf8_lossy(token)
            )),
        })
    }

    pub fn parse_object(&mut self) -> LexResult<Object> {
        self.parse_object_at_depth(0)
    }

    fn parse_object_at_depth(&mut self, depth: u32) -> LexResult<Object> {
        if depth > MAX_DEPTH {
            return Err(self.syntax("object nesting past the depth limit"));
        }
        self.skip_whitespace();
        let Some(first) = self.peek() else {
            return Err(self.eof());
        };
        match first {
            b'/' => {
                self.pos += 1;
                Ok(Object::Name(self.parse_name()?))
            }
            b'(' => {
                self.pos += 1;
                Ok(Object::String(self.parse_literal_string()?))
            }
            b'<' => {
                if self.buf.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    Ok(Object::Dict(self.parse_dict_body(depth)?))
                } else {
                    self.pos += 1;
                    Ok(Object::String(self.parse_hex_string()?))
                }
            }
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_whitespace();
                    match self.peek() {
                        None => return Err(self.eof()),
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(Object::Array(items));
                        }
                        _ => items.push(self.parse_object_at_depth(depth + 1)?),
                    }
                }
            }
            b']' | b'>' | b')' | b'}' | b'{' => {
                Err(self.syntax(format!("unexpected delimiter '{}'", first as char)))
            }
            b'0'..=b'9' | b'+' | b'-' | b'.' => self.parse_number_or_reference(),
            _ => {
                let at = self.offset();
                let token = self.read_regular();
                match token {
                    b"true" => Ok(Object::Bool(true)),
                    b"false" => Ok(Object::Bool(false)),
                    b"null" => Ok(Object::Null),
                    [] => Err(self.syntax("expected an object")),
                    other => Err(LexError {
                        offset: at,
                        kind: LexErrorKind::Syntax(format!(
                            "expected an object, found keyword {}",
                            String::from_utf8_lossy(other)
                        )),
                    }),
                }
            }
        }
    }

    fn parse_dict_body(&mut self, depth: u32) -> LexResult<Dict> {
        let mut dict = Dict::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => return Err(self.eof()),
                Some(b'>') => {
                    if self.buf.get(self.pos + 1) == Some(&b'>') {
                        self.pos += 2;
                        return Ok(dict);
                    }
                    return Err(self.syntax("stray '>' in dictionary"));
                }
                Some(b'/') => {
                    self.pos += 1;
                    let key = self.parse_name()?;
                    let value = self.parse_object_at_depth(depth + 1)?;
                    dict.set(key, value);
                }
                Some(other) => {
                    return Err(self.syntax(format!(
                        "expected a dictionary key, found '{}'",
                        other as char
                    )))
                }
            }
        }
    }

    fn parse_name(&mut self) -> LexResult<Name> {
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if !is_regular(b) {
                break;
            }
            self.pos += 1;
            if b == b'#' {
                let hi = self.bump().ok_or_else(|| self.eof())?;
                let lo = self.bump().ok_or_else(|| self.eof())?;
                match (hex_value(hi), hex_value(lo)) {
                    (Some(h), Some(l)) => out.push(h * 16 + l),
                    _ => return Err(self.syntax("malformed #xx escape in name")),
                }
            } else {
                out.push(b);
            }
        }
        Ok(Name(out))
    }

    fn parse_literal_string(&mut self) -> LexResult<Vec<u8>> {
        let mut out = Vec::new();
        let mut depth = 1usize;
        loop {
            let b = self.bump().ok_or_else(|| self.eof())?;
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(b);
                }
                b'\\' => {
                    let e = self.bump().ok_or_else(|| self.eof())?;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'(' | b')' | b'\\' => out.push(e),
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut value = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        value = value * 8 + u32::from(d - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((value & 0xff) as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'\r' => {
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                other => out.push(other),
            }
        }
    }

    fn parse_hex_string(&mut self) -> LexResult<Vec<u8>> {
        let mut nibbles = Vec::new();
        loop {
            let b = self.bump().ok_or_else(|| self.eof())?;
            if b == b'>' {
                break;
            }
            if is_whitespace(b) {
                continue;
            }
            match hex_value(b) {
                Some(v) => nibbles.push(v),
                None => return Err(self.syntax("non-hex digit in hex string")),
            }
        }
        if nibbles.len() % 2 == 1 {
            nibbles.push(0);
        }
        Ok(nibbles.chunks(2).map(|p| p[0] * 16 + p[1]).collect())
    }

    fn parse_number_or_reference(&mut self) -> LexResult<Object> {
        let at = self.offset();
        let token = self.read_regular();
        let number = parse_number(token).ok_or(LexError {
            offset: at,
            kind: LexErrorKind::Syntax(format!(
                "malformed number {}",
                String::from_utf8_lossy(token)
            )),
        })?;

        let Object::Integer(candidate) = number else {
            return Ok(number);
        };
        if !(0..=i64::from(u32::MAX)).contains(&candidate) {
            return Ok(number);
        }

        // `N G R` needs two tokens of lookahead; anything else rewinds.
        let save = self.pos;
        let reference = (|| {
            self.skip_whitespace();
            let generation = parse_unsigned(self.read_regular())?;
            if generation > u64::from(u16::MAX) {
                return None;
            }
            self.skip_whitespace();
            if self.read_regular() != b"R" {
                return None;
            }
            Some(ObjRef::new(candidate as u32, generation as u16))
        })();

        match reference {
            Some(r) => Ok(Object::Ref(r)),
            None => {
                self.pos = save;
                Ok(number)
            }
        }
    }
}

pub(crate) fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn parse_unsigned(token: &[u8]) -> Option<u64> {
    if token.is_empty() || !token.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Overlong runs of digits appear in damaged files; refuse rather than wrap.
    std::str::from_utf8(token).ok()?.parse::<u64>().ok()
}

/// Numbers in the wild include `6.`, `.5`, `--3` and `4.-2`. Accept the shapes
/// a real reader accepts; refuse anything genuinely unparseable.
pub(crate) fn parse_number(token: &[u8]) -> Option<Object> {
    if token.is_empty() {
        return None;
    }
    let mut text = String::new();
    let mut seen_dot = false;
    let mut seen_digit = false;
    for (i, &b) in token.iter().enumerate() {
        match b {
            b'+' | b'-' => {
                if i == 0 {
                    if b == b'-' {
                        text.push('-');
                    }
                } else if seen_digit {
                    break;
                }
            }
            b'.' => {
                if seen_dot {
                    break;
                }
                seen_dot = true;
                text.push('.');
            }
            b'0'..=b'9' => {
                seen_digit = true;
                text.push(b as char);
            }
            _ => return None,
        }
    }
    if !seen_digit {
        return None;
    }
    if seen_dot {
        let mut normalized = text;
        if normalized.starts_with('.') {
            normalized.insert(0, '0');
        } else if normalized.starts_with("-.") {
            normalized.insert(1, '0');
        }
        if normalized.ends_with('.') {
            normalized.push('0');
        }
        normalized
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(Object::Real)
    } else {
        match text.parse::<i64>() {
            Ok(v) => Some(Object::Integer(v)),
            // Integers past i64 exist only in damaged files; keep the value as
            // a real rather than losing the object. A literal so long that it
            // overflows f64 too is refused: PDF has no notation for infinity,
            // so writing one back would be a fabrication.
            Err(_) => text
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(Object::Real),
        }
    }
}

/// One indirect object as parsed out of a window.
pub(crate) struct Indirect {
    pub objref: ObjRef,
    pub object: Object,
    pub span: Span,
}

/// Parses `N G obj ... endobj` starting at the front of `buf`.
///
/// `length_of` resolves an indirect `/Length`; returning `None` (because the
/// xref is not loaded yet, or the target is missing) falls back to scanning
/// for `endstream`, which is also the recovery path for a wrong `/Length`.
///
/// `at_eof` says the window already reaches the end of the file, so an
/// incomplete object is damage rather than a signal to read more.
pub(crate) fn parse_indirect(
    buf: &[u8],
    base: u64,
    at_eof: bool,
    length_of: &dyn Fn(ObjRef) -> Option<i64>,
) -> LexResult<Indirect> {
    let mut lex = Lexer::new(buf, base);
    lex.skip_whitespace();
    let start = lex.offset();
    let number = lex.read_unsigned()?;
    let generation = lex.read_unsigned()?;
    if number > u64::from(u32::MAX) || generation > u64::from(u16::MAX) {
        return Err(LexError {
            offset: start,
            kind: LexErrorKind::Syntax("object or generation number out of range".into()),
        });
    }
    lex.expect_keyword(b"obj")?;
    let objref = ObjRef::new(number as u32, generation as u16);
    let object = lex.parse_object()?;

    if !lex.eat_keyword(b"stream") {
        // A missing `endobj` is common enough that it is not worth failing on,
        // but a window that ends just short of one would silently record a
        // span that stops before the object does.
        if !lex.eat_keyword(b"endobj") && !at_eof && lex.remaining().len() < ENDOBJ_SLACK {
            return Err(LexError {
                offset: base + buf.len() as u64,
                kind: LexErrorKind::Eof,
            });
        }
        let span = Span::new(start, lex.offset());
        return Ok(Indirect {
            objref,
            object,
            span,
        });
    }

    let Object::Dict(dict) = object else {
        return Err(LexError {
            offset: lex.offset(),
            kind: LexErrorKind::Syntax("`stream` follows a non-dictionary object".into()),
        });
    };

    // Per ISO 32000-1 7.3.8.1 the keyword is followed by CRLF or LF; a lone CR
    // occurs in the wild and costs nothing to accept.
    match lex.peek() {
        Some(b'\r') => {
            lex.seek(lex.position() + 1);
            if lex.peek() == Some(b'\n') {
                lex.seek(lex.position() + 1);
            }
        }
        Some(b'\n') => lex.seek(lex.position() + 1),
        Some(_) => {}
        None => return Err(lex.eof()),
    }

    let data_start = lex.position();
    let declared = match dict.get(b"Length") {
        Some(Object::Integer(n)) => Some(*n),
        Some(Object::Ref(r)) => length_of(*r),
        _ => None,
    }
    .filter(|n| *n >= 0);

    let need_more = || LexError {
        offset: base + buf.len() as u64,
        kind: LexErrorKind::Eof,
    };
    // `endstream` must be gone looking for over a window that could hold it,
    // otherwise the search finds nothing and the keyword is just past the edge.
    let can_search = |from: usize| at_eof || from + ENDSTREAM_SLACK <= buf.len();
    let no_endstream = || LexError {
        offset: base + data_start as u64,
        kind: LexErrorKind::Syntax("stream has no endstream".into()),
    };

    let data_end = match declared {
        Some(len) => {
            let end = data_start.saturating_add(len as usize);
            if end <= buf.len() && endstream_follows(buf, end) {
                end
            } else if !can_search(end.min(buf.len())) {
                return Err(need_more());
            } else {
                // Either the declared length overruns the file or `endstream`
                // is not where it claims: recover by finding the keyword.
                scan_for_endstream(buf, data_start).ok_or_else(no_endstream)?
            }
        }
        None if !can_search(data_start) => return Err(need_more()),
        // A stream with no `endstream` anywhere is truncated. Taking the rest
        // of the file as its data would hand back a plausible wrong stream on
        // a document that otherwise looks clean.
        None => scan_for_endstream(buf, data_start).ok_or_else(no_endstream)?,
    };

    let raw = buf[data_start..data_end].to_vec();
    lex.seek(data_end);
    lex.eat_keyword(b"endstream");
    if !lex.eat_keyword(b"endobj") && !at_eof && lex.remaining().len() < ENDOBJ_SLACK {
        return Err(need_more());
    }
    let span = Span::new(start, lex.offset());
    Ok(Indirect {
        objref,
        object: Object::Stream(Stream { dict, raw }),
        span,
    })
}

fn endstream_follows(buf: &[u8], at: usize) -> bool {
    let mut i = at;
    while i < buf.len() && is_whitespace(buf[i]) {
        i += 1;
    }
    buf[i..].starts_with(b"endstream")
}

/// Locates the stream data end by searching for `endstream`, dropping the one
/// EOL that precedes the keyword.
fn scan_for_endstream(buf: &[u8], data_start: usize) -> Option<usize> {
    let idx = find(&buf[data_start..], b"endstream")? + data_start;
    let mut end = idx;
    if end > data_start && buf[end - 1] == b'\n' {
        end -= 1;
    }
    if end > data_start && buf[end - 1] == b'\r' {
        end -= 1;
    }
    Some(end)
}

/// Reads an object stream's header pairs: `/N` pairs of `number offset`, with
/// each offset relative to `/First`. Shared by on-demand access and by repair,
/// which must not disagree about what an object stream contains.
///
/// `count` is clamped against the data because it comes out of the file: each
/// pair needs at least four bytes of header.
pub(crate) fn object_stream_entries(data: &[u8], count: usize, first: usize) -> Vec<(u32, usize)> {
    let mut lex = Lexer::new(data, 0);
    let mut entries = Vec::new();
    for _ in 0..count.min(data.len()) {
        let (Ok(number), Ok(relative)) = (lex.read_unsigned(), lex.read_unsigned()) else {
            break;
        };
        let Ok(number) = u32::try_from(number) else {
            break;
        };
        entries.push((number, first.saturating_add(relative as usize)));
    }
    entries
}

pub(crate) fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(crate) fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(src: &[u8]) -> Object {
        Lexer::new(src, 0).parse_object().expect("parses")
    }

    #[test]
    fn parses_scalars() {
        assert_eq!(obj(b"true"), Object::Bool(true));
        assert_eq!(obj(b"null"), Object::Null);
        assert_eq!(obj(b"-42"), Object::Integer(-42));
        assert_eq!(obj(b"6."), Object::Real(6.0));
        assert_eq!(obj(b"-.5"), Object::Real(-0.5));
        assert_eq!(
            obj(b"/Name#20With#20Spaces").as_name().unwrap().0,
            b"Name With Spaces"
        );
    }

    #[test]
    fn parses_strings() {
        assert_eq!(obj(b"(a\\(b\\)c)"), Object::String(b"a(b)c".to_vec()));
        assert_eq!(obj(b"(\\101)"), Object::String(b"A".to_vec()));
        assert_eq!(obj(b"<48656C6C6F>"), Object::String(b"Hello".to_vec()));
        assert_eq!(obj(b"<4>"), Object::String(vec![0x40]));
    }

    #[test]
    fn distinguishes_references_from_integer_pairs() {
        assert_eq!(obj(b"3 0 R"), Object::Ref(ObjRef::new(3, 0)));
        assert_eq!(
            obj(b"[1 2 3]"),
            Object::Array(vec![
                Object::Integer(1),
                Object::Integer(2),
                Object::Integer(3)
            ])
        );
    }

    #[test]
    fn truncated_input_reports_eof_not_syntax() {
        let err = Lexer::new(b"<< /Type /Page", 0).parse_object().unwrap_err();
        assert!(err.is_eof());
    }

    #[test]
    fn recovers_stream_length_when_the_dictionary_lies() {
        let src = b"7 0 obj\n<< /Length 999 >>\nstream\nabcdef\nendstream\nendobj\n";
        let parsed = parse_indirect(src, 0, true, &|_| None).expect("parses");
        assert_eq!(parsed.object.as_stream().unwrap().raw, b"abcdef");
    }
}
