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
use onionskin_cos::{CountingSource, Dict, Document, FileSource, ObjRef, Object};

/// The same depth cap the accessor applies, so the two walks agree about where
/// a hostile tree stops rather than about how deep it goes.
const MAX_DEPTH: usize = 64;

/// Pages compared per file. Enough to cross several subtree boundaries in a
/// balanced tree; the bench file covers the thousand-page case on its own.
const PAGES_PER_FILE: usize = 40;

/// Visits one naive walk may make. It carries the accessor's hazard too: `seen`
/// is a path set, so a node listed twice under one parent is walked twice, and
/// a tree of those costs 2^depth. Smaller than the accessor's own budget,
/// because a file that needs more than this is one the comparison should give
/// up on rather than one it should race.
const MAX_VISITS: usize = 200_000;

/// A page tree read without believing anything it says about itself.
struct Naive {
    /// Every leaf, in document order.
    pages: Vec<ObjRef>,
    /// Who each node was first reached from, recorded during the descent rather
    /// than read from `/Parent`, which producers get wrong. First-reached is
    /// the chain the accessor takes to the first occurrence of a node; a tree
    /// that shares a subtree between two parents would need per-occurrence
    /// chains, and `abandoned` covers those files instead.
    parent: BTreeMap<u32, ObjRef>,
    visits: usize,
    /// A node that would not parse, or a walk that ran out of budget. The file
    /// is then not a fair comparison: the accessor reports the error and this
    /// walk would have to invent a policy for it.
    abandoned: bool,
}

impl Naive {
    fn read(doc: &Document) -> Option<Naive> {
        let catalog = doc.catalog().ok()?;
        let root = catalog.get(b"Pages").and_then(Object::as_reference)?;
        let mut walk = Naive {
            pages: Vec::new(),
            parent: BTreeMap::new(),
            visits: 0,
            abandoned: false,
        };
        walk.descend(doc, root, &mut BTreeSet::new(), 0);
        (!walk.abandoned).then_some(walk)
    }

