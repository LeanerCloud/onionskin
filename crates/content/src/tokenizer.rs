//! The content stream lexer: ISO 32000-2 7.8.2 operands and operators.
//!
//! Streaming and allocation-light by design, because a page description is
//! millions of operations on the documents that matter. Each [`Operation`]
//! carries the byte range it occupied in the decoded stream, from its first
//! operand through its operator, which is what lets a text run be traced back
//! to the bytes that produced it.
//!
//! The lexer never fails. A content stream is the most-often-damaged part of a
//! real PDF, and the operators before the damage are still the page's text, so
//! bytes that make no sense are skipped and reported through
//! [`Warning::LexerResync`] rather than thrown.

use onionskin_cos::{Dict, Name, Object, Span};

use crate::error::Warning;

/// Operand stack ceiling. `SCN` with a DeviceN colour space is the widest real
/// operator at 33 operands; past 64 the stream is malformed and the oldest
/// operands are the ones that cannot belong to the operator about to arrive.
const MAX_OPERANDS: usize = 64;

/// Nesting cap for arrays and dictionaries inside one operand.
const MAX_DEPTH: usize = 32;

/// Longest keyword this lexer has to recognise. Content stream operators stop
/// at three characters (`BDC`, `SCN`), but the same lexer reads CMap programs,
/// where `begincodespacerange` is 19.
const MAX_OPERATOR: usize = 24;

/// A content stream operator, inline rather than heap-allocated because a page
/// description is millions of them.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Operator {
    bytes: [u8; MAX_OPERATOR],
    len: u8,
}

impl Operator {
    /// Truncates past [`MAX_OPERATOR`]. Nothing that long is a keyword either
    /// syntax defines, so the value only has to stay distinguishable.
    fn new(token: &[u8]) -> Operator {
        let len = token.len().min(MAX_OPERATOR);
        let mut bytes = [0u8; MAX_OPERATOR];
        bytes[..len].copy_from_slice(&token[..len]);
        Operator {
            bytes,
            len: len as u8,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    pub fn is(&self, name: &[u8]) -> bool {
        self.as_bytes() == name
    }
}

impl std::fmt::Debug for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.as_bytes()))
    }
}

impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.as_bytes()))
    }
}

/// One operator and the operands it consumed.
#[derive(Clone, Debug)]
pub struct Operation {
    pub operator: Operator,
    pub operands: Vec<Object>,
    /// Byte range in the decoded stream covering the first operand through the
    /// end of the operator token. For an inline image this covers `BI` through
    /// `EI`, image data included.
    pub span: Span,
}

impl Operation {
    /// The last `n` operands, in order, or `None` when there are fewer.
    ///
    /// ISO 32000-2 7.8.2 gives an operator the operands immediately preceding
    /// it, which is not the same as the first `n` on the stack: a malformed
    /// stream that leaves a stray operand underneath would otherwise shift
    /// every operand by one and make `Tf` read a number as its font name.
    pub fn tail(&self, n: usize) -> Option<&[Object]> {
        self.operands
            .len()
            .checked_sub(n)
            .map(|from| &self.operands[from..])
    }

    /// The last `n` operands as numbers. `None` unless all of them are
    /// present and numeric, so a truncated `Td` moves nothing rather than
    /// moving to an invented origin.
    pub fn numbers<const N: usize>(&self) -> Option<[f64; N]> {
        let tail = self.tail(N)?;
        let mut out = [0.0; N];
        for (slot, operand) in out.iter_mut().zip(tail) {
            *slot = number(operand)?;
        }
        Some(out)
    }

    /// The single operand an operator takes, as a number.
    pub fn number(&self) -> Option<f64> {
        number(self.operands.last()?)
    }
}

pub fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

