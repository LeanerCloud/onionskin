//! The graphics and text state machine of ISO 32000-2 8.4 and 9.4.
//!
//! Only what extraction needs runs here. Painting operators are consumed and
//! discarded rather than rejected: a page whose path operators errored would
//! be a page with no text, which is the failure this whole crate exists to
//! avoid.

use std::collections::BTreeMap;
use std::rc::Rc;

use onionskin_cos::{Dict, Document, ObjRef, Object, Span};
use onionskin_plugin_api::{PageIndex, PageQuad};

use crate::error::{Result, Warning};
use crate::font::{self, Code, Font, FontId};
use crate::matrix::Matrix;
use crate::page::{self, Content, ContentPart, Page};
use crate::run::{ByteProvenance, Glyph, Mapping, PageText, TextRun};
use crate::tokenizer::{Operation, Tokenizer};

/// Nesting cap for `Do`. ISO 32000-2 sets no limit; real documents nest three
/// or four deep, and past this the file is trying something else.
const MAX_XOBJECT_DEPTH: usize = 12;

/// `q` depth cap. The spec's minimum is 28; this is generous for real content
/// and bounds what a `q`-only stream can allocate.
const MAX_GSTACK: usize = 1024;

/// Glyphs kept per page. A dense page holds a few thousand; this is three
/// orders of magnitude of headroom and it bounds a hostile stream's memory.
const MAX_GLYPHS: usize = 1_000_000;

/// Open marked-content sequences tracked at once. Tagged documents nest a
/// handful; a stream with more than this is not describing a structure.
const MAX_MARKED: usize = 256;

/// A `TJ` adjustment moving the pen forward by at least this much of an em is
/// a word gap rather than a kern.
///
/// TeX and most typesetters emit no space character at all: the space between
/// two words is a number in the `TJ` array, and without this rule every such
/// document extracts as `Fortestingthealignment`. Real intra-word kerns run to
/// a few hundredths of an em; inter-word gaps start around a quarter. A fifth
/// of an em separates the two with room on both sides, and is the same
/// threshold `search::flatten` uses between runs.
const WORD_GAP_EM: f64 = 0.2;

/// Vertical writing advances one em per glyph by default, from the `/DW2`
/// default `[880 -1000]` of ISO 32000-2 9.7.4.3. Per-glyph `/W2` is not read.
const VERTICAL_ADVANCE: f64 = -1.0;
/// The `/DW2` default position vector: the glyph's vertical origin sits this
/// far above the baseline point.
const VERTICAL_ORIGIN: f64 = 0.88;

#[derive(Clone)]
struct GState {
    ctm: Matrix,
    font: Option<Rc<Font>>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    /// `Tz` as a factor, so 100 becomes 1.0.
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
    render_mode: i64,
}

impl GState {
    fn new(ctm: Matrix) -> GState {
        GState {
            ctm,
            font: None,
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
        }
    }
}

/// Text object state. `None` outside `BT`/`ET`; a showing operator outside a
/// text object is malformed, and the fallback is an identity text matrix,
/// which is what every reader does.
#[derive(Clone, Copy)]
struct TextObject {
    tm: Matrix,
    tlm: Matrix,
}

impl TextObject {
    fn new() -> TextObject {
        TextObject {
            tm: Matrix::IDENTITY,
            tlm: Matrix::IDENTITY,
        }
    }

    fn set_line(&mut self, m: Matrix) {
        self.tlm = m;
        self.tm = m;
    }

    fn next_line(&mut self, tx: f64, ty: f64) {
        self.set_line(Matrix::translate(tx, ty).then(&self.tlm));
    }
}

#[derive(Default)]
struct FontCache {
    by_ref: BTreeMap<ObjRef, Rc<Font>>,
    direct: BTreeMap<Vec<u8>, Rc<Font>>,
    next_direct: u32,
}

/// Extracts one page's text.
pub fn page_text(doc: &Document, page: &Page) -> Result<PageText> {
    let mut warnings = Vec::new();
    let content = page::content(doc, page, &mut warnings)?;

    let mut interpreter = Interpreter {
        doc,
        page: page.index,
        fonts: FontCache::default(),
        runs: Vec::new(),
        warnings,
        unmapped: BTreeMap::new(),
        missing_widths: BTreeMap::new(),
        glyphs: 0,
        fontless: 0,
        stack: Vec::new(),
    };
    let state = GState::new(page.base_ctm());
    interpreter.run(&content, &page.resources, state, 0);
    Ok(interpreter.finish())
}

