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

/// The new text drawn before placed glyph `before`, in `font` or a
/// standard font, with the pen put back afterwards.
pub(crate) fn insertion(
    editing: &Editing,
    text: &str,
    font: &Font,
    font_name: &Name,
    spacing: &Spacing,
    before: usize,
) -> std::result::Result<(Insertion, Option<&'static str>), String> {
    if let Some(codes) = own_codes(editing, text, font) {
        let advance = codes
            .iter()
            .map(|code| skip(spacing, *code, font.displacement(*code)))
            .sum();
        let bytes: Vec<u8> = codes.iter().flat_map(|code| code_bytes(*code)).collect();
        let mut draw = b"[".to_vec();
        draw.extend(hex(&bytes));
        draw.extend_from_slice(b"] TJ");
        return Ok((
            Insertion {
                before,
                draw,
                advance,
            },
            None,
        ));
    }
    let face = standard_face(&font.base_font);
    let undrawable: String = text
        .chars()
        .filter(|ch| *ch != '?' && encode_win_ansi(&ch.to_string()) == b"?")
        .collect();
    if !undrawable.is_empty() {
        return Err(undrawable);
    }
    let bytes = encode_win_ansi(text);
    let advance = bytes
        .iter()
        .map(|byte| {
            let code = Code {
                value: u32::from(*byte),
                cid: u32::from(*byte),
                len: 1,
            };
            let width = standard_text_width(face, &[*byte]).map(|w| w / 1000.0);
            skip(spacing, code, width)
        })
        .sum();
    let size = crate::redact::number(spacing.size);
    let mut draw = format!("/{} {size} Tf [", name_text(&editing.fallback_name(face))).into_bytes();
    draw.extend(hex(&bytes));
    draw.extend_from_slice(format!("] TJ /{} {size} Tf", name_text(font_name)).as_bytes());
    Ok((
        Insertion {
            before,
            draw,
            advance,
        },
        Some(face),
    ))
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
