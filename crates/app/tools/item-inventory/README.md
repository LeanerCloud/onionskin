# Item inventory

`item_inventory.py` proves that a file split relocated code without changing
it, by listing every item with a hash of its normalized body and comparing the
two listings as multisets. `procedure_mutation.py` mutation-tests the inventory
itself, `rule_calibration.py` breaks the inventory and checks that
`procedure_mutation.py` notices, `line_multiset.py` checks the same claim
without parsing Rust, and `item_inventory.py audit` reports what each
non-private item reaches.

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
spelled inside a literal. Code and data are told apart by `classify`, the
parser's own scanner, so a raw string, a byte string and a `'"'` are literals
and not a second, weaker guess at them.

The names are on the command line and in the `# elided` header, each with the
number of hops it absorbed, and `compare` refuses two listings elided by
different names. So a diff cannot be emptied by eliding one side of it, and a
name that matched something it was never meant for shows in its count: nothing
anchors `.g.` to the receiver the group belongs to, so the count is what the
claim is read against.

What the rule does not absorb, by construction, is the declaration of the state:
the fields change container, the sub-structs are new items, and a constructor
that spells the literal out really is a changed body. Those are reported, and
are to be read rather than hashed away.

Both halves are mutation-tested by `procedure_mutation.py`, which reroutes a
field through a synthetic group and asserts both that the rule absorbs the hop
and the rewrap it provokes, and that it still reports a lost statement, a
changed call, a renamed leaf, an undeclared group, the group passed whole, a hop
spelled inside a plain string, a raw string or a string after a `'"'`, a trailing
comma, and a statement moved across a brace. The last two are the ones a tidier
rule would absorb: rustfmt adds that comma when the hop pushes a line past the
margin, absorbing it would empty two thirds of P0b's own diff, and the same
normalization is already applied to signatures. A body is where a stray comma or
brace can be a real change, so the refusal is asserted and not merely stated.
One more case reads the header back and requires the hop count to equal a number
the suite knows independently, because no comparison can catch a counter that
lies.

`rule_calibration.py` is what stops all of that being decoration: it breaks the
rule eight ways in a scratch copy of the tool and requires the mutation suite to
catch each. Run both:

```
python3 procedure_mutation.py <files now>
python3 rule_calibration.py <files now>
```

The eight `frame-state-*.txt` files are the acceptance record for the one change
that has used the rule so far, P0b, which gathered ten of `ShellFrame`'s loose
fields into five sub-structs. Like the `tabs-split-*` set they are a record and
are not regenerated. Measured from `f349359` to `536487a` the comparison is
fifteen lines out and twenty-five in, and every one of them is accounted for:

* ten field lines leave `struct ShellFrame` and reappear under the five new
  containers, byte-identical in name, type and visibility;
* five `struct` items and the five `ShellFrame` fields naming them are new;
* `struct ShellFrame`'s own body and `ShellFrame::new` change, which is the
  restructuring itself;
* three function bodies carry a rustfmt rewrap that adds a brace or a comma,
  listed with their diffs in `frame-state-reflow.txt`.

The five names absorbed 135 hops between them, counted in the `# elided` header
and matching the number of reads the change reroutes.
`frame-state-calibration.txt` is the calibration run over the same files.

`frame-state-before.txt` predates the counted header, and like every record here
it is not regenerated. Re-emitting from `5ca2be6`'s tree with the tool as it
stands reproduces every item line and writes a header this one does not have, so
`compare` prints that side as `?` rather than as a zero it never measured.

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