    fn descend(&mut self, doc: &Document, node: ObjRef, seen: &mut BTreeSet<u32>, depth: usize) {
        self.visits += 1;
        if self.visits > MAX_VISITS {
            self.abandoned = true;
            return;
        }
        if depth >= MAX_DEPTH || !seen.insert(node.number) {
            return;
        }
        let Ok(parsed) = doc.get(node.number) else {
            self.abandoned = true;
            return;
        };
        let Some(dict) = parsed.object.as_dict().cloned() else {
            return;
        };
        let kids = match dict.get(b"Kids").map(|o| doc.resolve(o)) {
            Some(Ok(Object::Array(kids))) => kids,
            Some(Err(_)) => {
                self.abandoned = true;
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

    /// Whether the tree has an internal node below its root, which is what
    /// makes the `/Count` skip fire at all. A single `/Pages` node whose kids
    /// are all leaves exercises none of it.
    fn is_nested(&self) -> bool {
        self.pages
            .iter()
            .filter_map(|leaf| self.parent.get(&leaf.number))
            .any(|parent| self.parent.contains_key(&parent.number))
    }

    /// The nearest *usable* value of `key` at or above `page`, climbing the
    /// descent's own parent map.
    ///
    /// "Usable" is what the accessor means by it: a node whose entry does not
    /// pass `usable` does not shadow an ancestor whose entry does, so the climb
    /// keeps going rather than stopping at the first node that merely has the
    /// key. That is the rule under test, so it is spelled out here rather than
    /// borrowed.
    fn nearest<T>(
        &self,
        doc: &Document,
        page: ObjRef,
        key: &[u8],
        usable: impl Fn(Object) -> Option<T>,
    ) -> Option<T> {
        let mut current = Some(page);
        for _ in 0..MAX_DEPTH {
            let node = current?;
            let parsed = doc.get(node.number).ok()?;
            let dict = parsed.object.as_dict()?;
            if let Some(entry) = dict.get(key) {
                if let Ok(value) = doc.resolve(entry) {
                    if let Some(usable) = usable(value) {
                        return Some(usable);
                    }
                }
            }
            current = self.parent.get(&node.number).copied();
        }
        None
    }

    /// The rectangle the accessor would keep: four finite numbers enclosing a
    /// positive area, in the order the file wrote them. Indirect elements are
    /// resolved, the way the accessor resolves them.
    fn usable_rectangle(&self, doc: &Document, entry: Object) -> Option<[f64; 4]> {
        let Object::Array(items) = entry else {
            return None;
        };
        if items.len() < 4 {
            return None;
        }
        let mut v = [0.0f64; 4];
        for (slot, item) in v.iter_mut().zip(items.iter()) {
            *slot = match doc.resolve(item).ok()? {
                Object::Integer(i) => i as f64,
                Object::Real(r) => r,
                _ => return None,
            };
        }
        let ok = v.iter().all(|n| n.is_finite())
            && v[0].min(v[2]) < v[0].max(v[2])
            && v[1].min(v[3]) < v[1].max(v[3]);
        ok.then_some(v)
    }
}

fn usable_resources(entry: Object) -> Option<Dict> {
    match entry {
        Object::Dict(d) => Some(d),
        _ => None,
    }
}

/// What the sweep proved, as opposed to how many files it opened.
#[derive(Default)]
struct Coverage {
    /// Files whose page tree has an internal node below the root, so the
    /// `/Count` skip actually ran.
    nested: usize,
}

fn check(tally: &mut Tally, coverage: &mut Coverage, path: &Path) {
    let doc = match Document::open_path_repairing(path) {
        Ok((doc, _)) => doc,
        Err(e) => {
            tally.skip(path, e.category());
            return;
        }
    };
    // A tree with a node that will not parse is not a fair comparison: the
    // accessor reports the error and the naive walk would have to invent a
    // policy for it.
    let Some(naive) = Naive::read(&doc) else {
        tally.skip(path, "unreadable-page-tree");
        return;
    };

    // Page zero is the one `first_page` reaches by its own, shorter descent.
    // Checked before the single-page skip below, because the corpus is mostly
    // single-page files and they are as entitled to agree as the rest.
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

    // A one-page tree proves nothing about ordering.
    if naive.pages.len() < 2 {
        tally.skip(path, "single-page");
        return;
    }
    if naive.is_nested() {
        coverage.nested += 1;
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
        if node.resources != naive.nearest(&doc, *want, b"Resources", usable_resources) {
            wrong("wrong-resources", "/Resources disagrees".into());
            return;
        }
        let rect =
            |key: &[u8]| naive.nearest(&doc, *want, key, |o| naive.usable_rectangle(&doc, o));
        if node.media_box != rect(b"MediaBox") {
            wrong(
                "wrong-media-box",
                format!("/MediaBox came back as {:?}", node.media_box),
            );
            return;
        }
        if node.crop_box != rect(b"CropBox") {
            wrong(
                "wrong-crop-box",
                format!("/CropBox came back as {:?}", node.crop_box),
            );
            return;
        }
        let rotate = naive.nearest(&doc, *want, b"Rotate", |o| o.as_integer());
        if node.rotate != rotate {
            wrong(
                "wrong-rotate",
                format!("/Rotate came back as {:?}, not {rotate:?}", node.rotate),
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
    let mut coverage = Coverage::default();
    for path in pdfs_in(&root.join("seeds")) {
        check(&mut tally, &mut coverage, &path);
    }
    let external = corpus_dir("external");
    if let Some(dir) = &external {
        for path in pdfs_in(dir) {
            check(&mut tally, &mut coverage, &path);
        }
    } else {
        eprintln!("NOTE: external/ is absent, so only the seeds were compared");
    }
    tally.report();
    println!(
        "   of {} multi-page files, {} have a nested page tree",
        tally.passed.len(),
        coverage.nested
    );

    assert_eq!(
        tally.failure_count(),
        0,
        "the accessor and the naive walk disagree"
    );
    assert!(
        !tally.passed.is_empty(),
        "no multi-page file was checked, so nothing was proven"
    );
    // Floors, not today's numbers: without the external corpus the seeds carry
    // one multi-page file and no nested tree, and with it the counts are in the
    // dozens. A sweep that quietly stopped finding files would otherwise still
    // pass.
    if external.is_some() {
        assert!(
            tally.passed.len() >= 20,
            "only {} multi-page files were compared; the external corpus should carry dozens",
            tally.passed.len()
        );
        assert!(
            coverage.nested > 0,
            "no file with a nested page tree was compared, so the /Count skip never ran"
        );
    }
}

/// A page tree that costs 2^depth to walk while never repeating a node on any
/// one path, so neither the depth cap nor the cycle guard sees anything wrong.
///
/// Each internal node lists the same child twice. `seen` is a path set, so the
/// second listing is descended again from scratch; forty levels of that is a
/// trillion visits out of a file of forty objects. Before the visit budget this
/// did not fail, it hung, and `Document::page` is reachable from the fuzz
/// target.
#[test]
fn a_page_tree_that_doubles_at_every_level_is_refused_rather_than_walked() {
    const LEVELS: u32 = 40;

    let mut bodies: Vec<Vec<u8>> = vec![b"<</Type/Catalog/Pages 2 0 R>>".to_vec()];
    for node in 2..=(LEVELS + 1) {
        let kid = node + 1;
        bodies.push(format!("<</Type/Pages/Kids[{kid} 0 R {kid} 0 R]>>").into_bytes());
    }
    bodies.push(b"<</Type/Page/MediaBox[0 0 200 100]>>".to_vec());
    let refs: Vec<&[u8]> = bodies.iter().map(Vec::as_slice).collect();

    let doc = Document::open(Box::new(onionskin_cos::BytesSource::new(
        common::classic_pdf(&refs, &[]),
    )))
    .expect("the fixture opens clean");

    let started = std::time::Instant::now();
    let err = doc
        .page(1_000_000)
        .expect_err("a trillion-visit page tree must not resolve a page");
    assert_eq!(err.category(), "depth-exceeded", "{err}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "the walk took {:?}, which is not a bound",
        started.elapsed()
    );
}

/// The thousand-page bench file, or `None` after saying why.
///
/// `bench/` is generated by `corpus/make-bench.py` rather than fetched, so its
/// absence is a loud skip and not an `ONIONSKIN_CORPUS_REQUIRED` failure: that
/// flag guarantees the fetched corpus, and wiring the generator into CI belongs
/// to the package that adds the benches.
fn bench_file() -> Option<std::path::PathBuf> {
    let root = corpus_root().or_else(|| {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        None
    })?;
    let path = root.join("bench").join("pages-1000.pdf");
    if !path.is_file() {
        eprintln!(
            "SKIPPED: {} is absent; generate it with corpus/make-bench.py",
            path.display()
        );
        return None;
    }
    Some(path)
}

/// The `/Count` skip has to land on the right page. The corpus is almost all
/// single-level page trees, where the skip never fires, so the three-level
/// bench file is where the risk the plan named actually lives: a skip that is
/// off by one renumbers every page after it.
#[test]
fn skipping_subtrees_by_count_lands_on_the_same_pages_as_a_walk_that_does_not() {
    let Some(path) = bench_file() else {
        return;
    };
    let doc = Document::open_path(&path).expect("the bench file opens clean");
    let naive = Naive::read(&doc).expect("the bench page tree reads");
    assert_eq!(naive.pages.len(), 1000);
    assert!(
        naive.is_nested(),
        "the bench file's tree is flat, so it exercises no /Count skip"
    );

    for index in 0..naive.pages.len() {
        let node = doc.page(index).expect("every bench page resolves");
        assert_eq!(
            node.objref, naive.pages[index],
            "page {index} is object {} but the walk that reads no /Count says {}",
            node.objref.number, naive.pages[index].number
        );
    }
}

/// The `/Count` skip has to be worth having: reaching page 500 of a thousand
/// must read a fraction of what walking every node would.
#[test]
fn reaching_a_page_in_the_middle_reads_far_less_than_a_full_walk() {
    let Some(path) = bench_file() else {
        return;
    };

    let measure = |work: &dyn Fn(&Document)| -> u64 {
        let file = FileSource::open(&path).expect("the bench file opens");
        let (counting, stats) = CountingSource::new(Box::new(file));
        let document = Document::open(Box::new(counting)).expect("the bench file opens clean");
        // Opening is its own budget, measured in tests/lazy.rs.
        stats.reset();
        work(&document);
        stats.total()
    };

    let indexed = measure(&|doc| {
        doc.page(500).expect("page 500 resolves");
    });
    let full = measure(&|doc| {
        for index in 0..1000 {
            doc.page(index).expect("every page resolves");
        }
    });
    let file_len = std::fs::metadata(&path)
        .expect("the bench file stats")
        .len();

    println!("page(500) read {indexed} bytes; every page read {full}; the file is {file_len}");
    assert!(
        indexed * 20 < full,
        "page(500) read {indexed} bytes against {full} for the whole tree, which is no skip at all"
    );
    assert!(
        indexed * 50 < file_len,
        "page(500) read {indexed} bytes of a {file_len} byte file"
    );
}
