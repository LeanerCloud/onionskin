//! The overlay: every object and trailer key this session has changed, and
//! enough of what was there before to put all of it back.
//!
//! Nothing here writes into `cos::Document`'s own edit map. The two meet at
//! exactly one place: [`Overlay::pending_edits`] and
//! [`Overlay::trailer_edits`] build a fresh pair of maps for
//! `cos::Document::section_for` at save time. Projecting onto
//! [`PendingEdit::Set`] is one-to-one with nothing to decide, because M3 frees
//! no object number and so this module has no deletion to express.
//!
//! Two rules carry the module and both are about `before`.
//!
//! **The base-capture rule.** A change's `before` is captured when the edit is
//! made, by this precedence: the overlay's current state for that number if the
//! overlay has one; else the base's value if the base has the number; else
//! `None`. The overlay clause is not a refinement. Reading through the base
//! alone makes the *second* edit of an object un-undoable: base object 7 is A,
//! an edit to B captures `before = A`, and a second edit to C captures the base
//! again, A rather than B. One undo then restores A and loses two edits with a
//! green suite. `None` therefore means one thing only, "this number was not in
//! the overlay and not in the base", which is what the reservation counter
//! alone produces.
//!
//! **The collapse rule.** An overlay entry whose value equals the base's drops
//! out, by value and not by a dirty flag, so edit-then-undo-then-save appends
//! nothing. The trailer needs the same clause or the pre-save case breaks the
//! other way: an undo that correctly leaves `Cleared` for a key the base never
//! had would otherwise write `/Info null` into a document that never had one.
//! `Cleared` equals absent; `Set(v)` equals a base value of `v`.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Document as CosDocument, Name, Object, PendingEdit, XrefEntry};

use crate::session::Result;

/// One overlaid object.
///
/// There is deliberately no `Deleted` variant. M3 frees no object number, so a
/// removal is a rewrite of the referrer, and a variant for it would be a shape
/// no verb produces.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectState {
    pub generation: u16,
    pub object: Object,
}

impl ObjectState {
    pub fn new(generation: u16, object: Object) -> Self {
        ObjectState { generation, object }
    }

    pub(crate) fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + object_bytes(&self.object)
    }
}

/// One overlaid trailer key.
///
/// `Cleared` exists because the trailer is the root and has no referrer that
/// can stop naming a key, so undoing a key's creation has to say "clear it"
/// rather than forget it. There is deliberately no `NotInOverlay`: nothing
/// produces it, and its only reachable use was the wrong capture the
/// base-capture rule forbids, so cutting it makes that unexpressible.
#[derive(Clone, Debug, PartialEq)]
pub enum TrailerState {
    Cleared,
    Set(Object),
}

impl TrailerState {
    fn from_value(value: Option<&Object>) -> Self {
        match value {
            None => TrailerState::Cleared,
            Some(object) => TrailerState::Set(object.clone()),
        }
    }

    fn into_value(self) -> Option<Object> {
        match self {
            TrailerState::Cleared => None,
            TrailerState::Set(object) => Some(object),
        }
    }

    fn as_value(&self) -> Option<&Object> {
        match self {
            TrailerState::Cleared => None,
            TrailerState::Set(object) => Some(object),
        }
    }

    pub(crate) fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.as_value().map_or(0, object_bytes)
    }
}

/// One undoable step, at the granularity the section writer consumes.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Object {
        number: u32,
        before: Option<ObjectState>,
        after: Option<ObjectState>,
    },
    TrailerKey {
        key: Name,
        before: TrailerState,
        after: TrailerState,
    },
}

impl Change {
    /// A change that restores what was already there carries no information and
    /// is dropped before it reaches an [`crate::edit::Entry`], so that an undo
    /// never spends a step doing nothing the user can see.
    pub fn is_noop(&self) -> bool {
        match self {
            Change::Object { before, after, .. } => before == after,
            Change::TrailerKey { before, after, .. } => before == after,
        }
    }

    pub(crate) fn key(&self) -> ChangeKey {
        match self {
            Change::Object { number, .. } => ChangeKey::Object(*number),
            Change::TrailerKey { key, .. } => ChangeKey::TrailerKey(key.clone()),
        }
    }

    /// Both sides count: an entry holds the value it replaced as well as the
    /// value it wrote, and the history bound is over what is resident.
    pub(crate) fn resident_bytes(&self) -> usize {
        let sides = match self {
            Change::Object { before, after, .. } => {
                before.as_ref().map_or(0, ObjectState::resident_bytes)
                    + after.as_ref().map_or(0, ObjectState::resident_bytes)
            }
            Change::TrailerKey { key, before, after } => {
                key.as_bytes().len() + before.resident_bytes() + after.resident_bytes()
            }
        };
        std::mem::size_of::<Self>() + sides
    }
}

