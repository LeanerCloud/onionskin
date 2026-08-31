//! The find engine behind the viewer's Ctrl+F and, later, search-and-redact.
//!
//! Deliberately dumb: a literal substring search over the page's runs in
//! document order, with the options Acrobat's find bar and the current-document
//! half of its Advanced Search have. No stemming, no fuzzy matching, no layout
//! analysis. What it does owe the caller is exact provenance for every hit, so
//! a highlight lands on the right glyphs and a redaction rewrites the right
//! bytes.
//!
//! One thing it does not do, and the reason it is only half of the bidi story:
//! a hit has to sit in one run. A producer that emits a right-to-left line as
//! several visually ordered runs stores it in an order no substring search over
//! the joined text can find, and reordering visual runs back to logical order
//! needs a bidi implementation. Folding the presentation forms (below) covers
//! the single-run case; `known-issues.md` carries the rest.

use std::ops::Range;

use unicode_normalization::UnicodeNormalization as _;

use crate::run::{ByteProvenance, PageText, TextRun};
use crate::{PageIndex, PageQuad};

/// Acrobat's "Return Results Containing", for the current document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MatchMode {
    /// Match Exact Word Or Phrase: the needle, verbatim.
    #[default]
    Phrase,
    /// Any Of The Words: every occurrence of every word in the needle.
    AnyWord,
    /// All Of The Words: the same, but only on a page carrying every word.
    AllWords,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    /// The match must not be flanked by an alphanumeric character.
    pub whole_word: bool,
    pub mode: MatchMode,
}

/// One hit, with everything a caller needs to draw it and to change it.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub page: PageIndex,
    /// The matched text as it appears in the document, not as it was typed.
    pub text: String,
    /// Byte range in [`Flattened::text`].
    pub range: Range<usize>,
    pub quads: Vec<PageQuad>,
    /// Every content stream range that contributed a glyph to this hit. More
    /// than one when a hit straddles two showing operators.
    pub provenance: Vec<ByteProvenance>,
}

/// A page's runs joined into one string, with the map back to the runs.
#[derive(Clone, Debug, Default)]
pub struct Flattened {
    pub page: PageIndex,
    pub text: String,
    /// `(range in `text`, index into the page's runs)`, in document order.
    pieces: Vec<(Range<usize>, usize)>,
}

impl PageText {
    /// The page's text with the runs joined, and the map back from a byte
    /// offset in that string to the run it came from.
    pub fn flatten(&self) -> Flattened {
        flatten(self)
    }
}

impl Flattened {
    /// Runs overlapping a byte range of [`Flattened::text`], each with that
    /// range expressed in the run's own text.
    pub fn runs_for<'a>(
        &'a self,
        page: &'a PageText,
        range: Range<usize>,
    ) -> Vec<(&'a TextRun, Range<usize>)> {
        self.pieces
            .iter()
            .filter(|(piece, _)| piece.start < range.end && range.start < piece.end)
            .filter_map(|(piece, index)| {
                let run = page.runs.get(*index)?;
                let local = range.start.saturating_sub(piece.start)
                    ..(range.end.min(piece.end) - piece.start);
                Some((run, local))
            })
            .collect()
    }
}

/// Joins a page's runs into one searchable string.
///
/// Where a separator goes is the one place this crate looks at geometry.
/// Producers that position every word separately (TeX, matplotlib) emit no
/// space characters at all, so joining their runs with nothing would spell
/// `helloworld`. The rule: a run that starts on a different baseline than the
/// previous one ended on begins a new line, and a run separated by a visible
/// gap gets one space. Everything else is joined directly.
pub fn flatten(page: &PageText) -> Flattened {
    let mut out = Flattened {
        page: page.page,
        ..Default::default()
    };
    let mut previous: Option<&TextRun> = None;
    for (index, run) in page.runs.iter().enumerate() {
        if run.text.is_empty() {
            continue;
        }
        if let Some(previous) = previous {
            out.text.push_str(separator(previous, run));
        }
        let start = out.text.len();
        out.text.push_str(&run.text);
        out.pieces.push((start..out.text.len(), index));
        previous = Some(run);
    }
    out
}

