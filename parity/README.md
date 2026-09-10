# Parity Evidence

This directory holds the public protocol for Acrobat parity evidence. The
reference screenshots themselves are private local artifacts and must not be
committed.

Committed files:

- `parity/README.md`: capture and privacy protocol.
- `parity/.gitignore`: Git ignore rules for local screenshot output.
- `parity/tools/`: the capture, audit and measurement scripts. They contain no
  Adobe content and are deliberately outside the ignore rules.
- `docs/evidence/parity-reference.md`: public ledger for capture IDs, dates,
  Onionskin commits, local filenames, and verification status.
- `docs/evidence/pro-surface-reader.md`: the same discipline applied to the
  Pro-only toolsets, plus accessibility-tree evidence. Method and confidence
  model in `docs/plans/pro-toolset-reference-method.md`.

Private local output:

- `parity/reference/`: Acrobat reference screenshots captured from the installed
  Reader or Pro version named in the ledger.
- `parity/onionskin/`: matching Onionskin screenshots captured from a committed
  build.
- `parity/comparison/`: local diff images or reports that include Acrobat UI
  pixels.
- `parity/tmp/` and `parity/manifests/`: scratch output from local comparison
  runs.

Only the ledger metadata belongs in source control. Do not commit Acrobat
screenshots, derived comparison images, or generated manifests that include
private image contents.

## B7 Capture Protocol

1. Capture the reference UI locally from the installed Acrobat version named in
   the ledger. Keep the files under `parity/reference/`.
2. Capture the matching Onionskin state from a committed build. Keep the files
   under `parity/onionskin/`.
3. Compare the pairs locally. Keep generated diffs under `parity/comparison/`.
4. Record only metadata in `docs/evidence/parity-reference.md`: evidence ID,
   Acrobat version, Onionskin commit, local filenames, SHA-256 values, and the
   acceptance result.
5. Before committing, run `git check-ignore` or the `parity_privacy` regression
   test to prove private artifacts remain ignored.

## Accessibility-Tree Capture

A raw accessibility dump is treated exactly like a screenshot: it is a complete
extraction of another product's interface, so it stays local under
`parity/reference/`, and only the distilled table reaches the repository. The
tools are `parity/tools/ax-menus.js` for menu trees and
`parity/tools/ax-tree.js` for window, panel and dialog trees.

`parity/tools/capture-audit.py` must pass on a capture directory before any
frame in it is measured. It groups the directory by content hash and fails when
filenames outnumber distinct frames, which is what a click that did not land or
a modal that swallowed later clicks produces.

`parity/tools/measure-pane.py` turns one frame into numbers: a pane's edge, its
ink bands with their pitch, and the column profile of one band, all in logical
points. What it reports is ink extent, not control boxes, and a ledger line
derived from it says so.
