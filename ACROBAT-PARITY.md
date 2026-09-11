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

**403 rows: 255 planned / 16 partial / 80 out-of-scope. 52 implemented.**

323 rows (implemented plus planned and partial) are the parity target. The other 80 are the
deliberate no. By milestone: M2 67, M3 99, M4 2, M5 52, M6 46, post-1.0 57.
M4 carries only two rows because its deliverables (the MCP server, the CUPS and
Windows print backends) are mostly not Acrobat surface. 42 rows are marked
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
| Quick action toolbar (floating over the page) | partial | M2 | Toolbar, dragging, customization, and Select are live; Comment, Highlight, Draw, Fill text fields, and Add Sign remain disabled until their owning milestones. Evidence: M2-SHELL. |
| Right-hand side panel | implemented | M2 | The contextual host, empty state, and open/close behavior are live; tool-specific content starts at M3. Evidence: M2-SHELL and B3.7. |
| Page controls / bottom toolbar | implemented | M2 | Page, rotate, zoom, fit controls, invalid-zoom display, and numeric page-field semantics are live. Evidence: M2-SHELL and B3.7. |
| Global search field (tools and document text) | implemented | M2 | Tool lookup, current-document text search, and the no-document unavailable state are live. Evidence: M2-SEARCH and B3.7. |
| Convert (global bar entry point) | planned | M3 | The future button ships as one surface; its supported target list will remain deliberately smaller than Acrobat's. |
| Get a link to the document | out-of-scope | - | Cloud-tethered link sharing. |
| Undo / Redo icons on the global bar | planned | M3 | Same commands as the Edit menu; Acrobat surfaces both. |
| Save / Save As in the global bar | planned | M3 | Save appends an incremental section (core invariant). |
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
| Preferences dialog | partial | M2 | The live Documents, General, Page Display, and Search subset is persisted; later feature categories land with their owners. Adobe-account, cloud-storage, Tracker, Updater, Multimedia, and 3D categories remain out of scope. Evidence: M2-PREFS. |
| Keyboard shortcut remapping | implemented | M2 | Acrobat defaults are remappable through `keymap.json`, and the shortcut reference shows the effective bindings. Evidence: M2-PREFS. |
| Autosave and crash recovery | planned | M3 | Plan pins crash-recovery snapshot ranking as a unit test. |
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
| File > Create | planned | M3 | Sources limited: see "Create a PDF". |
| File > Save | planned | M3 | Appends an incremental update section; a no-op save writes nothing. |
| File > Save As | planned | M3 | Copy plus the same incremental discipline. |
| File > Save as Other | planned | M3 | The future menu will expose only supported sub-targets; PDF/X and Reader-Extended variants remain out of scope. |
| File > Export To | partial | M2 | Text, PNG, and SVG exports have First/Last settings, plus PNG resolution; stale successful prompt writes are refused and derived destinations cannot overwrite existing files. Office and HTML targets remain a deliberately reduced post-1.0 subset. Evidence: M2-EXPORT, M2-EXPORT-SETTINGS, and B4.2-B4.4. |
| File > Revert | planned | M3 | Cheap here: truncate to the previous generation. |
| File > Close / Close All | implemented | M2 | Both commands are live for the current tab set. Evidence: M2-SHELL. |
| File > Properties (Document Properties) | planned | M3 | `commands-core`. Acrobat's current unified UI documents five tabs: Description, Security, Fonts, Initial View, Custom. The classic Advanced tab is no longer listed; confirm against the screenshot corpus before building it. |
| File > Print | planned | M3 | `crates/print`. |
| File > Attach to Email | planned | M3 | Hands off to the OS mail client; no Adobe service involved. Plan does not name it; placed with `commands-core`. |
| File > Share / Send for comments | out-of-scope | - | Cloud-tethered web review flow. |
| File > Get Documents Signed | out-of-scope | - | Adobe Acrobat Sign, a cloud service. |
| File > Exit / Quit | implemented | M2 | The menu command exits through the shell action. Evidence: M2-SHELL. |
| Edit > Undo / Redo | planned | M3 | Undo is dropping edit-graph overlay nodes, not restoring snapshots. |
| Edit > Cut / Copy / Paste / Delete | planned | M3 | Scope is per active tool. |
| Edit > Select All / Deselect All | implemented | M2 | Both registry commands operate on the active document and page. Evidence: M2-SHELL. |
| Edit > Copy File to Clipboard | planned | M3 | (judgment) |
| Edit > Take a Snapshot | implemented | M2 | The menu activates `tools-basic` Snapshot and copies the selected raster region. Evidence: M2-BASIC. |
| Edit > Find (Ctrl+F) | implemented | M2 | Case, whole-word, highlight-all, next, and previous are live; bookmark/comment inclusion is not part of this row. Evidence: M2-SEARCH. |
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
| Bookmarks: create, rename, nest, set destination, delete | planned | M3 | Bookmark authoring is named in the plan's `commands-core` list. Acrobat's New Bookmarks From Structure needs the tagged tree and follows at M6. |
| Attachments pane (list, open, save) | partial | M2 | Listing, Save, prompt-error feedback, stale successful-write protection, and rendered pane body bounds are implemented; Open remains disabled. Evidence: M2-PANES, B3.1, B4.2, and B4.3. |
| Attachments: add and delete file attachments | planned | M3 | Attachment authoring is named in the plan's `commands-core` list. Distinct from attach-as-comment. |
| Bookmarks pane context menu (New Bookmark, Rename, Delete, Set Bookmark Destination, Wrap Long Bookmarks, Properties, New Bookmarks From Structure) | planned | M3 | Counted once for the whole menu per the context-menu carve-out; it activates with bookmark authoring in `commands-core`. New Bookmarks From Structure follows at M6 with the tagged tree. |
| Attachments pane context menu (Open, Save, Add, Delete, Edit Description, Search Attachments) | planned | M3 | Counted once for the whole menu; activates with attachment authoring in `commands-core`. |
| Signatures pane | partial | M2 | Signature listing, the M2 status surface, and rendered pane body bounds are implemented; cryptographic validation remains M6. Evidence: M2-PANES and B3.1. |
| Comments pane (list, sort, filter, reply, status) | planned | M3 | Ships with `tools-comment`. |
| Comments list context menu (Reply, Delete, Set Status, Mark With Checkmark, Properties, Make Current Properties Default) | planned | M3 | Counted once for the whole menu; ships with `tools-comment`. |
| Layers pane (show/hide optional content groups) | implemented | M2 | OCG listing, nested `/D /Order` hierarchy, omitted-group handling, visibility toggles, and rendered pane body bounds are implemented. Evidence: M2-LAYERS, M2-PANES, B3.1, and B3.2. |
| Layers pane context menu (Layer Properties, visibility and default-state commands) | partial | M2 | Show, Hide, Reset, and rendered menu access are implemented; Properties waits for M3, while merge/flatten remain post-1.0. Evidence: M2-PANES and B3.1. |
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
| Initial View settings (open zoom, layout, pane) | planned | M3 | Document Properties > Initial View. |
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
| Create from a single image file | planned | M3 | `codecs-common`: "Create PDF from images". |
| Create from multiple files | planned | M3 | Overlaps Combine files. |
| Create a blank page | planned | M3 | With `tools-organize`. |
| Create from the clipboard | planned | M3 | Image clipboard only. |
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
| Combine files into a single PDF | planned | M3 | Named in the plan's `commands-core` list. |
| Add files / add folders to the combine list | planned | M3 | |
| Reorder, preview and remove entries before combining | planned | M3 | |
| Expand a file and combine at page granularity | planned | M3 | |
| Combine into a PDF Portfolio | planned | post-1.0 | Portfolios are a named post-1.0 slot. |

