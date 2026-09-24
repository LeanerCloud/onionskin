//! Editing text: a line rewritten, and find and replace.
//!
//! Both work in two halves. What a page's content becomes is worked out
//! from the document as it is, through `content`'s line editing, which
//! rewrites the showing operators where they are; then the page is given
//! that content as one new stream, and any standard font the new text
//! needed, in one undo step. Marked content is kept as it was, so a tagged
//! page's structure still finds its text.
//!
//! What is refused, never guessed at: a line with a glyph whose character
//! is unknown, text inside a form XObject (editing it would change every
//! page that draws the form), and characters no font here can draw.

use std::collections::BTreeMap;
use std::ops::Range;

use onionskin_content::edit_text::{edit_lines, EditError, LineEdit};
use onionskin_content::{text_lines, TextLine};
use onionskin_cos::{flate_encode, Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

use crate::edit::Transaction;
use crate::pages::{dict_at, page_ref, resolve};
use crate::{Error, PageIndex, Result};

/// What resource names for standard fonts start with. A page font already
/// named so moves the prefix along.
const FALLBACK_PREFIX: &str = "OSF";

/// A page's new content, worked out and ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct PageEdit {
    pub page: PageIndex,
    content: Vec<u8>,
    fallbacks: Vec<(Name, &'static str)>,
}

/// How find and replace matches, as the Find toolbar's options say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// One occurrence of the text looked for, in a line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineMatch {
    pub page: PageIndex,
    /// The line, as [`page_lines`] numbers it.
    pub line: usize,
    /// Where in the line's text.
    pub range: Range<usize>,
}

fn refused(why: impl Into<String>) -> Error {
    Error::TextEdit(why.into())
}

/// Page `page`'s lines of text, as the document has them.
pub fn page_lines(doc: &CosDocument, page: PageIndex) -> Result<Vec<TextLine>> {
    Ok(text_lines(&onionskin_content::extract_page(doc, page)?))
}

/// Page `page` with each `(line, text)` rewritten.
pub fn rewrite_lines(
    doc: &CosDocument,
    page: PageIndex,
    lines: &[(&TextLine, String)],
) -> Result<PageEdit> {
    if let Some((line, _)) = lines.iter().find(|(line, _)| !line.is_mapped()) {
        return Err(refused(format!(
            "the line {:?} has characters whose text is unknown, so it cannot be edited",
            line.text
        )));
    }
    let loaded = onionskin_content::page(doc, page)?;
    let prefix = free_prefix(doc, &loaded.resources);
    let edits: Vec<LineEdit> = lines
        .iter()
        .map(|(line, text)| LineEdit {
            glyphs: line.glyphs.iter().map(|glyph| glyph.at).collect(),
            text: text.clone(),
        })
        .collect();
    let edited = edit_lines(doc, &loaded, &edits, &prefix).map_err(|error| match error {
        EditError::Content(error) => Error::from(error),
        other => refused(format!("the text cannot be written: {other}")),
    })?;
    if !edited.content.resources.is_empty() {
        return Err(refused(
            "the text is inside a form drawn on the page, and editing it is not supported",
        ));
    }
    Ok(PageEdit {
        page,
        content: edited.content.bytes,
        fallbacks: edited.fallbacks,
    })
}

/// A prefix no font resource of the page starts with.
fn free_prefix(doc: &CosDocument, resources: &Dict) -> String {
    let fonts = resources
        .get(b"Font")
        .and_then(|value| doc.resolve(value).ok())
        .and_then(|value| value.as_dict().cloned())
        .unwrap_or_default();
    let taken = |prefix: &str| {
        fonts
            .iter()
            .any(|(name, _)| name.as_bytes().starts_with(prefix.as_bytes()))
    };
    (0..)
        .map(|n| {
            if n == 0 {
                FALLBACK_PREFIX.to_owned()
            } else {
                format!("{FALLBACK_PREFIX}{n}_")
            }
        })
        .find(|prefix| !taken(prefix))
        .expect("a free prefix")
}

/// Every occurrence of `needle` on the pages `pages`, line by line.
pub fn find_in_lines(
    doc: &CosDocument,
    pages: impl IntoIterator<Item = PageIndex>,
    needle: &str,
    options: MatchOptions,
) -> Result<Vec<LineMatch>> {
    let mut found = Vec::new();
    for page in pages {
        for (line, text_line) in page_lines(doc, page)?.iter().enumerate() {
            for range in occurrences(&text_line.text, needle, options) {
                found.push(LineMatch { page, line, range });
            }
        }
    }
    Ok(found)
}

/// Where `needle` occurs in `text`, not overlapping, under `options`.
pub fn occurrences(text: &str, needle: &str, options: MatchOptions) -> Vec<Range<usize>> {
    let fold = |ch: char| {
        if options.case_sensitive {
            ch
        } else {
            ch.to_lowercase().next().unwrap_or(ch)
        }
    };
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let wanted: Vec<char> = needle.chars().map(fold).collect();
    let mut found = Vec::new();
    if wanted.is_empty() {
        return found;
    }
    let mut at = 0;
    while at + wanted.len() <= chars.len() {
        let hit = chars[at..at + wanted.len()]
            .iter()
            .zip(&wanted)
            .all(|((_, ch), want)| fold(*ch) == *want);
        let end = at + wanted.len();
        let bounded = !options.whole_word
            || (at == 0 || !chars[at - 1].1.is_alphanumeric())
                && (end == chars.len() || !chars[end].1.is_alphanumeric());
        if hit && bounded {
            let stop = chars.get(end).map_or(text.len(), |(index, _)| *index);
            found.push(chars[at].0..stop);
            at = end;
        } else {
            at += 1;
        }
    }
    found
}