struct Interpreter<'a> {
    doc: &'a Document,
    page: PageIndex,
    fonts: FontCache,
    runs: Vec<TextRun>,
    warnings: Vec<Warning>,
    unmapped: BTreeMap<String, usize>,
    missing_widths: BTreeMap<String, usize>,
    glyphs: usize,
    /// Showing operators that ran with no font selected.
    fontless: usize,
    /// Form XObjects currently being executed, for cycle detection.
    stack: Vec<ObjRef>,
}

impl Interpreter<'_> {
    fn finish(mut self) -> PageText {
        if self.glyphs >= MAX_GLYPHS {
            self.warnings
                .push(Warning::GlyphLimit { limit: MAX_GLYPHS });
        }
        if self.fontless > 0 {
            self.warnings.push(Warning::TextWithoutFont {
                count: self.fontless,
            });
        }
        for (font, count) in std::mem::take(&mut self.unmapped) {
            self.warnings.push(Warning::UnmappedGlyphs { font, count });
        }
        for (font, count) in std::mem::take(&mut self.missing_widths) {
            self.warnings.push(Warning::MissingWidths { font, count });
        }
        PageText {
            page: self.page,
            runs: self.runs,
            warnings: self.warnings,
        }
    }

    fn run(&mut self, content: &Content, resources: &Dict, initial: GState, depth: usize) {
        let mut lexer = Tokenizer::new(&content.bytes);
        let mut state = initial;
        let mut saved: Vec<GState> = Vec::new();
        // `q` operators past the cap are counted rather than stored, so their
        // matching `Q` pops nothing instead of popping somebody else's state.
        let mut unsaved = 0usize;
        let mut text: Option<TextObject> = None;
        // One entry per open marked-content sequence, holding its
        // `/ActualText` until a showing operator inside it consumes it.
        let mut marked: Vec<Sequence> = Vec::new();

        while let Some(op) = lexer.next_operation() {
            match op.operator.as_bytes() {
                b"q" => {
                    if saved.len() < MAX_GSTACK {
                        saved.push(state.clone());
                    } else {
                        unsaved += 1;
                    }
                }
                b"Q" => {
                    if unsaved > 0 {
                        unsaved -= 1;
                    } else if let Some(previous) = saved.pop() {
                        state = previous;
                    }
                }
                b"cm" => {
                    if let Some(m) = Matrix::from_operands(&op.operands) {
                        let next = m.then(&state.ctm);
                        // A degenerate cm turns every later quad into NaN,
                        // which is worse than ignoring the operator.
                        if next.is_finite() {
                            state.ctm = next;
                        }
                    }
                }
                b"gs" => self.ext_gstate(&op, resources, &mut state, depth),

                b"BT" => text = Some(TextObject::new()),
                b"ET" => text = None,

                b"Tc" => state.char_spacing = op.number().unwrap_or(state.char_spacing),
                b"Tw" => state.word_spacing = op.number().unwrap_or(state.word_spacing),
                b"Tz" => {
                    if let Some(percent) = op.number() {
                        state.horizontal_scale = percent / 100.0;
                    }
                }
                b"TL" => state.leading = op.number().unwrap_or(state.leading),
                b"Ts" => state.rise = op.number().unwrap_or(state.rise),
                b"Tr" => {
                    if let Some(mode) = op.operands.last().and_then(Object::as_integer) {
                        state.render_mode = mode;
                    }
                }
                b"Tf" => {
                    if let Some([name, size]) = op.tail(2) {
                        // A negative size is legal and mirrors the glyph; an
                        // absent one leaves the size alone.
                        if let Some(size) = crate::tokenizer::number(size) {
                            state.size = size;
                        }
                        if let Object::Name(name) = name {
                            state.font = self.font(resources, name.as_bytes());
                        }
                    }
                }

                b"Td" => {
                    if let Some([tx, ty]) = op.numbers::<2>() {
                        text.get_or_insert_with(TextObject::new).next_line(tx, ty);
                    }
                }
                b"TD" => {
                    if let Some([tx, ty]) = op.numbers::<2>() {
                        state.leading = -ty;
                        text.get_or_insert_with(TextObject::new).next_line(tx, ty);
                    }
                }
                b"Tm" => {
                    if let Some(m) = Matrix::from_operands(&op.operands) {
                        if m.is_finite() {
                            text.get_or_insert_with(TextObject::new).set_line(m);
                        }
                    }
                }
                b"T*" => {
                    let leading = state.leading;
                    text.get_or_insert_with(TextObject::new)
                        .next_line(0.0, -leading);
                }

                b"Tj" => {
                    let object = op.operands.last().cloned();
                    if let Some(Object::String(s)) = object {
                        let text = text.get_or_insert_with(TextObject::new);
                        self.show(content, &op, &[Show::Text(s)], &state, text, &mut marked);
                    }
                }
                b"TJ" => {
                    let Some(Object::Array(items)) = op.operands.last().cloned() else {
                        continue;
                    };
                    let parts: Vec<Show> = items
                        .iter()
                        .filter_map(|item| match item {
                            Object::String(s) => Some(Show::Text(s.clone())),
                            other => crate::tokenizer::number(other).map(Show::Adjust),
                        })
                        .collect();
                    let text = text.get_or_insert_with(TextObject::new);
                    self.show(content, &op, &parts, &state, text, &mut marked);
                }
                b"'" => {
                    let Some(Object::String(s)) = op.operands.last().cloned() else {
                        continue;
                    };
                    let leading = state.leading;
                    let text = text.get_or_insert_with(TextObject::new);
                    text.next_line(0.0, -leading);
                    self.show(content, &op, &[Show::Text(s)], &state, text, &mut marked);
                }
                b"\"" => {
                    let Some(Object::String(s)) = op.operands.last().cloned() else {
                        continue;
                    };
                    // aw ac string "
                    if let Some([word, char_, _]) = op.tail(3) {
                        state.word_spacing =
                            crate::tokenizer::number(word).unwrap_or(state.word_spacing);
                        state.char_spacing =
                            crate::tokenizer::number(char_).unwrap_or(state.char_spacing);
                    }
                    let leading = state.leading;
                    let text = text.get_or_insert_with(TextObject::new);
                    text.next_line(0.0, -leading);
                    self.show(content, &op, &[Show::Text(s)], &state, text, &mut marked);
                }

                b"Do" => {
                    let Some(Object::Name(name)) = op.operands.last() else {
                        continue;
                    };
                    let name = name.as_bytes().to_vec();
                    self.form_xobject(&name, resources, &state, depth);
                }

                b"BDC" | b"BMC" => {
                    if marked.len() < MAX_MARKED {
                        marked.push(self.sequence(&op, resources));
                    }
                }
                b"EMC" => {
                    marked.pop();
                }

                // Everything else is a painting, colour or clipping operator.
                // Extraction has no use for it and skipping it is not an error.
                _ => {}
            }
        }
        self.warnings.append(&mut lexer.warnings);
    }

    fn ext_gstate(&mut self, op: &Operation, resources: &Dict, state: &mut GState, depth: usize) {
        let Some(Object::Name(name)) = op.operands.last() else {
            return;
        };
        let Some(gs) = self.lookup(resources, b"ExtGState", name.as_bytes()) else {
            return;
        };
        let Some(gs) = gs.as_dict().cloned() else {
            return;
        };

        // /Font is [font size]; the font is always an indirect reference.
        if let Some(Ok(Object::Array(entry))) = gs.get(b"Font").map(|o| self.doc.resolve(o)) {
            if let Some(size) = entry.get(1).and_then(crate::tokenizer::number) {
                state.size = size;
            }
            if let Some(objref) = entry.first().and_then(Object::as_reference) {
                state.font = self.font_by_ref(objref);
            }
        }

        // A luminosity soft mask's group is where pgf and TikZ put the glyphs
        // a reader actually sees: the visible ink is a shading painted through
        // letter-shaped holes. Text that only exists in a mask is still text
        // on the page, and a search or a redaction that missed it would be
        // wrong in the direction that matters.
        if let Some(Ok(Object::Dict(smask))) = gs.get(b"SMask").map(|o| self.doc.resolve(o)) {
            if let Some(group) = smask.get(b"G").and_then(Object::as_reference) {
                self.run_form(group, resources, state, depth);
            }
        }
    }

    fn form_xobject(&mut self, name: &[u8], resources: &Dict, state: &GState, depth: usize) {
        let Some(entry) = resources
            .get(b"XObject")
            .and_then(|o| self.doc.resolve(o).ok())
            .and_then(|o| o.as_dict().and_then(|d| d.get(name).cloned()))
        else {
            return;
        };
        let Some(objref) = entry.as_reference() else {
            return;
        };
        self.run_form(objref, resources, state, depth);
    }

    /// Executes a form XObject's operators under the caller's state.
    fn run_form(&mut self, objref: ObjRef, resources: &Dict, state: &GState, depth: usize) {
        if self.stack.contains(&objref) {
            self.warnings.push(Warning::XObjectCycle { object: objref });
            return;
        }
        if depth >= MAX_XOBJECT_DEPTH {
            self.warnings
                .push(Warning::XObjectDepthExceeded { object: objref });
            return;
        }
        let Ok(parsed) = self.doc.get(objref.number) else {
            return;
        };
        let Some(stream) = parsed.object.as_stream() else {
            return;
        };
        // Image XObjects reach here too; only forms hold operators.
        if subtype(&stream.dict).as_deref() != Some(b"Form".as_slice()) {
            return;
        }
        let decoded =
            match crate::filter::decode(&stream.dict, &stream.raw, &|o| self.doc.resolve(o)) {
                Ok(d) => d,
                Err(e) => {
                    self.warnings.push(Warning::ContentPartFailed {
                        stream: objref,
                        detail: e.to_string(),
                    });
                    return;
                }
            };

        let mut inner = state.clone();
        if let Some(m) = stream
            .dict
            .get(b"Matrix")
            .and_then(|o| self.doc.resolve(o).ok())
            .and_then(|o| o.as_array().and_then(Matrix::from_operands))
        {
            let next = m.then(&inner.ctm);
            if next.is_finite() {
                inner.ctm = next;
            }
        }
        // The form's own /Resources when it has them, the caller's when it
        // does not. /BBox is a clip; text outside it is not extracted by
        // Acrobat either, but dropping text on a bad /BBox is the worse
        // failure, so it is deliberately not applied here.
        let inner_resources = stream
            .dict
            .get(b"Resources")
            .and_then(|o| self.doc.resolve(o).ok())
            .and_then(|o| o.as_dict().cloned())
            .unwrap_or_else(|| resources.clone());

        let content = Content {
            parts: vec![ContentPart {
                stream: objref,
                origin: parsed.origin,
                range: Span::new(0, decoded.len() as u64),
            }],
            bytes: decoded,
        };

        self.stack.push(objref);
        self.run(&content, &inner_resources, inner, depth + 1);
        self.stack.pop();
    }

    // ---- text showing -------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        content: &Content,
        op: &Operation,
        parts: &[Show],
        state: &GState,
        text: &mut TextObject,
        marked: &mut [Sequence],
    ) {
        let Some(font) = state.font.clone() else {
            // Nothing was drawn, so there is no text to extract. Saying so is
            // the point: a verifier reporting "no text here" has to be able to
            // tell an empty page from one it could not read.
            self.fontless += 1;
            return;
        };
        let vertical = font.vertical;
        let (ascent, descent) = font.extents();

        let mut out = String::new();
        let mut glyphs = Vec::new();
        for part in parts {
            match part {
                Show::Adjust(amount) => {
                    let ems = -amount / 1000.0;
                    let shift = ems * state.size;
                    let (tx, ty) = if vertical {
                        (0.0, shift)
                    } else {
                        (shift * state.horizontal_scale, 0.0)
                    };
                    text.tm = Matrix::translate(tx, ty).then(&text.tm);
                    // Vertical text advances down the page, so there a gap is
                    // the adjustment pushing further negative, not further
                    // positive.
                    let gap = if vertical { -ems } else { ems };
                    if gap >= WORD_GAP_EM && !out.is_empty() && !out.ends_with(char::is_whitespace)
                    {
                        out.push(' ');
                    }
                }
                Show::Text(bytes) => {
                    for code in font.decode(bytes) {
                        if self.glyphs >= MAX_GLYPHS {
                            break;
                        }
                        let advance = match font.displacement(code) {
                            Some(a) => a,
                            None => {
                                *self
                                    .missing_widths
                                    .entry(font.base_font.clone())
                                    .or_default() += 1;
                                0.0
                            }
                        };
                        let trm = Matrix::new(
                            state.size * state.horizontal_scale,
                            0.0,
                            0.0,
                            state.size,
                            0.0,
                            state.rise,
                        )
                        .then(&text.tm)
                        .then(&state.ctm);

                        let quad = self.quad(&trm, advance, ascent, descent, vertical);
                        let mapping = match font.unicode(code) {
                            Some(t) if !t.is_empty() => {
                                let start = out.len();
                                out.push_str(&t);
                                Mapping::Text(start..out.len())
                            }
                            _ => {
                                *self.unmapped.entry(font.base_font.clone()).or_default() += 1;
                                Mapping::Unmapped
                            }
                        };
                        glyphs.push(Glyph {
                            code: code.value,
                            cid: code.cid,
                            quad,
                            mapping,
                        });
                        self.glyphs += 1;

                        advance_text(text, state, code, advance, vertical);
                    }
                }
            }
        }

        if glyphs.is_empty() {
            return;
        }
        // ISO 32000-2 14.9.4: an /ActualText span says what its content really
        // spells, whatever its glyphs are addressed by. Accessible documents
        // use it exactly where the glyph codes are meaningless, so honouring it
        // is the difference between extracting the text and extracting nothing.
        //
        // The replacement belongs to the whole sequence, not to one operator
        // in it. The first showing operator inside the span carries it; every
        // later one keeps its glyphs positioned and contributes no text, which
        // is what stops a two-operator span from spelling the replacement and
        // then the tail it replaced.
        if let Some(span) = marked.iter_mut().rev().find_map(Sequence::actual_text) {
            match span.take() {
                Some(actual) => {
                    for glyph in &mut glyphs {
                        // Every glyph stands for the whole replacement, so
                        // selecting any part of it highlights all of them.
                        glyph.mapping = Mapping::Text(0..actual.len());
                    }
                    out = actual;
                }
                None => {
                    for glyph in &mut glyphs {
                        glyph.mapping = Mapping::Text(0..0);
                    }
                    out.clear();
                }
            }
        }

        let Some((provenance, clamped)) = provenance(content, op.span) else {
            return;
        };
        if clamped {
            let warning = Warning::ProvenanceClamped {
                stream: provenance.stream,
            };
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
        self.runs.push(TextRun {
            page: self.page,
            text: out,
            glyphs,
            provenance,
            font: font.id,
            font_name: font.base_font.clone(),
            size: state.size,
            render_mode: state.render_mode,
        });
    }

    /// The glyph's selection box, in `/QuadPoints` corner order: upper-left,
    /// upper-right, lower-left, lower-right, all relative to the direction the
    /// text runs rather than to the page, so a rotated run's quad stays a
    /// rectangle the caller can fill.
    fn quad(
        &self,
        trm: &Matrix,
        advance: f64,
        ascent: f64,
        descent: f64,
        vertical: bool,
    ) -> PageQuad {
        let (x0, x1, y0, y1) = if vertical {
            (
                -advance / 2.0,
                advance / 2.0,
                VERTICAL_ORIGIN + VERTICAL_ADVANCE,
                VERTICAL_ORIGIN,
            )
        } else {
            (0.0, advance, descent, ascent)
        };
        PageQuad {
            page: self.page,
            corners: [
                trm.apply(x0, y1),
                trm.apply(x1, y1),
                trm.apply(x0, y0),
                trm.apply(x1, y0),
            ],
        }
    }

    /// Classifies a `BMC` or `BDC` by whether it carries an `/ActualText`,
    /// whether that property list is written inline or named through the
    /// resource dictionary's `/Properties`.
    fn sequence(&self, op: &Operation, resources: &Dict) -> Sequence {
        match self.actual_text(op, resources) {
            Some(text) => Sequence::Actual(Some(text)),
            None => Sequence::Plain,
        }
    }

    fn actual_text(&self, op: &Operation, resources: &Dict) -> Option<String> {
        let properties = match op.operands.last()? {
            Object::Dict(d) => d.clone(),
            Object::Name(n) => self
                .lookup(resources, b"Properties", n.as_bytes())?
                .as_dict()
                .cloned()?,
            _ => return None,
        };
        let Object::String(bytes) = self.doc.resolve(properties.get(b"ActualText")?).ok()? else {
            return None;
        };
        let text = font::pdf_text_string(&bytes);
        (!text.is_empty()).then_some(text)
    }

    // ---- resources ----------------------------------------------------------

    fn lookup(&self, resources: &Dict, category: &[u8], name: &[u8]) -> Option<Object> {
        let dict = self.doc.resolve(resources.get(category)?).ok()?;
        let entry = dict.as_dict()?.get(name)?.clone();
        self.doc.resolve(&entry).ok()
    }

    fn font(&mut self, resources: &Dict, name: &[u8]) -> Option<Rc<Font>> {
        let entry = self
            .doc
            .resolve(resources.get(b"Font")?)
            .ok()?
            .as_dict()?
            .get(name)?
            .clone();

        if let Some(objref) = entry.as_reference() {
            return self.font_by_ref(objref);
        }
        // A font dictionary written straight into the resources. Keyed by
        // resource name, which is all that distinguishes one from another.
        if let Some(hit) = self.fonts.direct.get(name) {
            return Some(Rc::clone(hit));
        }
        let dict = entry.as_dict()?;
        let id = FontId::Direct(self.fonts.next_direct);
        self.fonts.next_direct += 1;
        let loaded = Rc::new(font::load(self.doc, dict, id, &mut self.warnings));
        self.report_font(&loaded);
        self.fonts.direct.insert(name.to_vec(), Rc::clone(&loaded));
        Some(loaded)
    }

    fn font_by_ref(&mut self, objref: ObjRef) -> Option<Rc<Font>> {
        if let Some(hit) = self.fonts.by_ref.get(&objref) {
            return Some(Rc::clone(hit));
        }
        let parsed = match self.doc.get(objref.number) {
            Ok(p) => p,
            Err(e) => {
                self.warnings.push(Warning::FontLoadFailed {
                    resource: objref.number.to_string(),
                    detail: e.to_string(),
                });
                return None;
            }
        };
        let Some(dict) = parsed.object.as_dict() else {
            self.warnings.push(Warning::FontLoadFailed {
                resource: objref.number.to_string(),
                detail: "font resource is not a dictionary".into(),
            });
            return None;
        };
        let loaded = Rc::new(font::load(
            self.doc,
            dict,
            FontId::Object(objref),
            &mut self.warnings,
        ));
        self.report_font(&loaded);
        self.fonts.by_ref.insert(objref, Rc::clone(&loaded));
        Some(loaded)
    }

    fn report_font(&mut self, font: &Font) {
        if let Some(name) = font.unsupported_cmap() {
            let warning = Warning::UnsupportedCMap {
                name: name.to_string(),
            };
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
    }
}

enum Show {
    Text(Vec<u8>),
    Adjust(f64),
}

/// One open marked-content sequence.
#[derive(Debug, PartialEq, Eq)]
enum Sequence {
    Plain,
    /// An `/ActualText` sequence. The `Option` is the replacement until a
    /// showing operator inside the sequence takes it; afterwards the sequence
    /// is still open and still suppresses text, which is what distinguishes
    /// "not this sequence" from "this sequence, already spelled".
    Actual(Option<String>),
}

impl Sequence {
    fn actual_text(&mut self) -> Option<&mut Option<String>> {
        match self {
            Sequence::Plain => None,
            Sequence::Actual(text) => Some(text),
        }
    }
}

/// ISO 32000-2 9.4.4: the displacement after showing one glyph.
fn advance_text(text: &mut TextObject, state: &GState, code: Code, advance: f64, vertical: bool) {
    let word = if code.is_word_space() {
        state.word_spacing
    } else {
        0.0
    };
    if vertical {
        let ty = VERTICAL_ADVANCE * state.size + state.char_spacing + word;
        text.tm = Matrix::translate(0.0, ty).then(&text.tm);
    } else {
        let tx = (advance * state.size + state.char_spacing + word) * state.horizontal_scale;
        text.tm = Matrix::translate(tx, 0.0).then(&text.tm);
    }
}

/// Turns an operator's span in the concatenated buffer into a span in the one
/// stream that holds it.
/// A span that runs past its part is clamped, and `true` is returned so the
/// caller can say so. ISO 32000-2 7.8.2 lets a page divide its content between
/// streams at any token boundary, so an operator can legally sit in a later
/// part than its operands; `ByteProvenance` names one stream, so such a run's
/// range reaches the end of the part it starts in and no further.
fn provenance(content: &Content, span: Span) -> Option<(ByteProvenance, bool)> {
    let (part, start) = content.locate(span.start as usize)?;
    let clamped = span.end > part.range.end;
    let end = span.end.min(part.range.end) - part.range.start;
    Some((
        ByteProvenance {
            stream: part.stream,
            origin: part.origin,
            decoded: Span::new(start, end.max(start)),
        },
        clamped,
    ))
}

fn subtype(dict: &Dict) -> Option<Vec<u8>> {
    match dict.get(b"Subtype")? {
        Object::Name(n) => Some(n.as_bytes().to_vec()),
        _ => None,
    }
}
