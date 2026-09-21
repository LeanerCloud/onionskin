# Acrobat Parity Scoreboard

Onionskin is a local, private, non-destructive PDF editor. It arranges its
workspace the way Adobe Acrobat's unified interface does (global bar, left tool
rail, quick action toolbar, left navigation panes, right side panel) and keeps the
same names, groupings and keyboard defaults for the workflows both applications
have, so someone who already knows those workflows does not have to learn new
ones. `PLAN.md`, section "GUI: Acrobat parity", holds the engineering target and
the legal boundaries; "Acrobat" appears throughout this file as a comparison, and
never as branding.

This file is the public inventory of that workflow surface. One row per Acrobat
tool, command, pane or viewer feature, with the status Onionskin claims for it and
the milestone that delivers it. It is the counterpart to Schist's README counts
("55 tools, 57 filters, all nine layer effects"): **parity is a number that can go
up, not an adjective.** Nobody gets to say "Onionskin has good Acrobat parity";
they say "Onionskin implements N of M rows".

## Counting convention

- **One row = one thing a user can point at**: a named tool in a toolset, a menu
  command, a navigation pane, a viewer capability. Sub-options of a single dialog
  are one row unless Acrobat gives them their own tool name.
- **Status** is exactly one of:
  - `implemented` - the real user-facing path supports the row's current Onionskin
    scope; a registry entry, placeholder, or disabled command is not enough.
  - `planned` - intended, but no usable user-facing subset has shipped yet.
  - `partial` - a usable subset has shipped, but some named behavior is missing;
    the Notes column names the gap. A deliberately reduced future target stays
    `planned` until its supported subset ships.
  - `out-of-scope` - will not be built. Every such row states why.
- **Milestone** maps to `PLAN.md`'s "Milestones" section (M0-M6, post-1.0). Where
  the plan names the feature, the milestone is the plan's. Where the plan names the
  owning plugin but not the individual command, the row inherits that plugin's
  milestone silently (`tools-comment` at M3, `tools-form` at M5, `tools-measure` at
  M6, and so on). Where neither holds, the milestone is a **judgment call**: the
  Notes column says `(judgment)` or explains the placement in words. Judgment rows
  are the ones a plan revision should confirm or move.
- Out-of-scope rows carry no milestone (`-`).
- Acrobat surfaces many commands in more than one place: Read Out Loud is a View
  menu item and part of the accessibility toolset, Crop pages sits under both Edit
  a PDF and Organize pages. **Each surface is a row**, because a workflow a user
  knows has to be where they reach for it; the Notes column cross-references the
  twin.
- **Context menus are the one carve-out**: they are counted once per surface, as a
  single row naming their commands, rather than once per command. A right-click
  menu almost entirely re-exposes commands already counted, so per-command rows
  would inflate the denominator without adding a distinct thing to build.
- Onionskin-only surface is **not** counted here: the skins/generations pane, the
  redaction verifier (guarantee test 3), generation rollback, the MCP server, the
  CLI. This file measures Acrobat's surface, not Onionskin's.
- The reference target is Acrobat Pro on the unified interface as documented on
  `helpx.adobe.com` in August 2026, plus Acrobat Reader 25.001 installed locally.
  Adobe reshapes this UI continuously; per `PLAN.md`'s risk list, each release
  cycle pins a dated screenshot set in `parity/reference/` rather than chasing the
  live product, and this file is re-reconciled against that set.

## Totals

**403 rows: 187 planned / 24 partial / 80 out-of-scope. 112 implemented.**

323 rows (implemented plus planned and partial) are the parity target. The other 80 are the
deliberate no. By milestone: M2 67, M3 100, M4 2, M5 52, M6 45, post-1.0 57.
M4 carries only two rows because its deliverables (the CUPS and Windows print
backends) are mostly not Acrobat surface; the MCP server moved to post-1.0. 42 rows are marked
`(judgment)`: their milestone does not follow from plan text and a plan revision
should confirm or move them. Recount with:

```sh
awk -F'|' '/^\|/ {gsub(/^ +| +$/,"",$3); if ($3 ~ /^(planned|partial|out-of-scope|implemented)$/) c[$3]++} \
  END {for (k in c) print c[k], k}' ACROBAT-PARITY.md
```

### Implementation evidence (reconciled 2026-09-11)

M3 P1c verification is in progress: the default-corpus lazy I/O failure is
reproduced, and the verified parser-window correction reduces the named
fixture's returned bytes by 28.1%, while its budget still fails. See
`docs/evidence/m3-p1c-read-windows.md`. The dated validated-dictionary reuse
checkpoint reduces through-first-page reads from the preceding 1,335,528-byte
suffix checkpoint to 1,072,232 bytes, a further 19.7% reduction and 42.3% below
the original main baseline. The unchanged budget assertion checks
through-first-page reads of 1,072,232 bytes (26.73% of the 4,010,934-byte
file), so it still fails; opening alone remains 1,070,056 bytes (26.68%), also
over budget. No feature row changes or P1c completion are claimed; corpus CI,
required-input enforcement, the shared helper, hosted timing/mutation evidence,
P1c integration and the remaining 25% performance work remain outstanding.

The evidence keys in changed rows refer to these live paths. `M2-AUDIT-T1-001`
is the window-only visual baseline, and `M2-AUDIT-T2-001` records the required
Find-and-Thumbnails state. The view/zoom reconciliation inspected source and
test definitions at `755842f`; its evidence is separate from C1.2's automated and
native checks at `4ddec23`, `c7f1afa`, and `62576fb` below. Test counts describe
their named verification, not a fresh run of every historical package.

P0c's local test-relocation checkpoint `fdf875c` preserves the suite across
eleven configurations; its source/configuration/mutation records are under
`crates/app/tools/item-inventory/p0c-test-relocation-*.txt`. Live test paths below
reflect that relocation. Native open/Find/tab/export smoke and window-only
captures passed at `d34b29f` on 2026-09-11, recorded in
`docs/evidence/milestone-screenshots.md`. Final integration remains pending;
this test-only change promotes no feature rows.

### Task B integration checkpoint, 2026-09-14

Corpus/parser integration is committed locally at
`432e1f7eb5facdc51239472d28c748388b445550`. The reviewed source fingerprints
are `reader.rs` `4b3b39d3be11728715aee9d62436d71ceb85b49d4527d622e2da11dc51b53da4`
and `parse.rs`
`ce2ce1e299912d8ee50aa0e266e5c4f2a55562c478d08852d0d05ca11fee899c`.
Required consumer targets passed as separate direct commands, and the local
amd64 Docker/`act` simulation exited 0 after its clean control and reviewed
negative controls. The simulation is not hosted CI or Windows acceptance. The
exact Isartor trace measured 941,160/4,010,934 returned bytes through first-page
access. Separately, independent review approved the native macOS scenario
evidence recorded in `docs/evidence/milestone-screenshots.md`, including the
stable Isartor window, Find, tab switching, invalid page rejection, native
Select All for the export fields, and the 160 by 360 pixel export. The
403-row totals and all matrix row statuses are unchanged; no parser or corpus
plumbing is promoted to an Acrobat feature row.

### M3 package reconciliation, 2026-09-21

Rows closed by M3 packages with evidence docs (P1b, P8, P9a, P9b, P9c, P10,
P11, P12, P13a, P13b, P14a) cite `docs/evidence/m3-*.md` directly in Notes.
Those docs record Linux runs only; no macOS, Windows or hosted-CI run is
claimed. A row closed in code but reachable only through a later package's
dialog or grid stays `planned` (Extract, Replace, Copy or move pages between
documents: P21). Text-entry comment rows are `partial` until the comments pane
(P20) edits `/Contents`. `Open an encrypted document` moves from M6 to M3 as
`partial` per ruling A. P1c and P2-P7 close no rows. Booklet and Poster / tile stay at
M3 here; the plan's ruling B move to M4 is not yet applied.

