//! Editing a line of text where it is: its glyphs taken out, and the new
//! text drawn in their place, from where the first of them was.
//!
//! The same interpreter that extracts text walks the page, the way
//! redaction does, so the glyphs that go are exactly the ones extraction
//! named. The operator that drew the line's first glyph is rewritten to
//! draw the new text there, in the line's own font and graphics state, and
//! then to put the pen back, so anything the operator or the text object
//! draws next lands where it always did. The rest of the line's glyphs are
//! stepped over, as redaction steps over what it removes.
//!
//! **Which font.** The line's own font, when it can draw every character:
//! through the codes the page already drew with it (which a subset is sure
//! to hold), or through its encoding when it is not a subset. Otherwise a
//! standard font of the same family, weight and slant, in WinAnsiEncoding,
//! under a resource name the caller gives and adds to the page. Text that
//! neither can draw is refused, never drawn as question marks.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Document, Name};

use crate::font::{encode_win_ansi, standard_text_width, Code, Font, FontId};
use crate::interpret;
use crate::page::Page;
use crate::redact::text::{hex, Insertion, Placed, Spacing};
use crate::redact::Rewritten;

/// A line to rewrite.
#[derive(Debug, Clone, PartialEq)]
pub struct LineEdit {
    /// The line's glyphs, as `(run, glyph)` in the page's extraction order.
    /// The new text starts where the first of them was.
    pub glyphs: Vec<(usize, usize)>,
    pub text: String,
    pub style: TextStyle,
}

/// How the new text looks, where it differs from the line's own.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextStyle {
    /// A standard font to set it in, in place of the line's.
    pub face: Option<&'static str>,
    /// A size in place of the line's.
    pub size: Option<f64>,
    /// An RGB fill colour, each part 0 to 1, in place of the line's.
    pub fill: Option<[f64; 3]>,
}

/// A page's content with its lines rewritten.
#[derive(Debug, Clone, PartialEq)]
pub struct EditedPage {
    pub content: Rewritten,
    /// The standard fonts some of the text is set in, because a line's own
    /// font could not draw it, and the resource names the content uses for
    /// them. The caller adds them to the page's fonts.
    pub fallbacks: Vec<(Name, &'static str)>,
}

/// Why a line could not be rewritten.
#[derive(Debug)]
pub enum EditError {
    Content(crate::Error),
    /// Characters no font available here can draw.
    Undrawable(String),
    /// A line's first glyph was not found where extraction put it.
    NotFound,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Content(error) => write!(f, "{error}"),
            EditError::Undrawable(characters) => {
                write!(f, "no font available can draw {characters:?}")
            }
            EditError::NotFound => write!(f, "the line is no longer where it was"),
        }
    }
}

impl std::error::Error for EditError {}

impl From<crate::Error> for EditError {
    fn from(error: crate::Error) -> Self {
        EditError::Content(error)
    }
}

/// Every character each font on the page was seen drawing, and the code it
/// drew it with.
pub(crate) type Seen = BTreeMap<FontId, BTreeMap<char, Code>>;

/// One line's new text, while the interpreter edits.
pub(crate) struct Pending {
    pub(crate) first: (usize, usize),
    pub(crate) text: String,
    pub(crate) style: TextStyle,
    /// Set once the text is drawn: the standard font it needed, or why it
    /// could not be drawn.
    pub(crate) outcome: Option<std::result::Result<Option<&'static str>, String>>,
}

/// What the interpreter carries while it edits.
pub(crate) struct Editing {
    pub(crate) lines: Vec<Pending>,
    pub(crate) fallback_prefix: String,
    pub(crate) seen: Seen,
}

impl Editing {
    /// The resource name standard font `face` is written under.
    pub(crate) fn fallback_name(&self, face: &str) -> Name {
        Name::new(&format!("{}{face}", self.fallback_prefix))
    }
}

