//! What Check Spelling reads and changes: the text of each comment, and
//! the value of each text field, in page order.
//!
//! A correction is one undo step. A comment is given its new text as the
//! Comments pane gives it; a field its new value with its appearance drawn
//! again. A passage that says something else by the time it is corrected
//! is refused, rather than changed where the word no longer is.

use std::ops::Range;

use onionskin_core::forms::{set_field_value, FieldKind, FieldValue};
use onionskin_core::review::set_contents;
use onionskin_core::Document;
use onionskin_cos::ObjRef;
use onionskin_plugin_api::CommandError;

use crate::Checker;

/// The undo step a correction is.
pub const LABEL: &str = "Check Spelling";

/// The kinds of annotation whose text is not a comment's.
const NOT_COMMENTS: [&str; 3] = ["Widget", "Link", "Popup"];

/// Where a passage's text lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Comment(ObjRef),
    Field(ObjRef),
}

/// A piece of the document's text that is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub source: Source,
    pub page: Option<usize>,
    /// What the dialog calls it: "Comment on page 2", "Field Name".
    pub label: String,
    pub text: String,
}

/// A word the dictionary does not know, in passage `passage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misspelling {
    pub passage: usize,
    pub range: Range<usize>,
    pub word: String,
}

fn failed(source: onionskin_core::Error) -> CommandError {
    CommandError::Edit {
        label: LABEL,
        source,
    }
}

fn refused(reason: &str) -> CommandError {
    CommandError::Failed {
        label: LABEL,
        reason: reason.to_owned(),
    }
}

/// Every comment's text and every text field's value, in page order,
/// comments first on each page.
pub fn passages(doc: &mut Document) -> Result<Vec<Passage>, CommandError> {
    let mut found: Vec<Passage> = doc
        .annotations()
        .map_err(failed)?
        .into_iter()
        .filter(|annotation| !NOT_COMMENTS.contains(&annotation.raw_subtype.as_str()))
        .filter_map(|annotation| {
            let text = annotation.contents.filter(|text| !text.trim().is_empty())?;
            Some(Passage {
                source: Source::Comment(annotation.objref),
                page: Some(annotation.page),
                label: format!("Comment on page {}", annotation.page + 1),
                text,
            })
        })
        .collect();
    let form = doc.form().map_err(failed)?;
    found.extend(form.fields.iter().filter_map(|field| {
        let FieldKind::Text {
            password: false, ..
        } = field.kind
        else {
            return None;
        };
        let text = Some(field.value.as_text()).filter(|text| !text.trim().is_empty())?;
        Some(Passage {
            source: Source::Field(field.objref),
            page: field.page(),
            label: format!("Field {}", field.name),
            text,
        })
    }));
    found.sort_by_key(|passage| {
        (
            passage.page.unwrap_or(usize::MAX),
            matches!(passage.source, Source::Field(_)),
        )
    });
    Ok(found)
}

/// Every word of `passages` that `checker` does not know, in order.
pub fn misspellings(checker: &Checker, passages: &[Passage]) -> Vec<Misspelling> {
    passages
        .iter()
        .enumerate()
        .flat_map(|(index, passage)| {
            checker
                .misspelled(&passage.text)
                .into_iter()
                .map(move |range| Misspelling {
                    passage: index,
                    word: passage.text[range.clone()].to_owned(),
                    range,
                })
        })
        .collect()
}

/// Put `replacement` in place of `range` of `passage`, as one undo step.
/// The passage's new text.
pub fn correct(
    doc: &mut Document,
    passage: &Passage,
    range: Range<usize>,
    replacement: &str,
    now: i64,
) -> Result<String, CommandError> {
    let current = passages(doc)?
        .into_iter()
        .find(|found| found.source == passage.source)
        .filter(|found| found.text == passage.text)
        .ok_or_else(|| refused("the text has changed since it was checked"))?;
    let (Some(before), Some(after)) = (
        current.text.get(..range.start),
        current.text.get(range.end..),
    ) else {
        return Err(refused("the word is not in the text"));
    };
    let text = format!("{before}{replacement}{after}");
    match passage.source {
        Source::Comment(annotation) => doc
            .edit_annotations(LABEL, |tx, _| set_contents(tx, annotation, &text, now))
            .map_err(failed)?,
        Source::Field(field) => {
            let form = doc.form().map_err(failed)?;
            let value = FieldValue::Text(text.clone());
            doc.edit_annotations(LABEL, |tx, _| {
                set_field_value(tx, &form, field, &value, None)
            })
            .map_err(failed)?;
        }
    }
    Ok(text)
}
