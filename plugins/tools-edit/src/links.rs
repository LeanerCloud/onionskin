//! Links through the plugin: make one, change it, delete it, make them from
//! the web addresses a page's text spells out, and remove the web ones.

use std::ops::Range;

use onionskin_core::links::{self, Link, LinkLook, LinkTarget};
use onionskin_core::{Document, ObjRef, PageIndex, RunCoverage};
use onionskin_plugin_api::CommandError;

fn edit_error(label: &'static str) -> impl Fn(onionskin_core::Error) -> CommandError {
    move |source| CommandError::Edit { label, source }
}

/// Make a link on `page` over `rect`, as one undo step.
pub fn create_link(
    doc: &mut Document,
    page: PageIndex,
    rect: [f64; 4],
    target: &LinkTarget,
    look: LinkLook,
) -> Result<ObjRef, CommandError> {
    let label = "Create Link";
    doc.edit_content(label, |tx, structure| {
        links::add_link(tx, structure, page, rect, target, look)
    })
    .map_err(edit_error(label))
}

/// Give `link` a new target and look, as one undo step.
pub fn edit_link(
    doc: &mut Document,
    link: ObjRef,
    target: &LinkTarget,
    look: LinkLook,
) -> Result<(), CommandError> {
    let label = "Edit Link";
    doc.edit_content(label, |tx, _| links::set_link(tx, link, target, look))
        .map_err(edit_error(label))
}

/// Delete `link` from `page`, as one undo step.
pub fn delete_link(doc: &mut Document, page: PageIndex, link: ObjRef) -> Result<(), CommandError> {
    let label = "Delete Link";
    doc.edit_content(label, |tx, _| {
        links::remove_link(tx, page, link).map(|_| ())
    })
    .map_err(edit_error(label))
}

/// The link `link`, as the document has it now.
pub fn find_link(doc: &mut Document, link: ObjRef) -> Result<Option<Link>, CommandError> {
    Ok(doc
        .links()
        .map_err(|source| CommandError::Failed {
            label: "Link Properties",
            reason: source.to_string(),
        })?
        .into_iter()
        .find(|found| found.objref == link))
}

/// Remove every link to a web page, as one undo step. How many went.
pub fn remove_web_links(doc: &mut Document) -> Result<usize, CommandError> {
    let label = "Remove Web Links";
    let pages: Vec<PageIndex> = (0..doc.page_count()).collect();
    doc.edit_content(label, |tx, _| links::remove_web_links(tx, &pages))
        .map_err(edit_error(label))
}

/// Make a link to each web address the text of `pages` spells out, where
/// no link is already, as one undo step: Acrobat's Create Links from URLs.
/// How many were made.
pub fn create_links_from_urls(
    doc: &mut Document,
    pages: &[PageIndex],
) -> Result<usize, CommandError> {
    let existing = doc.links().map_err(|source| CommandError::Failed {
        label: "Create Links from URLs",
        reason: source.to_string(),
    })?;
    let mut found = Vec::new();
    for &page in pages {
        let text = doc
            .page_text(page)
            .map_err(|source| CommandError::Page { page, source })?;
        let flattened = text.flatten();
        for piece in flattened.pieces() {
            let piece_text = &flattened.text[piece.range.clone()];
            for (range, url) in find_urls(piece_text) {
                let global = piece.range.start + range.start..piece.range.start + range.end;
                let Some(rect) = bounds(&text, piece, &global) else {
                    continue;
                };
                let centre = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
                if links::link_at(&existing, page, centre).is_none() {
                    found.push((page, rect, url));
                }
            }
        }
    }
    let label = "Create Links from URLs";
    let count = found.len();
    if count == 0 {
        return Ok(0);
    }
    doc.edit_content(label, |tx, structure| {
        for (page, rect, url) in &found {
            links::add_link(
                tx,
                structure,
                *page,
                *rect,
                &LinkTarget::Web(url.clone()),
                LinkLook::default(),
            )?;
        }
        Ok(())
    })
    .map_err(edit_error(label))?;
    Ok(count)
}

/// The box around the piece coverage for the flattened range.
fn bounds(
    page: &onionskin_core::PageText,
    piece: &onionskin_core::FlatPiece,
    range: &Range<usize>,
) -> Option<[f64; 4]> {
    let mut rect: Option<[f64; 4]> = None;
    for covered in piece.coverage_for(page, range.clone()) {
        let quads = match covered.coverage {
            RunCoverage::Decoded(local) => covered.run.quads_for_decoded(local),
            RunCoverage::WholeActualText => {
                covered.run.glyphs.iter().map(|glyph| glyph.quad).collect()
            }
        };
        for quad in quads {
            for (x, y) in quad.corners {
                let grown = rect.get_or_insert([x, y, x, y]);
                *grown = [
                    grown[0].min(x),
                    grown[1].min(y),
                    grown[2].max(x),
                    grown[3].max(y),
                ];
            }
        }
    }
    rect
}

/// The web addresses in `text`: each byte range and the address to open,
/// `http://` added to one written from `www.`. Punctuation that ends a
/// sentence or closes a bracket is not part of an address.
pub fn find_urls(text: &str) -> Vec<(Range<usize>, String)> {
    let mut found = Vec::new();
    let mut start = 0;
    for word in text.split_inclusive(char::is_whitespace) {
        let at = start;
        start += word.len();
        let trimmed = word.trim_end();
        let lead = trimmed.len() - trimmed.trim_start_matches(['(', '<', '[', '"', '\'']).len();
        let body = trimmed[lead..]
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '>', ']', '"', '\'']);
        let lower = body.to_ascii_lowercase();
        let url = if lower.starts_with("http://") || lower.starts_with("https://") {
            body.to_owned()
        } else if lower.starts_with("www.") {
            format!("http://{body}")
        } else {
            continue;
        };
        if body.len() <= "https://".len() || !body.contains('.') {
            continue;
        }
        found.push((at + lead..at + lead + body.len(), url));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_addresses_are_found_without_the_punctuation_around_them() {
        let text = "See https://example.com/a?b=c, or (www.example.org). Not http:// nor mail@x.";
        let found = find_urls(text);
        assert_eq!(found.len(), 2);
        assert_eq!(&text[found[0].0.clone()], "https://example.com/a?b=c");
        assert_eq!(found[0].1, "https://example.com/a?b=c");
        assert_eq!(&text[found[1].0.clone()], "www.example.org");
        assert_eq!(found[1].1, "http://www.example.org");
        assert!(find_urls("HTTPS://EXAMPLE.COM")[0].1.starts_with("HTTPS"));
        assert!(
            find_urls("https://localhost").is_empty(),
            "no dot, no address"
        );
    }
}
