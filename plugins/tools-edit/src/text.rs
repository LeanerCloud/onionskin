//! Editing text through the plugin: a line rewritten where it is, and find
//! and replace. Each is one undo step.
//!
//! A line is named by its page and its place among the page's lines, with
//! what it said when it was picked. The edit reads the lines again and
//! refuses when that line says something else now, rather than rewrite
//! whatever has taken its place.

use onionskin_core::text_edit::{
    self, find_in_lines, page_lines, replace_matches, rewrite_lines, write_page_edit, LineMatch,
    MatchOptions,
};
use onionskin_core::{Document, PageIndex, TextLine};
use onionskin_plugin_api::CommandError;

fn failed(label: &'static str) -> impl Fn(onionskin_core::Error) -> CommandError {
    move |source| CommandError::Edit { label, source }
}

fn refused(label: &'static str, reason: impl Into<String>) -> CommandError {
    CommandError::Failed {
        label,
        reason: reason.into(),
    }
}

/// The line under `(x, y)` on `page`, with its place among the page's
/// lines.
pub fn line_at(
    doc: &mut Document,
    page: PageIndex,
    (x, y): (f64, f64),
) -> Option<(usize, TextLine)> {
    let lines = page_lines(doc.structure().ok()?, page).ok()?;
    lines
        .into_iter()
        .enumerate()
        .find(|(_, line)| line.contains(x, y))
}

/// Make line `line` of `page`, which said `was`, say `text`.
pub fn edit_line(
    doc: &mut Document,
    page: PageIndex,
    line: usize,
    was: &str,
    text: &str,
) -> Result<(), CommandError> {
    const LABEL: &str = "Edit Text";
    if text == was {
        return Ok(());
    }
    let edit = {
        let structure = doc.structure().map_err(failed(LABEL))?;
        let lines = page_lines(structure, page).map_err(failed(LABEL))?;
        let found = lines
            .get(line)
            .filter(|found| found.text == was)
            .ok_or_else(|| refused(LABEL, "the line has changed since it was chosen"))?;
        rewrite_lines(structure, page, &[(found, text.to_owned())]).map_err(failed(LABEL))?
    };
    doc.edit_document(LABEL, |tx| write_page_edit(tx, &edit))
        .map_err(failed(LABEL))
}

/// The size new text is set at.
pub const NEW_TEXT_SIZE: f64 = 12.0;

/// Draw `text` as a new line on `page`, its baseline starting at `at`, as
/// one undo step. Nothing typed adds nothing.
pub fn add_text(
    doc: &mut Document,
    page: PageIndex,
    at: (f64, f64),
    text: &str,
) -> Result<(), CommandError> {
    const LABEL: &str = "Add Text";
    if text.trim().is_empty() {
        return Ok(());
    }
    doc.edit_document(LABEL, |tx| {
        text_edit::add_text(tx, page, at, NEW_TEXT_SIZE, text)
    })
    .map_err(failed(LABEL))
}

/// Every occurrence of `needle` in the document's text, line by line.
pub fn find(
    doc: &mut Document,
    needle: &str,
    options: MatchOptions,
) -> Result<Vec<LineMatch>, CommandError> {
    const LABEL: &str = "Find";
    let pages = 0..doc.page_count();
    let structure = doc.structure().map_err(failed(LABEL))?;
    find_in_lines(structure, pages, needle, options).map_err(failed(LABEL))
}

/// The match among `matches` drawn at `(x, y)` on `page`: which one the
/// find bar has on screen, for Replace to replace.
pub fn match_at(
    doc: &mut Document,
    matches: &[LineMatch],
    page: PageIndex,
    (x, y): (f64, f64),
) -> Option<LineMatch> {
    let lines = page_lines(doc.structure().ok()?, page).ok()?;
    matches
        .iter()
        .filter(|found| found.page == page)
        .find(|found| {
            lines.get(found.line).is_some_and(|line| {
                line.quads_for(&found.range).iter().any(|quad| {
                    let xs = quad.corners.map(|corner| corner.0);
                    let ys = quad.corners.map(|corner| corner.1);
                    let within = |values: [f64; 4], at: f64| {
                        values.iter().copied().fold(f64::MAX, f64::min) <= at
                            && at <= values.iter().copied().fold(f64::MIN, f64::max)
                    };
                    within(xs, x) && within(ys, y)
                })
            })
        })
        .cloned()
}

/// Replace `matches` with `replacement`, as one undo step named `label`.
/// How many were replaced.
pub fn replace(
    doc: &mut Document,
    matches: &[LineMatch],
    replacement: &str,
    label: &'static str,
) -> Result<usize, CommandError> {
    if matches.is_empty() {
        return Ok(0);
    }
    let edits = {
        let structure = doc.structure().map_err(failed(label))?;
        replace_matches(structure, matches, replacement).map_err(failed(label))?
    };
    doc.edit_document(label, |tx| {
        edits.iter().try_for_each(|edit| write_page_edit(tx, edit))
    })
    .map_err(failed(label))?;
    Ok(matches.len())
}
