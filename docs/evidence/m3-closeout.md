# M3 close-out: first edits, first print

Date: 2026-09-22. Every package in docs/plans/m3-edits-and-print.md has
landed with its own evidence file in this directory.

**The scoreboard.**

- No M3 row is left `planned`.
- 84 M3 rows are `implemented` and 13 are `partial`, each partial one with
  its gap named in its own notes.
- Rows moved on by their package, each with its reason: Line Weights and
  booklet / poster to M4, and the MCP server to post-1.0.
- The headline in ACROBAT-PARITY.md, 158 planned / 27 partial /
  80 out-of-scope / 138 implemented, is recounted by
  `acrobat_parity_headline_matches_every_inventory_row`.

**The last rows closed in this pass (P22 and two carried rows).** Each is in
m3-p22-shell-rows.md with its runs and mutations:

- Manage Tools;
- Automatically Scroll;
- Advanced Search: attachments and property criteria;
- Export Selection As (row 31, `partial`: no rich-text clipboard in GPUI);
- New Window on the same session, and the Window menu (row 8, `partial`: no
  Cascade or Tile, as GPUI cannot place a window);
- Copy or move pages between open documents;
- Summarize comments in the print output.

**Pending outside this environment.**

- **P16's manual acceptance on a Mac:**
  - printing to a real printer through PDFKit, now including the Summarize
    Comments second job;
  - the duplex settings reaching the print panel.

  The table is in m3-p16-macos-print.md.
- **Hosted CI.** No hosted CI run is claimed for any M3 package. Every run
  recorded here is local, on Linux x86-64.

**Known environmental failures**, unchanged through M3 and not caused by it:

- the canvas raster tests `a_snapshot_turns_with_the_view`,
  `a_zoom_change_keeps_the_raster_the_paint_will_scale`,
  `an_unmeasured_page_is_described_without_words_and_says_so` and
  `an_update_that_paints_nothing_leaves_no_frame_open`, which fail
  intermittently here;
- the three export rollback tests, which depend on filesystem permissions
  this sandbox does not enforce.
