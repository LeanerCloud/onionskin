//! The preview buffer: the bytes a reader sees before a save happens.
//!
//! `original ++ section_for(overlay, trailer)`, built by the same call the save
//! makes. Preview and save cannot disagree, because there is one builder rather
//! than two implementations that have to be kept in step.
//!
//! **The cache key is `(generation, filter)`, not the generation alone.** The
//! filter changes the bytes, so keying on the generation alone hands one mode's
//! buffer to another mode's request: the print dialog would show
//! Document-and-Markups while the user has Document-Only selected, and a
//! four-mode assertion would still pass because it never asks twice at one
//! generation.
//!
//! **The cache holds one entry, not one per mode.** Five live entries means
//! five live `Arc`s, so every buffer stays referenced and the buffer-reuse
//! strategy never fires; cycling the print dialog's modes on a large document
//! would cost a full copy each. Filtered previews are transient: built for a
//! render, used, dropped.

use std::sync::Arc;

use onionskin_cos::{BytesSource, Document as CosDocument};

use crate::annots::AnnotationFilter;
use crate::edit::EditSession;
use crate::Result;

/// One preview, and the read-only document opened from it.
pub(crate) struct PreviewBuffer {
    generation: u64,
    filter: AnnotationFilter,
    bytes: Arc<Vec<u8>>,
    /// Built lazily, once per buffer, and only when a structural read actually
    /// happens: a generation that is merely rendered never pays for an xref
    /// parse.
    structure: Option<CosDocument>,
}

impl PreviewBuffer {
    pub(crate) fn matches(&self, generation: u64, filter: AnnotationFilter) -> bool {
        self.generation == generation && self.filter == filter
    }

    pub(crate) fn bytes(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.bytes)
    }

    /// The document these bytes parse to.
    ///
    /// This is **not** the scratch `cos::Document` T4 rules out. That one would
    /// have to be mutated to build a section through, and cos cannot withdraw
    /// an edit. This one is opened read-only from bytes that already exist and
    /// is dropped with the buffer. Different documents, different problems.
    pub(crate) fn structure(&mut self) -> Result<&CosDocument> {
        if self.structure.is_none() {
            let (document, _provenance) = CosDocument::open_repairing(Box::new(
                BytesSource::from_shared(Arc::clone(&self.bytes)),
            ))?;
            self.structure = Some(document);
        }
        Ok(self.structure.as_ref().expect("just built"))
    }
}

/// Build the buffer for one generation and filter.
pub(crate) fn build(
    original: Arc<Vec<u8>>,
    base: &CosDocument,
    edit: &EditSession,
    page_count: usize,
    generation: u64,
    filter: AnnotationFilter,
) -> Result<PreviewBuffer> {
    let mut overlay = edit.pending_edits();

    // The filter has to write `/F` onto annotations that live in the base and
    // that the overlay does not hold, so this walk synthesizes entries for
    // them. It is O(pages + annotations) per filter change, not per frame, and
    // it is the cost the preview owes for being able to hide anything at all.
    if filter.hides_anything() {
        for (number, hidden) in filter.preview_overrides(base, page_count, &overlay)? {
            overlay.insert(number, hidden);
        }
    }

    let section = base.section_for(&overlay, &edit.trailer_edits())?;
    let bytes = match section {
        None => original,
        Some(section) => {
            let mut out = Vec::with_capacity(original.len() + section.len());
            out.extend_from_slice(&original);
            out.extend_from_slice(&section);
            Arc::new(out)
        }
    };

    Ok(PreviewBuffer {
        generation,
        filter,
        bytes,
        structure: None,
    })
}