pub struct Tokenizer<'a> {
    data: &'a [u8],
    pos: usize,
    operands: Vec<Object>,
    /// Where each operand on the stack began, so an operator's span reaches
    /// back to the oldest operand still on the stack rather than to one that
    /// fell off it.
    operand_starts: Vec<usize>,
    max_operands: usize,
    pub warnings: Vec<Warning>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Tokenizer::with_operand_limit(data, MAX_OPERANDS)
    }

    /// CMap programs put a whole section's entries on the operand stack before
    /// their terminating operator, so they need a much higher ceiling than a
    /// page description does.
    pub fn with_operand_limit(data: &'a [u8], max_operands: usize) -> Self {
        Tokenizer {
            data,
            pos: 0,
            operands: Vec::new(),
            operand_starts: Vec::new(),
            max_operands: max_operands.max(1),
            warnings: Vec::new(),
        }
    }

    pub fn next_operation(&mut self) -> Option<Operation> {
        loop {
            self.skip_trivia();
            if self.pos >= self.data.len() {
                return None;
            }
            let start = self.pos;
            match self.data[self.pos] {
                b'/' => {
                    let object = self.name();
                    self.push(object, start);
                }
                b'(' => {
                    let object = self.literal_string();
                    self.push_parsed(object, start);
                }
                b'<' => {
                    if self.data.get(self.pos + 1) == Some(&b'<') {
                        let object = self.dictionary(0);
                        self.push_parsed(object, start);
                    } else {
                        let object = self.hex_string();
                        self.push_parsed(object, start);
                    }
                }
                b'[' => {
                    let object = self.array(0);
                    self.push_parsed(object, start);
                }
                b @ (b']' | b'>' | b')' | b'}' | b'{') => {
                    // A closer with no opener, or a PostScript function brace
                    // that leaked into a content stream. Neither can appear in
                    // a well-formed one, so both are damage worth reporting.
                    self.warnings.push(Warning::LexerResync {
                        offset: self.pos as u64,
                        detail: format!("stray {}", b as char),
                    });
                    self.pos += 1;
                }
                b'+' | b'-' | b'.' | b'0'..=b'9' => {
                    let object = self.number_token();
                    self.push_parsed(object, start);
                }
                _ => {
                    let token = self.keyword();
                    if token.is_empty() {
                        // Nothing consumed: a delimiter the arms above do not
                        // claim. Step past it so the loop always advances.
                        self.warnings.push(Warning::LexerResync {
                            offset: self.pos as u64,
                            detail: format!("byte 0x{:02x}", self.data[self.pos]),
                        });
                        self.pos += 1;
                        continue;
                    }
                    match token {
                        b"true" => self.push(Object::Bool(true), start),
                        b"false" => self.push(Object::Bool(false), start),
                        b"null" => self.push(Object::Null, start),
                        b"BI" => return Some(self.inline_image(start)),
                        _ => {
                            let operator = Operator::new(token);
                            let from = self.operand_starts.first().copied().unwrap_or(start);
                            self.operand_starts.clear();
                            return Some(Operation {
                                operator,
                                operands: std::mem::take(&mut self.operands),
                                span: Span::new(from as u64, self.pos as u64),
                            });
                        }
                    }
                }
            }
        }
    }

    fn push(&mut self, object: Object, start: usize) {
        if self.operands.len() >= self.max_operands {
            self.operands.remove(0);
            self.operand_starts.remove(0);
        }
        self.operands.push(object);
        self.operand_starts.push(start);
    }

    fn push_parsed(&mut self, object: Option<Object>, start: usize) {
        match object {
            Some(object) => self.push(object, start),
            None => {
                self.warnings.push(Warning::LexerResync {
                    offset: start as u64,
                    detail: "unterminated operand".into(),
                });
            }
        }
    }

    fn skip_trivia(&mut self) {
        while self.pos < self.data.len() {
            match self.data[self.pos] {
                b'%' => {
                    while self.pos < self.data.len()
                        && !matches!(self.data[self.pos], b'\r' | b'\n')
                    {
                        self.pos += 1;
                    }
                }
                b if is_whitespace(b) => self.pos += 1,
                _ => return,
            }
        }
    }

    fn keyword(&mut self) -> &'a [u8] {
        let start = self.pos;
        while self.pos < self.data.len() && is_regular(self.data[self.pos]) {
            self.pos += 1;
        }
        &self.data[start..self.pos]
    }

    fn name(&mut self) -> Object {
        self.pos += 1; // the slash
        let mut out = Vec::new();
        while self.pos < self.data.len() && is_regular(self.data[self.pos]) {
            let b = self.data[self.pos];
            if b == b'#' {
                let hi = self.data.get(self.pos + 1).and_then(hex_value);
                let lo = self.data.get(self.pos + 2).and_then(hex_value);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push(hi * 16 + lo);
                    self.pos += 3;
                    continue;
                }
            }
            out.push(b);
            self.pos += 1;
        }
        Object::Name(Name(out))
    }

    fn number_token(&mut self) -> Option<Object> {
        let start = self.pos;
        while self.pos < self.data.len() && is_regular(self.data[self.pos]) {
            self.pos += 1;
        }
        parse_number(&self.data[start..self.pos])
    }

    fn literal_string(&mut self) -> Option<Object> {
        self.pos += 1; // the paren
        let mut out = Vec::new();
        let mut depth = 1usize;
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            match b {
                b'\\' => {
                    let Some(&next) = self.data.get(self.pos) else {
                        break;
                    };
                    self.pos += 1;
                    match next {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'0'..=b'7' => {
                            let mut value = u32::from(next - b'0');
                            for _ in 0..2 {
                                match self.data.get(self.pos) {
                                    Some(d @ b'0'..=b'7') => {
                                        value = value * 8 + u32::from(d - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(value as u8);
                        }
                        // A backslash before an end-of-line joins the lines.
                        b'\n' => {}
                        b'\r' => {
                            if self.data.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(Object::String(out));
                    }
                    out.push(b);
                }
                // ISO 32000-2 7.3.4.2: any end-of-line in a literal string is
                // stored as a single line feed.
                b'\r' => {
                    if self.data.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                other => out.push(other),
            }
        }
        // Unterminated: keep what was read. Dropping it would lose the last
        // line of text on every stream truncated mid-string.
        Some(Object::String(out))
    }

    fn hex_string(&mut self) -> Option<Object> {
        self.pos += 1; // the angle bracket
        let mut nibbles = Vec::new();
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if let Some(value) = hex_value(&b) {
                nibbles.push(value);
            }
        }
        if nibbles.len() % 2 == 1 {
            nibbles.push(0);
        }
        Some(Object::String(
            nibbles.chunks(2).map(|p| p[0] * 16 + p[1]).collect(),
        ))
    }

    fn array(&mut self, depth: usize) -> Option<Object> {
        if depth >= MAX_DEPTH {
            return None;
        }
        self.pos += 1; // the bracket
        let mut items = Vec::new();
        loop {
            self.skip_trivia();
            let Some(&b) = self.data.get(self.pos) else {
                break;
            };
            if b == b']' {
                self.pos += 1;
                break;
            }
            match self.nested(depth) {
                Some(object) => items.push(object),
                None => break,
            }
        }
        Some(Object::Array(items))
    }

    fn dictionary(&mut self, depth: usize) -> Option<Object> {
        if depth >= MAX_DEPTH {
            return None;
        }
        self.pos += 2; // the double angle bracket
        let mut dict = Dict::new();
        loop {
            self.skip_trivia();
            let Some(&b) = self.data.get(self.pos) else {
                break;
            };
            if b == b'>' {
                self.pos += 1;
                if self.data.get(self.pos) == Some(&b'>') {
                    self.pos += 1;
                }
                break;
            }
            if b != b'/' {
                // A value where a key belongs. Skip one token and try again
                // rather than abandoning the rest of the dictionary.
                if self.nested(depth).is_none() {
                    break;
                }
                continue;
            }
            let Object::Name(key) = self.name() else {
                break;
            };
            self.skip_trivia();
            match self.nested(depth) {
                Some(value) => dict.set(key, value),
                None => break,
            }
        }
        Some(Object::Dict(dict))
    }

    /// One operand inside an array or dictionary. `None` means the container is
    /// unterminated or holds something that is not an object.
    fn nested(&mut self, depth: usize) -> Option<Object> {
        self.skip_trivia();
        let start = self.pos;
        let b = *self.data.get(self.pos)?;
        let object = match b {
            b'/' => Some(self.name()),
            b'(' => self.literal_string(),
            b'[' => self.array(depth + 1),
            b'<' => {
                if self.data.get(self.pos + 1) == Some(&b'<') {
                    self.dictionary(depth + 1)
                } else {
                    self.hex_string()
                }
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number_token(),
            b']' | b'>' | b')' | b'}' | b'{' => None,
            _ => match self.keyword() {
                b"true" => Some(Object::Bool(true)),
                b"false" => Some(Object::Bool(false)),
                b"null" | b"" => Some(Object::Null),
                // A bare keyword inside a container is junk; treat it as null
                // so the container's remaining entries survive.
                _ => Some(Object::Null),
            },
        };
        // Guarantee forward progress even when a branch consumed nothing.
        if self.pos == start {
            self.pos += 1;
        }
        object
    }

    /// `BI ... ID <binary> EI`. The image data is not retained: extraction has
    /// no use for it, and lexing past it correctly is the whole point.
    fn inline_image(&mut self, start: usize) -> Operation {
        let mut dict = Dict::new();
        loop {
            self.skip_trivia();
            let Some(&b) = self.data.get(self.pos) else {
                break;
            };
            if b == b'/' {
                let Object::Name(key) = self.name() else {
                    break;
                };
                match self.nested(0) {
                    Some(value) => dict.set(key, value),
                    None => break,
                }
                continue;
            }
            let at = self.pos;
            if self.keyword() == b"ID" {
                break;
            }
            if self.pos == at {
                self.pos += 1;
            }
        }
        // Exactly one whitespace byte separates ID from the data.
        if self.data.get(self.pos).copied().is_some_and(is_whitespace) {
            self.pos += 1;
        }

        let declared = dict
            .get(b"L")
            .or_else(|| dict.get(b"Length"))
            .and_then(Object::as_integer)
            .and_then(|l| usize::try_from(l).ok());
        let data_start = self.pos;
        self.pos = match declared {
            Some(length) if data_start + length <= self.data.len() => data_start + length,
            _ => self.find_ei(data_start),
        };
        // Consume the EI that follows, whether it came from /L or the scan.
        self.skip_trivia();
        let at = self.pos;
        if self.keyword() != b"EI" {
            self.pos = at;
        }

        self.operands.clear();
        self.operand_starts.clear();
        Operation {
            operator: Operator::new(b"BI"),
            operands: vec![Object::Dict(dict)],
            span: Span::new(start as u64, self.pos as u64),
        }
    }

    /// Finds the end of inline image data with no declared length: whitespace,
    /// `EI`, then a delimiter. Binary data containing those five bytes in that
    /// arrangement is the known failure mode, and the reason `/L` is preferred.
    fn find_ei(&self, from: usize) -> usize {
        let mut i = from;
        while i + 2 < self.data.len() {
            if is_whitespace(self.data[i])
                && self.data[i + 1] == b'E'
                && self.data[i + 2] == b'I'
                && self.data.get(i + 3).is_none_or(|b| !is_regular(*b))
            {
                return i;
            }
            i += 1;
        }
        self.data.len()
    }
}

pub fn is_whitespace(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

pub fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

fn hex_value(b: &u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Longest numeric token that keeps its own precision. Past this the extra
/// digits cannot change an f64 anyway.
const MAX_NUMBER: usize = 40;

/// Lenient number parsing. Producers emit `4.`, `.5`, `--3` and `6.02e23`;
/// every reader takes the leading well-formed part, and refusing here would
/// drop the text that operator positioned.
///
/// The digits are copied into a canonical buffer and handed to the standard
/// library rather than accumulated by hand, because digit-by-digit
/// accumulation makes `-.002` come out as `-0.0020000000000000005`, and a
/// glyph position is not a place to invent error.
fn parse_number(token: &[u8]) -> Option<Object> {
    let mut negative = false;
    let mut i = 0usize;
    while let Some(&b) = token.get(i) {
        match b {
            b'+' => i += 1,
            b'-' => {
                negative = !negative;
                i += 1;
            }
            _ => break,
        }
    }

    let mut buffer = [0u8; MAX_NUMBER];
    let mut len = 0usize;
    let mut integer_digits = 0usize;
    let mut fraction_digits = 0usize;
    let mut push = |b: u8, len: &mut usize| {
        if *len < MAX_NUMBER {
            buffer[*len] = b;
            *len += 1;
        }
    };
    while let Some(&b @ b'0'..=b'9') = token.get(i) {
        push(b, &mut len);
        integer_digits += 1;
        i += 1;
    }
    let has_point = token.get(i) == Some(&b'.');
    if has_point {
        i += 1;
        if len == 0 {
            push(b'0', &mut len);
        }
        push(b'.', &mut len);
        while let Some(&b @ b'0'..=b'9') = token.get(i) {
            push(b, &mut len);
            fraction_digits += 1;
            i += 1;
        }
        if fraction_digits == 0 {
            push(b'0', &mut len);
        }
    }
    if integer_digits == 0 && fraction_digits == 0 {
        return None;
    }

    let text = std::str::from_utf8(&buffer[..len]).ok()?;
    if !has_point {
        if let Ok(value) = text.parse::<i64>() {
            return Some(Object::Integer(if negative { -value } else { value }));
        }
    }
    let value = text.parse::<f64>().ok()?;
    Some(Object::Real(if negative { -value } else { value }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(data: &[u8]) -> Vec<Operation> {
        let mut lexer = Tokenizer::new(data);
        let mut out = Vec::new();
        while let Some(op) = lexer.next_operation() {
            out.push(op);
        }
        out
    }

    #[test]
    fn spans_cover_operands_through_operator() {
        let data = b"BT /F1 18 Tf ET";
        let out = ops(data);
        assert_eq!(out.len(), 3);
        assert!(out[1].operator.is(b"Tf"));
        let span = out[1].span;
        assert_eq!(&data[span.start as usize..span.end as usize], b"/F1 18 Tf");
    }

    #[test]
    fn text_showing_operands_survive_escapes() {
        let out = ops(br"(a\(b\)c\101\n) Tj");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].operands, vec![Object::String(b"a(b)cA\n".to_vec())]);
    }

    #[test]
    fn tj_array_mixes_strings_and_kerns() {
        let out = ops(b"[(A) -250 (B)] TJ");
        let Object::Array(items) = &out[0].operands[0] else {
            panic!("not an array");
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[1], Object::Integer(-250));
    }

    #[test]
    fn inline_image_data_is_stepped_over_not_lexed() {
        // The image data holds bytes that look like operators and an unbalanced
        // paren; the Tj after EI is what proves the lexer resynchronised.
        let data = b"BI /W 2 /H 2 ID \x00Tj ( \xff\xfe EI (after) Tj";
        let out = ops(data);
        assert_eq!(out.len(), 2);
        assert!(out[0].operator.is(b"BI"));
        assert!(out[1].operator.is(b"Tj"));
        assert_eq!(out[1].operands, vec![Object::String(b"after".to_vec())]);
    }

    #[test]
    fn inline_image_honours_a_declared_length() {
        let data = b"BI /L 4 ID \xffEI\xff EI (x) Tj";
        let out = ops(data);
        assert_eq!(out.len(), 2);
        assert!(out[1].operator.is(b"Tj"));
    }

    #[test]
    fn comments_are_not_operators() {
        let out = ops(b"% Tj is a comment\n(real) Tj");
        assert_eq!(out.len(), 1);
        assert!(out[0].operator.is(b"Tj"));
    }

    #[test]
    fn lenient_numbers() {
        assert_eq!(parse_number(b"4."), Some(Object::Real(4.0)));
        assert_eq!(parse_number(b".5"), Some(Object::Real(0.5)));
        assert_eq!(parse_number(b"-.002"), Some(Object::Real(-0.002)));
        assert_eq!(parse_number(b"--3"), Some(Object::Integer(3)));
        assert_eq!(parse_number(b"12"), Some(Object::Integer(12)));
        assert_eq!(parse_number(b"."), None);
        assert_eq!(parse_number(b"Tj"), None);
    }

    #[test]
    fn an_overflowing_operand_stack_spans_only_the_operands_it_kept() {
        // Every token here is two bytes wide, so the expected span start is
        // arithmetic rather than a guess.
        const EXTRA: usize = 6;
        let data = "1 ".repeat(MAX_OPERANDS + EXTRA) + "Tj";
        let out = ops(data.as_bytes());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].operands.len(), MAX_OPERANDS);
        assert_eq!(
            out[0].span.start,
            (EXTRA * 2) as u64,
            "the span reaches back to operands that fell off the stack"
        );
    }

    #[test]
    fn a_stream_of_garbage_terminates() {
        let mut lexer = Tokenizer::new(b")>]}{ \x01\x02 ( unterminated");
        while lexer.next_operation().is_some() {}
        assert!(!lexer.warnings.is_empty());
    }

    #[test]
    fn nested_dictionary_operand_parses() {
        let out = ops(b"/OC << /Type /OCMD /OCGs [1 2] >> BDC");
        assert!(out[0].operator.is(b"BDC"));
        assert_eq!(out[0].operands.len(), 2);
        assert!(matches!(out[0].operands[1], Object::Dict(_)));
    }
}
