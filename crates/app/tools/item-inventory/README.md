# Item inventory

`item_inventory.py` proves that a file split relocated code without changing
it, by listing every item with a hash of its normalized body and comparing the
two listings as multisets. `procedure_mutation.py` mutation-tests the inventory
itself, `line_multiset.py` checks the same claim without parsing Rust, and
`item_inventory.py audit` reports what each non-private item reaches.

A record cannot name the commit that contains it, so `# emitted at` names the
commit whose tree the files were read from, which is the one before the record
lands. `git diff <that sha>..<the record's sha> -- <the listed paths>` is
expected to be empty, and is the check that the header is honest.

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

## Gating a restructuring, with `--elide`

A relocation leaves every body byte-identical. Gathering loose fields into a
sub-struct does not: every method that read `self.foo` now reads
`self.group.foo`, so every body hash moves at once and the comparison says
nothing. `--elide` is the rule that keeps the listing exact across that:

```
python3 item_inventory.py emit --at "$BASE" --elide menus,export <files at base> > before.txt
python3 item_inventory.py emit --at "$HEAD" --elide menus,export <files now>     > after.txt
python3 item_inventory.py compare before.txt after.txt
```

In the code of a body, and only there, the whitespace beside every `.` is
dropped and then `.g.` becomes `.`, to a fixed point. `x.g.field` hashes as
`x.field` did, however rustfmt rewrapped the chain the hop lengthened. The hop
goes and the leaf stays, so a field renamed on its way into a group is still a
difference, as is a group nobody declared, a group passed whole, and a hop
spelled inside a string literal. The names are on the command line and in the
`# elided` header, and `compare` refuses two listings elided differently, so a
diff cannot be emptied by eliding one side of it.

What the rule does not absorb, by construction, is the declaration of the state:
the fields change container, the sub-structs are new items, and a constructor
that spells the literal out really is a changed body. Those are reported, and
are to be read rather than hashed away. Both halves are mutation-tested by
`procedure_mutation.py`, which reroutes a field through a synthetic group and
asserts both that the rule absorbs the hop and that it still reports a lost
statement, a changed call, a renamed leaf, an undeclared group and a literal.

The seven `frame-state-*.txt` files are the acceptance record for the one change
that has used the rule so far, P0b, which gathered ten of `ShellFrame`'s loose
fields into five sub-structs. Like the `tabs-split-*` set they are a record and
are not regenerated. Measured from `f349359` to `efb49ee` the comparison is
fifteen lines out and twenty-five in, and every one of them is accounted for:

* ten field lines leave `struct ShellFrame` and reappear under the five new
  containers, byte-identical in name, type and visibility;
* five `struct` items and the five `ShellFrame` fields naming them are new;
* `struct ShellFrame`'s own body and `ShellFrame::new` change, which is the
  restructuring itself;
* three function bodies carry a rustfmt rewrap that adds a brace or a comma,
  listed with their diffs in `frame-state-reflow.txt`.

The listing says which items changed; it cannot say a file holds nothing
authored, because it excludes what is not an item. `frame-state-replay.txt`
answers that separately: each of the six files is reproduced from its base by
replaying the mechanical rewrite and nothing else, and every divergence from
what shipped is either a declaration P0b adds by design or one of those rustfmt
hunks. `accessible.rs` comes out byte-identical.

Everything else hashes exactly as it did: 326 of the 330 function bodies, across
135 rerouted reads. `frame-state-visibility.txt` is the `audit` diff that
answers the question the listing cannot: the ten fields moved module, and
`pub(super)` in each new module still names `chrome::tabs`.
