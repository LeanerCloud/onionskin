//! The render filter: which annotations a preview shows.
//!
//! **Nothing here is ever saved.** The filter produces object overrides for a
//! *preview* buffer, which the render path appends to the original bytes. It
//! never touches the edit session's overlay, so a document rendered with
//! markups hidden and then saved is byte-identical to one saved without ever
//! opening the filter.
//!
//! The mechanism is `/F` bit 2, Hidden, set on the excluded annotations. `/OC`
//! would be the other candidate and is not used: hayro ignores optional
//! content, so a filter built on it would hide nothing in our own renderer.
//!
//! **The filter is the mode and nothing else.** An earlier shape carried a mode
//! *and* a set of subtypes, which no caller can usefully supply: the mode
//! determines the set, so carrying both only invites the two to disagree.
//! [`AnnotationFilter::subtypes`] is a function over the mode.
//!
//! **One discrepancy in the plan, recorded rather than guessed.** P6's text
//! names four modes and P3's calls this "the five-mode enum". Acrobat's own
//! Comments-and-Forms dropdown has five, the fifth being "Document and
//! Comments". The four named in the plan are implemented here; the fifth is not
//! invented, because which subtypes it covers is a decision rather than a
//! deduction.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Document as CosDocument, Object, PendingEdit};

use super::author::with_hidden;
use super::model::Subtype;
use crate::Result;

/// What a preview shows. Acrobat's Comments-and-Forms dropdown, as far as M3
/// goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnnotationFilter {
    /// The page and nothing else: every markup annotation is hidden.
    DocumentOnly,
    /// Everything. The default, and the only mode that hides nothing.
    #[default]
    DocumentAndMarkups,
    /// Stamps survive, every other markup is hidden.
    DocumentAndStamps,
    /// Only form fields, which M3 does not author: every markup is hidden.
    FormFieldsOnly,
}

impl AnnotationFilter {
    /// The subtypes this mode draws. A function over the mode, never a field
    /// beside it.
    pub fn subtypes(self) -> BTreeSet<Subtype> {
        match self {
            AnnotationFilter::DocumentAndMarkups => Subtype::ALL.iter().copied().collect(),
            AnnotationFilter::DocumentOnly | AnnotationFilter::FormFieldsOnly => BTreeSet::new(),
            AnnotationFilter::DocumentAndStamps => [Subtype::Stamp].into_iter().collect(),
        }
    }

    /// Whether an annotation of this subtype is drawn. An annotation whose
    /// subtype `core` does not author, a `/Widget` or a `/Link`, is not a
    /// markup and is never hidden by a markup filter.
    pub fn shows(self, subtype: Option<Subtype>) -> bool {
        match subtype {
            None => true,
            Some(subtype) => self.subtypes().contains(&subtype),
        }
    }

    /// Whether this mode hides anything at all, which is what lets the preview
    /// skip the document walk in the common case.
    pub fn hides_anything(self) -> bool {
        self != AnnotationFilter::DocumentAndMarkups
    }

    /// The preview overrides this mode implies: every hidden annotation's
    /// dictionary with `/F` bit 2 set.
    ///
    /// This is deliberately a plain map rather than an `Overlay`, so it cannot
    /// be handed to the section writer as if it were an edit.
    ///
    /// **The cost this owes.** `/F` has to be written onto objects the overlay
    /// does not hold, so building a filtered preview walks the document to find
    /// them: O(pages + annotations) per filter change, not per frame.
    ///
    /// **The walk is over the merged view, not the base.** An annotation this
    /// session just authored lives only in `overlay`, so a walk that reads the
    /// base alone finds nothing to hide and hands back a buffer identical to
    /// the unfiltered one. Hiding a comment the moment after writing it is the
    /// first thing anyone tries.
    pub fn preview_overrides(
        self,
        doc: &CosDocument,
        page_count: usize,
        overlay: &BTreeMap<u32, PendingEdit>,
    ) -> Result<BTreeMap<u32, PendingEdit>> {
        let mut out = BTreeMap::new();
        if !self.hides_anything() {
            return Ok(out);
        }
        for index in 0..page_count {
            let page = doc.page(index)?;
            let page_dict = merged(doc, overlay, page.objref.number)?.unwrap_or(page.dict);
            let Some(annots) = page_dict.get(b"Annots") else {
                continue;
            };
            let items = merged_array(doc, overlay, annots)?;
            for item in &items {
                let Object::Ref(objref) = item else { continue };
                let Some(dict) = merged(doc, overlay, objref.number)? else {
                    continue;
                };
                let dict = &dict;
                let subtype = dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .and_then(Subtype::from_name);
                if self.shows(subtype) {
                    continue;
                }
                out.insert(
                    objref.number,
                    PendingEdit::Set {
                        generation: objref.generation,
                        object: Object::Dict(with_hidden(dict, true)),
                    },
                );
            }
        }
        Ok(out)
    }
}

/// The dictionary at a number as the session sees it: the overlay's value when
/// it has one, else the base's.
fn merged(
    doc: &CosDocument,
    overlay: &BTreeMap<u32, PendingEdit>,
    number: u32,
) -> Result<Option<onionskin_cos::Dict>> {
    match overlay.get(&number) {
        Some(PendingEdit::Set { object, .. }) => Ok(object.as_dict().cloned()),
        Some(PendingEdit::Delete { .. }) => Ok(None),
        None => Ok(doc
            .get(number)
            .ok()
            .and_then(|p| p.object.as_dict().cloned())),
    }
}

fn merged_array(
    doc: &CosDocument,
    overlay: &BTreeMap<u32, PendingEdit>,
    object: &Object,
) -> Result<Vec<Object>> {
    match object {
        Object::Array(items) => Ok(items.clone()),
        Object::Ref(objref) => match overlay.get(&objref.number) {
            Some(PendingEdit::Set {
                object: Object::Array(items),
                ..
            }) => Ok(items.clone()),
            Some(_) => Ok(Vec::new()),
            None => Ok(match doc.resolve(object)? {
                Object::Array(items) => items,
                _ => Vec::new(),
            }),
        },
        _ => Ok(Vec::new()),
    }
}