/// What a change addresses, so that two producers writing the same object
/// inside one transaction coalesce into one change rather than two.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ChangeKey {
    Object(u32),
    TrailerKey(Name),
}

/// What this session has changed, over a base document it never mutates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    states: BTreeMap<u32, ObjectState>,
    trailer: BTreeMap<Name, Option<Object>>,
    next_number: u32,
}

impl Overlay {
    /// The reservation counter starts above every number the base uses, so a
    /// reserved number can never collide with one the file already has.
    pub fn for_base(base: &CosDocument) -> Self {
        Overlay {
            states: BTreeMap::new(),
            trailer: BTreeMap::new(),
            next_number: base.next_object_number(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty() && self.trailer.is_empty()
    }

    pub fn object(&self, number: u32) -> Option<&ObjectState> {
        self.states.get(&number)
    }

    pub fn trailer_key(&self, key: &Name) -> Option<TrailerState> {
        self.trailer
            .get(key)
            .map(|value| TrailerState::from_value(value.as_ref()))
    }

    /// A number no object in the base or the overlay uses. Handing one out is
    /// the only thing that produces a `before: None`.
    pub fn reserve(&mut self) -> u32 {
        let number = self.next_number;
        self.next_number += 1;
        number
    }

    pub(crate) fn next_number(&self) -> u32 {
        self.next_number
    }

    pub(crate) fn set_next_number(&mut self, number: u32) {
        self.next_number = number;
    }

    /// The object half of the base-capture rule. Overlay first, base second,
    /// `None` only when neither has the number.
    pub fn capture_object(&self, base: &CosDocument, number: u32) -> Result<Option<ObjectState>> {
        if let Some(state) = self.states.get(&number) {
            return Ok(Some(state.clone()));
        }
        if !base_has(base, number) {
            return Ok(None);
        }
        let parsed = base.get(number)?;
        Ok(Some(ObjectState::new(
            parsed.objref.generation,
            parsed.object,
        )))
    }

    /// The trailer half, one level up. There is no third outcome, which is why
    /// [`TrailerState`] has two variants rather than three.
    pub fn capture_trailer(&self, base: &CosDocument, key: &Name) -> TrailerState {
        if let Some(state) = self.trailer_key(key) {
            return state;
        }
        TrailerState::from_value(base.trailer().get(key.as_bytes()))
    }

    /// Move the overlay to a change's `after`. Applying and reverting are the
    /// same operation against opposite sides, which is what keeps redo exact.
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Object { number, after, .. } => self.put_object(*number, after.clone()),
            Change::TrailerKey { key, after, .. } => {
                self.put_trailer(key.clone(), after.clone());
            }
        }
    }

    pub fn revert(&mut self, change: &Change) {
        match change {
            Change::Object { number, before, .. } => self.put_object(*number, before.clone()),
            Change::TrailerKey { key, before, .. } => {
                self.put_trailer(key.clone(), before.clone());
            }
        }
    }

    fn put_object(&mut self, number: u32, state: Option<ObjectState>) {
        match state {
            Some(state) => {
                self.states.insert(number, state);
            }
            None => {
                self.states.remove(&number);
            }
        }
    }

    fn put_trailer(&mut self, key: Name, state: TrailerState) {
        self.trailer.insert(key, state.into_value());
    }

    /// Drop every entry the document no longer needs.
    ///
    /// Two rules, and the second cannot be reached by the first.
    ///
    /// **Rule 1, by value.** An entry that now equals the base drops out. A
    /// dirty flag here would make edit-then-undo-then-save append an empty
    /// section.
    ///
    /// **Rule 2, by reachability.** An object this session *created* has no
    /// base value to compare against, so rule 1 can never drop it. Add an
    /// annotation and delete it again in one session: the page's `/Annots`
    /// collapses back to the base under rule 1, and the annotation dictionary
    /// and its appearance stream are left in the overlay with nothing naming
    /// them. The save would then append a section carrying two orphans, on a
    /// document the user changed and changed back. So an overlay-only object
    /// that nothing in the merged document reaches drops out too.
    pub fn collapse(&mut self, base: &CosDocument) -> Result<()> {
        let mut settled = Vec::new();
        for (number, state) in &self.states {
            if base_state_matches(base, *number, state)? {
                settled.push(*number);
            }
        }
        for number in settled {
            self.states.remove(&number);
        }
        let trailer = base.trailer();
        self.trailer
            .retain(|key, value| value.as_ref() != trailer.get(key.as_bytes()));
        self.drop_unreachable_new_objects(base)?;
        Ok(())
    }

