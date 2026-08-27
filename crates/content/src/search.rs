//! The find engine behind the viewer's Ctrl+F and, later, search-and-redact.
//!
//! Deliberately dumb: a literal substring search over the page's runs in
//! document order, with the two options Acrobat's find bar has. No stemming,
//! no fuzzy matching, no layout analysis. What it does owe the caller is exact
//! provenance for every hit, so a highlight lands on the right glyphs and a
//! redaction rewrites the right bytes.

use std::ops::Range;

use onionskin_plugin_api::{PageIndex, PageQuad};

use crate::run::{ByteProvenance, PageText, TextRun};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    /// The match must not be flanked by an alphanumeric character.
    pub whole_word: bool,
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
    let (haystack, offsets) = if options.case_sensitive {
        (flat.text.clone(), None)
    } else {
        let (folded, map) = fold(&flat.text);
        (folded, Some(map))
    };
    let pattern = if options.case_sensitive {
        needle.to_string()
    } else {
        fold(needle).0
    };

    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(hit) = haystack[from..].find(&pattern) {
        let start = from + hit;
        let end = start + pattern.len();
        // Advance by one byte-boundary so overlapping occurrences are all
        // found; `find` on a UTF-8 boundary guarantees `start + 1` is safe to
        // clamp up to the next boundary.
        from = next_boundary(&haystack, start + 1);

        let range = match &offsets {
            Some(map) => match (map.get(start), map.get(end)) {
                (Some(s), Some(e)) => *s..*e,
                (Some(s), None) => *s..flat.text.len(),
                _ => continue,
            },
            None => start..end,
        };
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

/// Lowercases a string and records, for every byte of the result, the byte
/// offset it came from. Case folding is not length preserving, so a hit in the
/// folded string has to be translated back before it can name glyphs.
fn fold(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len() + 1);
    for (offset, ch) in text.char_indices() {
        let before = folded.len();
        for lower in ch.to_lowercase() {
            folded.push(lower);
        }
        map.resize(folded.len(), offset);
        debug_assert!(folded.len() >= before);
    }
    map.push(text.len());
    (folded, map)
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
                    whole_word: false
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
            case_sensitive: false,
            whole_word: true,
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
        let (folded, map) = fold("A\u{0130}B");
        assert!(folded.len() > "A\u{0130}B".len() - 1);
        assert_eq!(map[0], 0);
        assert_eq!(*map.last().unwrap(), "A\u{0130}B".len());
    }

    #[test]
    fn overlapping_occurrences_are_all_found() {
        let p = page(vec![run("aaaa", 0.0, 0.0)]);
        assert_eq!(search(&p, "aa", SearchOptions::default()).len(), 3);
    }
}
