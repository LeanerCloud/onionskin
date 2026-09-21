//! Turning a selection's per-glyph quads into the quads an annotation writes.
//!
//! A selection hands back one quad per glyph. An annotation that wrote one
//! `/QuadPoints` entry per glyph would be enormous and would render as a row of
//! separate boxes with seams between them; Acrobat writes one quad per run of
//! glyphs that share a line.
//!
//! **The merge is per line, never a bounding box over everything.** A selection
//! that crosses two columns produces two quads, and collapsing it to its
//! bounding rectangle would paint the gutter between the columns and every line
//! of both columns from top to bottom. That failure passes any test built on a
//! single line of text, which is why `tests/markup.rs` has one built on two
//! columns.
//!
//! Corner order is preserved exactly as `PageQuad` documents it: upper-left,
//! upper-right, lower-left, lower-right, which is what Acrobat writes and what
//! readers expect, whatever ISO 32000-1's prose says.

use onionskin_core::PageQuad;

/// Two glyphs belong to one line when their vertical extents overlap by at
/// least this much of the smaller one. A fraction rather than an absolute
/// distance, because the answer has to be the same for 6pt and 60pt text.
const SAME_LINE_OVERLAP: f64 = 0.5;

/// Two glyphs on one line are contiguous when the gap between them is under
/// this multiple of the line's height. A space is well under it; a column
/// gutter is far over it, which is what keeps two columns apart.
const MAX_GAP: f64 = 1.5;

/// Merge per-glyph quads into one quad per contiguous run on a line.
pub fn merge(quads: &[PageQuad]) -> Vec<PageQuad> {
    let mut groups: Vec<Vec<PageQuad>> = Vec::new();
    for quad in quads {
        match groups.last_mut() {
            Some(group) if joins(group.last().expect("groups are never empty"), quad) => {
                group.push(*quad);
            }
            _ => groups.push(vec![*quad]),
        }
    }
    groups.iter().map(|group| envelope(group)).collect()
}

/// Whether `next` continues the line `previous` is on.
///
/// Both tests matter. Vertical overlap alone joins the last glyph of one column
/// to the first of the next, because they sit at the same height; the gap test
/// alone joins the end of one line to the start of the line below it when the
/// page is narrow.
fn joins(previous: &PageQuad, next: &PageQuad) -> bool {
    if previous.page != next.page {
        return false;
    }
    let (left_top, left_bottom) = vertical(previous);
    let (right_top, right_bottom) = vertical(next);
    let overlap = left_top.min(right_top) - left_bottom.max(right_bottom);
    let smaller = (left_top - left_bottom).min(right_top - right_bottom);
    if smaller <= 0.0 || overlap < smaller * SAME_LINE_OVERLAP {
        return false;
    }

    let (_, previous_right) = horizontal(previous);
    let (next_left, _) = horizontal(next);
    // Reading order can run right to left, and a negative gap is an overlap
    // rather than a distance, so the comparison is on the magnitude.
    (next_left - previous_right).abs() <= smaller * MAX_GAP
}

/// The smallest quad containing a group, which for glyphs on one line is the
/// line's own box.
fn envelope(group: &[PageQuad]) -> PageQuad {
    let page = group[0].page;
    let mut left = f64::MAX;
    let mut right = f64::MIN;
    let mut top = f64::MIN;
    let mut bottom = f64::MAX;
    for quad in group {
        let (quad_left, quad_right) = horizontal(quad);
        let (quad_top, quad_bottom) = vertical(quad);
        left = left.min(quad_left);
        right = right.max(quad_right);
        top = top.max(quad_top);
        bottom = bottom.min(quad_bottom);
    }
    PageQuad {
        page,
        // Upper-left, upper-right, lower-left, lower-right.
        corners: [(left, top), (right, top), (left, bottom), (right, bottom)],
    }
}

fn horizontal(quad: &PageQuad) -> (f64, f64) {
    let xs = quad.corners.map(|(x, _)| x);
    (
        xs.iter().copied().fold(f64::MAX, f64::min),
        xs.iter().copied().fold(f64::MIN, f64::max),
    )
}

fn vertical(quad: &PageQuad) -> (f64, f64) {
    let ys = quad.corners.map(|(_, y)| y);
    (
        ys.iter().copied().fold(f64::MIN, f64::max),
        ys.iter().copied().fold(f64::MAX, f64::min),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(x: f64, y: f64) -> PageQuad {
        PageQuad {
            page: 0,
            corners: [(x, y + 10.0), (x + 8.0, y + 10.0), (x, y), (x + 8.0, y)],
        }
    }

    #[test]
    fn glyphs_on_one_line_become_one_quad() {
        let line: Vec<PageQuad> = (0..5)
            .map(|i| glyph(10.0 + i as f64 * 8.0, 100.0))
            .collect();
        let merged = merge(&line);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].corners[0], (10.0, 110.0), "upper-left");
        assert_eq!(merged[0].corners[3], (50.0, 100.0), "lower-right");
    }

    #[test]
    fn two_lines_stay_two_quads() {
        let mut quads: Vec<PageQuad> = (0..3)
            .map(|i| glyph(10.0 + i as f64 * 8.0, 100.0))
            .collect();
        quads.extend((0..3).map(|i| glyph(10.0 + i as f64 * 8.0, 80.0)));
        assert_eq!(merge(&quads).len(), 2);
    }

    /// The case a bounding-box implementation gets wrong, at the unit level;
    /// `tests/markup.rs` has the same claim against a real document.
    #[test]
    fn two_columns_stay_two_quads() {
        let mut quads: Vec<PageQuad> = (0..3)
            .map(|i| glyph(10.0 + i as f64 * 8.0, 100.0))
            .collect();
        quads.extend((0..3).map(|i| glyph(300.0 + i as f64 * 8.0, 100.0)));
        let merged = merge(&quads);
        assert_eq!(
            merged.len(),
            2,
            "same line, far apart: a gutter is not a space"
        );
    }

    #[test]
    fn nothing_selected_is_no_quads() {
        assert!(merge(&[]).is_empty());
    }
}
