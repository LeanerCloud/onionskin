//! A content stream as it is rewritten: the operations kept, the ones
//! replaced, and the resources the replacements name.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Document, Name, Object};

use super::{NewResource, Rewritten};

/// What happens to one operation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Emit {
    /// Kept as it was written.
    Copy,
    /// Written as these bytes instead.
    Replace(Vec<u8>),
    /// Removed.
    Drop,
    /// Held back until a later operation decides it: a path under
    /// construction waits for the operator that paints it.
    Held,
}

pub(crate) struct Output {
    chunks: Vec<Vec<u8>>,
    changed: bool,
    uses: Vec<NewResource>,
    /// Names the stream's resources already use, which a new one must not.
    taken: BTreeSet<Vec<u8>>,
}

/// The names `resources` already gives its XObjects and graphics states.
pub(crate) fn taken_names(doc: &Document, resources: &Dict) -> BTreeSet<Vec<u8>> {
    let mut taken = BTreeSet::new();
    for category in [b"XObject".as_slice(), b"ExtGState"] {
        let names = resources
            .get(category)
            .and_then(|entry| doc.resolve(entry).ok())
            .and_then(|entry| entry.as_dict().cloned());
        for (name, _) in names.iter().flat_map(Dict::iter) {
            taken.insert(name.as_bytes().to_vec());
        }
    }
    taken
}

impl Output {
    pub(crate) fn new(taken: BTreeSet<Vec<u8>>) -> Output {
        Output {
            chunks: Vec::new(),
            changed: false,
            uses: Vec::new(),
            taken,
        }
    }

    /// Writes `original` as `emit` says; the chunk's index when anything was
    /// written, so it can be patched later.
    pub(crate) fn push(&mut self, original: &[u8], emit: Emit) -> Option<usize> {
        let bytes = match emit {
            Emit::Copy => original.to_vec(),
            Emit::Replace(bytes) => {
                self.changed = true;
                bytes
            }
            Emit::Drop => {
                self.changed = true;
                return None;
            }
            Emit::Held => return None,
        };
        self.chunks.push(bytes);
        Some(self.chunks.len() - 1)
    }

    /// Replaces an already written chunk.
    pub(crate) fn patch(&mut self, index: usize, bytes: Vec<u8>) {
        if let Some(chunk) = self.chunks.get_mut(index) {
            if *chunk != bytes {
                *chunk = bytes;
                self.changed = true;
            }
        }
    }

    /// A resource name this stream does not use yet.
    pub(crate) fn fresh_name(&mut self, counter: &mut u32) -> Name {
        loop {
            *counter += 1;
            let name = format!("OsRd{counter}").into_bytes();
            if self.taken.insert(name.clone()) {
                return Name(name);
            }
        }
    }

    pub(crate) fn uses(&mut self, resource: NewResource) {
        self.uses.push(resource);
    }

    pub(crate) fn finish(self) -> Rewritten {
        Rewritten {
            bytes: self.chunks.join(&b'\n'),
            changed: self.changed,
            resources: self.uses,
        }
    }
}

/// `name Do`, `name gs` and the like: an operator on a new resource name.
pub(crate) fn named(name: &Name, operator: &str) -> Vec<u8> {
    let mut out = onionskin_cos::object_bytes(&Object::Name(name.clone())).unwrap_or_default();
    out.push(b' ');
    out.extend_from_slice(operator.as_bytes());
    out
}

/// A number as a content stream writes it: no exponent, no trailing zeros.
pub(crate) fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" | "" => "0".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(number(12.0), "12");
        assert_eq!(number(-0.00001), "0");
        assert_eq!(number(1.23456), "1.2346");
        assert_eq!(number(-250.5), "-250.5");
    }

    #[test]
    fn chunks_are_kept_replaced_dropped_or_held() {
        let mut out = Output::new(BTreeSet::from([b"OsRd1".to_vec()]));
        assert_eq!(out.push(b"q", Emit::Copy), Some(0));
        assert_eq!(out.push(b"x", Emit::Held), None);
        let mut counter = 0;
        let name = out.fresh_name(&mut counter);
        assert_eq!(
            name.as_bytes(),
            b"OsRd2",
            "a name the resources use is skipped"
        );
        assert_eq!(
            out.push(b"/Im0 Do", Emit::Replace(named(&name, "Do"))),
            Some(1)
        );
        assert_eq!(out.push(b"(a) Tj", Emit::Drop), None);
        out.patch(0, b"Q".to_vec());
        let done = out.finish();
        assert_eq!(done.bytes, b"Q\n/OsRd2 Do");
        assert!(done.changed);
    }
}