| Evidence key | Live path | Automated or manual proof |
|---|---|---|
| M2-SHELL | `crates/app/src/shell/chrome/`, `crates/app/src/shell/{mod,input}.rs` | Global bar, tabs, rail, quick actions, page controls, side panel, theme, input, and shell tests plus screenshot `M2-AUDIT-T1-001`. |
| M2-PARITY-REF | `docs/evidence/parity-reference.md`, `parity/README.md`, `crates/app/tests/parity_privacy.rs` | Private Reader 25.001.20438 and Onionskin captures are hashed and compared for the Actual Size shell and document-Find baselines as `B7-REF-001/002`. B7-REF-003 pins the clean Reader Page Thumbnails reference and the exact `4132a99` code/test fix while its corrected Onionskin capture remains blocked. The executable privacy contract proves private roots are ignored and untracked. Eight required reference states remain incomplete. |
| M2-HOME | `crates/app/src/shell/home.rs`, `crates/app/src/shell/chrome/tabs/mod.rs` | Home list/thumbnail and recents tests. |
| M2-PREFS | `crates/app/src/{preferences,keymap}.rs`, `crates/app/src/shell/preferences_dialog.rs` | Preference persistence, keymap resolution, and dialog tests. |
| M2-PACKAGE | `packaging/{macos/Info.plist,linux/onionskin.desktop,windows/installer.nsi}` | `crates/app/tests/file_association.rs`. |
| M2-PANES | `crates/app/src/shell/panes/` | Pane action, rendering, context-menu tests, and the B3.1 rendered-bounds regression for activated pane bodies. |
| M2-A11Y | `crates/app/src/a11y/`, `crates/app/src/shell/chrome/tabs/accessible.rs`, `crates/app/tests/a11y_probe.rs`, `docs/spikes/m2-voiceover-acceptance.md` | P12 tree and platform probe evidence plus `11d5751`: grouped keyboard traversal, bidirectional GPUI/AccessKit focus transfer, background request publication, and demand-driven page text. Shell tests in `tabs/accessible.rs` include `a_screen_reader_cursor_takes_the_keys_off_a_text_field` and `focusing_a_text_field_moves_the_published_cursor_onto_it`. The macOS probe remains a required CI gate; real VoiceOver acceptance and Linux/Windows adapters remain open. |
| M2-REPAIR | `crates/core/src/session.rs`, `crates/app/src/shell/chrome/tabs/mod.rs` | Core repair tests and shell repaired-document notice test. |
| M2-VIEWER | `crates/app/src/shell/canvas.rs`, `crates/render/src/base.rs` | Core/render integration tests and the window-only visual baseline. |
| M2-LAZY | `crates/core/benches/open.rs`, `crates/cos/tests/lazy.rs` | Budgeted 1000-page open and lazy object-read tests. |
| M2-BASIC | `plugins/tools-basic/src/` | Real gesture tests for Hand, selection, Snapshot, Marquee Zoom, and Dynamic Zoom. |
| M2-VIEW-ZOOM | `crates/app/src/shell/{canvas,dialog}.rs`, `crates/app/src/shell/chrome/{global_bar,theme}.rs`, `crates/app/src/shell/chrome/tabs/{menu,accessible,dialogs,mod}.rs`, `plugins/tools-basic/src/zoom.rs` | Merge `cb87b72`: Fit Visible menu/keystroke and raster-content fitting, Dynamic Zoom drag, Read Mode chrome visibility, Full Screen chrome removal, and the preset Zoom To chooser. Tests include `the_fit_visible_keystroke_fits_the_pages_marks`, `choosing_a_magnification_applies_it_and_closes_the_chooser`, `read_mode_takes_the_top_bars_out_of_the_tree_and_keeps_the_page_controls`, `full_screen_leaves_no_chrome_described_and_escape_answers_it_first`, and `plugins/tools-basic/tests/gestures.rs` drag cases. Custom Zoom To entry and Full Screen presentation semantics remain gaps. |
| M2-SEARCH | `crates/app/src/shell/find_bar.rs`, `crates/app/src/shell/panes/results.rs`, `crates/app/src/shell/chrome/tool_search.rs` | Find routing, tool lookup, highlight, result navigation, and pane tests. |
| M2-EXPORT | `crates/core/src/session.rs`, `crates/plugin-api/src/codec.rs`, `plugins/codecs-common/src/`, `crates/app/src/shell/chrome/tabs/export.rs` | C1.1 snapshot `f0cbbcb` and background export `3ac647b`: source bytes are shared into a worker-owned document; Single output streams page chunks into one temporary file and publishes it atomically at completion, while PerPage publishes completed page files incrementally with one destination writer at a time. Visible and accessible progress follows completed pages, and cancellation cleans partial output before the one-job guard is released. Post-rebase proof includes 100 focused tab tests, 501 shell tests plus integrations, codec suites, headless suites, feature isolation, and scoped strict clippy. C1.2 settings are recorded separately below. |
| M2-EXPORT-SETTINGS | `crates/app/src/shell/chrome/export_dialog.rs`, `crates/app/src/shell/chrome/tabs/{export,menu}.rs`, `crates/app/src/shell/chrome/tabs/tests/{export_settings,input_values,native_input}.rs` | C1.2 is resolved by `4ddec23`, editable AX values by `c7f1afa`, and native Select All routing by `62576fb`. Tests cover validation, modal focus/bounds, stale prompts, subset filenames, PNG dimensions, and immutable Single output requests. Latest full shell run: 624 passed, 7 ignored; focused no-codecs input/value tests and strict clippy/format checks passed. Native checks at `62576fb` verify AXValue defaults 1/2/150, Cmd+A and Edit > Select All field replacement, invalid-input validation, corrected 2/2/144 values, Tab focus on Export, and page-2 PNG output at 144 DPI (160 by 360 pixels). Native output/capture evidence is in `docs/evidence/milestone-screenshots.md`. Home/no-document or missing-commands-core native Select All remains disabled; older modal geometry/focus, real VoiceOver, and cross-platform acceptance remain open. |
| M2-LAYERS | `crates/core/src/session.rs`, `crates/app/src/shell/panes/layers.rs` | Real-file render-change test and pane toggle-refresh tests. |

---

## Application shell

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Global bar (top bar) | implemented | M2 | Shipped with the quick-action top bar. Evidence: M2-SHELL. |
| Hamburger / main menu button | implemented | M2 | Opens File/Edit/View/Window/Help in the unified UI. Evidence: M2-SHELL. |
| Document tabs (multiple open documents) | implemented | M2 | Open, switch, and close paths are live. Evidence: M2-SHELL. |
| Document tab context menu (Close, Close Others, Close All, Show Containing Folder, Copy Path) | implemented | M2 | All named entries are live. Counted once per the context-menu carve-out. Evidence: M2-SHELL. (judgment) |
| All tools pane (left tool rail) | implemented | M2 | The registry-driven rail, compact icon rendering, and tool activation are live. Evidence: M2-SHELL and B3.8. |
| "View more" / expand full tool list | implemented | M2 | Expand and collapse are live. Evidence: M2-SHELL. |
| Quick action toolbar (floating over the page) | implemented | M2 | Toolbar, dragging, customization, Select, Comment (sticky note), Highlight and Draw are live; Fill text fields and Add Sign are disabled with their M5 reason. Evidence: M2-SHELL, docs/evidence/m3-p20b-comment-surfaces.md. |
| Right-hand side panel | implemented | M2 | The contextual host, empty state, and open/close behavior are live; it shows the active tool's use and, for a chosen comment, the properties inspector. Evidence: M2-SHELL, B3.7 and docs/evidence/m3-p20b-comment-surfaces.md. |
| Page controls / bottom toolbar | implemented | M2 | Page, rotate, zoom, fit controls, invalid-zoom display, and numeric page-field semantics are live. Evidence: M2-SHELL and B3.7. |
| Global search field (tools and document text) | implemented | M2 | Tool lookup, current-document text search, and the no-document unavailable state are live. Evidence: M2-SEARCH and B3.7. |
| Convert (global bar entry point) | implemented | M3 | A global-bar panel holds Create PDF From File, From Clipboard and From Multiple Files, the page exports, and Export All Images; the target list stays deliberately smaller than Acrobat's. Evidence: docs/evidence/m3-p14a-images.md. |
| Get a link to the document | out-of-scope | - | Cloud-tethered link sharing. |
| Undo / Redo icons on the global bar | implemented | M3 | Global bar buttons carrying the Edit menu entries' own availability; disabled with "Nothing to undo" rather than absent. Evidence: docs/evidence/m3-p18-save-undo.md. |
| Save / Save As in the global bar | implemented | M3 | Both are global bar buttons carrying the File menu entries' availability; Save appends an incremental section. Evidence: docs/evidence/m3-p18-save-undo.md. |
| Print button | planned | M3 | `crates/print`, macOS backend at M3. |
| Share button | out-of-scope | - | Cloud-tethered: Adobe cloud link sharing. Plan states all cloud-tethered surface is out of scope. |
| AI Assistant button | out-of-scope | - | Cloud-tethered generative service. Plan names AI Assistant explicitly as out of scope. |
| Account / profile menu | out-of-scope | - | Requires an Adobe account; Onionskin has no account system. |
| Acrobat notifications | out-of-scope | - | Cloud-tethered notification feed. |
| Home view: Recents | implemented | M2 | Local recents list is live. Evidence: M2-HOME. |
| Home view: Starred | planned | M3 | Local flag in app state. Acrobat stores starred files in Adobe cloud storage; Onionskin keeps the list on disk. (judgment) |
| Home view: list view / thumbnail view toggle | implemented | M2 | Both local Recents layouts are live. Evidence: M2-HOME. (judgment) |
| Home view: Adobe cloud storage | out-of-scope | - | Cloud-tethered; Onionskin's counter-pitch is local and private. |
| Home view: Box / Dropbox / Google Drive / OneDrive / SharePoint connectors | out-of-scope | - | Cloud-tethered third-party storage integrations. |
| Display theme (light / dark / system) | implemented | M2 | All three theme choices are live and persisted. Evidence: M2-SHELL. |
| Customize the quick action toolbar | implemented | M2 | Show/hide customization is live; disabled actions remain identifiable. Evidence: M2-SHELL. |
| Manage Tools / customize the tool rail | planned | M3 | Registry-driven: shows/hides registered plugins. (judgment) |
| Revert to the classic Acrobat interface | out-of-scope | - | Product-level decision: the unified UI is the parity target; the plan defers classic to a possible later theme, not a shipped toggle. |
| Preferences dialog | partial | M2 | The live Commenting, Documents, General, Page Display, and Search subset is persisted; later feature categories land with their owners. Adobe-account, cloud-storage, Tracker, Updater, Multimedia, and 3D categories remain out of scope. Evidence: M2-PREFS. |
| Keyboard shortcut remapping | implemented | M2 | Acrobat defaults are remappable through `keymap.json`, and the shortcut reference shows the effective bindings. Evidence: M2-PREFS. |
| Autosave and crash recovery | implemented | M3 | Every 30 s into an owner-only store; a recovery is offered when its own document next opens, ranked most recent first, and replays as one undoable edit. Evidence: docs/evidence/m3-p18-save-undo.md. |
| Window menu (New Window, Cascade, Tile, Minimize) | planned | M3 | Shell logic, no document dependency. (judgment) |
| Help menu (About, keyboard shortcuts) | implemented | M2 | About and the effective local shortcut reference are live; online help remains out of scope. Evidence: M2-SHELL. (judgment) |
| Check for updates / auto-update | planned | post-1.0 | Plan lists auto-update as a post-1.0 slot (Schist's Check for Updates path as template). |
| UI localization | planned | post-1.0 | Plan lists localization plus bidi/vertical text as post-1.0. |
| App-level accessibility tree (screen reader support for the UI) | partial | M2 | The macOS tree, grouped focus traversal, bidirectional GPUI/AccessKit focus transfer, background request publication, demand-driven page text, and editable field values are implemented. Export controls have bounds and visible keyboard focus; older modal geometry/focus and real VoiceOver acceptance remain open, and Linux/Windows adapters remain no-ops. Evidence: M2-A11Y and M2-EXPORT-SETTINGS. |
| Pinch-to-zoom and stylus pressure | partial | M2 | Pinch zoom and pressure propagation are live; no shipped pressure-aware ink tool exists yet. Evidence: M2-SHELL. |
| Register `.pdf` as openable ("Open with") | partial | M2 | macOS, Linux, and Windows declarations exist and never claim the default. Release builds include the shell feature; hosted release and packaged platform smoke tests remain open. Evidence: M2-PACKAGE and audit REPO-001. |
| Set as the default PDF viewer | out-of-scope | - | Deliberate product decision in the plan: never the default handler. |
| Display PDF in a browser / browser extension | out-of-scope | - | A browser plug-in is a separate product with its own sandbox and update channel. |
| Acrobat for Outlook / Office add-ins (PDFMaker) | out-of-scope | - | Requires shipping into Microsoft Office's add-in model and reading Office formats; a product in itself. |

## Menus

Acrobat's unified UI collapses the classic menu bar into the hamburger menu (and
keeps a native menu bar on macOS). Onionskin follows the same structure.