fn separator(previous: &TextRun, next: &TextRun) -> &'static str {
    if previous.text.ends_with(char::is_whitespace) || next.text.starts_with(char::is_whitespace) {
        return "";
    }
    let (Some(last), Some(first)) = (previous.glyphs.last(), next.glyphs.first()) else {
        return "";
    };
    // Lower-right of the glyph just drawn, lower-left of the one about to be.
    let from = last.quad.corners[3];
    let to = first.quad.corners[2];
    let baseline = (
        last.quad.corners[3].0 - last.quad.corners[2].0,
        last.quad.corners[3].1 - last.quad.corners[2].1,
    );
    let height = {
        let dx = last.quad.corners[0].0 - last.quad.corners[2].0;
        let dy = last.quad.corners[0].1 - last.quad.corners[2].1;
        (dx * dx + dy * dy).sqrt()
    };
    if height <= 0.0 || !height.is_finite() {
        return " ";
    }
    let length = (baseline.0 * baseline.0 + baseline.1 * baseline.1).sqrt();
    // A zero-width last glyph leaves no direction to measure against.
    let unit = if length > 0.0 {
        (baseline.0 / length, baseline.1 / length)
    } else {
        (1.0, 0.0)
    };
    let delta = (to.0 - from.0, to.1 - from.1);
    let along = delta.0 * unit.0 + delta.1 * unit.1;
    let across = -delta.0 * unit.1 + delta.1 * unit.0;

    if across.abs() > 0.5 * height {
        return "\n";
    }
    // Forward by a fifth of the line height reads as a word gap; backwards by
    // more than half means the producer jumped, not kerned.
    if along > 0.2 * height || along < -0.5 * height {
        return " ";
    }
    ""
}

/// Finds every occurrence of `needle` on the page.
pub fn search(page: &PageText, needle: &str, options: SearchOptions) -> Vec<Match> {
    let flat = flatten(page);
    search_flattened(page, &flat, needle, options)
}

pub fn search_flattened(
    page: &PageText,
    flat: &Flattened,
    needle: &str,
    options: SearchOptions,
) -> Vec<Match> {
    if needle.is_empty() {
        return Vec::new();
    }
    // The page folds once however many words are searched for: the fold walks
    // every character of the page, and a three-word search would otherwise
    // walk it three times.
    let folded = fold(&flat.text, options.case_sensitive);
    match options.mode {
        MatchMode::Phrase => find_all(page, flat, &folded, needle, options),
        MatchMode::AnyWord | MatchMode::AllWords => {
            let mut out = Vec::new();
            for word in needle.split_whitespace() {
                let hits = find_all(page, flat, &folded, word, options);
                if hits.is_empty() && options.mode == MatchMode::AllWords {
                    return Vec::new();
                }
                out.extend(hits);
            }
            out.sort_by_key(|hit| (hit.range.start, hit.range.end));
            out.dedup_by(|a, b| a.range == b.range);
            out
        }
    }
}

/// Every occurrence of one literal needle, in document order, against the
/// page folded once by the caller.
fn find_all(
    page: &PageText,
    flat: &Flattened,
    folded: &(String, Vec<usize>),
    needle: &str,
    options: SearchOptions,
) -> Vec<Match> {
    if needle.is_empty() {
        return Vec::new();
    }
    let (haystack, offsets) = folded;
    let pattern = fold(needle, options.case_sensitive).0;
    if pattern.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(hit) = haystack[from..].find(&pattern) {
        let start = from + hit;
        let end = start + pattern.len();
        // Advance by one byte-boundary so overlapping occurrences are all
        // found; `find` on a UTF-8 boundary guarantees `start + 1` is safe to
        // clamp up to the next boundary.
        from = next_boundary(haystack, start + 1);

        // Folding is not length preserving and not even character preserving:
        // the Turkish dotted capital I folds to two code points, so a needle
        // can match a strict prefix of one source character's folded form. The
        // hit is that whole character - mapping the folded end straight back
        // would name an empty range inside it, which is a match with no text
        // and no quads.
        let range = {
            let (Some(first), Some(last)) = (offsets.get(start), offsets.get(end - 1)) else {
                continue;
            };
            let width = flat.text[*last..].chars().next().map_or(0, char::len_utf8);
            *first..(last + width).min(flat.text.len())
        };
        if range.start >= range.end {
            continue;
        }
        if options.whole_word && !is_whole_word(&flat.text, &range) {
            continue;
        }

        let mut quads = Vec::new();
        let mut provenance = Vec::new();
        for (run, local) in flat.runs_for(page, range.clone()) {
            quads.extend(run.quads_for(local));
            if !provenance.contains(&run.provenance) {
                provenance.push(run.provenance);
            }
        }
        out.push(Match {
            page: page.page,
            text: flat.text[range.clone()].to_string(),
            range,
            quads,
            provenance,
        });
    }
    out
}

