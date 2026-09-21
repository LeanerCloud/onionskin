//! The file's own history: one generation per incremental section.
//!
//! A generation is a byte range, from the first byte after the previous
//! generation to the end of its own `%%EOF`. Reverting truncates at one's
//! `start`, which is why the range is what this hands out rather than an offset
//! into a table.

use onionskin_cos::{Document as CosDocument, Object};

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
    /// Rolling back to the newest generation discards nothing.
    AlreadyCurrent,
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
            RevertRefusal::AlreadyCurrent => write!(f, "this is already the current version"),
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

/// Where rolling back to `keep` truncates the file: the start of the
/// generation after it, so `keep` and everything older stays and everything
/// newer goes. Every newer generation is trailing, so unlike dropping one
/// generation this is always a truncation; it is refused only with unsaved
/// edits, or when nothing is newer.
pub(crate) fn roll_back_point(
    generations: &[Generation],
    keep: usize,
    has_unsaved_edits: bool,
) -> std::result::Result<u64, RevertRefusal> {
    if has_unsaved_edits {
        return Err(RevertRefusal::UnsavedEdits);
    }
    if keep >= generations.len() {
        return Err(RevertRefusal::NoSuchGeneration);
    }
    generations
        .get(keep + 1)
        .map(|next| next.start)
        .ok_or(RevertRefusal::AlreadyCurrent)
}

/// What the skins panel says about one generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationDetail {
    pub generation: Generation,
    /// Whether Onionskin wrote it: its trailer carries the save stamp, and
    /// the stamp names this generation's own first byte. A stamp carried
    /// forward into another writer's section names an earlier start, so it
    /// does not count.
    pub ours: bool,
    /// Who wrote it: the stamp's producer for ours, `/Info /Producer` as of
    /// this generation otherwise.
    pub producer: Option<String>,
    /// When: the stamp's date for ours, `/Info /ModDate` (or, for the
    /// original, `/CreationDate`) otherwise.
    pub date: Option<String>,
}

/// Every generation of `bytes`, described. Each is read by opening the file
/// as it was when that generation ended, over the same buffer.
pub(crate) fn details(
    bytes: &std::sync::Arc<Vec<u8>>,
    generations: &[Generation],
) -> Vec<GenerationDetail> {
    generations
        .iter()
        .map(|generation| {
            let source = onionskin_cos::BytesSource::prefix(
                std::sync::Arc::clone(bytes),
                generation.end as usize,
            );
            match CosDocument::open(Box::new(source)) {
                Ok(doc) => describe(&doc, *generation),
                Err(_) => GenerationDetail {
                    generation: *generation,
                    ours: false,
                    producer: None,
                    date: None,
                },
            }
        })
        .collect()
}

fn describe(doc: &CosDocument, generation: Generation) -> GenerationDetail {
    let dict = |object: Option<&Object>| {
        object
            .and_then(|object| doc.resolve(object).ok())
            .and_then(|object| object.as_dict().cloned())
    };
    let text = |dict: &Option<onionskin_cos::Dict>, key: &str| {
        dict.as_ref()
            .and_then(|dict| dict.get(key.as_bytes()))
            .and_then(|value| doc.resolve(value).ok())
            .and_then(|value| match value {
                Object::String(bytes) => Some(onionskin_content::pdf_text_string(&bytes)),
                _ => None,
            })
    };
    let stamp = dict(doc.trailer().get(crate::save::SECTION_STAMP.as_bytes()));
    let ours = generation.index > 0
        && stamp
            .as_ref()
            .and_then(|stamp| stamp.get(b"Start"))
            .and_then(Object::as_integer)
            == Some(generation.start as i64);
    if ours {
        return GenerationDetail {
            generation,
            ours,
            producer: text(&stamp, "Producer"),
            date: text(&stamp, "Date"),
        };
    }
    let info = dict(doc.trailer().get(b"Info"));
    let date_key = if generation.index == 0 {
        "CreationDate"
    } else {
        "ModDate"
    };
    GenerationDetail {
        generation,
        ours,
        producer: text(&info, "Producer"),
        date: text(&info, date_key),
    }
}