| Item | Status | Milestone | Notes |
|---|---|---|---|
| File > Open | implemented | M2 | File picker, path open, and repaired-document notice are live. Evidence: M2-SHELL and M2-REPAIR. |
| File > Open Recent | implemented | M2 | The persisted local list is live. Evidence: M2-HOME. |
| File > Create | implemented | M3 | Create PDF From File, From Clipboard and From Multiple Files; the new document is saved to a chosen path before it opens. Sources limited: see "Create a PDF". Evidence: docs/evidence/m3-p14a-images.md. |
| File > Save | implemented | M3 | cmd-s; appends an incremental section; disabled with "No unsaved changes" on a clean document. Evidence: docs/evidence/m3-p18-save-undo.md. |
| File > Save As | implemented | M3 | cmd-shift-s; the tab follows the document to the new file and the original is untouched. Evidence: docs/evidence/m3-p18-save-undo.md. |
| File > Save as Other | implemented | M3 | A panel lists one entry per installed export codec; with none installed the entry is disabled with a reason. PDF/X and Reader-Extended variants remain out of scope. Evidence: docs/evidence/m3-p13a-properties.md. |
| File > Export To | partial | M2 | Text, PNG, and SVG exports have First/Last settings, plus PNG resolution; stale successful prompt writes are refused and derived destinations cannot overwrite existing files. Office and HTML targets remain a deliberately reduced post-1.0 subset. Evidence: M2-EXPORT, M2-EXPORT-SETTINGS, and B4.2-B4.4. |
| File > Revert | implemented | M3 | Discards unsaved edits by reopening the file; the history goes with them. Evidence: docs/evidence/m3-p18-save-undo.md. |
| File > Close / Close All | implemented | M2 | Both commands are live for the current tab set. Evidence: M2-SHELL. |
| File > Properties (Document Properties) | partial | M3 | `cmd-d` dialog with Description, Security (read-only), Fonts, Initial View and Custom tabs; one Apply is one undo step. The five-tab list is unconfirmed: the screenshot corpus has no capture of this dialog. Description omits PDF version, page size, tagged and fast web view. Evidence: docs/evidence/m3-p13a-properties.md. |
| File > Print | planned | M3 | `crates/print`. |
| File > Attach to Email | implemented | M3 | Hands the saved file to the OS mail client (Mail on macOS, `xdg-email` elsewhere on Unix) as one argument, never through a shell; disabled until unsaved changes are saved. Windows has no request to make yet and says so. Evidence: docs/evidence/m3-p13c-file-edit-menus.md. |
| File > Share / Send for comments | out-of-scope | - | Cloud-tethered web review flow. |
| File > Get Documents Signed | out-of-scope | - | Adobe Acrobat Sign, a cloud service. |
| File > Exit / Quit | implemented | M2 | The menu command exits through the shell action. Evidence: M2-SHELL. |
| Edit > Undo / Redo | implemented | M3 | cmd-z / cmd-shift-z; undo past a save makes the document dirty again (T1). Evidence: docs/evidence/m3-p18-save-undo.md. |
| Edit > Cut / Copy / Paste / Delete | implemented | M3 | Answered by the active tool through the plugin API; the text tool copies, and a verb no tool claims is disabled with the reason. Evidence: docs/evidence/m3-p13c-file-edit-menus.md. |
| Edit > Select All / Deselect All | implemented | M2 | Both registry commands operate on the active document and page. Evidence: M2-SHELL. |
| Edit > Copy File to Clipboard | implemented | M3 | Puts the saved file's `file://` URI on the clipboard. Evidence: docs/evidence/m3-p13c-file-edit-menus.md. |
| Edit > Take a Snapshot | implemented | M2 | The menu activates `tools-basic` Snapshot and copies the selected raster region. Evidence: M2-BASIC. |
| Edit > Find (Ctrl+F) | implemented | M2 | Case, whole-word, highlight-all, next, and previous are live; Include Comments finds text in comments (M3); Include Bookmarks is not yet offered. Evidence: M2-SEARCH, docs/evidence/m3-p20b-comment-surfaces.md. |
| Find toolbar > Replace text | planned | M5 | Find-and-replace is named in the plan's M5 list. It writes text, so it belongs to `tools-edit` rather than to viewer search. |
| Edit > Advanced Search, current document | partial | M2 | Current-document case, whole-word, phrase, any-word, all-word, and result paths are live; the dedicated Advanced Search surface and stemming remain missing. Evidence: M2-SEARCH. |
| Advanced Search > include attachments | planned | M3 | Acrobat searches attached files two levels deep. (judgment) |
| Advanced Search > document-property criteria (author, dates, keywords, metadata) | planned | M3 | (judgment) |
| Edit > Advanced Search across multiple PDFs / a folder / an index | planned | post-1.0 | Needs the multi-document index; single-document search ships first. Boolean Query and Proximity are multi-document options in Acrobat and land here, not on the single-document row. |
| Edit > Check Spelling (in comments and form fields) | planned | M5 | Spell check is named in the plan's M5 list. |
| Edit > Look Up Selected Word | planned | post-1.0 | Platform dictionary only; the web lookup Acrobat uses is out of scope. |
| Edit > Preferences | implemented | M2 | Opens the live persisted preference subset. Evidence: M2-PREFS. |
| View > Rotate View | implemented | M2 | View-only rotation is live and remains distinct from page mutation. Evidence: M2-SHELL. |
| View > Page Display > Single Page | implemented | M2 | Live layout mode. Evidence: M2-SHELL. |
| View > Page Display > Single Page Continuous | implemented | M2 | Live default layout and scroll-budget mode. Evidence: M2-SHELL. |
| View > Page Display > Two Page View | implemented | M2 | Live layout mode. Evidence: M2-SHELL. |
| View > Page Display > Two Page Scrolling | implemented | M2 | Live layout mode. Evidence: M2-SHELL. |
| View > Page Display > Show Cover Page in Two Page View | implemented | M2 | Live two-page cover toggle. Evidence: M2-SHELL. |
| View > Page Display > Automatically Scroll | planned | M3 | (judgment) |
| View > Page Display > Overprint Preview | planned | post-1.0 | A rendering toggle that simulates overprinting ink, so it belongs to `render`, not to the prepress Output Preview tool listed as out of scope. Deferred until hayro can express it. (judgment) |
| View > Zoom > Zoom In / Zoom Out / Zoom To | partial | M2 | Zoom In, Zoom Out, and a dedicated Zoom To chooser with 12 presets from 25% to 3200% are live; custom percentage entry remains missing. Evidence: M2-SHELL and M2-VIEW-ZOOM. |
| View > Zoom > Actual Size / Fit Page / Fit Width / Fit Height / Fit Visible | implemented | M2 | All five commands are live. Fit Visible uses the rendered page's content bounds and reports blank or unrendered pages explicitly. Evidence: M2-SHELL and M2-VIEW-ZOOM. |
| View > Zoom > Marquee Zoom | implemented | M2 | `tools-basic` click and marquee zoom are live. Evidence: M2-BASIC. |
| View > Zoom > Dynamic Zoom | implemented | M2 | Menu and rail select the tool; dragging up zooms in and down zooms out around the press point. Evidence: M2-BASIC and M2-VIEW-ZOOM. |
| View > Zoom > Loupe Tool | planned | post-1.0 | Loupe and Pan & Zoom windows are a named post-1.0 slot. |
| View > Zoom > Pan & Zoom | planned | post-1.0 | Loupe and Pan & Zoom windows are a named post-1.0 slot. |
| View > Zoom > Reflow | planned | post-1.0 | Viewer-side reflow shares the machinery the plan defers with reflowing text edit. |
| View > Tools (open a toolset) | implemented | M2 | Opens and closes the registry-driven tool rail. Evidence: M2-SHELL. |
| View > Show/Hide > Navigation Panes | implemented | M2 | Show/hide, the button strip, and activated pane bodies are live with rendered-bounds coverage. Evidence: M2-PANES and B3.1. |
| View > Show/Hide > Toolbar Items / Page Controls | implemented | M2 | Quick actions and page controls can be shown or hidden. Evidence: M2-SHELL. |
| View > Show/Hide > Rulers, Grid, Guides, Snap to Grid | planned | M6 | Grouped with `tools-measure`, as Acrobat groups grids/guides with measuring. |
| View > Show/Hide > Line Weights | planned | M3 | Moved from M2 in plan review: the hayro patch it needed was cut, and the correct semantics are constant hairline width when off, not a width floor. Ships disabled with a reason at M2. |
| View > Page Navigation (First/Previous/Next/Last, Page..., Previous/Next View) | implemented | M2 | All named navigation paths are live. Evidence: M2-SHELL. |
| View > Display Theme (System Theme, Light grey, Dark grey) | implemented | M2 | System, light, and dark themes cover the live shell surfaces. Evidence: M2-SHELL. |
| View > Read Mode | implemented | M2 | Hides global/tab bars, the tool rail, panes, and quick actions while retaining page controls; Escape restores the normal view. Evidence: M2-VIEW-ZOOM. |
| View > Full Screen Mode | partial | M2 | Native entry/exit, complete chrome hiding, full viewport use, and Escape handling are live; presentation semantics remain missing. Evidence: M2-SHELL and M2-VIEW-ZOOM. |
| View > Read Out Loud | planned | M6 | `tools-accessibility` via platform TTS (AVSpeech / SAPI / speech-dispatcher). |
| View > Split / Spreadsheet Split / Remove Split | planned | post-1.0 | Two panes, or four synchronized panes over one document. Not named in the plan. (judgment) |
| View > New Window (second window on the same document) | planned | M3 | (judgment) |
| E-Sign menu | out-of-scope | - | Adobe Acrobat Sign, a cloud service, end to end. |

