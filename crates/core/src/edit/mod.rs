//! The edit graph: one place that knows what has changed, one stack that can
//! take it back, and one typed vocabulary every plugin speaks.
//!
//! [`Overlay`] holds the state, [`History`] holds the steps, [`DocumentEdit`]
//! is the vocabulary, and [`EditSession`] is the only thing that touches all
//! three. A caller never moves the overlay without recording the step, because
//! the only way in is [`EditSession::transact`].

mod history;
mod overlay;
mod verb;

use std::collections::BTreeMap;

use onionskin_cos::{Document as CosDocument, Name, Object};

pub use history::{Entry, History, MAX_HISTORY_BYTES};
pub use overlay::{Change, ObjectState, Overlay, TrailerState};
pub use verb::DocumentEdit;

use crate::protection::EditKind;
use crate::session::Result;
use overlay::ChangeKey;

/// The document's edit state: what has changed and how to walk it back.
#[derive(Clone, Debug)]
pub struct EditSession {
    overlay: Overlay,
    history: History,
    /// Incremented by every commit, undo, redo, rebase and forget.
    ///
    /// The document's caches key on this, so an edit made through
    /// `Document::edit_mut` invalidates them without the caller having to
    /// remember to. A convention every caller must follow is a convention some
    /// caller will not.
    epoch: u64,
}

impl EditSession {
    pub fn for_base(base: &CosDocument) -> Self {
        EditSession {
            overlay: Overlay::for_base(base),
            history: History::default(),
            epoch: 0,
        }
    }

    /// Changes whenever what a reader should see changes.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// A session whose undo stack is bounded at `max_bytes` rather than at
    /// [`MAX_HISTORY_BYTES`]. The eviction rule is the same one; this only
    /// makes it reachable without allocating a quarter of a gigabyte, which is
    /// what the bound test needs.
    pub fn with_history_bound(base: &CosDocument, max_bytes: usize) -> Self {
        EditSession {
            overlay: Overlay::for_base(base),
            history: History::with_bound(max_bytes),
            epoch: 0,
        }
    }

    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn is_dirty(&self) -> bool {
        !self.history.is_at_saved_mark()
    }

