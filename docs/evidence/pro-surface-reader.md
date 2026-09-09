# Pro-only surface evidence, captured from the installed Reader

Companion to `docs/plans/pro-toolset-reference-method.md`, which defines the
method and the confidence model this file records results under. Extends the B7
protocol in `parity/README.md` and the ledger in
`docs/evidence/parity-reference.md`; it does not replace either.

Reference build: Adobe Acrobat Reader **25.001.20438**, the version already
pinned by `docs/evidence/parity-reference.md`, read from the installed bundle's
`CFBundleShortVersionString` on 2026-09-09.

Privacy rule, unchanged: image bytes stay under the ignored `parity/` tree. This
file records control names, ordering, grouping, enabled state, keyboard
shortcuts and geometry. Those are measurements, not artwork.

---

## E1. Session capability probe, 2026-09-09T23:12Z

The two instruments do not fail together, and that is worth recording once
rather than rediscovering.

| Instrument | Result | Evidence |
|---|---|---|
| `screencapture -x` of the whole display, session locked, displays asleep | All-black frame. `screencapture -R` fails outright with `could not create image from rect`. | 8,416,800 of 8,416,800 luminance samples below 10 |
| `screencapture -x` after `caffeinate -u`, session still locked | Renders the login window, not the desktop. No application window is composited. | 8,416,592 of 8,416,800 samples above 10, all of them lock screen |
| System Events accessibility, menu bar of `AdobeReader` | Full menu tree readable: titles, index, separators, submenu nesting, enabled state, mark characters, command key and modifiers. | `E2` below, 209 rendered lines from a live query |
| System Events accessibility, windows of `AdobeReader` | `count of windows` returns 0 while the session is locked, so no window, panel, dialog or sheet tree is readable. | repeated at 23:07Z and 23:12Z, both 0 |

Consequence for the method: **menu-surface facts are capturable from an
unattended session; window-surface facts are not.** The B7 ledger already
recorded the pixel half of this ("WindowServer rejects the window rectangle
against the inactive displays"). The new fact is that the accessibility half
survives display sleep but not session lock, and that the menu bar survives
both.

---

## E2. Pro-gated entries in the Reader menu bar

Captured live at 2026-09-09T23:04Z by
`osascript -l JavaScript parity/tools/ax-menus.js AdobeReader`, rendered by
`parity/tools/ax-render-menus.py`. Raw dump kept locally, uncommitted.

Confidence class **A** for every row: the running pinned build reported it about
itself. Index is the item's position in its own menu including separators, so it
is a statement about ordering and grouping, not a coordinate.

### File menu, whole menu, in order

| # | Title | Enabled | Shortcut | Pro-gated |
|---|---|---|---|---|
| 0 | Open... | yes | cmd+O | no |
| 1 | Open Recent Files | yes | | no |
| 2 | Create PDF | yes | | yes |
| 3 | Combine Files | yes | | yes |
| 4 | separator | | | |
| 5 | Save As... | yes | cmd+shift+S | no |
| 6 | Convert to Word, Excel or PowerPoint | yes | | yes |
| 7 | Save as Text... | yes | | no |
| 8 | separator | | | |
| 9 | Compress File | yes | | yes |
| 10 | Password Protect | yes | | yes |
| 11 | Request e-signatures | yes | | yes |
| 12 | Share File | yes | | no (cloud) |
| 13 | separator | | | |
| 14 | Print... | yes | cmd+P | no |
| 15 | Find | yes | cmd+F | no |
| 16 | Advanced Search | yes | cmd+shift+F | no |
| 17 | separator | | | |
| 18 | Document properties... | yes | cmd+D | no |
| 19 | separator | | | |
| 20 | Close File | yes | cmd+W | no |

### Edit menu, Pro-gated block

The Edit menu carries a contiguous eight-item Pro block between two separators,
at indices 5 to 13. Every one reports `AXEnabled = true` with no document open,
which is the mechanical form of the claim in `docs/plans/parity-goal.md` candor
item 1: the entry is live, and the paywall is somewhere past the click.

| # | Title | Enabled | Toolset it opens |
|---|---|---|---|
| 5 | Edit a PDF | yes | Edit a PDF |
| 6 | Add Text | yes | Edit a PDF |
| 7 | Add Image | yes | Edit a PDF |
| 8 | separator | | |
| 9 | Delete Pages | yes | Organize pages |
| 10 | Rotate Pages | yes | Organize pages |
| 11 | Redact a PDF | yes | Redact |
| 12 | Scan and OCR | yes | Scan and OCR |
| 13 | Prepare Form | yes | Prepare a form |

Two further Edit-menu submenus are relevant and are **not** Pro-gated in the
same way, because their children report disabled rather than live:

- `Edit > Protection` (index 17): `Security properties` live; `Revoke document`,
  `View audit history`, `Synchronize for offline` and `Manage document security
  account` all report `AXEnabled = false`. These are account-tethered, which is
  a different gate from the purchase gate and produces a different observable.
- `Edit > Check Spelling` (index 15): three live children, no gate.

### View menu, for contrast

The View menu contains no Pro-gated entries. Its whole content is Reader
functionality, which is why M2 could be measured from this build without any of
this work.

---

## E3. Rail and panel captures

Pending. See section 6 of the method document.