## Navigation panes

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Page thumbnails pane | implemented | M2 | Activation, lazy rendering, scrolling, selection, navigation, and rendered pane body bounds are implemented. `4132a99` also rearms the canvas poll loop when the pane queues work after initial rendering has settled; its GPUI-loop-only regression prevents black placeholders. Evidence: M2-PANES, B3.1, and the partially populated B7-REF-003 ledger row pending corrected visual capture. |
| Page thumbnails pane context menu (Insert Pages, Extract Pages, Replace Pages, Delete Pages, Rotate Pages, Crop Pages, Page Properties, Embed / Remove All Page Thumbnails, Reduce / Enlarge Page Thumbnails) | partial | M2 | The menu and thumbnail sizing are implemented and accessible; page-mutating commands remain disabled with command-specific M3/M5 owner reasons. Evidence: M2-PANES, B3.1, and B3.6. |
| Bookmarks pane (view and navigate) | implemented | M2 | Outline hierarchy, destination navigation, and rendered pane body bounds are implemented. Evidence: M2-PANES and B3.1. |
| Bookmarks: create, rename, nest, set destination, delete | implemented | M3 | Pane behaviour over `core`: new, rename, set destination to the current page, nest, move out, delete with subtree; each is one undo step. Drag-to-reorder is not built. New Bookmarks From Structure follows at M6. Evidence: docs/evidence/m3-p13b-bookmarks-attachments.md. |
| Attachments pane (list, open, save) | partial | M2 | Listing, Save, prompt-error feedback, stale successful-write protection, and rendered pane body bounds are implemented; Open remains disabled. Evidence: M2-PANES, B3.1, B4.2, and B4.3. |
| Attachments: add and delete file attachments | implemented | M3 | Add Attachment… and per-row Delete in the pane; delete also removes file-attachment comments carrying the stream. Distinct from attach-as-comment. Evidence: docs/evidence/m3-p13b-bookmarks-attachments.md. |
| Bookmarks pane context menu (New Bookmark, Rename, Delete, Set Bookmark Destination, Wrap Long Bookmarks, Properties, New Bookmarks From Structure) | partial | M3 | New, Rename, Set Destination, Nest, Move Out and Delete are live and disabled with the document's reason when it may not be edited. Wrap Long Bookmarks and Properties are not offered; New Bookmarks From Structure follows at M6. Counted once per the context-menu carve-out. Evidence: docs/evidence/m3-p13b-bookmarks-attachments.md. |
| Attachments pane context menu (Open, Save, Add, Delete, Edit Description, Search Attachments) | partial | M3 | Add, Save and Delete are live; Open stays disabled until M5's trust list, and Edit Description and Search Attachments are not offered. Counted once for the whole menu. Evidence: docs/evidence/m3-p13b-bookmarks-attachments.md. |
| Signatures pane | partial | M2 | Signature listing, the M2 status surface, and rendered pane body bounds are implemented; cryptographic validation remains M6. Evidence: M2-PANES and B3.1. |
| Comments pane (list, sort, filter, reply, status) | implemented | M3 | Lists every comment the edited document holds, with replies, status and checkmark; follows edits made anywhere. Sits in the left column for now. Evidence: docs/evidence/m3-p20a-comments-pane.md. |
| Comments list context menu (Reply, Delete, Set Status, Mark With Checkmark, Properties, Make Current Properties Default) | implemented | M3 | Reply, Edit Text, Set Status, Check, Mark as Read/Unread, Delete, Properties and Make Current Properties Default. Evidence: docs/evidence/m3-p20a-comments-pane.md, docs/evidence/m3-p20b-comment-surfaces.md. |
| Layers pane (show/hide optional content groups) | implemented | M2 | OCG listing, nested `/D /Order` hierarchy, omitted-group handling, visibility toggles, and rendered pane body bounds are implemented. Evidence: M2-LAYERS, M2-PANES, B3.1, and B3.2. |
| Layers pane context menu (Layer Properties, visibility and default-state commands) | partial | M2 | Show, Hide, Reset, rendered menu access, and a read-only Layer Properties dialog (visibility and lock per layer) are implemented; renaming, intent changes, merge and flatten remain post-1.0 layer editing. Evidence: M2-PANES, B3.1, and docs/evidence/m3-p13a-properties.md. |
| Layers: import as layers, merge, flatten, layer properties | planned | post-1.0 | Layer editing (import, merge, flatten OCGs) is a named post-1.0 slot. |
| Content pane (document object tree) | planned | M6 | With `tools-accessibility`. |
| Tags pane (structure tree) | planned | M6 | `core` owns the tagged-PDF structure tree; this is its UI. |
| Order pane (reading order) | planned | M6 | Reading-order view/repair is named in the plan for M6. |
| Accessibility report pane | planned | M6 | Output of the rule-based checker. |
| Security Settings pane | planned | M6 | With `tools-protect`. |
| Destinations pane | planned | post-1.0 | Named destinations are a named post-1.0 slot. |
| Articles pane | planned | post-1.0 | Articles are a named post-1.0 slot. |
| Model Tree pane | out-of-scope | - | The navigation pane for 3D annotations, and 3D is named out of scope in the plan's GUI parity bullet. |
| Standards pane | out-of-scope | - | PDF/A-PDF/X compliance reporting belongs to preflight, permanently out of scope. |

## Viewer and reading features

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Open a damaged or malformed PDF (repair on open) | implemented | M2 | Repair-on-open and the user notice are live; repaired incremental save remains a separate guarantee. Evidence: M2-REPAIR. |
| Lazy open of very large documents | implemented | M2 | Lazy object loading and the 1000-page open budget are exercised. Evidence: M2-LAZY. |
| Page rendering | implemented | M2 | hayro base raster, tiny-skia overlays, and damage-tracked tile composition are live. Evidence: M2-VIEWER. |
| Pan / Hand tool | implemented | M2 | `tools-basic` Hand drag semantics are live. Evidence: M2-BASIC. |
| Text selection | implemented | M2 | Document-order text selection and plain copy are live. Evidence: M2-BASIC. |
| Select region / Snapshot | implemented | M2 | Region selection and bounded background clipboard PNG snapshot are live, with stale async completion guards. Evidence: M2-BASIC. |
| Page canvas and text-selection context menu (Copy, Copy With Formatting, Export Selection As, Highlight Text, Add Note To Text, Edit Text, Redact Text, Create Link, Take A Snapshot, Add Bookmark, Rotate, Print, page commands) | partial | M2 | The menu plus Copy, bounded background Snapshot, view rotation, second-right-click repositioning, and command-specific disabled reasons are live; future editing/print commands remain disabled. Evidence: M2-SHELL, M2-BASIC, and B3.6. |
| Copy with formatting / Export selected text | planned | M3 | (judgment) |
| Find toolbar (highlight all, next, previous) | implemented | M2 | The Edit > Find bar, highlight-all, next, and previous paths are live. Evidence: M2-SEARCH. |
| Search results pane | implemented | M2 | Multi-hit results, click-to-navigate, and rendered pane body bounds are implemented. Evidence: M2-SEARCH, M2-PANES, and B3.1. |
| Embedded search index (Manage Embedded Index) | planned | post-1.0 | Embedded search indexes are a named post-1.0 slot. |
| Catalog (full-text index across a folder of PDFs) | out-of-scope | - | A batch indexing product in itself, with its own `.pdx` format and update lifecycle. |
| Initial View settings (open zoom, layout, pane) | implemented | M3 | Document Properties > Initial View sets layout, navigation pane, magnification and open page; open honours them over preferences. A Full Screen page mode is not honoured. Evidence: docs/evidence/m3-p13a-properties.md. |
| Page transitions / set up a PDF as a presentation | planned | post-1.0 | Presentation authoring, the companion to Full Screen mode. Not named in the plan. (judgment) |
| Reading a tagged PDF with a screen reader | planned | M6 | Document-side accessibility; the app-side tree is M2. Read Out Loud has its own rows under Menus and Prepare for accessibility. |
| Liquid Mode | out-of-scope | - | Cloud-tethered Adobe reflow service; named out of scope in the plan. |

