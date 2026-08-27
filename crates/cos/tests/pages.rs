//! The indexed page accessor, checked against a walk that takes no shortcuts.
//!
//! [`Document::page`] skips whole subtrees by their `/Count`, which is the
//! point of it and also the one thing that can silently renumber every later
//! page. So the sweep here compares it against a naive walk that reads no
//! `/Count` at all, and resolves the inheritable attributes by climbing back
//! up the tree rather than by carrying them down. Same page, same object, same
//! attributes, for every index of every multi-page corpus file.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::{corpus_dir, corpus_root, pdfs_in, Tally};
use onionskin_cos::{Dict, Document, ObjRef, Object};

/// The same depth cap the accessor applies, so the two walks agree about where
/// a hostile tree stops rather than about how deep it goes.
const MAX_DEPTH: usize = 64;

/// Pages compared per file. Enough to cross several subtree boundaries in a
/// balanced tree; the bench file covers the thousand-page case on its own.
const PAGES_PER_FILE: usize = 40;

/// A page tree read without believing anything it says about itself.
struct Naive {
    /// Every leaf, in document order.
    pages: Vec<ObjRef>,
    /// Who each node was first reached from, recorded during the descent
    /// rather than read from `/Parent`, which producers get wrong.
    parent: BTreeMap<u32, ObjRef>,
    /// A node that would not parse. The file is then not a fair comparison:
    /// the accessor reports the error and this walk would have to invent a
    /// policy for it.
    broken: bool,
}

impl Naive {
    fn read(doc: &Document) -> Option<Naive> {
        let catalog = doc.catalog().ok()?;
        let root = catalog.get(b"Pages").and_then(Object::as_reference)?;
        let mut walk = Naive {
            pages: Vec::new(),
            parent: BTreeMap::new(),
            broken: false,
        };
        walk.descend(doc, root, &mut BTreeSet::new(), 0);
        (!walk.broken).then_some(walk)
    }

    fn descend(&mut self, doc: &Document, node: ObjRef, seen: &mut BTreeSet<u32>, depth: usize) {
        if depth >= MAX_DEPTH || !seen.insert(node.number) {
            return;
        }
        let Ok(parsed) = doc.get(node.number) else {
            self.broken = true;
            return;
        };
        let Some(dict) = parsed.object.as_dict().cloned() else {
            return;
        };
        let kids = match dict.get(b"Kids").map(|o| doc.resolve(o)) {
            Some(Ok(Object::Array(kids))) => kids,
            Some(Err(_)) => {
                self.broken = true;
                return;
            }
            _ => {
                self.pages.push(parsed.objref);
                seen.remove(&node.number);
                return;
            }
        };
        for kid in kids {
            if let Some(kid) = kid.as_reference() {
                self.parent.entry(kid.number).or_insert(node);
                self.descend(doc, kid, seen, depth + 1);
            }
        }
        seen.remove(&node.number);
    }

    /// The nearest value of `key` at or above `page`, climbing the descent's
    /// own parent map.
    fn nearest(&self, doc: &Document, page: ObjRef, key: &[u8]) -> Option<Object> {
        let mut current = Some(page);
        for _ in 0..MAX_DEPTH {
            let node = current?;
            let parsed = doc.get(node.number).ok()?;
            let dict = parsed.object.as_dict()?;
            if let Some(entry) = dict.get(key) {
                if let Ok(value) = doc.resolve(entry) {
                    return Some(value);
                }
            }
            current = self.parent.get(&node.number).copied();
        }
        None
    }
}

/// The rectangle the accessor would keep: four finite numbers enclosing a
/// positive area, in the order the file wrote them.
fn usable_rectangle(entry: Option<Object>) -> Option<[f64; 4]> {
    let Some(Object::Array(items)) = entry else {
        return None;
    };
    if items.len() < 4 {
        return None;
    }
    let mut v = [0.0f64; 4];
    for (slot, item) in v.iter_mut().zip(items.iter()) {
        *slot = match item {
            Object::Integer(i) => *i as f64,
            Object::Real(r) => *r,
            _ => return None,
        };
    }
    let ok = v.iter().all(|n| n.is_finite())
        && v[0].min(v[2]) < v[0].max(v[2])
        && v[1].min(v[3]) < v[1].max(v[3]);
    ok.then_some(v)
}

