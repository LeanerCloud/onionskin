//! The Fonts tab: every font the document's pages name, read from their
//! resources.
//!
//! Read from the resource dictionaries rather than from extracted text, so a
//! font that draws no text a reader can extract (a symbol font, a font only a
//! Form XObject uses) is still listed, and listing costs no content-stream
//! interpretation. Form XObjects are followed, because a font a page uses
//! through one is a font the page uses.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Document as CosDocument, ObjRef, Object};

use crate::Result;

/// How deep Form XObjects are followed. A form that draws itself is a loop;
/// the visited set stops that, and this stops a merely deep chain.
const MAX_FORM_DEPTH: usize = 16;

/// One font, as the Fonts tab lists it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FontEntry {
    /// `/BaseFont`, without a subset prefix.
    pub name: String,
    /// `/Subtype`: `Type1`, `TrueType`, `Type0`, `Type3`...
    pub kind: String,
    /// Whether the file carries the font program.
    pub embedded: bool,
    /// Whether that program is a subset (`ABCDEF+Name`).
    pub subset: bool,
}

/// Every distinct font the pages name, sorted by name.
pub fn document_fonts(doc: &CosDocument) -> Result<Vec<FontEntry>> {
    let count = usize::try_from(doc.page_count()?).unwrap_or(0);
    let mut walk = Walk {
        doc,
        seen_fonts: BTreeSet::new(),
        seen_forms: BTreeSet::new(),
        fonts: BTreeSet::new(),
    };
    for index in 0..count {
        if let Some(resources) = doc.page(index)?.resources {
            walk.resources(&resources, 0);
        }
    }
    Ok(walk.fonts.into_iter().collect())
}

struct Walk<'a> {
    doc: &'a CosDocument,
    seen_fonts: BTreeSet<ObjRef>,
    seen_forms: BTreeSet<ObjRef>,
    fonts: BTreeSet<FontEntry>,
}

impl Walk<'_> {
    fn resources(&mut self, resources: &Dict, depth: usize) {
        if let Some(Object::Dict(fonts)) = self.dict_value(resources.get(b"Font")) {
            for (_, font) in fonts.iter() {
                self.font(font);
            }
        }
        if depth >= MAX_FORM_DEPTH {
            return;
        }
        if let Some(Object::Dict(xobjects)) = self.dict_value(resources.get(b"XObject")) {
            for (_, xobject) in xobjects.iter() {
                self.form(xobject, depth);
            }
        }
    }

    fn dict_value(&self, value: Option<&Object>) -> Option<Object> {
        self.doc.resolve(value?).ok()
    }

    fn font(&mut self, font: &Object) {
        if let Object::Ref(objref) = font {
            if !self.seen_fonts.insert(*objref) {
                return;
            }
        }
        let Ok(Object::Dict(dict)) = self.doc.resolve(font) else {
            return;
        };
        self.fonts.insert(entry(self.doc, &dict));
    }

    fn form(&mut self, xobject: &Object, depth: usize) {
        let Object::Ref(objref) = xobject else {
            return;
        };
        if !self.seen_forms.insert(*objref) {
            return;
        }
        let Ok(Object::Stream(stream)) = self.doc.resolve(xobject) else {
            return;
        };
        let is_form = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_some_and(|name| name.as_bytes() == b"Form");
        if !is_form {
            return;
        }
        if let Some(Object::Dict(resources)) = self.dict_value(stream.dict.get(b"Resources")) {
            self.resources(&resources, depth + 1);
        }
    }
}

fn entry(doc: &CosDocument, font: &Dict) -> FontEntry {
    let name_of = |dict: &Dict, key: &[u8]| {
        dict.get(key)
            .and_then(|value| doc.resolve(value).ok())
            .and_then(|value| {
                value
                    .as_name()
                    .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
            })
    };
    let kind = name_of(font, b"Subtype").unwrap_or_else(|| "Unknown".to_owned());
    let base = name_of(font, b"BaseFont").unwrap_or_else(|| "(unnamed)".to_owned());
    let (subset, name) = split_subset(&base);
    // A composite font's program hangs off its descendant; a Type 3 font's
    // glyphs are content streams in the font itself.
    let described = descendant(doc, font).unwrap_or_else(|| font.clone());
    let embedded = kind == "Type3" || has_program(doc, &described);
    FontEntry {
        name,
        kind,
        embedded,
        subset: subset && embedded,
    }
}

/// `ABCDEF+Name`: six capitals and a plus mark a subset.
fn split_subset(base: &str) -> (bool, String) {
    match base.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => {
            (true, rest.to_owned())
        }
        _ => (false, base.to_owned()),
    }
}

fn descendant(doc: &CosDocument, font: &Dict) -> Option<Dict> {
    match doc.resolve(font.get(b"DescendantFonts")?).ok()? {
        Object::Array(items) => match doc.resolve(items.first()?).ok()? {
            Object::Dict(dict) => Some(dict),
            _ => None,
        },
        _ => None,
    }
}

fn has_program(doc: &CosDocument, font: &Dict) -> bool {
    let Some(Ok(Object::Dict(descriptor))) =
        font.get(b"FontDescriptor").map(|value| doc.resolve(value))
    else {
        return false;
    };
    [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
        .into_iter()
        .any(|key| descriptor.get(key).is_some())
}

#[cfg(test)]
mod tests {
    use super::split_subset;

    #[test]
    fn a_subset_prefix_is_six_capitals_and_a_plus() {
        assert_eq!(split_subset("ABCDEF+Garamond"), (true, "Garamond".into()));
        assert_eq!(
            split_subset("Abcdef+Garamond"),
            (false, "Abcdef+Garamond".into())
        );
        assert_eq!(split_subset("AB+Garamond"), (false, "AB+Garamond".into()));
        assert_eq!(split_subset("Helvetica"), (false, "Helvetica".into()));
    }
}
