# M1 spike (a): lazy `cos` parse, incremental save, repair

Verdict: the lazy + repair + incremental-save design HOLDS. Keep it. No
re-planning needed for M1. Reviewed and revised; the findings below were fixed
on the branch with regression tests that fail against the pre-fix code.

## What was proven

- **Lazy (decision 11).** Open reads the header, the last 2 KB and the xref
  sections; object bodies parse on demand through a read window that
  quadruples until the object completes. Reaching the first page of a 66 MB,
  72-page file reads **19,613 bytes of 66,611,457 (0%) in 2.2 ms**. A repeat
  lookup reads nothing. Asserted by a counting `Source`, not by inspection.
- **Byte spans (decision 7).** 19,185 in-file and 16,495 object-stream spans
  verified: each in-file span starts at its own `N G obj` header, ends at
  `endobj`/`endstream`, and no two overlap. A compressed object reports its
  container's span plus its own span inside the decoded container.
- **Round-trip (guarantee test 1).** Open then save-unchanged returns
  `None` from `incremental_section()` and appends nothing. **Zero failures
  over 3301 corpus files.**
- **Incremental save (guarantee test 2).** An edit produces
  `original ++ one section` with `/Prev`; the file reopens clean with the edit
  visible; truncating at `original_len()` gives back the byte-exact original
  and undoes the edit. A second edit appends a second generation and rolls
  back the same way.
- **Repair (guarantee test 6).** All 15 malformed files repair, save over
  intact original bytes, reopen and recover their seed's page count.
  `Document::open` refuses every one with `Error::RepairRequired`. The 28
  genuinely damaged files in the external corpora meet the same contract.

## Corpus tally (honest, per corpus)

| Corpus | Files | Round-trip | Failed | Encrypted | Needed repair |
|---|---|---|---|---|---|
| seeds | 3 | 3 | 0 | 0 | 0 |
| pdf-association (all) | 70 | 61 | 0 | 5 | 4 |
| verapdf | 2907 | 2899 | 0 | 4 | 4 |
| hayro/pdfs/custom | 279 | 261 | 0 | 13 | 5 |
| hayro-corpus | 41 | 23 | 0 | 12 | 6 |
| hayro/pdfs/other | 1 | 1 | 0 | 0 | 0 |
| malformed | 15 | n/a, repair | 0 | 0 | 15 all repaired |
| hayro/pdfs/load, fuzzed | 80 | n/a, robustness | 0 | 1 | 62 not-a-pdf, 1 unrecoverable |

No encryption, xref-stream or object-stream failure category exists: 490
verapdf files use xref streams and 77 use object streams and they all
round-trip. The encryption skip counts match `grep -l /Encrypt` on each corpus
exactly, so detection has no false positives and misses nothing.

Damage found in the wild: broken-xref-section 14, junk-before-header 7,
missing-%%EOF 7, root-recovered-by-scan 8, missing-startxref 6,
xref-offsets-wrong 6, truncated-tail 2, object-stream-lost 2.

## API shape (seeds the real M1 build)

```rust
Document::open(Box<dyn Source>) -> Result<Document>            // refuses damage
Document::open_repairing(Box<dyn Source>) -> Result<(Document, Provenance)>
Document::open_path / open_path_repairing
doc.get(u32) -> Result<Parsed>                                 // { objref, object, origin }
doc.resolve(&Object) / catalog() / page_count() / first_page()
doc.set_object / add_object / set_trailer_entry / set_info_field
doc.has_pending_changes() / incremental_section() / save_to_vec() / original_len()
enum Origin { File(Span), ObjectStream { container, container_span, within }, Pending }
enum Provenance { Clean, Repaired(RepairReport) }              // typed RepairReasons
trait Source { len, read_at }                                  // File / Bytes / Counting
```

The clean/repaired distinction is enforced by having two constructors rather
than a flag: `open` returns `Err(RepairRequired(report))` and the repairing one
cannot hand back a document without its `Provenance`. `Origin::Pending` is a
distinct variant so an unsaved edit cannot present a fabricated span at offset
zero.

## Design decisions worth keeping

- **Open-time validation is bounded, not exhaustive.** Checking only that
  `/Root` and `/Pages` resolve through the xref catches every globally broken
  xref in the corpus without reading more of the file. Validating every entry
  would have destroyed the lazy budget for no measurable gain. Repair turned
  out to be the exception, not the rule: ~0.9% of the corpus.