## Toolset: Edit a PDF

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Edit text (line-level) | planned | M5 | Backed by `text-engine` system-font matching and fallback, with fsType embedding permissions enforced. The reflow half of Acrobat's text editing is the next row, not a permanent reduction of this one. |
| Reflowing text edit (text repours across the paragraph or page) | planned | post-1.0 | Named as a post-1.0 slot in the plan, and explicitly out of scope pre-1.0. |
| Change font, size, colour, alignment, spacing of edited text | planned | M5 | The future command is constrained by what the embedded subset and its fsType bits permit. |
| Add text (new text box) | planned | M5 | (judgment) |
| Edit images and objects (move, resize, rotate, flip, crop, align) | planned | M5 | |
| Replace image | planned | M5 | |
| Add image | planned | M5 | |
| Extract / save image | planned | M5 | (judgment) |
| Add or edit links | planned | M5 | Named in the plan's `tools-edit` list. Link actions: go to a page view, open a file, open a web page, custom (Link Properties). |
| Auto-create links from URLs | planned | M5 | (judgment) |
| Remove web links | planned | M5 | (judgment) |
| Crop pages | planned | M5 | Named in the plan's `tools-edit` list. |
| Header and footer: add, update, remove | planned | M5 | Named in the plan's `tools-edit` list. |
| Watermark: add, update, remove | planned | M5 | Named in the plan's `tools-edit` list. |
| Background: add, update, remove | planned | M5 | Named in the plan's `tools-edit` list. |
| Bates numbering: add, remove, add to file names | planned | M5 | Named in the plan's `tools-edit` list. |
| Edit a scanned PDF (OCR-then-edit) | planned | post-1.0 | Depends on the post-1.0 `neural` OCR slot. |
| Edit a signed or certified PDF | planned | M6 | Structural for Onionskin: an incremental update is the only legal way, and the plan makes it the default save path. |
| Generate or edit an image with Adobe Express / Firefly | out-of-scope | - | Cloud-tethered generative service. |

## Toolset: Create a PDF

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Create from a single image file | implemented | M3 | PNG, JPEG and TIFF through `codecs-common`; GIF, BMP and WebP are not imported. Evidence: docs/evidence/m3-p14a-images.md. |
| Create from multiple files | implemented | M3 | File > Create PDF From Multiple Files… opens the Combine dialog under its own title. Evidence: docs/evidence/m3-p12-combine-split.md. |
| Create a blank page | implemented | M3 | `tools-organize` inserts a blank page after the current one, at its size. Evidence: docs/evidence/m3-p11-organize.md. |
| Create from the clipboard | implemented | M3 | Image clipboard only (PNG, JPEG, TIFF), through the one pasteboard read. Evidence: docs/evidence/m3-p14a-images.md. |
| Create from a scanner | planned | post-1.0 | Scanner capture (ICA/TWAIN/WIA) is a named post-1.0 slot. |
| Create from a web page (web capture) | out-of-scope | - | Requires bundling an HTML engine and a paginating layout pass; a product in itself. |
| Create from an Office document | out-of-scope | - | Reading DOCX/XLSX/PPTX at fidelity is a product in itself; the plan shrinks the codec surface to image import/export. |
| Acrobat PDFMaker (create from inside Office) | out-of-scope | - | Ships into Microsoft Office's add-in model; a separate product. |
| Adobe PDF printer (virtual print driver) | out-of-scope | - | Named out of scope in the plan's GUI parity bullet: every supported OS already prints to PDF, so a virtual printer driver would duplicate the platform for nothing. |
| Acrobat Distiller (PostScript to PDF) | out-of-scope | - | A PostScript interpreter is a product in itself. |
| Generate presentation (AI) | out-of-scope | - | Cloud-tethered generative service (Acrobat Studio / PDF Spaces). |

## Toolset: Combine files

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Combine files into a single PDF | implemented | M3 | File > Combine Files… runs in the background and opens the result; pages are transitive copies, metadata is fresh, and structure is kept only when every input is tagged. No per-file bookmarks. Evidence: docs/evidence/m3-p12-combine-split.md. |
| Add files / add folders to the combine list | implemented | M3 | Add Files and Add Folder (PDFs directly inside, sorted by name). Evidence: docs/evidence/m3-p12-combine-split.md. |
| Reorder, preview and remove entries before combining | partial | M3 | Move Up, Move Down and Remove are live; the preview is a text label (page count and selected pages), not a thumbnail. Evidence: docs/evidence/m3-p12-combine-split.md. |
| Expand a file and combine at page granularity | implemented | M3 | A page field expands the selected file to a page list such as "9-10, 1, 3". Evidence: docs/evidence/m3-p12-combine-split.md. |
| Combine into a PDF Portfolio | planned | post-1.0 | Portfolios are a named post-1.0 slot. |

## Toolset: Organize pages

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Rotate pages (left / right) | implemented | M3 | Edit menu and canvas context menu rotate the current page either way, writing `/Rotate`. Multi-page selection arrives with the Organize grid row. Evidence: docs/evidence/m3-p11-organize.md. |
| Reorder / move pages | implemented | M3 | Move the current page earlier or later from the Edit menu; drag reordering arrives with the Organize grid row. Evidence: docs/evidence/m3-p11-organize.md. |
| Insert pages (from file, blank, clipboard) | partial | M3 | Blank insertion is live from the Edit menu. Insert from file exists in `tools-organize` but its dialog waits for P21; insert from clipboard is not built. Evidence: docs/evidence/m3-p11-organize.md. |
| Insert > From Web Page | out-of-scope | - | The same web capture ruled out under Create a PDF: it needs a bundled HTML engine and a paginating layout pass. The menu entry does not exist rather than existing and failing. |
| Delete pages | implemented | M3 | Deletes the current page from the Edit menu, with tagged-structure cleanup; multi-page selection arrives with the Organize grid row. Evidence: docs/evidence/m3-p11-organize.md. |
| Extract pages | planned | M3 | Named in the plan's `tools-organize` list. `extract_pages_to` exists in `tools-organize`, but no user path reaches it until P21's dialog. Evidence: docs/evidence/m3-p11-organize.md. |
| Split (by page count, file size, or top-level bookmarks) | implemented | M3 | File > Split Document… offers all three; output is all-or-nothing and refused on encrypted documents. Split runs on the UI thread. Evidence: docs/evidence/m3-p12-combine-split.md. |
| Replace pages | planned | M3 | Named in the plan's `tools-organize` list. The one-undo-step replace exists in `tools-organize`, but no user path reaches it until P21's dialog. Evidence: docs/evidence/m3-p11-organize.md. |
| Copy or move pages between open documents | planned | M3 | The copy and move functions exist in `tools-organize`, but no user path reaches them until P21's grid. Evidence: docs/evidence/m3-p11-organize.md. (judgment) |
| Renumber pages / page labels | partial | M3 | A "number pages from 1" command is live; label styles, prefixes and ranges wait for P21's dialogs. Evidence: docs/evidence/m3-p11-organize.md. |
| Crop pages (from Organize) | planned | M5 | Same command as Edit a PDF > Crop; delivered with `tools-edit`. |
| Page thumbnail zoom and multi-select in the Organize grid | planned | M3 | (judgment) |

## Toolset: Compress a PDF

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Compress a PDF (one-click, size level) | planned | M3 | `commands-core`: "compress/flatten export". This is a flattening rewrite, not an incremental save, and says so in the UI. |
| Reduce File Size (compatibility target) | planned | M3 | |
| PDF Optimizer (advanced dialog: images, fonts, transparency, discard objects, discard user data, clean up) | planned | post-1.0 | The full dialog is a large surface; the one-click path covers the common case first. |
| Audit space usage | planned | post-1.0 | Cheap given byte-span fidelity, but no consumer before the Optimizer dialog exists. |

## Toolset: Export a PDF

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Export to Microsoft Word (.docx) | planned | post-1.0 | The supported subset is a post-1.0 target and will remain deliberately lossy; no usable subset has shipped. |
| Export to Microsoft Excel (.xlsx) | planned | post-1.0 | The supported subset will remain lossy because table recovery from a content stream is approximate; none has shipped. |
| Export to Microsoft PowerPoint (.pptx) | planned | post-1.0 | A deliberately lossy post-1.0 target; no usable subset has shipped. |
| Export to Rich Text Format (.rtf) | planned | post-1.0 | A deliberately lossy post-1.0 target; no usable subset has shipped. |
| Export to HTML | planned | post-1.0 | Layout-to-flow conversion will remain deliberately lossy; no usable subset has shipped. |
| Export to plain text / accessible text | partial | M2 | Plain document-order text export supports a selected page range; tagged accessible reading order remains M6. Evidence: M2-EXPORT and M2-EXPORT-SETTINGS. |
| Export pages to PNG | implemented | M2 | Selected page ranges and PNG resolution are live; filenames retain absolute page numbers padded to the document's page-count width, with no-overwrite destination reservation. Evidence: M2-EXPORT, M2-EXPORT-SETTINGS, and B4.4. |
| Export pages to SVG | implemented | M2 | Selected page ranges and per-page vector export are live with page-count width and no-overwrite destination reservation. Evidence: M2-EXPORT, M2-EXPORT-SETTINGS, and B4.4. |
| Export pages to JPEG / JPEG2000 / TIFF | partial | M3 | JPEG (with Quality) and TIFF are live with DPI; JPEG 2000 is missing because no pure-Rust encoder exists at usable quality and a C dependency is not added. Evidence: docs/evidence/m3-p14a-images.md. |
| Export all images in a document | implemented | M3 | Asks for a folder and never overwrites; skipped names are reported. Inline images and `/SMask` merging are out of scope. Evidence: docs/evidence/m3-p14a-images.md. (judgment) |
| Export to XML / XML spreadsheet | planned | post-1.0 | Low demand relative to cost, and no plan consumer. (judgment) |
| Export to PostScript / EPS | out-of-scope | - | PostScript generation is a print-production concern, and print production is permanently out of scope. |
| Save as PDF/A (archivable) | out-of-scope | - | PDF/A conversion is preflight work: it means colour conversion, font embedding and compliance verification, permanently out of scope. |
| Save as PDF/X (press-ready) | out-of-scope | - | PDF/X is preflight work too: colour conversion, font embedding and compliance verification. |
| Save as Reader Extended PDF | out-of-scope | - | Reader extensions are cryptographically enabled by Adobe; not reproducible outside Adobe. |