## Toolset: Organize pages

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Rotate pages (left / right) | planned | M3 | Named in the plan's `tools-organize` list. |
| Reorder / move pages | planned | M3 | Named in the plan's `tools-organize` list. |
| Insert pages (from file, blank, clipboard) | planned | M3 | Named in the plan's `tools-organize` list. |
| Insert > From Web Page | out-of-scope | - | The same web capture ruled out under Create a PDF: it needs a bundled HTML engine and a paginating layout pass. The menu entry does not exist rather than existing and failing. |
| Delete pages | planned | M3 | Named in the plan's `tools-organize` list. |
| Extract pages | planned | M3 | Named in the plan's `tools-organize` list. |
| Split (by page count, file size, or top-level bookmarks) | planned | M3 | Named in the plan's `commands-core` list. |
| Replace pages | planned | M3 | Named in the plan's `tools-organize` list. |
| Copy or move pages between open documents | planned | M3 | (judgment) |
| Renumber pages / page labels | planned | M3 | Page labels are named in the plan's `tools-organize` list. |
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
| Export pages to JPEG / JPEG2000 / TIFF | planned | M3 | Not named in the plan; same codec crate, one milestone later. |
| Export all images in a document | planned | M3 | (judgment) |
| Export to XML / XML spreadsheet | planned | post-1.0 | Low demand relative to cost, and no plan consumer. (judgment) |
| Export to PostScript / EPS | out-of-scope | - | PostScript generation is a print-production concern, and print production is permanently out of scope. |
| Save as PDF/A (archivable) | out-of-scope | - | PDF/A conversion is preflight work: it means colour conversion, font embedding and compliance verification, permanently out of scope. |
| Save as PDF/X (press-ready) | out-of-scope | - | PDF/X is preflight work too: colour conversion, font embedding and compliance verification. |
| Save as Reader Extended PDF | out-of-scope | - | Reader extensions are cryptographically enabled by Adobe; not reproducible outside Adobe. |