/// Folds a string for matching and records, for every byte of the result, the
/// byte offset it came from. Neither fold below is length preserving, so a hit
/// in the folded string has to be translated back before it can name glyphs.
///
/// Two folds, both applied one character at a time:
///
/// * **Compatibility composition (NFKC), always.** A producer stores what it
///   shaped, so Arabic arrives as Presentation Forms-A and -B and `fi` arrives
///   as one ligature glyph. A user types base letters. Folding both sides to
///   the base letters is what makes the typed word match the drawn one.
/// * **Lowercasing, unless the caller asked for case sensitivity.** Case is
///   the only thing that option is about; a case-sensitive search still has to
///   fold presentation forms, because the alphabets that use them have no case.
///
/// Per character rather than over the whole string on purpose: NFKC composes
/// across character boundaries, and a fold that merged two source characters
/// into one would leave the map unable to name either.
fn fold(text: &str, case_sensitive: bool) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len() + 1);
    for (offset, ch) in text.char_indices() {
        // ASCII is NFKC-stable, and most text is ASCII.
        if ch.is_ascii() {
            push_folded(&mut folded, ch, case_sensitive);
        } else {
            for composed in std::iter::once(ch).nfkc() {
                push_folded(&mut folded, composed, case_sensitive);
            }
        }
        map.resize(folded.len(), offset);
    }
    map.push(text.len());
    (folded, map)
}

fn push_folded(out: &mut String, ch: char, case_sensitive: bool) {
    if case_sensitive {
        out.push(ch);
    } else {
        out.extend(ch.to_lowercase());
    }
}

