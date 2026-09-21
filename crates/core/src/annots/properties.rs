//! A comment's properties as Acrobat's Properties dialog shows them: its
//! colour, opacity, author and subject.
//!
//! Changing one is one edit that writes the keys and draws the appearance
//! again, because colour and opacity are drawn into `/AP`: a `/C` changed
//! and an appearance left alone is a comment that says red and shows yellow.

use onionskin_cos::{Name, ObjRef, Object};

use super::appearance::numbers;
use super::author::{pdf_date, text_string};
use super::model::Color;
use super::review::{dict_of, redraw};
use crate::edit::Transaction;
use crate::Result;

/// What the Properties inspector edits. `None` in `author` or `subject`
/// removes the key.
#[derive(Clone, Debug, PartialEq)]
pub struct CommentProperties {
    pub color: Option<Color>,
    /// `/CA`, clamped to `0.0..=1.0`.
    pub opacity: f64,
    pub author: Option<String>,
    pub subject: Option<String>,
}

impl Default for CommentProperties {
    fn default() -> Self {
        CommentProperties {
            color: None,
            opacity: 1.0,
            author: None,
            subject: None,
        }
    }
}

/// Write `properties` onto `annotation`, stamp `/M`, and draw its
/// appearance again.
pub fn set_properties(
    tx: &mut Transaction<'_>,
    annotation: ObjRef,
    properties: &CommentProperties,
    now: i64,
) -> Result<()> {
    let (mut dict, generation) = dict_of(tx, annotation)?;
    match properties.color {
        Some(color) => dict.set(
            Name::new("C"),
            numbers(&[color.red, color.green, color.blue]),
        ),
        None => {
            dict.remove(b"C");
        }
    }
    let opacity = properties.opacity.clamp(0.0, 1.0);
    if opacity < 1.0 {
        dict.set(Name::new("CA"), Object::Real(opacity));
    } else {
        dict.remove(b"CA");
    }
    for (key, value) in [
        ("T", properties.author.as_deref()),
        ("Subj", properties.subject.as_deref()),
    ] {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => dict.set(Name::new(key), text_string(value)),
            None => {
                dict.remove(key.as_bytes());
            }
        }
    }
    dict.set(Name::new("M"), Object::String(pdf_date(now).into_bytes()));
    redraw(tx, &mut dict)?;
    tx.put_object(annotation.number, generation, Object::Dict(dict))
}
