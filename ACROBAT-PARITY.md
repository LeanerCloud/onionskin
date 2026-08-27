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
  - `planned` - intended in full, not built yet.
  - `partial` - intended, but a deliberately reduced subset; the row will never
    read "complete" and the Notes column says what is cut.
  - `out-of-scope` - will not be built. Every such row states why.
- **Nothing is implemented yet.** There is no `implemented` value in this file
  today. It gets added the first time a row ships, and from then on the headline
  number is `implemented / total`.
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

**403 rows: 311 planned / 12 partial / 80 out-of-scope. 0 implemented.**

323 rows (planned plus partial) are the parity target. The other 80 are the
deliberate no. By milestone: M2 67, M3 99, M4 2, M5 52, M6 46, post-1.0 57.
M4 carries only two rows because its deliverables (the MCP server, the CUPS and
Windows print backends) are mostly not Acrobat surface. 42 rows are marked
`(judgment)`: their milestone does not follow from plan text and a plan revision
should confirm or move them. Recount with:

```sh
awk -F'|' '/^\|/ {gsub(/^ +| +$/,"",$3); if ($3 ~ /^(planned|partial|out-of-scope|implemented)$/) c[$3]++} \
  END {for (k in c) print c[k], k}' ACROBAT-PARITY.md
```

---