fn next_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn is_whole_word(text: &str, range: &Range<usize>) -> bool {
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontId;
    use crate::run::{Glyph, Mapping};
    use onionskin_cos::{ObjRef, Origin, Span};

    fn run(text: &str, x: f64, y: f64) -> TextRun {
        let glyphs: Vec<Glyph> = text
            .char_indices()
            .enumerate()
            .map(|(i, (offset, ch))| Glyph {
                code: ch as u32,
                cid: ch as u32,
                quad: PageQuad {
                    page: 0,
                    corners: [
                        (x + i as f64 * 10.0, y + 10.0),
                        (x + (i + 1) as f64 * 10.0, y + 10.0),
                        (x + i as f64 * 10.0, y),
                        (x + (i + 1) as f64 * 10.0, y),
                    ],
                },
                mapping: Mapping::Text(offset..offset + ch.len_utf8()),
            })
            .collect();
        TextRun {
            page: 0,
            text: text.to_string(),
            glyphs,
            provenance: ByteProvenance {
                stream: ObjRef::new(1, 0),
                origin: Origin::File(Span::new(0, 1)),
                decoded: Span::new(0, 1),
            },
            font: FontId::Object(ObjRef::new(9, 0)),
            font_name: "Test".into(),
            size: 10.0,
            render_mode: 0,
        }
    }

    fn page(runs: Vec<TextRun>) -> PageText {
        PageText {
            page: 0,
            runs,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn adjacent_runs_join_without_a_separator() {
        let p = page(vec![run("Hel", 0.0, 0.0), run("lo", 30.0, 0.0)]);
        assert_eq!(flatten(&p).text, "Hello");
    }

    #[test]
    fn a_visible_gap_becomes_one_space() {
        let p = page(vec![run("Hello", 0.0, 0.0), run("world", 60.0, 0.0)]);
        assert_eq!(flatten(&p).text, "Hello world");
    }

    #[test]
    fn a_new_baseline_becomes_a_newline() {
        let p = page(vec![run("Hello", 0.0, 20.0), run("world", 0.0, 0.0)]);
        assert_eq!(flatten(&p).text, "Hello\nworld");
    }

    #[test]
    fn case_insensitive_by_default_and_exact_on_request() {
        let p = page(vec![run("Hello World", 0.0, 0.0)]);
        assert_eq!(search(&p, "hello", SearchOptions::default()).len(), 1);
        assert_eq!(
            search(
                &p,
                "hello",
                SearchOptions {
                    case_sensitive: true,
                    ..SearchOptions::default()
                }
            )
            .len(),
            0
        );
    }

    #[test]
    fn whole_word_rejects_a_substring_hit() {
        let p = page(vec![run("the theatre", 0.0, 0.0)]);
        let options = SearchOptions {
            whole_word: true,
            ..SearchOptions::default()
        };
        let hits = search(&p, "the", options);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].range, 0..3);
        assert_eq!(search(&p, "the", SearchOptions::default()).len(), 2);
    }

    #[test]
    fn a_hit_spanning_two_runs_reports_both_sources() {
        let p = page(vec![run("Hel", 0.0, 0.0), run("lo", 30.0, 0.0)]);
        let hits = search(&p, "ello", SearchOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "ello");
        assert_eq!(hits[0].quads.len(), 4);
    }

    #[test]
    fn quads_come_back_for_the_matched_characters_only() {
        let p = page(vec![run("abcdef", 0.0, 0.0)]);
        let hits = search(&p, "cd", SearchOptions::default());
        assert_eq!(hits[0].quads.len(), 2);
        assert_eq!(hits[0].quads[0].corners[2].0, 20.0);
    }

    #[test]
    fn folding_maps_back_through_a_length_changing_character() {
        // Turkish dotted capital I lowercases to two code points.
        let (folded, map) = fold("A\u{0130}B", false);
        assert!(folded.len() > "A\u{0130}B".len() - 1);
        assert_eq!(map[0], 0);
        assert_eq!(*map.last().unwrap(), "A\u{0130}B".len());
    }

    #[test]
    fn the_offset_map_names_the_source_character_of_every_folded_byte() {
        // One character per fold class: ASCII, a presentation form that grows
        // (lam-alef is two letters), and one that stays one character.
        let text = "a\u{FEFB}\u{FE8E}";
        let (folded, map) = fold(text, true);

        assert_eq!(folded, "a\u{0644}\u{0627}\u{0627}");
        assert_eq!(map.len(), folded.len() + 1);
        assert_eq!(map[0], 0);
        for (byte, source) in map.iter().enumerate().take(folded.len()) {
            assert!(text.is_char_boundary(*source), "byte {byte} names {source}");
        }
        assert_eq!(*map.last().unwrap(), text.len());
    }

    #[test]
    fn presentation_forms_match_the_base_letters_a_user_types() {
        // One word written with the Forms-B contextual glyphs a shaper emits:
        // alef isolated, lam initial, lam medial, heh final.
        let shaped = "\u{FE8D}\u{FEDF}\u{FEE0}\u{FEEA}";
        let typed = "\u{0627}\u{0644}\u{0644}\u{0647}";
        let p = page(vec![run(shaped, 0.0, 0.0)]);

        let hits = search(&p, typed, SearchOptions::default());
        assert_eq!(hits.len(), 1);
        // The hit names the characters the page actually drew, and one quad
        // per drawn glyph, not per folded character.
        assert_eq!(hits[0].text, shaped);
        assert_eq!(hits[0].quads.len(), shaped.chars().count());
    }

    #[test]
    fn a_case_sensitive_search_still_folds_presentation_forms() {
        let p = page(vec![run("\u{FEDF}\u{FEE0}", 0.0, 0.0)]);
        let options = SearchOptions {
            case_sensitive: true,
            ..SearchOptions::default()
        };

        assert_eq!(search(&p, "\u{0644}\u{0644}", options).len(), 1);
    }

    #[test]
    fn a_latin_ligature_matches_its_letters_and_keeps_one_quad() {
        let p = page(vec![run("of\u{FB01}ce", 0.0, 0.0)]);

        let hits = search(&p, "office", SearchOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "of\u{FB01}ce");
        // Five glyphs were drawn for six folded characters.
        assert_eq!(hits[0].quads.len(), 5);
    }

    #[test]
    fn a_needle_matching_one_letter_of_a_ligature_returns_the_whole_glyph() {
        let p = page(vec![run("\u{FB01}n", 0.0, 0.0)]);

        let hits = search(&p, "i", SearchOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "\u{FB01}");
        assert_eq!(hits[0].quads.len(), 1);
    }

    #[test]
    fn a_right_to_left_phrase_split_across_runs_is_not_found() {
        // The deferred half of bidi, pinned rather than skipped: the page draws
        // the two words in visual order, so the logical phrase is not a
        // substring of the joined text and no fold can make it one.
        let p = page(vec![
            run("\u{FEE4}\u{FEE0}", 0.0, 0.0),
            run("\u{FEDF}\u{FEDF}", 40.0, 0.0),
        ]);

        assert_eq!(flatten(&p).text, "\u{FEE4}\u{FEE0} \u{FEDF}\u{FEDF}");
        // The logical phrase reads the second run first.
        assert_eq!(
            search(
                &p,
                "\u{0644}\u{0644} \u{0645}\u{0644}",
                SearchOptions::default()
            )
            .len(),
            0
        );
        // Each word on its own is found, which is what run-local folding buys.
        assert_eq!(
            search(&p, "\u{0644}\u{0644}", SearchOptions::default()).len(),
            1
        );
        assert_eq!(
            search(&p, "\u{0645}\u{0644}", SearchOptions::default()).len(),
            1
        );
    }

    #[test]
    fn any_of_the_words_finds_every_word_in_document_order() {
        let p = page(vec![run("red green blue green", 0.0, 0.0)]);
        let options = SearchOptions {
            mode: MatchMode::AnyWord,
            ..SearchOptions::default()
        };

        let hits = search(&p, "green red", options);
        assert_eq!(
            hits.iter().map(|hit| hit.range.start).collect::<Vec<_>>(),
            vec![0, 4, 15]
        );
        assert_eq!(hits[0].text, "red");
    }

    #[test]
    fn all_of_the_words_needs_every_word_on_the_page() {
        let present = page(vec![run("red green blue", 0.0, 0.0)]);
        let partial = page(vec![run("red blue", 0.0, 0.0)]);
        let options = SearchOptions {
            mode: MatchMode::AllWords,
            ..SearchOptions::default()
        };

        assert_eq!(search(&present, "green red", options).len(), 2);
        assert!(search(&partial, "green red", options).is_empty());
    }

    #[test]
    fn a_word_repeated_in_the_needle_reports_each_hit_once() {
        let p = page(vec![run("red red", 0.0, 0.0)]);
        for mode in [MatchMode::AnyWord, MatchMode::AllWords] {
            let options = SearchOptions {
                mode,
                ..SearchOptions::default()
            };
            assert_eq!(search(&p, "red red", options).len(), 2, "{mode:?}");
        }
    }

    #[test]
    fn word_modes_honour_whole_word_and_case() {
        let p = page(vec![run("Theatre the", 0.0, 0.0)]);
        let options = SearchOptions {
            case_sensitive: false,
            whole_word: true,
            mode: MatchMode::AnyWord,
        };

        let hits = search(&p, "the", options);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].range.start, "Theatre ".len());
        assert_eq!(
            search(
                &p,
                "the",
                SearchOptions {
                    case_sensitive: true,
                    ..options
                }
            )
            .len(),
            1
        );
    }

    #[test]
    fn a_needle_matching_part_of_a_folded_character_returns_that_character() {
        // The Turkish dotted capital I lowercases to two code points, so the
        // needle "i" matches a strict prefix of one source character's folded
        // form. The hit is that character, not an empty range between two of
        // its own bytes.
        let p = page(vec![run("\u{0130}stanbul", 0.0, 0.0)]);
        let hits = search(&p, "i", SearchOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "\u{0130}");
        assert_eq!(hits[0].range, 0.."\u{0130}".len());
        assert_eq!(hits[0].quads.len(), 1);
    }

    #[test]
    fn overlapping_occurrences_are_all_found() {
        let p = page(vec![run("aaaa", 0.0, 0.0)]);
        assert_eq!(search(&p, "aa", SearchOptions::default()).len(), 3);
    }
}