- **A repair section contains no object bytes.** For a rebuilt document the
  appended section is an xref table pointing back into the original bytes.
  Nothing is rewritten, which is a stronger form of the core invariant than
  planned. The exception is objects reachable only through an object stream,
  which the section must carry a copy of.
- **A missing `%%EOF` alone keeps the cheap delta section.** Every other kind
  of damage means a reader following `/Prev` lands back in the damage, so the
  section carries a full table and omits `/Prev`.
- **Generation numbers are advisory for lookup.** `locate_at` matches on the
  object number only, because producers get generations wrong and every reader
  ignores them. Nothing is fabricated: `Parsed.objref` carries the generation
  the object's own bytes declare, not the one the xref claimed.
- **`%PDF-` is not always in the first 1024 bytes** and offsets in such files
  are header-relative. Trying the raw and the biased offset per object handles
  both without a mode flag.

## Review findings, fixed on the branch

| # | Severity | Finding | Fix |
|---|---|---|---|
| F1 | HIGH | `/Index [i64::MAX-2 8]` overflowed `start + i` in `read_stream`: debug panic, release wrap | Object numbers computed with `checked_add` + `u32::try_from`; the row is consumed either way so later subsections stay aligned |
| F2 | HIGH | The byte-span test asserted `compressed > 0` while walking the corpus root, so a seeds-only checkout (what CI has) went red | The compressed half is asserted only when `external/` is present, and skipped loudly when it is not |
| F3 | MEDIUM | Editing an xref-stream file copied `/Type /XRef`, `/W`, `/Index`, `/Filter`, `/DecodeParms` and a stale `/Length` into the classic trailer the new section writes | One `trailer_for_new_section` helper, shared with the repair path so the two cannot drift |
| F4 | LOW | `locate_at` ignored the generation silently | Kept lenient, deliberately, and documented at the site (see above) |
| F5 | plan | No cargo-fuzz target, which PLAN.md requires from M1 and which would have caught F1 | `crates/cos/fuzz` (own workspace, cargo-fuzz layout) with an `open` target driving both constructors and asserting spans stay inside the file |

Two earlier adversarial review rounds fixed 20+ issues before these, including
a stack overflow from a self-referential object stream, `/Prev` + `/XRefStm`
chain ordering, a silent partial-inflate salvage, decompression bombs, a
128 GB allocation reachable from `/Columns`, a quadratic repair scan, a stream
that swallowed the rest of the file and still reported `Clean`, and a byte span
that stopped short of `endobj` when the object was exactly one window long.

## Carry-forward to the real build

All five landed in `crates/cos` after the spike merged; the list below is
kept as the record of what the spike decided to defer, not as open work.
What is still open is in `known-issues.md`.

1. **Mid-session scan escalation.** Repair-on-open is decided once, at open. An
   object whose xref offset is wrong after a clean open is
   `Err(MissingObject)` rather than a lazy re-scan. Right fail-loud default and
   it never fired in the corpus, but the real `cos` will want to escalate to a
   scan mid-session, and `Provenance` must become mutable when it does.
2. **Silent `endstream` recovery needs a `RepairReason`, before M5.** When
   `/Length` is wrong the parser recovers by finding the keyword and says
   nothing. That is what Acrobat does and refusing would reject files real
   readers open, but redaction verification cannot trust a stream boundary it
   was not told was guessed.
3. **Save materializes the whole file.** `save_to_vec` reads the source into
   memory. Fine for a spike; the real save must stream the original and append.
4. **Guarantee tests 1 and 6 are one mechanism.** Both ask "does anything but
   the appended section differ from the original". `core` should get one save
   API, not two.
5. **No delete API yet (M2).** `set_object` and `add_object` exist; removing an
   object from the document has no verb.

## Running it

The corpus is gitignored apart from `corpus/seeds/`, so the tests find it
through `$ONIONSKIN_CORPUS` (defaulting to `<workspace>/corpus`) and skip
loudly when it is absent. `ONIONSKIN_CORPUS_REQUIRED=1` turns that skip into a
failure, which is what CI should set once the corpus is fetched.

The fuzz target is its own workspace and needs nightly:

```text
cargo +nightly fuzz run open -- -max_len=65536
```
