# M3 P6 verification: annotations as appended objects

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P2 and P4.

Linux x86-64, stable toolchain. **No macOS, Windows or hosted-CI run is claimed
here.**

## Runs

- `cargo test -p onionskin-core --test annots`: 9 tests, all passing.
- `cargo test -p onionskin-core`: 13 suites, all passing.
- `cargo test -p onionskin-app --no-default-features`: passing, including the
  40 guarantee tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.

## The pair that carries the package

The plan asks for a structural test and a render test and says the mutation
that must break them is removing the appearance stream. With
`normal_appearance` emitting an empty content stream:

| Test | Under the mutation |
| --- | --- |
| `every_subtype_authors_a_structurally_correct_annotation` | passes |
| `text_markup_writes_quadpoints_in_the_order_readers_expect` | passes |
| `dates_are_written_in_pdf_format_with_a_timezone` | passes |
| `undoing_an_annotation_restores_the_pages_annots_exactly` | passes |
| `an_annotation_renders_and_its_hidden_twin_does_not` | **fails** |
| `the_appearance_lands_on_the_rect_at_every_zoom` | **fails** |
| `the_filter_hides_by_mode_and_never_touches_the_saved_document` | **fails** |

Six pass, three fail, and the split is exactly along the structural/render line
the plan predicted. Neither half alone would have caught it.

## The review risks, answered

- **Coordinate space.** Every generator writes `/BBox [0 0 w h]` with an
  identity `/Matrix`, where `w` and `h` are the `/Rect`'s own dimensions, and
  draws with the rect's lower-left corner as the origin. 12.5.5 maps the
  transformed bbox onto the rect, so this mapping is the identity at every
  zoom. `the_appearance_lands_on_the_rect_at_every_zoom` renders at zoom 1 and
  zoom 2 and asserts every coordinate doubled, which is the assertion a
  single-zoom test cannot make.
- **Quad order.** Written upper-left, upper-right, lower-left, lower-right.
  12.5.6.10's prose says "counterclockwise", which is a different order that
  essentially no producer follows; Acrobat writes the order above and readers
  expect it. Stated in `Quad`'s own documentation and asserted by
  `text_markup_writes_quadpoints_in_the_order_readers_expect`.
- **Dates.** `/CreationDate` and `/M` both written as
  `D:YYYYMMDDHHmmSSZ00'00'`, in UTC with an explicit offset. UTC rather than
  local: a local offset needs a timezone database this crate does not carry,
  and an offset written wrong is worse than one written honestly as UTC.
- **Author name.** A field on `Annotation`, never a lookup. Nothing here reads
  the OS user name, because filling a real name into a file the person is about
  to send somewhere, without being asked, is not a default anyone chose.
- **The filter never mutates the document.** `preview_overrides` returns a
  `BTreeMap<u32, PendingEdit>` for a preview buffer and is deliberately not an
  `Overlay`, so it cannot reach the section writer by accident. The filter test
  asserts the saved bytes are identical after asking the filter anything.

## Independent reader

`the_reader_agrees_with_an_independent_reader_on_real_files` compares
`read_annotations` against **pikepdf 10.5.1** on three files from
`corpus/external/verapdf/PDF_UA-1/7.18 Annotations`, chosen because this project
did not write them. They carry `/Widget`, `/Link` and `/Popup`, none of which
`core::annots` authors: those read back with `subtype: None` and a populated
`raw_subtype`, which is what stops a comment pane silently dropping annotations
it cannot author. pdfannots is not available in this environment, which is why
pikepdf is the oracle here as it was for P4.

## Limits

- **The `/Annots` reader's seam with P3 is provisional.** The plan says this
  reader goes through P3's `structure()`; P3 has not landed, so `read` takes the
  base document and the session's pending edits and prefers pending. Same
  guarantee, different argument list, and the note is in the module.
- **FreeText draws its frame, not its text.** Laying out glyphs needs a font
  resource that P9a's free-text tool owns along with the font it picked. Same
  for `Stamp`, whose artwork belongs to P10.
- **No Acrobat round-trip.** Not automatable, as the plan says. The
  independent-reader test is what stands in for it.
- The render assertions use `BaseRaster::content_bounds` on an otherwise blank
  page, so they are geometric rather than pixel-exact. A rendering regression
  that keeps the bounding box would not be caught here.