fn resources_of(entry: Option<Object>) -> Option<Dict> {
    match entry {
        Some(Object::Dict(d)) => Some(d),
        _ => None,
    }
}

fn check(tally: &mut Tally, path: &Path) {
    let doc = match Document::open_path_repairing(path) {
        Ok((doc, _)) => doc,
        Err(e) => {
            tally.skip(path, e.category());
            return;
        }
    };
    // A tree with a node that will not parse is not a fair comparison: the
    // accessor reports the error and the naive walk would have to invent a
    // policy for it. A one-page tree proves nothing about ordering.
    let Some(naive) = Naive::read(&doc) else {
        tally.skip(path, "unreadable-page-tree");
        return;
    };
    if naive.pages.len() < 2 {
        tally.skip(path, "single-page");
        return;
    }

    for (index, want) in naive.pages.iter().enumerate().take(PAGES_PER_FILE) {
        let node = match doc.page(index) {
            Ok(node) => node,
            Err(e) => {
                tally.fail(path, e.category(), &format!("page {index}: {e}"));
                return;
            }
        };
        let mut wrong = |what: &str, detail: String| {
            tally.fail(path, what, &format!("page {index}: {detail}"));
        };
        if node.objref != *want {
            wrong(
                "wrong-page",
                format!(
                    "the accessor says object {} and the naive walk says {}",
                    node.objref.number, want.number
                ),
            );
            return;
        }
        if node.index != index {
            wrong("wrong-index", format!("came back as index {}", node.index));
            return;
        }
        if node.resources != resources_of(naive.nearest(&doc, *want, b"Resources")) {
            wrong("wrong-resources", "/Resources disagrees".into());
            return;
        }
        if node.media_box != usable_rectangle(naive.nearest(&doc, *want, b"MediaBox")) {
            wrong(
                "wrong-media-box",
                format!("/MediaBox came back as {:?}", node.media_box),
            );
            return;
        }
        if node.crop_box != usable_rectangle(naive.nearest(&doc, *want, b"CropBox")) {
            wrong(
                "wrong-crop-box",
                format!("/CropBox came back as {:?}", node.crop_box),
            );
            return;
        }
        let rotate = naive
            .nearest(&doc, *want, b"Rotate")
            .as_ref()
            .and_then(Object::as_integer);
        if node.rotate != rotate {
            wrong(
                "wrong-rotate",
                format!("/Rotate came back as {:?}, not {rotate:?}", node.rotate),
            );
            return;
        }
    }

    // Page zero is the one `first_page` reaches by its own, shorter descent.
    if let (Ok(node), Ok(first)) = (doc.page(0), doc.first_page()) {
        if node.objref != first.objref {
            tally.fail(
                path,
                "first-page-disagrees",
                &format!(
                    "page(0) is object {} and first_page() is {}",
                    node.objref.number, first.objref.number
                ),
            );
            return;
        }
    }

    // One past the end says so, rather than wrapping or repeating the last.
    match doc.page(naive.pages.len()) {
        Err(e) if e.category() == "no-such-page" => {}
        Err(e) => {
            tally.fail(path, e.category(), &format!("past the end: {e}"));
            return;
        }
        Ok(node) => {
            tally.fail(
                path,
                "page-past-the-end",
                &format!(
                    "asked for page {} of {} and got object {}",
                    naive.pages.len(),
                    naive.pages.len(),
                    node.objref.number
                ),
            );
            return;
        }
    }

    tally.pass(path);
}

#[test]
fn the_indexed_accessor_agrees_with_a_walk_that_reads_no_count() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };

    let mut tally = Tally::new("indexed page accessor");
    for path in pdfs_in(&root.join("seeds")) {
        check(&mut tally, &path);
    }
    let external = corpus_dir("external");
    if let Some(dir) = &external {
        for path in pdfs_in(dir) {
            check(&mut tally, &path);
        }
    } else {
        eprintln!("NOTE: external/ is absent, so only the seeds were compared");
    }
    tally.report();

    assert_eq!(
        tally.failure_count(),
        0,
        "the accessor and the naive walk disagree"
    );
    assert!(
        !tally.passed.is_empty(),
        "no multi-page file was checked, so nothing was proven"
    );
}
