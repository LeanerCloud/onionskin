# Item inventory

`item_inventory.py` proves that a file split relocated code without changing
it, by listing every item with a hash of its normalized body and comparing the
two listings as multisets. `procedure_mutation.py` mutation-tests the inventory
itself, `line_multiset.py` checks the same claim without parsing Rust, and
`item_inventory.py audit` reports what each non-private item reaches.

Each emitted listing carries a `# items:` census, because the item count is the
denominator any claim about it is read against and a reader counting by hand
will count something else. Two rules in particular: a trait's default methods
are items under that trait, never under an `impl`, and `impl` and `mod` headers
are deduplicated across files, since one `impl` block legitimately becomes
several when a split moves its methods apart.

The six `tabs-split-*.txt` files are the acceptance record for one change, the
split of `crates/app/src/shell/chrome/tabs.rs` into `chrome/tabs/`. They are a
record, not a fixture: **they are not regenerated, and they are expected to
disagree with the current tree** as soon as anyone edits those files. Their
`# emitted at` headers say which revision each was read at, and the baseline is
load-bearing: measured from `5ac2085` the split has three differences, measured
from `fa5a194` it has four, the fourth being `run_canvas_context_command`
becoming exhaustive.

To gate a later relocation, do not diff against these. Emit `before` at that
change's own base and `after` at its head, and read that diff:

```
python3 item_inventory.py emit --at "$(git rev-parse --short "$BASE")" <files at base> > before.txt
python3 item_inventory.py emit --at "$(git rev-parse --short HEAD)" <files now>    > after.txt
python3 item_inventory.py compare before.txt after.txt
python3 procedure_mutation.py <files now>
```

`compare` exits 0 only when the two are equal once visibility-only changes are
paired off, and it prints those pairs so they can be judged rather than
trusted. `procedure_mutation.py` exits 0 only when every one of its deliberate
mutations was reported, which is what stops the comparison from being
decorative.