## Toolset: Add comments

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Sticky note | implemented | M3 | Placed by a click at the icon's upper-left corner; a field opens beside it and Enter finishes. Evidence: docs/evidence/m3-p9a-notes-and-free-text.md. Typed in place: docs/evidence/m3-typing-in-place.md. |
| Highlight text | implemented | M3 | Quad points come from `content`'s glyph mapping, one quad per line run; appearance uses `/BM /Multiply`. Evidence: docs/evidence/m3-p8-text-markup.md. |
| Underline text | implemented | M3 | Shares the markup tool's drag and rail slot. Evidence: docs/evidence/m3-p8-text-markup.md. |
| Strikethrough text | implemented | M3 | Shares the markup tool's drag and rail slot. Evidence: docs/evidence/m3-p8-text-markup.md. |
| Insert text at cursor (caret markup) | implemented | M3 | The caret annotation is written and a field opens for the inserted text. Evidence: docs/evidence/m3-p8-text-markup.md. Typed in place: docs/evidence/m3-typing-in-place.md. |
| Replace text (strikeout plus replacement note) | implemented | M3 | Strike-out and `/IRT`-linked reply commit as one transaction; a field opens for the replacement text. Evidence: docs/evidence/m3-p8-text-markup.md. Typed in place: docs/evidence/m3-typing-in-place.md. |
| Add text comment (typewriter) | implemented | M3 | Creates the `/FreeText` with `/DA` and appearance from one style; the text is typed in place and the appearance is redrawn from `/DA`. Evidence: docs/evidence/m3-p9a-notes-and-free-text.md. Typed in place: docs/evidence/m3-typing-in-place.md. |
| Text box | implemented | M3 | Sized by the drag; `/DA` and appearance name the same font. Text is typed in place, one line at a time; wrapping is on explicit newlines only. Evidence: docs/evidence/m3-p9a-notes-and-free-text.md. Typed in place: docs/evidence/m3-typing-in-place.md. |
| Callout | implemented | M3 | Drag runs from target to box; a three-point `/CL` leader is written and drawn, and the text is typed in place. Evidence: docs/evidence/m3-p9a-notes-and-free-text.md. Typed in place: docs/evidence/m3-typing-in-place.md. |
| Draw freehand (ink) | implemented | M3 | Pressure sets per-segment width in the appearance; one stroke is one undo step. The Draw quick action has a tool. Evidence: docs/evidence/m3-p9b-ink.md. |
| Erase ink | implemented | M3 | Splits strokes under the eraser and recomputes `/Rect`; an emptied annotation is removed. Evidence: docs/evidence/m3-p9b-ink.md. |
| Line | implemented | M3 | Evidence: docs/evidence/m3-p9c-shapes.md. |
| Arrow | implemented | M3 | Evidence: docs/evidence/m3-p9c-shapes.md. |
| Rectangle | implemented | M3 | Evidence: docs/evidence/m3-p9c-shapes.md. |
| Oval | implemented | M3 | Ellipse inscribed in the dragged rectangle. Evidence: docs/evidence/m3-p9c-shapes.md. |
| Polygon | implemented | M3 | A double-click on the last corner, or a click on the first, finishes it. Evidence: docs/evidence/m3-p9c-shapes.md. |
| Connected lines (polyline) | implemented | M3 | A double-click on the last point finishes it. Evidence: docs/evidence/m3-p9c-shapes.md. |
| Cloud | implemented | M3 | Cloud intensity is fixed at 1 until the properties inspector (row: Comment properties). Evidence: docs/evidence/m3-p9c-shapes.md. |
| Attach a file as a comment | implemented | M3 | Embeds the chosen file with a paperclip appearance; the file is chosen before the click. Listed in the Attachments pane after reopen. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |
| Record an audio comment | out-of-scope | - | A sound annotation is rich media, which the plan's GUI parity bullet rules out because no crate could own a capture and playback stack. |
| Comment properties (colour, opacity, author, subject, default) | implemented | M3 | The side panel's inspector for the chosen comment: eight swatches, four opacities, author and subject, each an undoable edit that redraws the appearance; Make Current Properties Default sets the next comment of that kind's colour and opacity. docs/evidence/m3-p20b-comment-surfaces.md |
| Comments list: sort, filter, reply, set status, checkmark, read/unread | implemented | M3 | Sort, filter, reply, status, checkmark, edit text and delete are live and undoable; read/unread is the session's own state and never written to the file. Evidence: docs/evidence/m3-p20a-comments-pane.md, docs/evidence/m3-p20b-comment-surfaces.md. |
| Summarize comments (generate a summary PDF) | implemented | M3 | Two layouts (comments only, page then comments), opened in a tab; refused on encrypted documents. Connector-line and on-page sequence-number layouts are not offered. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |
| Print comments (document and markups, summary only) | planned | M3 | With `crates/print`. |
| Import / export comments as FDF or XFDF | planned | post-1.0 | XFDF form-data interchange is a named post-1.0 slot; comment interchange rides with it. |
| Enable commenting for Reader users (Reader-extended PDF) | out-of-scope | - | Adobe-signed Reader extensions; not reproducible outside Adobe. |
| Commenting preferences | implemented | M3 | Preferences > Commenting sets the author name that signs every comment, reply, status and dynamic stamp, in every open tab at once. Acrobat's display options (pop-up font and opacity, print notes) are not offered. Evidence: docs/evidence/m3-p20a-comments-pane.md, docs/evidence/m3-p20b-comment-surfaces.md. |

## Toolset: Add stamps

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Place a stamp | implemented | M3 | Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |
| Standard business stamps (Approved, Draft, Confidential, ...) | implemented | M3 | 12 stamps, artwork generated in-house from one table. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |
| Sign Here stamp category | implemented | M3 | 5 stamps. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |
| Dynamic stamps (name, date, time from identity and clock) | partial | M3 | 5 stamps filled natively from the clock, in UTC, with the name from Preferences > Commenting; local time is not offered. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md, docs/evidence/m3-p20b-comment-surfaces.md. |
| Create a custom stamp | implemented | M3 | From a PDF page or an image, stored under the tool's data folder. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |
| Manage stamps (delete stamps and categories) | implemented | M3 | Edit > Stamps…; only custom stamps can be deleted, and an emptied category's folder is removed. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |
| Paste clipboard image as stamp | implemented | M3 | Edit menu and the Stamps dialog; each paste replaces `Pasted/Clipboard Image`. Evidence: docs/evidence/m3-p10-stamps-attachments-summary.md. |

## Toolset: Fill & Sign

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Add text | planned | M5 | `tools-fill-sign`. |
| Add checkmark / cross / dot | planned | M5 | |
| Add circle / line | planned | M5 | |
| Sign yourself: create signature (type, draw, image) | planned | M5 | Local appearance only; not a cryptographic signature. |
| Sign yourself: add initials | planned | M5 | |
| Save and reuse a signature locally | planned | M5 | |
| Sync signature across devices via an Adobe account | out-of-scope | - | Cloud-tethered account sync. |
| Request e-signatures | out-of-scope | - | Adobe Acrobat Sign, a cloud service. |

## Toolset: Prepare a form

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Auto-detect form fields in an existing document | planned | M5 | |
| Text field | planned | M5 | `tools-form` AcroForm fields with appearance streams. |
| Check box | planned | M5 | |
| Radio button | planned | M5 | |
| List box | planned | M5 | |
| Dropdown (combo box) | planned | M5 | |
| Button (push button) | planned | M5 | |
| Image field | planned | M5 | |
| Date field | planned | M5 | |
| Digital signature field | planned | M5 | Field at M5; signing at M6. |
| Barcode field | planned | post-1.0 | Barcode form fields are a named post-1.0 slot. |
| Field properties: General, Appearance, Position, Options, Actions | planned | M5 | |
| Field properties: Format, Validate, Calculate | planned | M5 | The intended subset will be backed by `scripting` (Boa): `AFNumber_Format`, `AFSimple_Calculate`, and related helpers. Anything outside that subset must get a visible notice rather than a silent wrong value. No usable subset has shipped. |
| Tab order / form field navigation | planned | M5 | |
| Fill in a form (as an end user) | planned | M5 | Guarantee test 7: the JS-forms corpus fills like Acrobat. |
| Clear form | planned | M5 | |
| Auto-Complete form entries (Off / Basic / Advanced, plus the editable entry list) | planned | M5 | Named in the plan's M5 list. It stays local, which is exactly why the stored entry list has to be inspectable and clearable. |
| Import / export form data (FDF, XFDF, XML) | planned | post-1.0 | XFDF is a named post-1.0 slot. |
| Distribute a form (email or internal server) | out-of-scope | - | A distribution and response-collection workflow, cloud- and server-tethered. |
| Track forms / Forms Tracker / collect responses | out-of-scope | - | A cloud- and server-tethered response-collection workflow; it only exists once a form has been distributed. |
| Create a web form | out-of-scope | - | Cloud-hosted service. |
| XFA / LiveCycle Designer forms | out-of-scope | - | Legal posture rule 5: XFA is Adobe-specified, deprecated in PDF 2.0, and outside the clean ISO patent story. Future open handling must detect it and show an explicit read-only notice; that notice has not shipped. |
| Document-level and interactive JavaScript beyond the forms API | out-of-scope | - | Decision 9 limits future scripting to a forms subset with no I/O and a fuel budget. Unsupported JavaScript must eventually produce a visible notice; that notice has not shipped. |

