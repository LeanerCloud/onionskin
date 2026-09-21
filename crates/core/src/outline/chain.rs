//! The one walk over an outline's sibling chains, shared by the reader and
//! the writer so they cannot disagree about which items exist.
//!
//! A chain is a parent's `/First` followed along `/Next`. The walk takes each
//! item at most once across the whole outline, which is what stops a cycle
//! or a shared subtree, and spends a budget per item, which is what stops a
//! chain that never repeats but never ends. An item that is not a dictionary
//! ends its chain: nothing after it can be reached honestly.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, ObjRef, Object};

use crate::Result;

/// A hostile `/Outlines` can chain siblings without ever repeating a node on
/// one path, so the cycle set alone does not bound the walk. Both caps are
/// generous against real files: the largest outline in the corpus is under
/// 3000 entries, and nesting past 32 is a producer bug rather than a
/// structure a reader has to serve.
pub(crate) const MAX_ITEMS: usize = 20_000;
pub(crate) const MAX_DEPTH: usize = 32;

/// Where the walk has been, for one whole outline.
pub(crate) struct Walk {
    seen: BTreeSet<u32>,
    budget: usize,
}

impl Walk {
    pub(crate) fn new() -> Self {
        Self {
            seen: BTreeSet::new(),
            budget: MAX_ITEMS,
        }
    }

    /// The items on `parent`'s chain, in order, each with its dictionary.
    pub(crate) fn children(
        &mut self,
        parent: &Dict,
        fetch: &mut dyn FnMut(ObjRef) -> Result<Option<Dict>>,
    ) -> Result<Vec<(ObjRef, Dict)>> {
        let mut items = Vec::new();
        let mut next = parent.get(b"First").and_then(Object::as_reference);
        while let Some(node) = next {
            if self.budget == 0 || !self.seen.insert(node.number) {
                break;
            }
            self.budget -= 1;
            let Some(dict) = fetch(node)? else {
                break;
            };
            next = dict.get(b"Next").and_then(Object::as_reference);
            items.push((node, dict));
        }
        Ok(items)
    }
}