    /// Rule 2. Skipped entirely unless the overlay holds an object the base
    /// does not, which is what keeps an ordinary editing session from paying
    /// for a reachability walk it cannot need.
    fn drop_unreachable_new_objects(&mut self, base: &CosDocument) -> Result<()> {
        let created: Vec<u32> = self
            .states
            .keys()
            .copied()
            .filter(|number| !base_has(base, *number))
            .collect();
        if created.is_empty() {
            return Ok(());
        }

        let reached = self.reachable(base)?;
        for number in created {
            if !reached.contains(&number) {
                self.states.remove(&number);
            }
        }
        Ok(())
    }

    /// Every object number reachable from the merged trailer, where "merged"
    /// means the overlay's value for a number when it has one and the base's
    /// otherwise.
    fn reachable(&self, base: &CosDocument) -> Result<BTreeSet<u32>> {
        let mut reached = BTreeSet::new();
        let mut queue: Vec<Object> = Vec::new();

        // The merged trailer: the base's keys, with this session's overrides
        // applied and its cleared keys removed.
        for (key, value) in base.trailer().iter() {
            match self.trailer.get(key) {
                Some(Some(replacement)) => queue.push(replacement.clone()),
                Some(None) => {}
                None => queue.push(value.clone()),
            }
        }
        for value in self.trailer.values().flatten() {
            queue.push(value.clone());
        }

        while let Some(object) = queue.pop() {
            match object {
                Object::Ref(objref) => {
                    if !reached.insert(objref.number) {
                        continue;
                    }
                    match self.states.get(&objref.number) {
                        Some(state) => queue.push(state.object.clone()),
                        None => {
                            if base_has(base, objref.number) {
                                queue.push(base.get(objref.number)?.object);
                            }
                        }
                    }
                }
                Object::Array(items) => queue.extend(items),
                Object::Dict(dict) => queue.extend(dict.iter().map(|(_, v)| v.clone())),
                Object::Stream(stream) => queue.extend(stream.dict.iter().map(|(_, v)| v.clone())),
                _ => {}
            }
        }
        Ok(reached)
    }

    /// The projection the section writer consumes. `Set` is the only variant
    /// this can produce.
    pub fn pending_edits(&self) -> BTreeMap<u32, PendingEdit> {
        self.states
            .iter()
            .map(|(number, state)| {
                (
                    *number,
                    PendingEdit::Set {
                        generation: state.generation,
                        object: state.object.clone(),
                    },
                )
            })
            .collect()
    }

    pub fn trailer_edits(&self) -> BTreeMap<Name, Option<Object>> {
        self.trailer.clone()
    }

    /// After a save the overlay's contents have become the base. The
    /// reservation counter does not reset: it is rebased by the caller against
    /// the reopened document.
    pub fn clear(&mut self) {
        self.states.clear();
        self.trailer.clear();
    }
}

/// Whether the base document has a live object under this number. A free entry
/// is not a value, so it is not something a `before` can be captured from.
fn base_has(base: &CosDocument, number: u32) -> bool {
    matches!(
        base.xref().get(number),
        Some(XrefEntry::InFile { .. }) | Some(XrefEntry::InObjectStream { .. })
    )
}

fn base_state_matches(base: &CosDocument, number: u32, state: &ObjectState) -> Result<bool> {
    if !base_has(base, number) {
        return Ok(false);
    }
    let parsed = base.get(number)?;
    Ok(parsed.objref.generation == state.generation && parsed.object == state.object)
}

/// Resident size of an object, counting the bytes it owns rather than the
/// bytes it would serialize to. A stream is its raw data, which is what makes
/// one entry expensive.
fn object_bytes(object: &Object) -> usize {
    match object {
        Object::Null | Object::Bool(_) | Object::Integer(_) | Object::Real(_) => 0,
        Object::String(bytes) => bytes.len(),
        Object::Name(name) => name.as_bytes().len(),
        Object::Ref(_) => 0,
        Object::Array(items) => items
            .iter()
            .map(|item| std::mem::size_of::<Object>() + object_bytes(item))
            .sum(),
        Object::Dict(dict) => dict_bytes(dict),
        Object::Stream(stream) => dict_bytes(&stream.dict) + stream.raw.len(),
    }
}

fn dict_bytes(dict: &onionskin_cos::Dict) -> usize {
    dict.iter()
        .map(|(key, value)| {
            key.as_bytes().len() + std::mem::size_of::<Object>() + object_bytes(value)
        })
        .sum()
}