## Toolset: Redact a PDF

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Mark text for redaction | planned | M5 | |
| Mark images / regions for redaction | planned | M5 | Plan: image region scrub. |
| Mark whole pages for redaction | planned | M5 | |
| Find text and redact (search and redact, including patterns) | planned | M5 | Search lives in `content`, shared with viewer Ctrl+F. |
| Redaction properties (fill colour or none, overlay text, font, auto-size, repeat, alignment, outline and fill opacity) | planned | M5 | |
| Redaction code sets and the Redaction Code Editor | planned | M5 | Acrobat ships U.S. FOIA and U.S. Privacy Act sets and supports add, rename, import and export of custom sets. |
| Apply redactions | planned | M5 | The one destructive path: a flattening rewrite, never an incremental save. Onionskin's verifier runs as part of this command (not counted: no Acrobat equivalent). |
| Sanitize document / remove hidden information | planned | M5 | The full sweep is named in the plan's `redact` crate: scripts, hidden layers, deleted and cropped content, attachments and actions, on top of metadata. Acrobat folds it into the Apply flow. |

## Toolset: Protect a PDF

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Protect using a password (open password) | planned | M6 | `tools-protect`; `cos` parses encryption from M1 and writes it at M6. Acrobat's levels: 128-bit AES and 256-bit AES. |
| Restrict editing and printing (permissions password) | planned | M6 | |
| Set permission details (printing allowed, changes allowed, copy, accessibility) | planned | M6 | |
| Open an encrypted document | partial | M3 | Ruling A: empty-user-password documents open read-only, with editing disabled at open and an open-time notice. Documents with a user password are refused; writing encrypted files and `/P` enforcement are M6. Accepted regression: where `/P` bit 4 allows modification, other editors may let the user change the document. Evidence: docs/evidence/m3-p1b-encryption.md. |
| Remove security | planned | M6 | |
| Encrypt with a certificate | planned | M6 | `crypto` handles the recipient list. |
| Encrypt only file attachments | planned | post-1.0 | A narrow variant of certificate and password encryption, with no plan consumer. (judgment) |
| Security policies (save and reuse a security setting) | planned | post-1.0 | Convenience layer over the same primitives. |
| Adobe Experience Manager / LiveCycle Rights Management policies | out-of-scope | - | Server-tethered enterprise DRM. |
| Protected View / Enhanced Security / privileged locations | out-of-scope | - | These configure Acrobat's sandbox around document JavaScript, embedded media and network access. Onionskin's `scripting` sandbox has no I/O and no network at all, so there is nothing to loosen or tighten. |
| JavaScript preferences (enable or disable document JavaScript) | planned | M5 | Decision 9: a user-facing preference disables document JavaScript entirely, mirroring Acrobat's. Ships with `scripting` at M5. |
| Trust Manager: allow or block links and attachment opening | planned | M5 | The per-site and per-type trust list. It is the same trust surface as the JS-disable preference decision 9 names, so it ships with `scripting` at M5 rather than drifting to post-1.0. (judgment) |
| Document properties > Security tab | planned | M6 | (judgment) |

## Toolset: Use a certificate (digital signatures)

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Validate an existing signature | planned | M6 | Guarantee test 4. |
| Signature properties and validation report | planned | M6 | |
| Preserve signature validity across edits | planned | M6 | Structural: incremental update is the only legal way to annotate a signed PDF, and it is the default save path. |
| Digitally sign with a certificate | planned | M6 | PAdES signing. |
| Certify a document (visible or invisible) | planned | M6 | (judgment) |
| Manage digital IDs | planned | M6 | Platform keystores: Keychain, CNG, PKCS#11. |
| Create a self-signed digital ID | planned | M6 | Acrobat offers self-signed and CA-issued IDs; `crypto` can mint the former. |
| Create and manage signature appearances (name, logo, imported graphic, which fields show) | planned | M6 | Signature appearance management is named in the plan's M6 list. Distinct from the Fill & Sign scribble, which is not cryptographic. |
| Signature verification preferences (auto-validate on open, revocation checking) | planned | M6 | (judgment) |
| Timestamp a document (RFC 3161 timestamp server) | planned | M6 | Timestamping is named in the plan's M6 list. |
| Long-term validation (LTV) enablement | planned | M6 | Named in the plan's M6 list, alongside timestamping. |
| Trusted identities / manage trusted certificates | planned | M6 | Trusted-identity management is named in the plan's M6 list; backed by the platform trust store. |
| Adobe Approved Trust List (AATL) and EUTL | out-of-scope | - | Adobe-operated trust programme distributed through Adobe's update channel; Onionskin uses the platform trust store instead. |
| Lock a document after signing | planned | M6 | (judgment) |

## Toolset: Scan & OCR

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Scan a document to PDF | planned | post-1.0 | Scanner capture (ICA/TWAIN/WIA) is a named post-1.0 slot. |
| Scanner presets and settings | planned | post-1.0 | Acrobat's presets: Autodetect Color Mode, Black & White Document, Grayscale Document, Color Image, Color Document, plus Custom Scan. Rides with scanner capture. |
| Recognize text (OCR) in this file | planned | post-1.0 | The `neural` slot: tract-based OCR for scanned PDFs. |
| Recognize text in multiple files | planned | post-1.0 | Same slot. |
| OCR language and output settings (searchable image, editable text) | planned | post-1.0 | Same slot. |
| Correct recognized text (review OCR suspects) | planned | post-1.0 | Same slot. |
| Enhance a scanned document (Deskew, Descreen, Background Removal, Text Sharpening) | planned | post-1.0 | Scan enhancement (deskew, descreen, background removal) is a named post-1.0 slot. |
| Enhance a camera image (document photo cleanup) | planned | post-1.0 | Rides with the named post-1.0 scan-enhancement work. |

## Toolset: Measure objects

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Distance tool | planned | M6 | Named in the plan's `tools-measure` list. |
| Perimeter tool | planned | M6 | Named in the plan's `tools-measure` list. |
| Area tool | planned | M6 | Named in the plan's `tools-measure` list. |
| Scale ratio and units | planned | M6 | |
| 2D snap settings (endpoints, midpoints, intersections; sensitivity, snap hint colour) | planned | M6 | (judgment) |
| Enable Measurement Markup (persist a measurement as an annotation) | planned | M6 | (judgment) |
| Measurement Info panel (live measurement, delta X/Y, active scale) | planned | M6 | (judgment) |
| Geospatial location tool (read coordinates off a map PDF) | out-of-scope | - | Geospatial PDFs are named out of scope in the plan's GUI parity bullet: no crate owns coordinate systems and no plan milestone claims them. |
| Register / edit geospatial map info | out-of-scope | - | Authoring the map registration a geospatial PDF carries; geospatial PDFs are out of scope in the plan's GUI parity bullet. |
| Measure 3D objects | out-of-scope | - | 3D only; see Rich media and 3D. |

## Toolset: Prepare for accessibility

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Check for accessibility (rule-based Accessibility Checker) | planned | M6 | Plan: a checker like Acrobat Pro's. Also eats its own dog food in guarantee test 8. |
| Accessibility report | planned | M6 | |
| Automatically tag a PDF (autotag) | planned | M6 | Local. |
| Cloud-based auto-tagging | out-of-scope | - | Cloud-tethered Adobe service. |
| Fix reading order (Reading Order tool) | planned | M6 | Named in the plan's `tools-accessibility` list. |
| Edit structure with the Tags and Content panes | planned | M6 | (judgment) |
| Set alternate text for figures | planned | M6 | (judgment) |
| Set document language | planned | M6 | (judgment) |
| Set document title | planned | M6 | (judgment) |
| Table editor / table summary | planned | M6 | (judgment) |
| Keep the structure tree valid through every edit | planned | M5 | Not a command but a property Acrobat is expected to hold: an accessible document stays accessible through editing. `core` owns the tree; guarantee test 8 enforces it from `tools-edit` onward. |
| Read Out Loud (from the accessibility toolset) | planned | M6 | Same feature as the View menu item. Acrobat's submenu: Activate Read Out Loud, Read This Page Only, Read To End of Document, Pause, Stop. |
| Accessibility Setup Assistant | planned | post-1.0 | A preferences wizard; the underlying preferences ship with the shell. Reflow, which it configures, has its own row under Menus. |

## Toolset: Use print production

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Output Preview (separations, ink coverage, colour warnings) | out-of-scope | - | Print production is permanently out of scope in the plan: prepress is a product in itself, with its own colour engine and certification expectations. |
| Preflight (profiles, checks, fixups, reports, droplets, libraries, variables) | out-of-scope | - | Same. Named explicitly in the plan as permanently out of scope. |
| Convert Colors | out-of-scope | - | Same; needs a full ICC transform engine as a semantic operation, not a display concern. |
| Ink Manager | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Add Printer Marks | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Fix Hairlines | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Transparency Flattener Preview | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Set Page Boxes (media, crop, bleed, trim, art) | planned | M5 | In scope per the plan: `tools-edit` crop covers advanced page boxes at M5. Only the prepress half of this toolset (Output Preview, Marks and Bleeds, trapping) stays out. |
| Trap Presets | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Edit Object (prepress object editor) | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Save as PDF/X, PDF/A, PDF/E from print production | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Output intents | out-of-scope | - | Print production, permanently out of scope: prepress is a product in itself. |
| Colour settings and colour-managing documents | out-of-scope | - | ICC handling for display is hayro's job; colour management as an authored document property belongs to prepress. |

