//! The accessibility checker's structure checks: whether a tagged
//! document's structure tree and its content still agree.
//!
//! `core`'s invariant covers the tree against the document's objects: pages
//! and objects it names exist, the `/ParentTree` resolves. This adds the
//! tree against the content: every marked-content id an element names is
//! one its page's content opens, and every one the content opens is named.
//! Guarantee test 8 grades every edit with it, which is the point of it:
//! the checker Acrobat's users run is the one the edits are proved by.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_core::{check as check_invariant, read_structure, Kid, Structure, Violation};
use onionskin_cos::{Document as CosDocument, ObjRef};

/// One way a tagged document's tree and content disagree.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Finding {
    /// The tree against the document's objects, as `core`'s invariant has it.
    Structure(String),
    /// An element names marked content page `page` does not open.
    ContentMissing { page: usize, mcid: i64 },
    /// Page `page` opens marked content no element names.
    ContentUnnamed { page: usize, mcid: i64 },
    /// Page `page` opens marked content but has no `/StructParents`, so a
    /// reader cannot get from its content to the tree.
    PageUnlinked { page: usize },
}

/// Everything the checker found. Empty is a pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: BTreeSet<Finding>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// What this report finds that `before` did not: what an edit broke.
    pub fn new_since(&self, before: &Report) -> Vec<Finding> {
        self.findings
            .difference(&before.findings)
            .cloned()
            .collect()
    }
}

/// Check `doc`. An untagged document passes with nothing to say.
pub fn check(doc: &CosDocument) -> onionskin_core::Result<Report> {
    let structure = read_structure(doc)?;
    let Structure::Tagged(tree) = &structure else {
        return Ok(Report::default());
    };
    let page_count = doc.page_count()? as usize;
    let mut findings: BTreeSet<Finding> = check_invariant(doc, &structure, page_count)?
        .violations
        .iter()
        .map(describe)
        .collect();
    let named = named_content(tree.elements.values());
    for index in 0..page_count {
        let page = onionskin_content::page(doc, index)?;
        let opened = onionskin_content::page_mcids(doc, index)?;
        let names = named.get(&page.objref.number).cloned().unwrap_or_default();
        findings.extend(
            names
                .difference(&opened)
                .map(|&mcid| Finding::ContentMissing { page: index, mcid }),
        );
        findings.extend(
            opened
                .difference(&names)
                .map(|&mcid| Finding::ContentUnnamed { page: index, mcid }),
        );
        if !opened.is_empty() && page.dict.get(b"StructParents").is_none() {
            findings.insert(Finding::PageUnlinked { page: index });
        }
    }
    Ok(Report { findings })
}

/// Every marked-content id the elements name, by page object number.
fn named_content<'a>(
    elements: impl Iterator<Item = &'a onionskin_core::Element>,
) -> BTreeMap<u32, BTreeSet<i64>> {
    let mut named: BTreeMap<u32, BTreeSet<i64>> = BTreeMap::new();
    for element in elements {
        for kid in &element.kids {
            let (page, mcid): (Option<ObjRef>, i64) = match kid {
                Kid::Mcid(mcid) => (element.page, *mcid),
                Kid::MarkedContent { page, mcid } => (page.or(element.page), *mcid),
                Kid::Element(_) | Kid::Object { .. } => continue,
            };
            if let Some(page) = page {
                named.entry(page.number).or_default().insert(mcid);
            }
        }
    }
    named
}

fn describe(violation: &Violation) -> Finding {
    Finding::Structure(format!("{violation:?}"))
}