    /// Collect every change one label's worth of work makes into a single
    /// entry. Two producers writing the same object inside one transaction
    /// coalesce into one change whose `before` is the state before the
    /// transaction and whose `after` is the last write, in either order.
    ///
    /// An aborted transaction leaves the overlay untouched, including any
    /// object numbers it reserved: an abort cannot leak a number.
    ///
    /// **Refused where the base's security does not allow it**, before the
    /// body runs: refusing here - the one door every tool, command and verb
    /// goes through - is what makes the editing gate a property of the
    /// document rather than a flag someone has to remember to check. A
    /// transaction is a change of [`EditKind::Content`]; see
    /// [`EditSession::transact_as`] and `core::protection`.
    pub fn transact<F, T>(&mut self, base: &CosDocument, label: &'static str, body: F) -> Result<T>
    where
        F: FnOnce(&mut Transaction<'_>) -> Result<T>,
    {
        self.transact_as(base, label, EditKind::Content, body)
    }

    /// [`EditSession::transact`] for a change of `kind`, which the base's
    /// security may allow where it allows no other.
    pub fn transact_as<F, T>(
        &mut self,
        base: &CosDocument,
        label: &'static str,
        kind: EditKind,
        body: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut Transaction<'_>) -> Result<T>,
    {
        crate::protection::edit_as(base, kind).map_err(crate::Error::Protected)?;
        let reserved_before = self.overlay.next_number();
        let mut tx = Transaction {
            base,
            overlay: &mut self.overlay,
            changes: Vec::new(),
            index: BTreeMap::new(),
        };
        let outcome = body(&mut tx).and_then(|value| tx.drop_orphans().map(|()| value));
        let changes = tx.finish();
        match outcome {
            Err(error) => {
                rollback(&mut self.overlay, &changes);
                self.overlay.set_next_number(reserved_before);
                Err(error)
            }
            Ok(value) => {
                self.epoch += 1;
                self.overlay.collapse(base)?;
                let kept: Vec<Change> = changes.into_iter().filter(|c| !c.is_noop()).collect();
                if !kept.is_empty() {
                    self.history.push(Entry::new(label, kept));
                }
                Ok(value)
            }
        }
    }

    /// Apply one typed verb as its own undoable step.
    pub fn apply(&mut self, base: &CosDocument, edit: DocumentEdit) -> Result<()> {
        self.transact(base, edit.label(), |tx| edit.apply(tx))
    }

    pub fn undo(&mut self, base: &CosDocument) -> Result<bool> {
        let Some(changes) = self.history.undo().map(|e| e.changes().to_vec()) else {
            return Ok(false);
        };
        rollback(&mut self.overlay, &changes);
        self.overlay.collapse(base)?;
        self.epoch += 1;
        Ok(true)
    }

    pub fn redo(&mut self, base: &CosDocument) -> Result<bool> {
        let Some(changes) = self.history.redo().map(|e| e.changes().to_vec()) else {
            return Ok(false);
        };
        for change in &changes {
            self.overlay.apply(change);
        }
        self.overlay.collapse(base)?;
        self.epoch += 1;
        Ok(true)
    }

    /// After a save the overlay's contents have become the base, so the overlay
    /// empties and the reservation counter rebases against the reopened
    /// document. The history survives: undoing across a save is the case
    /// [`TrailerState::Cleared`] exists for.
    pub fn rebase(&mut self, reopened: &CosDocument) {
        self.overlay.clear();
        self.overlay.set_next_number(reopened.next_object_number());
        self.history.mark_saved();
        self.epoch += 1;
    }

    /// Forget the overlay and the whole stack, in both directions, and report
    /// clean against `base`.
    ///
    /// `revert_to` needs this and nothing else does. Left out, the stack goes
    /// on describing bytes that no longer exist: every captured `before` was
    /// read against a base the truncation just removed, so an undo after a
    /// revert would restore objects into a document that never had them.
    /// Refusing a revert on unsaved edits does not cover that, because the
    /// entries below the saved mark are exactly the ones that survive it.
    pub fn forget(&mut self, base: &CosDocument) {
        self.overlay = Overlay::for_base(base);
        self.history = History::default();
        self.epoch += 1;
    }

    /// What the section writer consumes, as a fresh pair of maps. Nothing is
    /// ever written into cos's own edit map.
    pub fn pending_edits(&self) -> BTreeMap<u32, onionskin_cos::PendingEdit> {
        self.overlay.pending_edits()
    }

    pub fn trailer_edits(&self) -> BTreeMap<Name, Option<Object>> {
        self.overlay.trailer_edits()
    }
}

fn rollback(overlay: &mut Overlay, changes: &[Change]) {
    for change in changes.iter().rev() {
        overlay.revert(change);
    }
}

/// The write surface inside a transaction. Every write captures its own
/// `before` by the base-capture rule before it moves the overlay, so a reader
/// later in the same transaction sees the earlier write.
pub struct Transaction<'a> {
    base: &'a CosDocument,
    overlay: &'a mut Overlay,
    changes: Vec<Change>,
    index: BTreeMap<ChangeKey, usize>,
}

impl<'a> Transaction<'a> {
    /// The document the transaction edits, as it was opened: what a verb reads
    /// a structure tree from, and what an importer measures its source against.
    /// Read-only, like every other way in: the only writes go through
    /// [`Transaction::put_object`] and [`Transaction::set_trailer`].
    pub fn base(&self) -> &'a CosDocument {
        self.base
    }

    /// The current value at a number: the overlay's if it has one, else the
    /// base's, else `None`.
    pub fn object(&self, number: u32) -> Result<Option<ObjectState>> {
        self.overlay.capture_object(self.base, number)
    }

    /// The current value of a trailer key, resolved the same way.
    pub fn trailer_value(&self, key: &[u8]) -> Option<Object> {
        match self.overlay.capture_trailer(self.base, &Name(key.to_vec())) {
            TrailerState::Cleared => None,
            TrailerState::Set(object) => Some(object),
        }
    }

    /// A number no object in the base or the overlay uses.
    pub fn reserve(&mut self) -> u32 {
        self.overlay.reserve()
    }

    pub fn put_object(&mut self, number: u32, generation: u16, object: Object) -> Result<()> {
        // A number written directly, rather than handed out by `reserve`, is
        // still taken: the next `reserve` must not hand it out again. Replaying
        // a recovery file writes numbers the reopened document never had,
        // and the first edit after it would otherwise land on one of them
        // (T3).
        if number >= self.overlay.next_number() {
            self.overlay.set_next_number(number + 1);
        }
        let before = self.overlay.capture_object(self.base, number)?;
        let after = Some(ObjectState::new(generation, object));
        self.record(Change::Object {
            number,
            before,
            after,
        });
        Ok(())
    }

    pub fn set_trailer(&mut self, key: Name, value: Option<Object>) -> Result<()> {
        let before = self.overlay.capture_trailer(self.base, &key);
        let after = match value {
            None => TrailerState::Cleared,
            Some(object) => TrailerState::Set(object),
        };
        self.record(Change::TrailerKey { key, before, after });
        Ok(())
    }

    /// One change per address. A second write to the same address keeps the
    /// first write's `before`, which is the state before the transaction, and
    /// takes the later `after`.
    fn record(&mut self, change: Change) {
        self.overlay.apply(&change);
        match self.index.get(&change.key()) {
            Some(&at) => self.changes[at] = merge(&self.changes[at], change),
            None => {
                self.index.insert(change.key(), self.changes.len());
                self.changes.push(change);
            }
        }
    }

    /// Record, as changes of this transaction, the objects an earlier edit
    /// created that nothing names any more.
    ///
    /// Collapse drops such objects from the overlay (its rule 2), and a drop
    /// that is not recorded cannot be undone: delete a bookmark made earlier
    /// in the session, undo the delete, and the outline names a dictionary
    /// the overlay no longer holds. Recorded here, the drop is an ordinary
    /// change with the object as its `before` and nothing as its `after`, so
    /// undo puts the object back and redo takes it away again. An object
    /// created and orphaned within this one transaction merges to a no-op.
    fn drop_orphans(&mut self) -> Result<()> {
        for number in self.overlay.unreachable_new_objects(self.base)? {
            let before = self.overlay.capture_object(self.base, number)?;
            self.record(Change::Object {
                number,
                before,
                after: None,
            });
        }
        Ok(())
    }

    fn finish(self) -> Vec<Change> {
        self.changes
    }
}

fn merge(existing: &Change, later: Change) -> Change {
    match (existing, later) {
        (Change::Object { before, .. }, Change::Object { number, after, .. }) => Change::Object {
            number,
            before: before.clone(),
            after,
        },
        (Change::TrailerKey { before, .. }, Change::TrailerKey { key, after, .. }) => {
            Change::TrailerKey {
                key,
                before: before.clone(),
                after,
            }
        }
        // Unreachable: the index is keyed by variant as well as address, so a
        // slot only ever merges with its own kind.
        (_, later) => later,
    }
}
