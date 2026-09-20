//! The render filter: which annotations a preview shows.
//!
//! **Nothing here is ever saved.** The filter produces a map of object
//! overrides for a *preview* buffer, which the render path hands to a reader
//! alongside the original bytes. It never touches the edit session's overlay,
//! so a document rendered with markups hidden and then saved is byte-identical
//! to one saved without ever opening the filter.
//!
//! The mechanism is `/F` bit 2, Hidden, set on the excluded annotations.
//! `/OC` would be the other candidate and is not used: hayro ignores optional
//! content, so a filter built on it would hide nothing in our own renderer.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Document as CosDocument, Object, PendingEdit};

use super::author::with_hidden;
use super::model::Subtype;
use crate::Result;

/// What a preview shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    /// The page and nothing else: every markup annotation is hidden.
    DocumentOnly,
    /// Everything. The default, and the only mode that hides nothing.
    DocumentAndMarkups,
    /// Stamps survive, every other markup is hidden.
    DocumentAndStamps,
    /// Only form fields, which M3 does not author: every markup is hidden.
    FormFieldsOnly,
}

/// A rendering mode, plus any individual subtypes the caller wants kept
/// whatever the mode says.
#[derive(Clone, Debug)]
pub struct AnnotationFilter {
    pub mode: RenderMode,
    /// Subtypes shown in addition to whatever `mode` allows. Empty is the
    /// common case.
    pub also_show: BTreeSet<Subtype>,
}

impl Default for AnnotationFilter {
    fn default() -> Self {
        AnnotationFilter {
            mode: RenderMode::DocumentAndMarkups,
            also_show: BTreeSet::new(),
        }
    }
}

impl AnnotationFilter {
    pub fn new(mode: RenderMode) -> Self {
        AnnotationFilter {
            mode,
            also_show: BTreeSet::new(),
        }
    }

    /// Whether an annotation of this subtype is drawn.
    pub fn shows(&self, subtype: Option<Subtype>) -> bool {
        if let Some(subtype) = subtype {
            if self.also_show.contains(&subtype) {
                return true;
            }
        }
        match self.mode {
            RenderMode::DocumentAndMarkups => true,
            RenderMode::DocumentOnly | RenderMode::FormFieldsOnly => false,
            RenderMode::DocumentAndStamps => subtype == Some(Subtype::Stamp),
        }
    }

    /// The preview overrides this filter implies: every hidden annotation's
    /// dictionary with `/F` bit 2 set.
    ///
    /// The returned map is for a preview buffer. It is deliberately not an
    /// `Overlay` and cannot be handed to the section writer by accident.
    pub fn preview_overrides(
        &self,
        doc: &CosDocument,
        page_count: usize,
    ) -> Result<BTreeMap<u32, PendingEdit>> {
        let mut out = BTreeMap::new();
        if self.mode == RenderMode::DocumentAndMarkups && self.also_show.is_empty() {
            return Ok(out);
        }
        for index in 0..page_count {
            let page = doc.page(index)?;
            let Some(annots) = page.dict.get(b"Annots") else {
                continue;
            };
            let Object::Array(items) = doc.resolve(annots)? else {
                continue;
            };
            for item in &items {
                let Object::Ref(objref) = item else { continue };
                let resolved = doc.resolve(item)?;
                let Some(dict) = resolved.as_dict() else {
                    continue;
                };
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