/// Rewrite the lines `edits` names on `page`. A standard font is named
/// `fallback_prefix` followed by its own name, so the caller picks a prefix
/// no font resource of the page starts with.
pub fn edit_lines(
    doc: &Document,
    page: &Page,
    edits: &[LineEdit],
    fallback_prefix: &str,
) -> std::result::Result<EditedPage, EditError> {
    let mut targets = BTreeSet::new();
    let mut lines = Vec::new();
    for edit in edits {
        let first = *edit.glyphs.first().ok_or(EditError::NotFound)?;
        targets.extend(edit.glyphs.iter().copied());
        lines.push(Pending {
            first,
            text: edit.text.clone(),
            style: edit.style,
            outcome: None,
        });
    }
    let editing = Editing {
        lines,
        fallback_prefix: fallback_prefix.to_owned(),
        seen: interpret::seen_codes(doc, page)?,
    };
    let (content, editing) = interpret::edit_page(doc, page, targets, editing)?;
    let mut faces = BTreeSet::new();
    let mut undrawable = String::new();
    for line in &editing.lines {
        match &line.outcome {
            None => return Err(EditError::NotFound),
            Some(Err(characters)) => undrawable.push_str(characters),
            Some(Ok(face)) => faces.extend(*face),
        }
    }
    if !undrawable.is_empty() {
        return Err(EditError::Undrawable(undrawable));
    }
    Ok(EditedPage {
        content,
        fallbacks: faces
            .into_iter()
            .map(|face| (editing.fallback_name(face), face))
            .collect(),
    })
}

/// What the line's state was when its first glyph was drawn: its font by
/// resource name, its size and spacing, and the operators that set its fill
/// colour.
pub(crate) struct LineState<'a> {
    pub(crate) font: &'a Font,
    pub(crate) font_name: &'a Name,
    pub(crate) spacing: &'a Spacing,
    pub(crate) fill: &'a [u8],
}

/// The new text drawn before placed glyph `before`, in the line's font or
/// a standard one, at its size or `style`'s, with the font, size, colour
/// and pen put back afterwards.
pub(crate) fn insertion(
    editing: &Editing,
    text: &str,
    style: &TextStyle,
    line: &LineState<'_>,
    before: usize,
) -> std::result::Result<(Insertion, Option<&'static str>), String> {
    let (bytes, widths, face) =
        match own_codes(editing, text, line.font).filter(|_| style.face.is_none()) {
            Some(codes) => (
                codes
                    .iter()
                    .flat_map(|code| code_bytes(*code))
                    .collect::<Vec<u8>>(),
                codes
                    .iter()
                    .map(|code| (*code, line.font.displacement(*code)))
                    .collect::<Vec<_>>(),
                None,
            ),
            None => {
                let face = style
                    .face
                    .unwrap_or_else(|| standard_face(&line.font.base_font));
                let (bytes, widths) = standard_codes(text, face)?;
                (bytes, widths, Some(face))
            }
        };
    let size = style.size.unwrap_or(line.spacing.size);
    let drawn = Spacing {
        size,
        ..*line.spacing
    };
    // `TJ` numbers are thousandths of the size in force, which is the
    // line's own once it is put back.
    let rescale = if line.spacing.size == 0.0 {
        1.0
    } else {
        size / line.spacing.size
    };
    let advance = widths
        .iter()
        .map(|(code, width)| skip(&drawn, *code, *width) * rescale)
        .sum();
    Ok((
        Insertion {
            before,
            draw: draw(editing, style, line, &bytes, face, size),
            advance,
        },
        face,
    ))
}

/// The operators that draw `bytes`, and put the line's state back.
fn draw(
    editing: &Editing,
    style: &TextStyle,
    line: &LineState<'_>,
    bytes: &[u8],
    face: Option<&'static str>,
    size: f64,
) -> Vec<u8> {
    let number = crate::redact::number;
    let mut out = Vec::new();
    if let Some([r, g, b]) = style.fill {
        out.extend_from_slice(format!("{} {} {} rg ", number(r), number(g), number(b)).as_bytes());
    }
    let switched = face.is_some() || size != line.spacing.size;
    if switched {
        let name = face.map_or_else(
            || line.font_name.clone(),
            |face| editing.fallback_name(face),
        );
        out.extend_from_slice(format!("/{} {} Tf ", name_text(&name), number(size)).as_bytes());
    }
    out.push(b'[');
    out.extend(hex(bytes));
    out.extend_from_slice(b"] TJ");
    if switched {
        out.extend_from_slice(
            format!(
                " /{} {} Tf",
                name_text(line.font_name),
                number(line.spacing.size)
            )
            .as_bytes(),
        );
    }
    if style.fill.is_some() {
        out.push(b' ');
        if line.fill.is_empty() {
            out.extend_from_slice(b"0 g");
        } else {
            out.extend_from_slice(line.fill);
        }
    }
    out
}