/// The pages' new content with `matches` replaced by `replacement`, a page
/// at a time.
pub fn replace_matches(
    doc: &CosDocument,
    matches: &[LineMatch],
    replacement: &str,
) -> Result<Vec<PageEdit>> {
    let mut by_page: BTreeMap<PageIndex, BTreeMap<usize, Vec<Range<usize>>>> = BTreeMap::new();
    for found in matches {
        by_page
            .entry(found.page)
            .or_default()
            .entry(found.line)
            .or_default()
            .push(found.range.clone());
    }
    let mut edits = Vec::new();
    for (page, lines) in by_page {
        let text_lines = page_lines(doc, page)?;
        let mut rewrites = Vec::new();
        for (index, mut ranges) in lines {
            let line = text_lines
                .get(index)
                .ok_or_else(|| refused("the text is no longer where it was found"))?;
            ranges.sort_by_key(|range| range.start);
            let mut text = String::new();
            let mut from = 0;
            for range in ranges {
                let kept = line
                    .text
                    .get(from..range.start)
                    .ok_or_else(|| refused("the text is no longer where it was found"))?;
                text.push_str(kept);
                text.push_str(replacement);
                from = range.end;
            }
            text.push_str(line.text.get(from..).unwrap_or_default());
            rewrites.push((line, text));
        }
        edits.push(rewrite_lines(doc, page, &rewrites)?);
    }
    Ok(edits)
}

/// Give the page its new content, as one stream, and the standard fonts
/// the new text is set in.
pub fn write_page_edit(tx: &mut Transaction<'_>, edit: &PageEdit) -> Result<()> {
    let page_object = page_ref(tx, edit.page)?;
    let mut page = dict_at(tx, page_object)?;
    let raw = flate_encode(&edit.content);
    let mut dict = Dict::new();
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    dict.set(Name::new("Length"), Object::Integer(raw.len() as i64));
    let number = tx.reserve();
    tx.put_object(number, 0, Object::Stream(Stream { dict, raw }))?;
    page.set(Name::new("Contents"), Object::Ref(ObjRef::new(number, 0)));
    if !edit.fallbacks.is_empty() {
        let inherited = crate::pages::inherited_resources(tx, page_object)?;
        let mut resources = dict_of(tx, inherited.as_ref())?;
        let mut fonts = dict_of(tx, resources.get(b"Font"))?;
        for (name, face) in &edit.fallbacks {
            let font = standard_font(face);
            let number = tx.reserve();
            tx.put_object(number, 0, Object::Dict(font))?;
            fonts.set(name.clone(), Object::Ref(ObjRef::new(number, 0)));
        }
        resources.set(Name::new("Font"), Object::Dict(fonts));
        page.set(Name::new("Resources"), Object::Dict(resources));
    }
    tx.put_object(
        page_object.number,
        page_object.generation,
        Object::Dict(page),
    )
}

fn dict_of(tx: &Transaction<'_>, value: Option<&Object>) -> Result<Dict> {
    Ok(match resolve(tx, value)? {
        Some(Object::Dict(dict)) => dict,
        _ => Dict::new(),
    })
}

fn standard_font(face: &str) -> Dict {
    let mut font = Dict::new();
    font.set(Name::new("Type"), Object::name("Font"));
    font.set(Name::new("Subtype"), Object::name("Type1"));
    font.set(Name::new("BaseFont"), Object::name(face));
    font.set(Name::new("Encoding"), Object::name("WinAnsiEncoding"));
    font
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found<'a>(
        text: &'a str,
        needle: &str,
        case_sensitive: bool,
        whole_word: bool,
    ) -> Vec<&'a str> {
        occurrences(
            text,
            needle,
            MatchOptions {
                case_sensitive,
                whole_word,
            },
        )
        .into_iter()
        .map(|range| &text[range])
        .collect()
    }

    #[test]
    fn occurrences_follow_case_and_whole_word() {
        assert_eq!(
            found("Cat cat scatter", "cat", false, false),
            ["Cat", "cat", "cat"]
        );
        assert_eq!(found("Cat cat scatter", "cat", true, false), ["cat", "cat"]);
        assert_eq!(found("Cat cat scatter", "cat", false, true), ["Cat", "cat"]);
        assert_eq!(
            found("aaaa", "aa", true, false),
            ["aa", "aa"],
            "not overlapping"
        );
        assert_eq!(found("Ünïcode ÜNÏ", "ünï", false, false), ["Ünï", "ÜNÏ"]);
        assert!(found("text", "", false, false).is_empty());
        assert!(found("te", "text", false, false).is_empty());
    }
}