## Application shell

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Global bar (top bar) | planned | M2 | Plan: "quick-action top bar" from the first release. |
| Hamburger / main menu button | planned | M2 | Houses File/Edit/View/Window/Help in the unified UI. |
| Document tabs (multiple open documents) | planned | M2 | Plan pins tab-strip logic as unit tests. |
| Document tab context menu (Close, Close Others, Close All, Show Containing Folder, Copy Path) | planned | M2 | Counted once for the whole menu per the context-menu carve-out. (judgment) |
| All tools pane (left tool rail) | planned | M2 | Plan: "The plugin taxonomy IS Acrobat's tool rail." Registry `describe` output should read like this pane. |
| "View more" / expand full tool list | planned | M2 | Same pane, collapsed state. |
| Quick action toolbar (floating over the page) | planned | M2 | Acrobat's defaults: Select, Comment, Highlight, Draw, Fill text fields, Add Sign or Initials. Draggable within the document view. |
| Right-hand side panel | planned | M2 | Contextual panel host; populated per toolset from M3 on. |
| Page controls / bottom toolbar | planned | M2 | Page-number box, up/down arrows, rotate icon, "Display the page at 100% magnification" zoom control. |
| Global search field (tools and document text) | planned | M2 | Text search is `content`'s full-text index; tool search is registry lookup. |
| Convert (global bar entry point) | partial | M3 | Same surface class as File > Export To: the button ships whole, the target list behind it does not. |
| Get a link to the document | out-of-scope | - | Cloud-tethered link sharing. |
| Undo / Redo icons on the global bar | planned | M3 | Same commands as the Edit menu; Acrobat surfaces both. |
| Save / Save As in the global bar | planned | M3 | Save appends an incremental section (core invariant). |
| Print button | planned | M3 | `crates/print`, macOS backend at M3. |
| Share button | out-of-scope | - | Cloud-tethered: Adobe cloud link sharing. Plan states all cloud-tethered surface is out of scope. |
| AI Assistant button | out-of-scope | - | Cloud-tethered generative service. Plan names AI Assistant explicitly as out of scope. |
| Account / profile menu | out-of-scope | - | Requires an Adobe account; Onionskin has no account system. |
| Acrobat notifications | out-of-scope | - | Cloud-tethered notification feed. |
| Home view: Recents | planned | M2 | Local recents list only. |
| Home view: Starred | planned | M3 | Local flag in app state. Acrobat stores starred files in Adobe cloud storage; Onionskin keeps the list on disk. (judgment) |
| Home view: list view / thumbnail view toggle | planned | M2 | (judgment) |
| Home view: Adobe cloud storage | out-of-scope | - | Cloud-tethered; Onionskin's counter-pitch is local and private. |
| Home view: Box / Dropbox / Google Drive / OneDrive / SharePoint connectors | out-of-scope | - | Cloud-tethered third-party storage integrations. |
| Display theme (light / dark / system) | planned | M2 | GPUI theming in `app`. |
| Customize the quick action toolbar | planned | M2 | Which quick actions show, per Acrobat's toolbar customization. |
| Manage Tools / customize the tool rail | planned | M3 | Registry-driven: shows/hides registered plugins. (judgment) |
| Revert to the classic Acrobat interface | out-of-scope | - | Product-level decision: the unified UI is the parity target; the plan defers classic to a possible later theme, not a shipped toggle. |
| Preferences dialog | partial | M2 | Acrobat's dialog carries roughly 30 categories (Accessibility, Commenting, Documents, Forms, Full Screen, General, Identity, JavaScript, Language, Measuring 2D/3D/Geo, Multimedia, Page Display, Reading, Reviewing, Search, Security, Security Enhanced, Signatures, Spelling, Tracker, Trust Manager, Units & Guides, Updater, and more). Onionskin mirrors the ones whose feature it builds: Accessibility, Commenting, Documents, Forms, Full Screen, General, Identity, JavaScript, Measuring, Page Display, Reading, Search, Security, Signatures, Spelling, Trust Manager, Units & Guides. Trust Manager is mirrored because it is the JavaScript and attachment trust surface decision 9 already commits to, and lands with `scripting` at M5. Adobe-account, cloud-storage, Tracker, Updater, Multimedia and 3D categories are out of scope, so the dialog is permanently a subset. Individual categories ship with their feature, not all at M2. |
| Keyboard shortcut remapping | planned | M2 | Acrobat defaults, remappable via `keymap.json` (Schist's mechanism). |
| Autosave and crash recovery | planned | M3 | Plan pins crash-recovery snapshot ranking as a unit test. |
| Window menu (New Window, Cascade, Tile, Minimize) | planned | M3 | Shell logic, no document dependency. (judgment) |
| Help menu (About, keyboard shortcuts) | planned | M2 | Online help targets are out of scope; local shortcut reference is not. (judgment) |
| Check for updates / auto-update | planned | post-1.0 | Plan lists auto-update as a post-1.0 slot (Schist's Check for Updates path as template). |
| UI localization | planned | post-1.0 | Plan lists localization plus bidi/vertical text as post-1.0. |
| App-level accessibility tree (screen reader support for the UI) | planned | M2 | AccessKit into the GPUI fork; M1 spike de-risks it, live from the first release. |
| Pinch-to-zoom and stylus pressure | planned | M2 | The IAmJSD GPUI fork exists for exactly this. |
| Register `.pdf` as openable ("Open with") | planned | M2 | Plan: joins the "Open with" menu, never takes files off Acrobat. |
| Set as the default PDF viewer | out-of-scope | - | Deliberate product decision in the plan: never the default handler. |
| Display PDF in a browser / browser extension | out-of-scope | - | A browser plug-in is a separate product with its own sandbox and update channel. |
| Acrobat for Outlook / Office add-ins (PDFMaker) | out-of-scope | - | Requires shipping into Microsoft Office's add-in model and reading Office formats; a product in itself. |

## Menus

Acrobat's unified UI collapses the classic menu bar into the hamburger menu (and
keeps a native menu bar on macOS). Onionskin follows the same structure.

| Item | Status | Milestone | Notes |
|---|---|---|---|
| File > Open | planned | M2 | Including the repair path for malformed files (decision 10). |
| File > Open Recent | planned | M2 | Local list. |
| File > Create | planned | M3 | Sources limited: see "Create a PDF". |
| File > Save | planned | M3 | Appends an incremental update section; a no-op save writes nothing. |
| File > Save As | planned | M3 | Copy plus the same incremental discipline. |
| File > Save as Other | partial | M3 | Only the sub-targets that have their own rows below; the PDF/X and Reader-Extended variants are out of scope. |
| File > Export To | partial | M3 | The menu exists from M3 with the image and text targets; the Office and HTML targets stay partial forever. See "Export a PDF". |
| File > Revert | planned | M3 | Cheap here: truncate to the previous generation. |
| File > Close / Close All | planned | M2 | |
| File > Properties (Document Properties) | planned | M3 | `commands-core`. Acrobat's current unified UI documents five tabs: Description, Security, Fonts, Initial View, Custom. The classic Advanced tab is no longer listed; confirm against the screenshot corpus before building it. |
| File > Print | planned | M3 | `crates/print`. |
| File > Attach to Email | planned | M3 | Hands off to the OS mail client; no Adobe service involved. Plan does not name it; placed with `commands-core`. |
| File > Share / Send for comments | out-of-scope | - | Cloud-tethered web review flow. |
| File > Get Documents Signed | out-of-scope | - | Adobe Acrobat Sign, a cloud service. |
| File > Exit / Quit | planned | M2 | |
| Edit > Undo / Redo | planned | M3 | Undo is dropping edit-graph overlay nodes, not restoring snapshots. |
| Edit > Cut / Copy / Paste / Delete | planned | M3 | Scope is per active tool. |
| Edit > Select All / Deselect All | planned | M2 | |
| Edit > Copy File to Clipboard | planned | M3 | (judgment) |
| Edit > Take a Snapshot | planned | M2 | `tools-basic` snapshot. |
| Edit > Find (Ctrl+F) | planned | M2 | The plan's M2 line names Ctrl+F with whole-word and case options, highlight-all and next/previous. Acrobat adds Include bookmarks and Include comments. |
| Find toolbar > Replace text | planned | M5 | Find-and-replace is named in the plan's M5 list. It writes text, so it belongs to `tools-edit` rather than to viewer search. |
| Edit > Advanced Search, current document | planned | M2 | Return Results Containing: Match Exact Word Or Phrase / Any Of The Words / All Of The Words, plus Stemming. |
| Advanced Search > include attachments | planned | M3 | Acrobat searches attached files two levels deep. (judgment) |
| Advanced Search > document-property criteria (author, dates, keywords, metadata) | planned | M3 | (judgment) |
| Edit > Advanced Search across multiple PDFs / a folder / an index | planned | post-1.0 | Needs the multi-document index; single-document search ships first. Boolean Query and Proximity are multi-document options in Acrobat and land here, not on the single-document row. |
| Edit > Check Spelling (in comments and form fields) | planned | M5 | Spell check is named in the plan's M5 list. |
| Edit > Look Up Selected Word | planned | post-1.0 | Platform dictionary only; the web lookup Acrobat uses is out of scope. |
| Edit > Preferences | planned | M2 | |
| View > Rotate View | planned | M2 | View-only rotation, distinct from page rotation. |
| View > Page Display > Single Page | planned | M2 | |
| View > Page Display > Single Page Continuous | planned | M2 | Default, and the mode the 60 fps scroll budget is measured in. |
| View > Page Display > Two Page View | planned | M2 | |
| View > Page Display > Two Page Scrolling | planned | M2 | |
| View > Page Display > Show Cover Page in Two Page View | planned | M2 | |
| View > Page Display > Automatically Scroll | planned | M3 | (judgment) |
| View > Page Display > Overprint Preview | planned | post-1.0 | A rendering toggle that simulates overprinting ink, so it belongs to `render`, not to the prepress Output Preview tool listed as out of scope. Deferred until hayro can express it. (judgment) |
| View > Zoom > Zoom In / Zoom Out / Zoom To | planned | M2 | |
| View > Zoom > Actual Size / Fit Page / Fit Width / Fit Height / Fit Visible | planned | M2 | |
| View > Zoom > Marquee Zoom | planned | M2 | `tools-basic` zoom. |
| View > Zoom > Dynamic Zoom | planned | M2 | |
| View > Zoom > Loupe Tool | planned | post-1.0 | Loupe and Pan & Zoom windows are a named post-1.0 slot. |
| View > Zoom > Pan & Zoom | planned | post-1.0 | Loupe and Pan & Zoom windows are a named post-1.0 slot. |
| View > Zoom > Reflow | planned | post-1.0 | Viewer-side reflow shares the machinery the plan defers with reflowing text edit. |
| View > Tools (open a toolset) | planned | M2 | |
| View > Show/Hide > Navigation Panes | planned | M2 | Individual panes have their own rows. |
| View > Show/Hide > Toolbar Items / Page Controls | planned | M2 | |
| View > Show/Hide > Rulers, Grid, Guides, Snap to Grid | planned | M6 | Grouped with `tools-measure`, as Acrobat groups grids/guides with measuring. |
| View > Show/Hide > Line Weights | planned | M2 | Named in the plan's M2 shell list as the line-weights view toggle. |
| View > Page Navigation (First/Previous/Next/Last, Page..., Previous/Next View) | planned | M2 | |
| View > Display Theme (System Theme, Light grey, Dark grey) | planned | M2 | Acrobat extends the theme to menus, context menus, scroll bars and the comments pane. |
| View > Read Mode | planned | M2 | |
| View > Full Screen Mode | planned | M2 | Named in the plan's M2 shell list. |
| View > Read Out Loud | planned | M6 | `tools-accessibility` via platform TTS (AVSpeech / SAPI / speech-dispatcher). |
| View > Split / Spreadsheet Split / Remove Split | planned | post-1.0 | Two panes, or four synchronized panes over one document. Not named in the plan. (judgment) |
| View > New Window (second window on the same document) | planned | M3 | (judgment) |
| E-Sign menu | out-of-scope | - | Adobe Acrobat Sign, a cloud service, end to end. |

## Navigation panes

| Item | Status | Milestone | Notes |
|---|---|---|---|
| Page thumbnails pane | planned | M2 | Named in the plan's M2 shell list. |
| Page thumbnails pane context menu (Insert Pages, Extract Pages, Replace Pages, Delete Pages, Rotate Pages, Crop Pages, Page Properties, Embed / Remove All Page Thumbnails, Reduce / Enlarge Page Thumbnails) | planned | M2 | The plan's M2 shell list names thumbnails with context-menu page commands, while the commands themselves belong to `tools-organize` at M3. Both readings are kept: the menu surface exists at M2 and its page-mutating entries activate at M3, disabled until then rather than absent. Counted once for the whole menu per the context-menu carve-out. |
| Bookmarks pane (view and navigate) | planned | M2 | Named in the plan's M2 shell list. |
| Bookmarks: create, rename, nest, set destination, delete | planned | M3 | Bookmark authoring is named in the plan's `commands-core` list. Acrobat's New Bookmarks From Structure needs the tagged tree and follows at M6. |
| Attachments pane (list, open, save) | planned | M2 | Named in the plan's M2 shell list. |
| Attachments: add and delete file attachments | planned | M3 | Attachment authoring is named in the plan's `commands-core` list. Distinct from attach-as-comment. |
| Bookmarks pane context menu (New Bookmark, Rename, Delete, Set Bookmark Destination, Wrap Long Bookmarks, Properties, New Bookmarks From Structure) | planned | M3 | Counted once for the whole menu per the context-menu carve-out; it activates with bookmark authoring in `commands-core`. New Bookmarks From Structure follows at M6 with the tagged tree. |
| Attachments pane context menu (Open, Save, Add, Delete, Edit Description, Search Attachments) | planned | M3 | Counted once for the whole menu; activates with attachment authoring in `commands-core`. |
| Signatures pane | planned | M2 | Listing at M2; validation and status reporting at M6. |
| Comments pane (list, sort, filter, reply, status) | planned | M3 | Ships with `tools-comment`. |
| Comments list context menu (Reply, Delete, Set Status, Mark With Checkmark, Properties, Make Current Properties Default) | planned | M3 | Counted once for the whole menu; ships with `tools-comment`. |
| Layers pane (show/hide optional content groups) | planned | M2 | Named in the plan's M2 shell list: layers with OCG visibility toggles. |
| Layers pane context menu (Layer Properties, visibility and default-state commands) | planned | M2 | Counted once for the whole menu. Its merge and flatten entries belong to post-1.0 layer editing and stay disabled until then. |
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
| Open a damaged or malformed PDF (repair on open) | planned | M2 | Decision 10; guarantee test 6. Acrobat-grade scan-and-rebuild in `cos`. |
| Lazy open of very large documents | planned | M2 | Decision 11 budget: time-to-first-page under 200 ms on the 1000-page corpus file. |
| Page rendering | planned | M2 | hayro base raster plus tiny-skia overlays, composited into damage-tracked tiles. |
| Pan / Hand tool | planned | M2 | `tools-basic`. |
| Text selection | planned | M2 | Via `content` byte-span mapping. |
| Select region / Snapshot | planned | M2 | `tools-basic`. |
| Page canvas and text-selection context menu (Copy, Copy With Formatting, Export Selection As, Highlight Text, Add Note To Text, Edit Text, Redact Text, Create Link, Take A Snapshot, Add Bookmark, Rotate, Print, page commands) | planned | M2 | Counted once for the whole menu per the context-menu carve-out. It exists at M2 with the viewer entries live; each editing entry activates with the plugin that owns it, the way the thumbnails menu does. |
| Copy with formatting / Export selected text | planned | M3 | (judgment) |
| Find toolbar (highlight all, next, previous) | planned | M2 | The in-document bar Edit > Find opens. Named in the plan's M2 line. |
| Search results pane | planned | M2 | |
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
| Change font, size, colour, alignment, spacing of edited text | planned | M5 | Constrained at runtime by what the embedded subset and its fsType bits permit; the command itself is complete. |
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
| Export to Microsoft Word (.docx) | partial | post-1.0 | Plan: "Export-to-Office lands post-1.0 at best and is marked partial forever (full-fidelity DOCX export is a product in itself)." |
| Export to Microsoft Excel (.xlsx) | partial | post-1.0 | Same reason; table recovery from a content stream is lossy by nature. |
| Export to Microsoft PowerPoint (.pptx) | partial | post-1.0 | Same reason. |
| Export to Rich Text Format (.rtf) | partial | post-1.0 | The plan marks Export-to-Office, RTF and HTML partial forever: full-fidelity document export is a product in itself. |
| Export to HTML | partial | post-1.0 | The plan marks Export-to-Office, RTF and HTML partial forever: layout-to-flow conversion is lossy for the same reason. |
| Export to plain text / accessible text | planned | M2 | Falls out of `content`'s byte-span text extraction; accessible text ordering improves at M6. |
| Export pages to PNG | planned | M2 | Named in the plan's `codecs-common` list. |
| Export pages to SVG | planned | M2 | Named in the plan's `codecs-common` list. |
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
| Dynamic stamps (name, date, time from identity and clock) | planned | M3 | Acrobat drives these with the same `AF*` JavaScript helpers `scripting` implements at M5; at M3 Onionskin fills them natively from the system clock and the identity preference. |
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
| Field properties: Format, Validate, Calculate | partial | M5 | Backed by `scripting` (Boa) covering the Acrobat forms API subset: `AFNumber_Format`, `AFSimple_Calculate` and friends. Anything outside that subset gets a visible notice rather than silent wrong values. |
| Tab order / form field navigation | planned | M5 | |
| Fill in a form (as an end user) | planned | M5 | Guarantee test 7: the JS-forms corpus fills like Acrobat. |
| Clear form | planned | M5 | |
| Auto-Complete form entries (Off / Basic / Advanced, plus the editable entry list) | planned | M5 | Named in the plan's M5 list. It stays local, which is exactly why the stored entry list has to be inspectable and clearable. |
| Import / export form data (FDF, XFDF, XML) | planned | post-1.0 | XFDF is a named post-1.0 slot. |
| Distribute a form (email or internal server) | out-of-scope | - | A distribution and response-collection workflow, cloud- and server-tethered. |
| Track forms / Forms Tracker / collect responses | out-of-scope | - | A cloud- and server-tethered response-collection workflow; it only exists once a form has been distributed. |
| Create a web form | out-of-scope | - | Cloud-hosted service. |
| XFA / LiveCycle Designer forms | out-of-scope | - | Legal posture rule 5: XFA is Adobe-specified, deprecated in PDF 2.0 and outside the clean ISO patent story. Behaviour: detected on open and shown read-only with an explicit notice, nothing more. |
| Document-level and interactive JavaScript beyond the forms API | out-of-scope | - | Decision 9: scope is the forms subset, sandboxed with no I/O and a fuel budget; anything beyond is surfaced as a visible notice. |

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
| Advanced Print Setup dialog | partial | M3 | The plan splits this dialog by name: Print as Image and Print to File are in scope; Output, Marks and Bleeds, PostScript options and print colour management stay with the out-of-scope print-production surface. |
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
| PDF layers (view and toggle) | planned | M2 | Optional content groups, visible from the M2 Layers pane; see that row. |
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
printer driver are now named out of scope, and RTF and HTML are ratified as
partial forever. What follows is the remainder: the rows above marked
`(judgment)`, grouped. This is input to the next plan revision, not a backlog.

| Area | Why it matters |
|---|---|
| The thumbnails context menu sits on both sides of an M2/M3 line | The plan's M2 shell list names "thumbnails with context-menu page commands", while every command in that menu belongs to `tools-organize` at M3. The scoreboard resolves it by shipping the menu at M2 with its page-mutating entries disabled until M3, and says so in the row rather than quietly picking one milestone. The plan should adopt that reading or move one of the two lines. |
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
