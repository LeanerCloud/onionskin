//! Fix-up 6: article threads.
//!
//! An article is a ring of beads, each naming its page with `/P`, linked
//! through `/N` and `/V`. A page's own `/B` key lists the beads that start on
//! it, and the first draft of this plan called `/B` "survives untouched" -
//! which is true of the key and false of the ring it points into. Deleting a
//! page leaves the surviving beads' `/N` and `/V` naming beads on a page that
//! is gone, and under the free-nothing rule those beads are still there, still
//! parse, and are no longer reachable through the page tree.
//!
//! So the ring is re-linked past them, and a thread whose beads have all gone
//! leaves `/Threads`.
//!
//! A ring that still walks but visits a garbage bead passes any reachability
//! check, which is why the test for this walks the ring and compares the bead
//! set rather than asking whether it terminates.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::rewrite::{dict_at, resolve};
use crate::edit::Transaction;
use crate::Result;

/// What a thread repair dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dropped {
    pub(crate) beads: usize,
    pub(crate) threads: usize,
}

pub(crate) fn repair(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    removed: &BTreeSet<u32>,
) -> Result<Dropped> {
    if removed.is_empty() {
        return Ok(Dropped::default());
    }
    let catalog = dict_at(tx, catalog_ref)?;
    let threads_entry = catalog.get(b"Threads").cloned();
    let Some(Object::Array(threads)) = resolve(tx, threads_entry.as_ref())? else {
        return Ok(Dropped::default());
    };

    let mut report = Dropped::default();
    let mut kept_threads = Vec::with_capacity(threads.len());
    for thread in &threads {
        match repair_one(tx, thread, removed, &mut report)? {
            true => kept_threads.push(thread.clone()),
            false => report.threads += 1,
        }
    }
    if report == Dropped::default() {
        return Ok(report);
    }

    match threads_entry.as_ref().and_then(Object::as_reference) {
        Some(objref) if !kept_threads.is_empty() => {
            tx.put_object(
                objref.number,
                objref.generation,
                Object::Array(kept_threads),
            )?;
        }
        _ => {
            let mut catalog = dict_at(tx, catalog_ref)?;
            if kept_threads.is_empty() {
                catalog.remove(b"Threads");
            } else {
                catalog.set(Name::new("Threads"), Object::Array(kept_threads));
            }
            tx.put_object(
                catalog_ref.number,
                catalog_ref.generation,
                Object::Dict(catalog),
            )?;
        }
    }
    Ok(report)
}

/// Re-link one thread's ring. `false` means every bead went and the thread
/// itself should be dropped.
fn repair_one(
    tx: &mut Transaction<'_>,
    thread: &Object,
    removed: &BTreeSet<u32>,
    report: &mut Dropped,
) -> Result<bool> {
    let Some(Object::Dict(dict)) = resolve(tx, Some(thread))? else {
        return Ok(true);
    };
    let Some(first) = dict.get(b"F").and_then(Object::as_reference) else {
        return Ok(true);
    };

    // The ring, walked once through `/N` from the first bead. Read whole
    // before anything is written, for the same reason the outline is.
    let mut ring: Vec<ObjRef> = Vec::new();
    let mut beads: BTreeMap<u32, Dict> = BTreeMap::new();
    let mut at = first;
    for _ in 0..100_000 {
        let Ok(bead) = dict_at(tx, at) else {
            break;
        };
        if beads.insert(at.number, bead.clone()).is_some() {
            break;
        }
        ring.push(at);
        let Some(next) = bead.get(b"N").and_then(Object::as_reference) else {
            break;
        };
        if next.number == first.number {
            break;
        }
        at = next;
    }

    let survivors: Vec<ObjRef> = ring
        .iter()
        .copied()
        .filter(|objref| {
            beads[&objref.number]
                .get(b"P")
                .and_then(Object::as_reference)
                .is_none_or(|page| !removed.contains(&page.number))
        })
        .collect();
    report.beads += ring.len() - survivors.len();
    if survivors.len() == ring.len() {
        return Ok(true);
    }
    if survivors.is_empty() {
        return Ok(false);
    }

    // Re-linked as a ring: the last bead's `/N` closes back to the first, and
    // every `/V` mirrors it. A two-bead ring has each naming the other, and a
    // one-bead ring names itself, which is what ISO 32000-1 12.4.3 asks for.
    for (index, objref) in survivors.iter().enumerate() {
        let next = survivors[(index + 1) % survivors.len()];
        let previous = survivors[(index + survivors.len() - 1) % survivors.len()];
        let mut bead = beads[&objref.number].clone();
        bead.set(Name::new("N"), Object::Ref(next));
        bead.set(Name::new("V"), Object::Ref(previous));
        tx.put_object(objref.number, objref.generation, Object::Dict(bead))?;
    }

    // The thread's `/F` has to name a bead that is still in the ring.
    if !survivors.iter().any(|objref| objref.number == first.number) {
        if let Some(objref) = thread.as_reference() {
            let mut dict = dict.clone();
            dict.set(Name::new("F"), Object::Ref(survivors[0]));
            tx.put_object(objref.number, objref.generation, Object::Dict(dict))?;
        }
    }
    Ok(true)
}
