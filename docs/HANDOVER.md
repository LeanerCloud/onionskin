# Handover, 2026-09-24

Where the work stopped, what is verified, and what to pick up next. The
plan for everything remaining is `docs/plans-remaining.md` (work packages
WP0 to WP24); this file only says where along it things are.

## Scoreboard

`ACROBAT-PARITY.md`: **403 rows: 83 planned / 44 partial / 80
out-of-scope. 196 implemented.** The guarantee test
`acrobat_parity_headline_matches_every_inventory_row` checks the headline.

- **M2 to M5:** nothing is left planned; 38 rows are partial, each with
  its gap named in the row. The largest gaps are the text engine (WP10,
  WP12) and forms (WP14).
- **M6:** 46 rows. The Measure tools, password security, signature
  validation (guarantee 4), trusted certificates and the Security
  Settings pane are done. The accessibility half and signing are not.

## This session's commits (on top of `416a896`)

| Commit | Package | State |
|---|---|---|
| `ecb2861` corpus: signed fixtures written by pyHanko | WP1 | done |
| `287effc` crypto: checking a CMS signature | WP1 | done |
| `6cd2834` core: validating the document's signatures | WP1 | done |
| `0d24095` app: Signatures pane verdicts, Signature Properties | WP1 | done |
| `7e6e07a` guarantee 4, evidence, parity rows | WP1 | done |
| `c1bedbe` crypto: trust paths, checked against `openssl verify` | WP3 | done |
| `e34c20b` core: the signer's identity | WP3 | done |
| `e96ad3e` app: trusted certificates, Preferences > Signatures | WP3 | done |
| `3519d6e` app: the Security Settings pane | WP3 | done |
| `556e0c6` docs: WP3 evidence (`m6-trust.md`) and parity | WP3 | done |
| `c1860fc` app: Zoom To takes a typed magnification | WP2 | done |
| `6610142` Layer Properties: name, intent, default state | WP2 | done |
| `59cf44c` **wip:** attachments' Edit Description | WP2 | **unfinished** |

Evidence for each finished package is in `docs/evidence/`:
`m6-signature-validation.md`, `m6-trust.md`, `m3-shell-leftovers.md`
(WP2's so far).

## Pick up here: WP2, attachments' Edit Description

`59cf44c` builds, and `cargo test -p onionskin-core --test
attachment_description` plus the app's attachment tests pass. To finish:

1. A window test that opens the Attachments pane's menu, Edit
   Description, types, saves, and reads the description back.
2. Read `/Desc` back with qpdf or pikepdf in the core test, as the WP2
   plan asks.
3. The same package's remaining rows: Open for embedded PDFs, Search
   Attachments, Wrap Long Bookmarks and Bookmark Properties, Properties
   Description facts, local time in dynamic stamps, and search stemming.
4. A section in `m3-shell-leftovers.md` and the parity rows for each, as
   the Zoom To and Layer Properties sections do.

After WP2, the plan's order is WP4 (Tags and Content panes, M6), then
WP5 (organize and print leftovers, M3/M4).

## Verifying

```sh
. /tmp/envs.sh   # this container's toolchain environment; not in the repo
cargo fmt --all --check
cargo clippy --workspace --all-targets --features onionskin-app/shell-test-support
cargo clippy -p onionskin-app --no-default-features --features shell
cargo test -p onionskin-crypto
cargo test -p onionskin-core --test signatures
cargo test -p onionskin-app --features shell-test-support --lib
cargo test -p onionskin-app --test guarantees --test signature_guarantee
```

Known, and not from this work:

- clippy's one warning, `a11y::Shared::record` never used.
- The app lib suite has 6 failures in this container: 3 export rollback
  tests that fail when run as root, and 3 canvas raster tests
  (`a_zoom_change_keeps_the_raster_the_paint_will_scale`,
  `an_unmeasured_page_is_described_without_words_and_says_so`, and
  `an_update_that_paints_nothing_leaves_no_frame_open` or
  `a_snapshot_turns_with_the_view`), which vary between runs.
- `pdfsig` (poppler-utils) and `openssl` are needed for the independent
  checks; without them those halves skip and say so.
- Coverage (tarpaulin, llvm engine): the signature and trust code in
  crypto and core is at 83%. Tarpaulin's default engine segfaults on
  core's tests, so use `--engine llvm`. The app-side signature files were
  not measured, for disk space.

## Waiting on you

From `docs/plans-remaining.md` section 3. None of these blocks the next
package.

- **Decisions:** D4 (the `openjp2` crate, for JPEG 2000 export) and D5
  (which spelling dictionaries may be bundled).
- **Needed to close rows here:**
  - Acrobat: guarantee 7's recorded values, a Properties screenshot, and
    one Acrobat Reader run to back the interoperability claims (D6).
  - A Mac: Keychain IDs, VoiceOver, and the print acceptance runs.
  - A Windows host: printing, CNG IDs, the accessibility adapter.
  - Access to a timestamp server URL.
  - A GPUI fork, or accepting that window tiling and rich-text copy stay
    partial.