/// `text` in WinAnsiEncoding for standard font `face`, with each code's
/// width, or the characters it cannot draw.
#[allow(clippy::type_complexity)]
fn standard_codes(
    text: &str,
    face: &str,
) -> std::result::Result<(Vec<u8>, Vec<(Code, Option<f64>)>), String> {
    let undrawable: String = text
        .chars()
        .filter(|ch| *ch != '?' && encode_win_ansi(&ch.to_string()) == b"?")
        .collect();
    if !undrawable.is_empty() {
        return Err(undrawable);
    }
    let bytes = encode_win_ansi(text);
    let widths = bytes
        .iter()
        .map(|byte| {
            let code = Code {
                value: u32::from(*byte),
                cid: u32::from(*byte),
                len: 1,
            };
            (
                code,
                standard_text_width(face, &[*byte]).map(|w| w / 1000.0),
            )
        })
        .collect();
    Ok((bytes, widths))
}

/// The codes `font` draws the text with, if it can draw all of it.
fn own_codes(editing: &Editing, text: &str, font: &Font) -> Option<Vec<Code>> {
    let seen = editing.seen.get(&font.id);
    text.chars()
        .map(|ch| {
            seen.and_then(|codes| codes.get(&ch).copied())
                .or_else(|| font.code_for(ch))
        })
        .collect()
}

fn skip(spacing: &Spacing, code: Code, advance: Option<f64>) -> f64 {
    spacing
        .skip(&Placed {
            part: 0,
            code,
            advance,
            removed: true,
        })
        .unwrap_or(0.0)
}

fn code_bytes(code: Code) -> Vec<u8> {
    let bytes = code.value.to_be_bytes();
    bytes[4 - usize::from(code.len).clamp(1, 4)..].to_vec()
}

fn name_text(name: &Name) -> String {
    String::from_utf8_lossy(name.as_bytes()).into_owned()
}

/// The standard font nearest `base_font`: its family by name (a serif, a
/// monospace, or sans), then bold and italic by name.
pub fn standard_face(base_font: &str) -> &'static str {
    let name = base_font.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
    let bold = has(&["bold", "black", "heavy", "semibold", "demi"]);
    let italic = has(&["italic", "oblique"]);
    let family = if has(&["courier", "mono", "consol", "code"]) {
        0
    } else if has(&[
        "times", "serif", "roman", "georgia", "garamond", "minion", "cambria",
    ]) && !has(&["sans"])
    {
        1
    } else {
        2
    };
    const FACES: [[&str; 4]; 3] = [
        [
            "Courier",
            "Courier-Bold",
            "Courier-Oblique",
            "Courier-BoldOblique",
        ],
        [
            "Times-Roman",
            "Times-Bold",
            "Times-Italic",
            "Times-BoldItalic",
        ],
        [
            "Helvetica",
            "Helvetica-Bold",
            "Helvetica-Oblique",
            "Helvetica-BoldOblique",
        ],
    ];
    FACES[family][usize::from(bold) + 2 * usize::from(italic)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_is_chosen_by_family_weight_and_slant() {
        assert_eq!(standard_face("Arial"), "Helvetica");
        assert_eq!(standard_face("Arial-BoldMT"), "Helvetica-Bold");
        assert_eq!(standard_face("TimesNewRomanPS-ItalicMT"), "Times-Italic");
        assert_eq!(standard_face("DejaVuSerif-BoldItalic"), "Times-BoldItalic");
        assert_eq!(standard_face("DejaVuSans"), "Helvetica");
        assert_eq!(standard_face("CourierNewPS-BoldMT"), "Courier-Bold");
        assert_eq!(standard_face("SourceCodePro-Oblique"), "Courier-Oblique");
    }

    #[test]
    fn a_code_is_written_in_as_many_bytes_as_it_was_read() {
        let code = |value, len| Code {
            value,
            cid: value,
            len,
        };
        assert_eq!(code_bytes(code(0x41, 1)), [0x41]);
        assert_eq!(code_bytes(code(0x0041, 2)), [0x00, 0x41]);
        assert_eq!(code_bytes(code(0x41, 0)), [0x41]);
    }
}