## Toolset: Add comments

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Sticky note | planned | M3 | Named in the plan's `tools-comment` list. |
| Highlight text | planned | M3 | Named in the plan's `tools-comment` list. Quad points come from `content`'s glyph mapping. |
| Underline text | planned | M3 | Named in the plan's `tools-comment` list. |
| Strikethrough text | planned | M3 | Named in the plan's `tools-comment` list. |
| Insert text at cursor (caret markup) | planned | M3 | |
| Replace text (strikeout plus replacement note) | planned | M3 | |
| Add text comment (typewriter) | planned | M3 | Plan lists "text box"; this is its sibling. |
| Text box | planned | M3 | Named in the plan's `tools-comment` list. |
| Callout | planned | M3 | |
| Draw freehand (ink) | planned | M3 | With stylus pressure, per the GPUI fork. |
| Erase ink | planned | M3 | |
| Line | planned | M3 | |
| Arrow | planned | M3 | |
| Rectangle | planned | M3 | |
| Oval | planned | M3 | |
| Polygon | planned | M3 | |
| Connected lines (polyline) | planned | M3 | |
| Cloud | planned | M3 | |
| Attach a file as a comment | planned | M3 | Named in the plan's `tools-comment` list. |
| Record an audio comment | out-of-scope | - | A sound annotation is rich media, which the plan's GUI parity bullet rules out because no crate could own a capture and playback stack. |
| Comment properties (colour, opacity, author, subject, default) | planned | M3 | |
| Comments list: sort, filter, reply, set status, checkmark, read/unread | planned | M3 | |
| Summarize comments (generate a summary PDF) | planned | M3 | |
| Print comments (document and markups, summary only) | planned | M3 | With `crates/print`. |
| Import / export comments as FDF or XFDF | planned | post-1.0 | XFDF form-data interchange is a named post-1.0 slot; comment interchange rides with it. |
| Enable commenting for Reader users (Reader-extended PDF) | out-of-scope | - | Adobe-signed Reader extensions; not reproducible outside Adobe. |
| Commenting preferences | planned | M3 | |

## Toolset: Add stamps

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Place a stamp | planned | M3 | Named in the plan's `tools-comment` list. |
| Standard business stamps (Approved, Draft, Confidential, ...) | planned | M3 | Icon artwork redrawn in-house, per the plan's legal line. |
| Sign Here stamp category | planned | M3 | |
| Dynamic stamps (name, date, time from identity and clock) | planned | M3 | Acrobat drives these with the same `AF*` JavaScript helpers `scripting` will implement at M5; at M3 Onionskin will fill them natively from the system clock and identity preference. |
| Create a custom stamp | planned | M3 | |
| Manage stamps (delete stamps and categories) | planned | M3 | |
| Paste clipboard image as stamp | planned | M3 | |

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
| Open an encrypted document | planned | M6 | |
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
