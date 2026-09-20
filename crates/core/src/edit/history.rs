//! The undo stack: what was changed, in what order, and how far back it still
//! reaches.
//!
//! [`crate::ViewHistory`] is the shape the cursor semantics are copied from and
//! is explicitly not the thing extended: a view state is a position and
//! replaying one can never alter a document, while everything here can.
//!
//! **The bound is over bytes, not entries.** One page reorder on a 1000-page
//! file rewrites about a thousand page dicts and captures a thousand base
//! values, so a single entry can hold roughly two thousand objects; a thousand
//! highlights is the same entry count and three orders of magnitude smaller.
//! Counting entries would bound the cheap case and not the expensive one.
//!
//! **Eviction is visible.** Dropping the oldest entries past the bound is not
//! allowed to leave the Edit menu claiming a reach it does not have, so
//! [`History::reach`] shrinks with the stack and [`History::forgotten`] says
//! how many steps went. Dropping past the saved mark takes the mark with it and
//! [`History::forgot_saved_mark`] reports that, because a session that can no
//! longer return to its last save has to say so rather than appear clean.

use super::Change;

/// The stack's bound, as total resident bytes across every retained entry.
///
/// The figure is derived rather than chosen, and `benches/edit_entry.rs` is
/// where it is derived: it measures the worst single entry M3 can produce and
/// asserts it against this constant, so the two cannot drift apart.
pub const MAX_HISTORY_BYTES: usize = 256 * 1024 * 1024;

/// One transaction's worth of changes, at most one per address.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    label: &'static str,
    changes: Vec<Change>,
}

impl Entry {
    pub(crate) fn new(label: &'static str, changes: Vec<Change>) -> Self {
        Entry { label, changes }
    }

    /// The Edit menu's "Undo <label>".
    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub(crate) fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .changes
                .iter()
                .map(Change::resident_bytes)
                .sum::<usize>()
    }
}

/// The applied prefix is `entries[..cursor]`; the redo tail is everything from
/// `cursor` on.
#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<Entry>,
    cursor: usize,
    saved_mark: Option<usize>,
    resident: usize,
    forgotten: usize,
    forgot_saved_mark: bool,
    max_bytes: usize,
}

impl Default for History {
    fn default() -> Self {
        History::with_bound(MAX_HISTORY_BYTES)
    }
}

impl History {
    pub fn with_bound(max_bytes: usize) -> Self {
        History {
            entries: Vec::new(),
            cursor: 0,
            saved_mark: Some(0),
            resident: 0,
            forgotten: 0,
            forgot_saved_mark: false,
            max_bytes,
        }
    }

    /// How many steps undo can still take. This is what the Edit menu reports,
    /// and it is the retained count rather than the count ever recorded.
    pub fn reach(&self) -> usize {
        self.cursor
    }

    /// One retained entry, oldest first. Indices shift when eviction drops
    /// the oldest entries, which is why [`History::forgotten`] exists rather
    /// than a stable identifier nothing needs.
    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    pub fn redo_reach(&self) -> usize {
        self.entries.len() - self.cursor
    }

    /// How many entries eviction has dropped. Non-zero means the stack is
    /// shorter than the session, which the user is told rather than left to
    /// discover.
    pub fn forgotten(&self) -> usize {
        self.forgotten
    }

    pub fn forgot_saved_mark(&self) -> bool {
        self.forgot_saved_mark
    }

    pub fn resident_bytes(&self) -> usize {
        self.resident
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.entries
            .get(self.cursor.checked_sub(1)?)
            .map(Entry::label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.entries.get(self.cursor).map(Entry::label)
    }

    /// True when the document is byte-identical to its last save. An evicted
    /// mark can never say "clean" again, which is the conservative direction.
    pub fn is_at_saved_mark(&self) -> bool {
        self.saved_mark == Some(self.cursor)
    }

    /// Moved only by a save.
    pub fn mark_saved(&mut self) {
        self.saved_mark = Some(self.cursor);
        self.forgot_saved_mark = false;
    }

    /// A new edit after an undo truncates the redo tail: the future that was
    /// undone is no longer reachable once a different one is chosen.
    pub fn push(&mut self, entry: Entry) {
        self.truncate_redo_tail();
        self.resident += entry.resident_bytes();
        self.entries.push(entry);
        self.cursor += 1;
        self.evict_to_bound();
    }

    /// The entry to revert, and the cursor step, as one operation so a caller
    /// cannot take one without the other.
    pub fn undo(&mut self) -> Option<&Entry> {
        self.cursor = self.cursor.checked_sub(1)?;
        self.entries.get(self.cursor)
    }

    pub fn redo(&mut self) -> Option<&Entry> {
        let entry = self.entries.get(self.cursor)?;
        self.cursor += 1;
        Some(entry)
    }

    fn truncate_redo_tail(&mut self) {
        for entry in self.entries.drain(self.cursor..) {
            self.resident -= entry.resident_bytes();
        }
        if self.saved_mark.is_some_and(|mark| mark > self.cursor) {
            self.saved_mark = None;
            self.forgot_saved_mark = true;
        }
    }

    /// Drop the oldest entries until the stack fits. The newest entry is never
    /// evicted even when it alone exceeds the bound: the alternative is an edit
    /// that cannot be undone at all, which is worse than one over budget.
    fn evict_to_bound(&mut self) {
        let mut dropped = 0;
        while self.resident > self.max_bytes && self.cursor > 1 {
            let entry = self.entries.remove(0);
            self.resident -= entry.resident_bytes();
            self.cursor -= 1;
            dropped += 1;
        }
        if dropped == 0 {
            return;
        }
        self.forgotten += dropped;
        match self.saved_mark {
            Some(mark) => match mark.checked_sub(dropped) {
                Some(shifted) => self.saved_mark = Some(shifted),
                None => {
                    self.saved_mark = None;
                    self.forgot_saved_mark = true;
                }
            },
            None => self.forgot_saved_mark = true,
        }
    }
}