## Toolset: Use guided actions (Action Wizard)

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Run a shipped action | planned | post-1.0 | Guided actions (Action Wizard) are a named post-1.0 slot: a natural fit over the registry and MCP. |
| Create a new action | planned | post-1.0 | Guided actions (Action Wizard) are a named post-1.0 slot: a natural fit over the registry and MCP. |
| Edit, rename, copy, delete an action | planned | post-1.0 | Guided actions (Action Wizard) are a named post-1.0 slot: a natural fit over the registry and MCP. |
| Import / export an action | planned | post-1.0 | Guided actions (Action Wizard) are a named post-1.0 slot: a natural fit over the registry and MCP. |
| Choose files to process (file, folder, open documents) | planned | post-1.0 | Guided actions (Action Wizard) are a named post-1.0 slot: a natural fit over the registry and MCP. |
| Create and manage custom commands | planned | post-1.0 | Guided actions (Action Wizard) are a named post-1.0 slot: a natural fit over the registry and MCP. |
| Make Accessible action | planned | post-1.0 | Guided actions (Action Wizard) are a named post-1.0 slot: a natural fit over the registry and MCP. |

## Toolset: Compare files

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Compare two documents | planned | post-1.0 | Compare Files is a named post-1.0 slot in the plan. |
| Compare text only | planned | post-1.0 | Same. |
| Side-by-side and single-file result views | planned | post-1.0 | Same. |
| Navigate changes (first, previous, next) | planned | post-1.0 | Same. |
| Filter changes by type | planned | post-1.0 | Same. |
| Comparison report / change list | planned | post-1.0 | Same. |

## Printing

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Print dialog | planned | M3 | Decision 13: `crates/print` is our own pipeline, with Acrobat print-dialog parity. macOS (NSPrintOperation) at M3. |
| Print on Linux (CUPS) | planned | M4 | |
| Print on Windows | planned | M4 | |
| Page range and subset (all, current, custom, odd/even) | planned | M3 | |
| Page sizing and handling: Fit, Actual size, Shrink oversized, Custom scale | planned | M3 | Named in the plan's print-parity list. |
| Multiple pages per sheet (N-up) | planned | M3 | Named in the plan's print-parity list. |
| Booklet | planned | M3 | Named in the plan's print-parity list. |
| Poster / tile | planned | M3 | Named in the plan's `crates/print` dialog-parity list, so it ships with the M3 dialog. |
| Print on both sides / duplex | planned | M3 | (judgment) |
| Orientation (auto, portrait, landscape) | planned | M3 | (judgment) |
| Comments & Forms (Document, Document and Markups, Document and Stamps, Form Fields Only) | planned | M3 | (judgment) |
| Page Setup dialog (paper size, orientation) | planned | M3 | (judgment) |
| Summarize comments in the print output | planned | M3 | (judgment) |
| Print as image | planned | M3 | Named in the plan's print-parity list. |
| Print to file / print to PDF | planned | M3 | The plan makes the print-to-PDF-file backend the first one, so print output is testable in CI. |
| Advanced Print Setup dialog | planned | M3 | The future supported subset includes Print as Image and Print to File; Output, Marks and Bleeds, PostScript options, and print colour management remain out of scope. No usable subset has shipped. |
| Print colour PDFs (separations, colour handling) | out-of-scope | - | Separation printing and colour handling are prepress work, permanently out of scope. |
| Print a PDF Portfolio | planned | post-1.0 | Rides with portfolios. |

## Sharing, reviews and cloud services

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Share a link for viewing or commenting | out-of-scope | - | Cloud-tethered; the plan names web-based review flows as out of scope. |
| Send for comments / shared review | out-of-scope | - | Cloud-tethered web review flow. |
| Publish comments, check for new comments | out-of-scope | - | Cloud-tethered; part of the same review flow. |
| Review Tracker | out-of-scope | - | Tracks cloud reviews; nothing to track without them. |
| Approval workflows | out-of-scope | - | Cloud-tethered review workflow. |
| Shared reviews on SharePoint or Office 365 | out-of-scope | - | Server-tethered. |
| Send and track (document tracking) | out-of-scope | - | Cloud-tethered. |
| AI Assistant: summarize, ask questions, cite sources | out-of-scope | - | Cloud-tethered generative service; named out of scope in the plan. |
| AI Assistant: compare documents, rewrite text, generate content | out-of-scope | - | Cloud-tethered generative service. |
| Audio summaries / podcast-style overviews | out-of-scope | - | Cloud-tethered generative service. |
| PDF Spaces (AI knowledge hub over a file set) | out-of-scope | - | Same, and it is an Acrobat Studio service rather than a local tool. |
| Adobe Express design tools inside Acrobat | out-of-scope | - | Cloud-tethered creative service. |
| Collect online payments | out-of-scope | - | Cloud-tethered commerce service. |

## Rich media, 3D and other document features

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Add video / audio to a page | out-of-scope | - | Rich media is named out of scope in the plan's GUI parity bullet: no crate could own a media playback stack. |
| Play embedded video, audio and multimedia | out-of-scope | - | Needs the same bundled media playback stack. |
| Add multimedia buttons and interactive objects | out-of-scope | - | They exist to drive the media stack Onionskin does not bundle. Form push buttons are in scope and have their own row. |
| Comment on video (timed comments) | out-of-scope | - | Needs the same bundled media playback stack. |
| Add a 3D model (U3D / PRC) | out-of-scope | - | 3D is named out of scope in the plan's GUI parity bullet: no crate could own a 3D engine and a second geometry format family. |
| View, rotate and cut away a 3D model | out-of-scope | - | Needs the same embedded 3D engine. |
| 3D views, 3D comments, 3D measurement | out-of-scope | - | Needs the same embedded 3D engine. |
| PDF layers (view and toggle) | implemented | M2 | Optional-content parsing, nested `/D /Order` hierarchy, omitted-group handling, render toggles, and the M2 Layers pane body are implemented. Evidence: M2-LAYERS, M2-PANES, B3.1, and B3.2. |
| PDF articles (article threads) | planned | post-1.0 | Articles are a named post-1.0 slot, reading and authoring both. |
| Geospatial PDFs | out-of-scope | - | Named out of scope in the plan's GUI parity bullet, alongside rich media and 3D. |
| PDF Portfolios (create, customize, sort, publish, search) | planned | post-1.0 | A named post-1.0 slot. |
| Asian, Cyrillic and right-to-left text | partial | post-1.0 | Viewing rides on hayro from M2; editing and shaping for bidi and vertical text is a named post-1.0 slot. |

## What `PLAN.md` still does not name

The plan has since absorbed most of what this section used to list: guided
actions, background, replace pages, page labels, bookmark and attachment
authoring, layer editing, articles, named destinations, embedded search indexes,
barcode fields, the full sanitize sweep, spell check, find-and-replace, the
JS-disable preference, form auto-complete, scan enhancement, poster/tile, Full
Screen, line weights, Loupe and Pan & Zoom, signature appearances, trusted
identities, timestamping and LTV. Rich media, 3D, geospatial PDFs and the virtual
printer driver are now named out of scope, while RTF and HTML are deliberately
lossy post-1.0 targets. What follows is the remainder: the rows above marked
`(judgment)`, grouped. This is input to the next plan revision, not a backlog.

| Area | Why it matters |
|---|---|
| The thumbnails context menu crosses M2/M3/M5 lines | The M2 shell ships the menu and thumbnail sizing. Most document mutations belong to `tools-organize` at M3, while Crop belongs to `tools-edit` at M5. Entries stay disabled until their owner ships, and the row states that split. |
| Context menus as a class | The GUI parity section cites Acrobat's context menus as part of what transfers, but only the thumbnails menu is named anywhere. Seven context-menu surfaces now carry rows here (page canvas, thumbnails, bookmarks, attachments, comments list, layers, document tabs); their milestones are inherited, not stated. |
| Viewer modes the M2 line does not enumerate | Split and Spreadsheet Split, page transitions and presentation setup, Overprint Preview, Automatically Scroll. Full Screen, line weights, Loupe and Pan & Zoom were the rest of this class and the plan now names them, which is why these four stand out. |
| Home view | The plan describes the document window in detail and never mentions the no-document-open state: Recents, Starred, the list/thumbnail toggle. Acrobat users meet it first. |
| Shell chrome outside the document | Manage Tools, the Window and Help menus, New Window, Copy File to Clipboard. Small, but a shell that lacks them reads as unfinished. |
| Print dialog options beyond the plan's six | The `crates/print` parity list names page ranges, scaling, N-up, booklet, poster/tile and print-as-image. The dialog also has duplex, orientation, Comments & Forms, Page Setup and print-time comment summaries, all of which users touch. |
| Accessibility authoring detail | The plan names the checker, reading-order repair and Read Out Loud. Making a document accessible also needs alternate text, document language, document title, the table editor and Tags/Content pane editing. |
| Measure detail | The plan names distance, perimeter and area with scale. The toolset also has snap settings, measurement markup and the Measurement Info panel. |
| Trust surface beyond signing | Certify, lock-after-signing, signature verification preferences, security policies, attachment-only encryption, the Document Properties Security tab, and where Trust Manager lives. M6 names signing, timestamping, LTV and trusted identities but stops there; this file puts Trust Manager at M5 with `scripting`, which the plan should confirm. |
| Search depth | Searching inside attachments and by document property are plain Acrobat search options with no plan line. |
| Edit a PDF detail | Add-text-box, extract image, auto-create links from URLs, remove web links. The `tools-edit` list names editing and replacing, not adding and extracting. |
| Codec detail | JPEG, JPEG 2000 and TIFF page export, export-all-images and XML export sit next to the plan's named PNG and SVG. |
| Catalog (a full-text index across a folder of PDFs) | The plan names embedded search indexes as post-1.0 but says nothing about Acrobat's multi-file Catalog. This file rules it out as a batch indexing product in itself; that call is the scoreboard's, not the plan's. |
| Organize interaction detail | Copying or moving pages between open documents, and thumbnail zoom and multi-select in the Organize grid, are interaction affordances rather than commands, which is why the plan's command list misses them. |
