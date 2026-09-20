//! The file's own history: one generation per incremental section.
//!
//! A generation is a byte range, from the first byte after the previous
//! generation to the end of its own `%%EOF`. Reverting truncates at one's
//! `start`, which is why the range is what this hands out rather than an offset
//! into a table.

use onionskin_cos::Document as CosDocument;

use crate::Result;

/// One generation of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation {
    /// Position in the file, oldest first. Generation 0 is the original
    /// document.
    pub index: usize,
    pub start: u64,
    pub end: u64,
}

impl Generation {
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end == self.start
    }
}

pub(crate) fn generations(doc: &CosDocument) -> Result<Vec<Generation>> {
    Ok(doc
        .sections()?
        .into_iter()
        .enumerate()
        .map(|(index, section)| Generation {
            index,
            start: section.start,
            end: section.end,
        })
        .collect())
}

/// Why a revert was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevertRefusal {
    /// The session has edits that are not in the file. Truncating would throw
    /// them away without saying so.
    UnsavedEdits,
    /// The target is not the last generation. Truncating to a middle one would
    /// have to rewrite everything after it, which is not a truncation.
    NotTrailing,
    /// There is no generation with that index.
    NoSuchGeneration,
    /// Reverting to generation 0 from generation 0 is a no-op, and a session
    /// with no appended sections has nothing to revert.
    NothingToRevert,
}

impl std::fmt::Display for RevertRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RevertRefusal::UnsavedEdits => {
                write!(
                    f,
                    "this document has unsaved edits; save or undo them first"
                )
            }
            RevertRefusal::NotTrailing => write!(
                f,
                "only the most recent generation can be reverted, because a revert truncates"
            ),
            RevertRefusal::NoSuchGeneration => write!(f, "no generation with that index"),
            RevertRefusal::NothingToRevert => write!(f, "this document has one generation"),
        }
    }
}

/// Where a revert of `target` truncates the file, or why it is refused.
///
/// `target` is the generation being **dropped**, not the one being returned to,
/// which is what makes "the target is not a trailing section" a meaningful
/// refusal: only the last generation can be removed by truncating, because
/// removing a middle one would mean rewriting everything after it, and that is
/// not a truncation. Generation 0 is the original document and can never be
/// dropped.
pub(crate) fn truncation_point(
    generations: &[Generation],
    target: usize,
    has_unsaved_edits: bool,
) -> std::result::Result<u64, RevertRefusal> {
    if has_unsaved_edits {
        return Err(RevertRefusal::UnsavedEdits);
    }
    if generations.len() < 2 {
        return Err(RevertRefusal::NothingToRevert);
    }
    let Some(generation) = generations.get(target) else {
        return Err(RevertRefusal::NoSuchGeneration);
    };
    if target == 0 {
        return Err(RevertRefusal::NothingToRevert);
    }
    if target + 1 != generations.len() {
        return Err(RevertRefusal::NotTrailing);
    }
    Ok(generation.start)
}
