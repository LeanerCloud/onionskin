use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct CompositeTally<K> {
    pages: BTreeMap<K, PageTally>,
}

#[derive(Debug)]
struct PageTally {
    incarnation: u64,
    observed: u64,
    total: u64,
}

impl<K: Ord> CompositeTally<K> {
    pub fn observe(&mut self, page: K, incarnation: u64, cumulative: u64) {
        use std::collections::btree_map::Entry;

        match self.pages.entry(page) {
            Entry::Vacant(entry) => {
                entry.insert(PageTally {
                    incarnation,
                    observed: cumulative,
                    total: cumulative,
                });
            }
            Entry::Occupied(mut entry) => {
                let tally = entry.get_mut();
                assert!(
                    incarnation >= tally.incarnation,
                    "cache incarnation decreased from {} to {incarnation}",
                    tally.incarnation
                );
                if incarnation > tally.incarnation {
                    tally.total = tally
                        .total
                        .checked_add(cumulative)
                        .expect("composite total overflowed");
                    tally.incarnation = incarnation;
                    tally.observed = cumulative;
                } else {
                    assert!(
                        cumulative >= tally.observed,
                        "composite counter decreased from {} to {cumulative} within incarnation \
                         {incarnation}",
                        tally.observed
                    );
                    tally.total = tally
                        .total
                        .checked_add(cumulative - tally.observed)
                        .expect("composite total overflowed");
                    tally.observed = cumulative;
                }
            }
        }
    }

    pub fn total(&self) -> u64 {
        self.pages.values().fold(0_u64, |total, page| {
            total
                .checked_add(page.total)
                .expect("composite total overflowed")
        })
    }

    pub fn pages(&self) -> usize {
        self.pages.len()
    }
}
