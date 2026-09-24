# Onionskin: gap analysis and work plan to 1.0

Baseline: `master` at `a74aafd`, plus the uncommitted `ACROBAT-PARITY.md` edit that promotes the password-security rows. I only read files. Nothing was built or run. The two Measure rows that are still `partial` (Scale ratio and units, 2D snap settings) are treated as done, as you asked. Their leftover notes are in the appendix at the end.

## 0. What I found before the rows

1. **`docs/evidence/m6-security.md` does not exist.** Six rows in the uncommitted `ACROBAT-PARITY.md` diff cite it (Protect Using Password, Restrict editing, Permission details, Open an encrypted document, Remove Security, and the Document Properties Security tab). `git status` shows only `ACROBAT-PARITY.md` modified. That doc has to be written and committed with the scoreboard change first (WP0).
2. **`crates/text-engine` is an empty crate** (5 lines of doc comment, no dependencies). Every M5 text row, and every M6 appearance that wants a font other than the standard 14, waits on it.
3. **`plugins/tools-protect` is empty** (20 lines, registers nothing). **`crates/crypto` has no signature code.** It has 1,370 lines and all of it is the standard security handler. **`core/src/signatures.rs` only lists fields**, and it says so itself.
4. **`plugins/tools-accessibility` holds only the structural checker** (`checker.rs`, 106 lines). `core::structure::Element` has no `/Alt`, `/ActualText`, `/Lang`, `/A` attributes and no RoleMap resolution. `content` knows which MCIDs a page opens (`page_mcids`), but text runs, images and paths do not record which marked-content sequence holds them.
5. **Some scoreboard notes are stale:**
   - Pinch-to-zoom and stylus pressure says "no shipped pressure-aware ink tool", but `m3-p9b-ink.md` shipped one.
   - Insert pages says "Insert from file ... waits for P21", but P21 landed: `OrganizeAction::InsertFromFile` in `crates/app/src/shell/organize/view.rs`, wired in `tabs/page_grid.rs`.
   - Print comments says summary printing "waits on row 96", and that row ("Summarize comments in the print output") is implemented (`crates/print/src/appendix.rs`).
6. **Tools in the container for checking our output independently:**
   - qpdf, pdfinfo, pypdf 3.17.4, pikepdf 10.5.1.
   - **pdfsig** (poppler's signature validator), **openssl** (`cms`, `ts`, `ocsp`, `ca`), **pdfimages**, **gs**.
   - Pillow with JPEG 2000 support.
   - Xvfb and xvfb-run.
   - **pyhanko** is one `pip install` away: pypi answers 200. It signs, validates, timestamps and does public-key (certificate) encryption.
   - **softhsm2** can be installed with apt (a dry run resolves it), which gives a PKCS#11 token to test against.
7. **Network through the proxy:** crates.io and pypi work. Public timestamp servers are blocked: `timestamp.digicert.com` and `freetsa.org` both return 403 at CONNECT. Timestamp and revocation tests therefore have to use local servers built with openssl.
8. **GPUI fork limits:** `PlatformWindow` has `resize`, `minimize` and `zoom`, but nothing that sets a window's position. `ClipboardEntry` is `String | Image` only. Cascade/Tile and Copy With Formatting cannot be done without changing the fork.
9. **Crypto dependency problem:** the stable RustCrypto `rsa` crate (0.9) carries RUSTSEC-2023-0071 (Marvin timing), and `rsa` 0.10 is still `rc.18`. `deny.toml` says "a vulnerability gets no exception here". `ring` 0.17.14 is already in `Cargo.lock` through gpui's rustls and passes deny. It signs and verifies RSA (PKCS#1 v1.5 and PSS) and ECDSA P-256/P-384, and it verifies legacy SHA-1 RSA. It cannot decrypt RSA and cannot generate RSA keys. This forces a decision from you (section 3, D1).

## 1. Every remaining row

Size key: S is under about 300 lines, M is 300 to 1000, L is over 1000.

"Closable here" means the gap can be closed and proved in this Linux container with the tools listed above. Where it says no, the reason is given.

### M2 (13 rows, all partial)

| # | Row (short) | Status | Exists today | Gap to `implemented` | Closable here | Size | Pkg |
|---|---|---|---|---|---|---|---|
| 1 | Preferences dialog | partial | `crates/app/src/preferences.rs` (8 categories: Commenting, Documents, Forms, General, JavaScript, Page Display, Search, Trust Manager), `shell/preferences_dialog.rs` | Acrobat's in-scope categories that are missing: Accessibility, Full Screen, Identity, Reading, Security, Signatures, Spelling, Units & Guides. Each arrives with the package that owns its setting. | yes, as the owning packages land | S (the closeout itself) | WP24, after WP3/6/7/11/18/20/21/22 |
| 2 | File > Export To | partial | M2 export, `plugins/codecs-common` | Office and HTML targets. PLAN.md says these are "partial forever". | no, by plan design (post-1.0) | - | none |
| 3 | App-level accessibility tree | partial | `crates/app/src/a11y/`, `accesskit_macos` only (`crates/app/Cargo.toml`) | Linux and Windows adapters are no-ops. Real VoiceOver acceptance and older modal geometry/focus are open. | partly. The Linux adapter (`accesskit_unix`, MIT/Apache, zbus already locked) can be written and probed under Xvfb with at-spi. Windows needs a Windows host. VoiceOver needs a Mac and a person. | M | WPX (Linux part optional in WP4) |
| 4 | Pinch-to-zoom and stylus pressure | partial | pressure-aware ink shipped (`m3-p9b-ink.md`) | Only the stale note. Real trackpad or stylus hardware has never been exercised. | yes as a scoreboard reconciliation. A hardware check would need a Mac trackpad. | S (docs) | WP0 |
| 5 | Register `.pdf` as openable | partial | `packaging/{macos,linux,windows}`, `crates/app/tests/file_association.rs` | Hosted release and packaged-platform smoke tests. | Linux yes (`desktop-file-validate`, `xdg-mime query` in the container). macOS/Windows need those hosts and hosted CI. | S | WPX |
| 6 | Advanced Search, current document | partial | `chrome/advanced_search/`, `core/src/search.rs`, `content/src/search.rs` | Stemming. | yes: `rust-stemmers` 1.2 (MIT, Snowball) or a hand-written Porter stemmer | S | WP2 |
| 7 | Zoom In / Out / Zoom To | partial | `shell/dialog.rs` (Zoom To, 12 presets) | Typing a custom percentage. | yes | S | WP2 |
| 8 | Full Screen Mode | partial | native entry/exit, chrome hidden (M2-VIEW-ZOOM) | Presentation semantics: one page at a time, click/arrow advance, loop, auto-advance, background colour, cursor hiding, honouring `/PageMode /FullScreen`, and a Full Screen preferences category. | yes | M | WP22 |
| 9 | Attachments pane (list, open, save) | partial | `panes/attachments.rs`, `panes/attachment_menu.rs`, `core/src/attachments.rs` | Open is disabled. The Trust Manager (M5) now says attachments are never opened in another application. So: open a PDF attachment in an Onionskin tab, and keep other types save-only as a stated judgment. | yes | S | WP2 |
| 10 | Signatures pane | partial | `core/src/signatures.rs` (listing), `panes/signatures.rs` (`VALIDATION_NOTE`, "Signed, not checked") | Validation status per signature, Validate All, and opening Signature Properties. | yes | (in WP1) | WP1 |
| 11 | Layers pane context menu | partial | `panes/layers.rs`, `core/src/layers.rs`, read-only Layer Properties dialog | Editable Layer Properties: name, default state, intent, and initial view/print/export state (`/Usage`, `/AS`). Merge and flatten belong to their own post-1.0 row. | yes | M | WP2 |
| 12 | Page canvas / text-selection context menu | partial | `shell/context_menu.rs`, `tabs/export_selection.rs` | Copy With Formatting is disabled. | no: needs a rich-text clipboard entry in the GPUI fork | M | WPX |
| 13 | Export to plain text / accessible text | partial | codecs-common text export, `content::extract` | "Text (Accessible)": tagged documents exported in structure-tree order, with alt text for figures. | yes, once runs carry MCIDs (WP4) | S (after WP4) | WP4 |

### M3 (12 rows, all partial)

| # | Row (short) | Status | Exists today | Gap | Closable here | Size | Pkg |
|---|---|---|---|---|---|---|---|
| 14 | Window menu (Cascade, Tile) | partial | `tabs/windows.rs` | Cascade and Tile. | no: the GPUI fork cannot position a window. On Wayland it is impossible even with a fork change (clients cannot place windows), so Linux support is X11 only. | M (fork) + S (app) | WPX |
| 15 | File > Properties | partial | `chrome/properties_dialog/`, `core/src/metadata/` | The Description tab lacks PDF version, page size, Tagged PDF and Fast Web View. There is no Advanced tab (reading options: language, binding, DisplayDocTitle). The claim that there are five tabs is unconfirmed because no screenshot of the dialog exists. | yes. Confirming the tab list needs your Acrobat Pro reference screenshot. | S+S | WP2 (facts) + WP7 (Advanced tab) |
| 16 | Bookmarks pane context menu | partial | `panes/bookmarks.rs`, `panes/bookmark_edit.rs`, `core/src/outline/` | Wrap Long Bookmarks (a view toggle), Bookmark Properties (style `/F`, colour `/C`, action), New Bookmarks From Structure. | yes | S+S+S | WP2 (wrap, properties), WP4 (from structure) |
| 17 | Attachments pane context menu | partial | `panes/attachment_menu.rs` | Open (see row 9), Edit Description (`/Desc` on the filespec, undoable), Search Attachments (opens Advanced Search with Include PDF Attachments on). | yes | S | WP2 |
| 18 | Copy with formatting / Export selected text | partial | Export Selection As RTF/text | Copy With Formatting to the clipboard. | no: GPUI fork clipboard | (in #12) | WPX |
| 19 | Reorder, preview, remove before combining | partial | `chrome/combine_dialog.rs` | A thumbnail preview in place of the text label. | yes (render worker thumbnails) | S | WP5 |
| 20 | Insert pages (from file, blank, clipboard) | partial | blank + from file (P21 grid) | From clipboard, plus the stale note. | yes: reuse Create PDF From Clipboard (`tabs/create.rs`) | S | WP5 |
| 21 | Renumber pages / page labels | partial | `core/src/pages/{labels,ops}.rs` (`set_page_labels(LabelRange)`, `LabelStyle`), `plugins/tools-organize/src/labels.rs` (only "Number Pages From 1") | The Page Labels dialog: page range, style, prefix, start, and extend numbering. The core already supports all of it. | yes | S-M | WP5 |
| 22 | Export pages to JPEG / JPEG 2000 / TIFF | partial | JPEG and TIFF in codecs-common | JPEG 2000. | yes, if you accept `openjp2` 0.6.1 (BSD-2-Clause, a pure-Rust port of OpenJPEG made with c2rust, heavy `unsafe`). Pillow in the container checks the output. JPEG 2000 patents have expired (legal rule 7). | S | WP23 |
| 23 | Print comments (summary only) | partial | `crates/print/src/appendix.rs` | Printing only the summary, without the document, plus the stale note. | yes | S | WP5 |
| 24 | Dynamic stamps (name, date, time) | partial | `plugins/tools-comment/src/stamp/mod.rs` (UTC) | Local time with its offset. | yes: `localtime_r` through `libc` (already locked) on Unix, `GetDynamicTimeZoneInformation` through `windows-sys` on Windows. Do not use the `time` crate's local offset, which is unsound on multithreaded Unix. | S | WP2 |
| 25 | Duplex | partial | imposition, macOS `PMSetDuplex`, CUPS `sides=` | A sheet out of a real duplex printer, and the macOS backend run. | no: needs a duplex printer and a Mac | - | WPX |

### M4 (3 rows, all partial)

| # | Row | Status | Exists today | Gap | Closable here | Size | Pkg |
|---|---|---|---|---|---|---|---|
| 26 | Print on Windows | partial | GDI backend, type-checked through `onionskin_check_windows` | Has never run on Windows. | no: needs a Windows host and a printer or a PDF printer | - | WPX |
| 27 | Booklet | partial | `crates/print/src/booklet.rs` | Binding list shape (Left, Right, Left Tall, Right Tall), auto-rotate per page, native macOS driver and default-retention acceptance. | the Tall variants and auto-rotate yes. Native acceptance needs a Mac. | S | WP5 + WPX |
| 28 | Poster / tile | partial | `crates/print/src/poster.rs` | Printed tile labels (page, tile and date in the margin). Native units, margins, cut-mark fidelity, Tile-only-large-pages. | labels and Tile-only-large-pages yes. Native fidelity needs a Mac and Acrobat reference captures. | S | WP5 + WPX |

### M5 (13 rows, all partial)

| # | Row | Status | Exists today | Gap | Closable here | Size | Pkg |
|---|---|---|---|---|---|---|---|
| 29 | Edit > Check Spelling | partial | `plugins/spelling` (spellbook, SCOWL en_US), `chrome/spelling_dialog.rs` | Other languages, checking while typing, Change All. | yes. Bundled dictionaries must be licence-checked; alternatively offer "Add dictionary…" for a user's own Hunspell files. | M | WP21 |
| 30 | Edit text (line-level) | partial | `content/src/edit_text.rs`, `core/src/text_edit.rs`, `plugins/tools-edit/src/{text,text_tool}.rs` | System-font matching and embedding, fsType enforcement, characters outside WinAnsi, text in form XObjects, several fonts within one line. | yes: system fonts exist in the container (`fc-list` shows 404) | L | WP10 |
| 31 | Change font, size, colour, alignment, spacing | partial | line editor with standard fonts, size, colour | Alignment, character and word spacing, non-standard fonts. | yes | M | WP12 (fonts from WP10) |
| 32 | Add text (new text box) | partial | Add Text tool, one line | A box that wraps, and fonts beyond the standard 14. | yes | M | WP12 |
| 33 | Edit images and objects | partial | `plugins/tools-edit/src/{image_tool,images}.rs`, `core/src/image_edit.rs` | Crop an image, align several, arrange (stacking order), vector objects (paths), inline images, free-angle rotation. | yes | L | WP19 |
| 34 | Auto-detect form fields | partial | `plugins/tools-form/src/detect.rs` (rules) | Comb boxes, table cells, fields with no line or box. Acrobat's detection is trained, so the scoreboard needs a judgment on how close counts as done. | the comb and table rules yes. Matching Acrobat's detector is not a closable target. | M | WP14 |
| 35 | Button (push button) | partial | `core/src/forms/{author,appearance}.rs`, `chrome/field_dialog/` | Actions (set and run) and icons (a push button's up/down/rollover appearances). | yes. Submit would need a network, so it is left out as a judgment. | M | WP14 |
| 36 | Field properties: General, Appearance, Position, Options, Actions | partial | `chrome/field_dialog/`, `core/src/forms/properties.rs` | Actions tab, font choice, border width and style. | yes. Standard 14 fonts first, embedded ones after WP10. | M | WP14 |
| 37 | Field properties: Format, Validate, Calculate | partial | `core/src/forms/properties.rs`, `plugins/tools-form/src/scripts.rs`, `crates/scripting` | Simplified field notation, and editing the calculation order (`/CO`). | yes | M | WP14 |
| 38 | Tab order / field navigation | partial | fill Tab in `shell/field_editor.rs`, `plugins/tools-form/src/fill.rs` | Honour `/Tabs` (R, C, S), set the tab order while preparing a form, stop on check boxes, radio buttons, list boxes and buttons. | yes | M | WP14 |
| 39 | Fill in a form | partial | `plugins/tools-form/src/{fill,replay}.rs`, `crates/scripting` | Guarantee 7 (values recorded in Acrobat), and keystroke scripts run as each key is typed. | keystroke-as-typed yes. Guarantee 7 needs you to run Acrobat (`corpus/js-forms/README.md`). | M + external | WP14 + WPX |
| 40 | Auto-Complete (Off / Basic / Advanced) | partial | Preferences > Forms, `autocomplete.json` | Advanced (fills in the likeliest entry as you type). | yes | S | WP14 |
| 41 | Redaction properties | partial | `plugins/redact`, `chrome/redact_dialog/` | Fill opacity, and an overlay font other than Helvetica. | opacity now. The font choice can offer the standard 14 now and embedded fonts after WP10. | S | WP12 |

### M6 (33 rows, all planned)

| # | Row | Exists today | Gap | Closable here | Size | Pkg |
|---|---|---|---|---|---|---|
| 42 | View > Show/Hide > Rulers, Grid, Guides, Snap to Grid | nothing (Line Weights sits beside it) | Rulers, grid with its preferences, guides dragged from the rulers, Snap to Grid applied to the drawing tools, Units & Guides preferences. | yes | M | WP20 |
| 43 | View > Read Out Loud | nothing | Platform speech. Acrobat's submenu: Activate, Read This Page Only, Read To End, Pause/Resume, Stop, Deactivate. Reading preferences. | code yes. Linux over SSIP can be smoke-run by installing speech-dispatcher and espeak-ng. Audible acceptance on macOS/Windows needs those hosts. | M | WP18 |
| 44 | Content pane | nothing | A tree of content containers and items in stream order, with highlight, and Create Artifact. | yes | M | WP4 (view), WP13 (edit) |
| 45 | Tags pane | `core/src/structure/read.rs` (reader only) | A structure tree pane: navigate, highlight content, properties. Editing in WP13. | yes | M | WP4, WP13 |
| 46 | Order pane | nothing | Top-level elements per page, drag to reorder. | yes | M | WP16 |
| 47 | Accessibility report pane | nothing | The checker's results pane: Passed / Failed / Needs manual check, with Fix, Skip, Explain, Check Again. | yes | M | WP7 |
| 48 | Security Settings pane | encryption state in `core/src/security.rs`, Properties Security tab | Acrobat's padlock navigation pane on a secured document: the method, what is restricted, and a Permission Details button. Cheap now that password security has landed. | yes | S | WP3 |
| 49 | Reading a tagged PDF with a screen reader | `app/src/a11y/tree.rs` (document node plus page text) | Build the document's accessibility nodes from the structure tree: headings with levels, paragraphs, lists, tables with header cells, figures announced by their alt text, languages. | the tree yes (unit and headless probe). A real screen-reader session needs a Mac and a person. | M | WP4 (+ WPX acceptance) |
| 50 | Edit a signed or certified PDF | incremental save by default | Enforce DocMDP P1/P2/P3 and FieldMDP locks at the edit door. Warn before rewrites (Compress, Reduce, Protect, Remove Security, Apply Redactions) that invalidate signatures. Report a signature as "valid, with allowed changes after it". | yes | M | WP8 (after WP1) |
| 51 | Encrypt with a certificate | nothing | The `/Adobe.PubSec` handler (`adbe.pkcs7.s5`), recipient list with per-recipient permissions, opening with a digital ID. | yes, but RSA decryption needs your call on D1. pyhanko is the independent check; qpdf does not support PubSec. | L | WP17 |
| 52 | Validate an existing signature | `core/src/signatures.rs` (listing only) | CMS parsing, ByteRange digest, signature check, which revision it covers, what changed after it, trust status. | yes | L | WP1 |
| 53 | Signature properties and validation report | nothing | Signature Properties: Summary, Document (revisions, View Signed Version), Signer (certificate viewer), Date/Time, Legal Notice. | yes | M | WP1 |
| 54 | Preserve signature validity across edits | save path is incremental | Guarantee 4 (`crates/app/tests/guarantees.rs:202`, currently `#[ignore]`), a signed corpus, and CI steps that generate it and then run the test with it required. | yes (corpus from pyhanko or openssl, checked with pdfsig) | M | WP1 |
| 55 | Digitally sign with a certificate | `/Sig` field tool (M5), cos keeps a signature's `/Contents` in the clear (`cos/src/encrypt.rs:55`) | PAdES B-B (`ETSI.CAdES.detached`) plus `adbe.pkcs7.detached`. Reserve `/Contents` and `/ByteRange` in the section, digest, sign, patch. Sign an existing field, or draw a new one. | yes. Acrobat cross-check recommended. | L | WP8 |
| 56 | Certify (visible or invisible) | nothing | `/Perms /DocMDP` with a DocMDP transform, P1/P2/P3 chosen in the dialog. | yes | S (on top of WP8) | WP8 |
| 57 | Manage digital IDs | nothing | ID store: digital ID files (PKCS#12), PKCS#11 modules, Keychain (macOS), Windows/CNG IDs. Add, remove, export certificate, set default use. | PKCS#12 and PKCS#11 (SoftHSM2) yes. Keychain and CNG can be written and type-checked here, but running them needs a Mac and a Windows host. | L | WP6 |
| 58 | Create a self-signed digital ID | nothing | New ID wizard: name, organization, email, country, key algorithm, use; saved as PKCS#12 with a password. | ECDSA through `ring` yes. RSA 2048 needs D1. | M | WP6 |
| 59 | Signature appearances | Fill & Sign's scribble in `chrome/signature_dialog/`, which is not cryptographic | Configure Signature Appearance: text shown (name, date, distinguished name, reason, location, labels, logo), graphic (none, name, imported PDF or image, drawn), several named styles kept locally. | yes | M | WP9 |
| 60 | Signature verification preferences | nothing | Verify on open, revocation checking, time basis (current / secure (timestamp) / signing time), and what to do with DocMDP. | yes | S | WP3 (+ revocation in WP11) |
| 61 | Timestamp a document (RFC 3161) | nothing | Signature timestamp token as an unsigned attribute, document timestamp (`/DocTimeStamp`, `ETSI.RFC3161`), timestamp server list. | code yes, against a local `openssl ts` server. Public servers are blocked here (403), so a live check needs a network that reaches one, or a server address you provide. | M | WP11 |
| 62 | LTV enablement | nothing | Gather OCSP responses and CRLs, write `/DSS` (`/Certs`, `/OCSPs`, `/CRLs`, `/VRI`) in an incremental section, validate offline from the DSS. | yes, against a local `openssl ocsp` responder and a local CRL | L | WP11 |
| 63 | Trusted identities | nothing | Trusted certificate store: import, export, trust settings (signatures, certified documents), "Add to Trusted Certificates" from a signature, optional system roots. | yes | M | WP3 |
| 64 | Lock a document after signing | nothing | A `/Lock` on the signature field (FieldMDP: All / Include / Exclude), "Lock document after signing". The edit door then refuses locked fields. | yes | S | WP8 |
| 65 | Check for accessibility | structural checker (`plugins/tools-accessibility/src/checker.rs`) | Acrobat's rule set: 32 checks in 7 groups (Document, Page Content, Forms, Alternate Text, Tables, Lists, Headings), with the manual-check items. | yes | L | WP7 |
| 66 | Accessibility report | nothing | The report in the checker pane, plus the HTML report file Acrobat can write. | yes | S | WP7 |
| 67 | Automatically tag a PDF | nothing | Layout analysis into a new tree. Write `/MarkInfo`, `/StructTreeRoot`, `/ParentTree`, and BDC/EMC into the content streams. Artifact the page marks. | yes | L | WP15 |
| 68 | Fix reading order (Reading Order tool) | nothing | Draw regions and tag them as Text, Figure, Heading N, Table or Background; numbered overlay of the order; Show Order. | yes | L | WP16 |
| 69 | Edit structure with Tags and Content panes | nothing | New tag, change type, move, delete, properties, find untagged content, make artifact. | yes | L | WP13 |
| 70 | Set alternate text | nothing | Set Alternate Text: step through figures, show each one, enter alt text or mark it decorative (made an artifact). | yes | M | WP13 |
| 71 | Set document language | nothing | Catalog `/Lang` (Properties > Advanced > Reading Options) and each tag's language. | yes | S | WP7 |
| 72 | Set document title | Description title exists (P13a) | Title plus `/ViewerPreferences /DisplayDocTitle` (Initial View "Show Document Title"), and the checker's Fix. | yes | S | WP7 |
| 73 | Table editor / table summary | nothing | TH/TD, Scope, RowSpan/ColSpan (`/A /O /Table`), Summary, headers by ID. | yes | M | WP13 |
| 74 | Read Out Loud (from the accessibility toolset) | nothing | Same feature as row 43, reached from the toolset. | as row 43 | (in #43) | WP18 |

Totals: 74 rows (41 partial, 33 planned). Of these, 58 can be fully closed here. 11 need something you have to provide (section 3). 5 can be closed here only in part.

## 2. Work packages, in recommended order

The order puts first what the 1.0 guarantees and the most rows depend on, then alternates M6 work with the earlier milestones' leftovers.

- Signature validation comes first because guarantee 4 is a plan guarantee, needs nothing outside the container, and the trust, signing, timestamp and LTV packages all build on it.
- The structure foundation (WP4) comes early because nine accessibility rows depend on it.
- `text-engine` is split in two so the fonts land before the layout work that uses them.

Each package is one commit series with its own evidence doc in the repo's usual shape (what was built, how it was checked, "Not claimed"), and a scoreboard edit that recounts the totals under `acrobat_parity_headline_matches_every_inventory_row`.

---

### WP0. Housekeeping (S, docs only)
- **Rows:** reconcile #4 (Pinch/stylus → implemented, citing m3-p9b), and the stale notes on #20 and #23.
- **Files:** `docs/evidence/m6-security.md` (new; the six security rows already cite it), `ACROBAT-PARITY.md`, and the PLAN.md M6 status line (password security landed). Guarantee 4's wording says "the UI and MCP report it". MCP moved to post-1.0, so that should say "the UI" until then.
- **Tests:** the parity headline test only.
- **Risk:** none. Committing the scoreboard without the evidence doc leaves six rows citing a file that does not exist.

### WP1. M6 signature validation and guarantee 4 (L, about 2,500 lines) → `docs/evidence/m6-signature-validation.md`
- **Rows:** #52 Validate, #53 Signature properties, #54 Preserve validity (guarantee 4), #10 Signatures pane (M2).
- **Files:**
  - `crates/crypto/src/signature/{mod,cms,verify,algorithms}.rs` (new).
  - `crates/crypto/Cargo.toml`: `cms` 0.2, `x509-cert` 0.2, `der` 0.7, `spki` 0.7, `const-oid`, `sha1` for legacy digests, and either `ring` or RustCrypto `rsa`/`p256`/`p384` per D1. Keep to the der-0.7 family: `cms` 0.3 is still a pre-release and `x509-cert` 0.3 is on der 0.8.
  - `crates/core/src/signatures.rs` (extend), and a new `crates/core/src/signatures/changes.rs`.
  - `crates/app/src/shell/panes/signatures.rs`, and new `crates/app/src/shell/chrome/signature_properties/`.
  - `crates/app/tests/guarantees.rs` (un-ignore guarantee 4), new `corpus/make-signed.py`, `.github/workflows/ci.yml`.
- **Design:**
  - `crypto::signature::verify(contents: &[u8], signed: &mut dyn Read, sub_filter) -> CmsCheck { digest_matches, signature_valid, signer: Certificate, embedded_chain: Vec<Certificate>, claimed_time, algorithms, sub_filter }`.
  - It handles `adbe.pkcs7.detached`, `ETSI.CAdES.detached` and `adbe.pkcs7.sha1`. `adbe.x509.rsa_sha1` is optional (a deprecated legacy format; a judgment call).
  - It checks the `messageDigest` signed attribute and the signature over the DER-encoded signed attributes. RSA PKCS#1 v1.5, RSA-PSS and ECDSA P-256/P-384.
  - `core`: `SignatureField` gains `byte_range`, `contents`, `sub_filter` and `revision` (the index of the section in `cos::Document::sections()` whose end equals the end of the ByteRange).
  - `core::validate(doc) -> Vec<Validation { integrity: Intact | Altered | Malformed(why), coverage: WholeRevision | Partial, later: ChangeSummary, identity: Trusted | Unknown | Invalid(why), mdp: Allowed | Disallowed(Vec<Change>) }>`.
  - `ChangeSummary` compares each later section's xref entries against the signed revision and classifies the objects: annotations added, changed or deleted; form fields filled; pages added; signatures added; other. It then judges them against DocMDP P and FieldMDP. This is also what a signed document edited in Onionskin is reported with.
  - In the app, "View Signed Version" opens the document truncated at that revision. The generations machinery already truncates.
- **Tests:**
  - `corpus/make-signed.py` builds fixtures from a test CA generated at build time: RSA-2048/SHA-256, ECDSA P-256, RSA-PSS, SHA-1 legacy, CAdES and PKCS#7, certified P1/P2/P3, two signatures, signed-then-annotated by another tool, a tampered byte inside the ByteRange, a truncated `/Contents`, a ByteRange that does not cover the file, and an encrypted-then-signed file. pikepdf and openssl are enough to build them; pyhanko is quicker to write with. The corpus is generated, not tracked, so it needs both CI steps.
  - Unit tests in crypto with DER fixtures generated by `openssl cms -sign`.
  - `crates/core/tests/signatures.rs`: our verdict matches `pdfsig -nocert` and pyhanko's `validate` on every fixture.
  - Guarantee 4: for each fixture, add a note through `tools-comment`, save, re-validate (intact, changes allowed), and pdfsig agrees.
  - Headless shell tests for pane statuses and the dialog. Accessibility tree entries for the verdict.
- **Risks:**
  - The D1 crypto choice.
  - BER-encoded (indefinite-length) CMS from old signers: `der` is DER-only, so a small BER-to-DER normaliser for `/Contents` is probably needed.
  - Classifying changes after a signature is where validators disagree. Pin to ISO 32000-2 12.8.2.2 and let pdfsig/pyhanko adjudicate.
  - Until WP3, the signer identity is always "unknown". The pane must say "Validity unknown", never "valid".

### WP2. M2/M3 shell leftovers (M, about 900 lines) → `docs/evidence/m3-shell-leftovers.md`
- **Rows:** #7 Zoom To custom, #6 stemming, #9 and #17 attachments (Open for PDFs, Edit Description, Search Attachments), #16 Wrap Long Bookmarks and Bookmark Properties, #15 Properties Description facts, #24 local time in dynamic stamps, #11 editable Layer Properties.
- **Files:** `shell/dialog.rs`, `chrome/advanced_search/`, `core/src/search.rs`, `panes/{attachments,attachment_menu,bookmarks,bookmark_edit,layers}.rs`, `core/src/{attachments,layers}.rs`, `core/src/outline/`, `chrome/properties_dialog/{model,render}.rs`, `plugins/tools-comment/src/stamp/mod.rs`.
- **Design:**
  - Zoom To gets an editable percentage field, clamped to 1 to 6400.
  - The `core::search` option `stem: bool` compares Snowball stems per word.
  - `core::attachments::set_description(tx, name, text)`. Open decodes the embedded file into a new read-only tab when its bytes start with `%PDF`; anything else is refused with the Trust Manager reason.
  - `core::outline::set_style(tx, item, bold, italic, colour)`.
  - `core::layers::set_properties(tx, ocg, name, intent, default_on, usage)` writes `/OCProperties /D /ON /OFF` and `/AS`.
  - Description facts: header version or catalog `/Version`, current page size, `/MarkInfo /Marked`, and linearization found in the first object.
  - Stamp time: a `local_offset()` helper per platform, formatted as `D:YYYYMMDDHHmmSS+HH'mm'`.
- **Tests:** core unit tests for each writer, with qpdf `--check` and pikepdf read-back of `/Desc`, `/OCProperties` and outline `/F`/`/C`. Headless shell tests for each menu entry, including disabled reasons on a protected document. The stemmer's word pairs. A local-time test with `TZ=Asia/Kolkata` for a half-hour offset.
- **Risks:** Layer Properties writes touch `/OCProperties`, which hayro reads, so render-change tests are needed. Opening an attachment as a document with no path means Save As is the only save.

### WP3. M6 Security Settings pane, trusted identities, verification preferences (M, about 1,200 lines) → `docs/evidence/m6-trust.md`
- **Rows:** #48 Security Settings pane, #63 Trusted identities, #60 Verification preferences (without revocation, which comes in WP11).
- **Files:** `crates/crypto/src/trust/{store,path}.rs` (new), `crates/core/src/signatures.rs`, `crates/app/src/shell/panes/security.rs` (new `NavigationPane::SecuritySettings`, shown only on secured documents as Acrobat does), `crates/app/src/shell/chrome/trusted_identities.rs`, `crates/app/src/preferences.rs` (Signatures category).
- **Design:**
  - `trust::Store`: certificates in an app-data folder with owner-only permissions, plus `trust.json` recording per certificate `{use_for_signatures, use_for_certified}`.
  - `trust::build_path(leaf, pool, anchors, at: SystemTime) -> Result<Path, PathError>`: issuer/subject and AKI/SKI chaining, signature check, validity at the chosen time, `basicConstraints` CA, `keyUsage`. There is no mature RustCrypto path-validation crate, so it is written by hand. `rustls-webpki` is built for TLS server certificates and its EKU and name rules do not fit document signing.
  - Optional "use the system roots": `rustls-native-certs`, already in the lock.
  - The signature panel's "Add to Trusted Certificates".
- **Tests:** chains generated with openssl (expired leaf, missing intermediate, CA flag missing, wrong keyUsage), with the verdict compared to `openssl verify -purpose any -attime`. Fixtures from WP1 turning "unknown" into "valid" once trusted. Pane shell tests on `corpus/encrypted/*.pdf`.
- **Risk:** the time basis must default to Acrobat's (current time). "Signing time" is claimed by the signer, so trusting it must be a stated choice.

### WP4. M6 structure foundation: Tags and Content panes (view), accessible text, screen-reader tree (L, about 2,200 lines) → `docs/evidence/m6-structure-panes.md`
- **Rows:** #45 Tags pane (view and navigate), #44 Content pane (view), #13 accessible text export (M2), #49 screen reader, #16 New Bookmarks From Structure (M3). Optional: the Linux AccessKit adapter (#3).
- **Files:**
  - `crates/content/src/interpret.rs` (a marked-content stack on every run, image and path), `content/src/{run,placements,shapes}.rs`.
  - `crates/core/src/structure/read.rs` (`Element` gains `alt`, `actual_text`, `lang`, `title`, `expansion`, `attributes`, and a resolved `standard_type` through `/RoleMap`), new `core/src/structure/content_map.rs`.
  - `plugins/codecs-common` (text export), `crates/app/src/shell/panes/{tags,content}.rs`, `crates/app/src/a11y/tree.rs`, `core/src/outline/`.
- **Design:**
  - `content::MarkedRef { mcid: Option<i64>, tag: Name, artifact: bool, depth }` on each item.
  - `core::structure::content_of(doc, element) -> Vec<(page, item bounds, text)>`, and `reading_order(doc) -> Vec<Block>`: a depth-first walk with RoleMap resolved.
  - The panes are lazy trees in the style of `panes/bookmarks.rs`. "Highlight Content" draws the element's quads as a canvas overlay.
  - The export picks structure order when the document is tagged and document order otherwise.
  - The accessibility tree maps H1 to H6 to `Role::Heading` with a level, P to `Paragraph`, L/LI to `List`/`ListItem`, Table/TR/TH/TD to table roles, Figure to `Image` with alt text as its label, and Lang to the node's language.
- **Tests:**
  - Over the 432 tagged PDF/UA files (`corpus/tagged`, veraPDF): every MCID the tree names is found in content.
  - Text order compared with `pdftotext` for untagged documents. For tagged ones, compared with pikepdf walking the StructTreeRoot in a small Python script.
  - a11y tree unit tests on hand-made tagged fixtures. Headless pane tests.
  - Guarantee 8 keeps running: nothing here writes, except New Bookmarks From Structure, which is graded by the checker.
- **Risk:** marked content that spans text operators, nested BDC inside form XObjects (`/StructParents` on forms is already listed as not claimed), and speed on large pages. The MCID stack must cost nothing when not asked for, like `page_mcids`.

### WP5. M3/M4 organize and print leftovers (M, about 900 lines) → `docs/evidence/m3-organize-print-leftovers.md`
- **Rows:** #20 Insert from clipboard, #21 Page Labels dialog, #19 Combine thumbnails, #23 summary-only printing, #28 poster labels and Tile-only-large-pages, #27 booklet Tall variants and auto-rotate.
- **Files:** `shell/organize/view.rs`, `tabs/page_grid.rs`, `tabs/create.rs`, new `chrome/page_labels_dialog.rs`, `plugins/tools-organize/src/labels.rs`, `chrome/combine_dialog.rs`, `crates/print/src/{appendix,poster,booklet,job}.rs`, `chrome/print_dialog/`.
- **Design:**
  - `OrganizeAction::InsertFromClipboard` reads the pasteboard once, as Create does, then goes through `codecs-common` import into `insert_pages_from`.
  - The dialog builds `Vec<LabelRange>`, merged with the existing ranges ("extend numbering" drops the entry).
  - `JobContent::SummaryOnly`.
  - `PosterSettings { labels: bool, tile_only_large: bool }`, with the label text drawn in the margin through the sheet renderer.
  - `Binding::{Left, Right, LeftTall, RightTall}`, `auto_rotate: bool`.
- **Tests:** imposition unit tests (pure functions), file-backend output checked with pdfinfo and pypdf (page count, rotation, label text found by `pdftotext`), page labels read back by pypdf's `page_labels`, a headless Combine thumbnail test, and the CUPS capture harness from `m4-cups.md` for summary-only.
- **Risk:** the printed poster label format is Acrobat's and can be matched only against a reference capture. Say so under "Not claimed" if none is available.

### WP6. M6 digital IDs (L, about 1,800 lines) → `docs/evidence/m6-digital-ids.md`
- **Rows:** #57 Manage digital IDs, #58 Create a self-signed ID. The Security Settings console (Digital IDs, PKCS#11 Modules) lives in this dialog.
- **Files:**
  - `crates/crypto/src/identity/{mod,pkcs12,pkcs11,keychain,cng,selfsigned}.rs` (new).
  - `plugins/tools-protect/src/lib.rs` (first real registrations: Manage Digital IDs, Add Digital ID).
  - `crates/app/src/shell/chrome/digital_ids/` (new), `preferences.rs` (Identity category: name, organization, email, which feed the self-signed ID and dynamic stamps).
- **Design:**
  - `trait Identity { fn chain(&self) -> Vec<Certificate>; fn sign(&self, digest: &[u8], alg: SigAlg) -> Result<Vec<u8>>; fn decrypt_key(&self, wrapped: &[u8]) -> Result<Vec<u8>> /* WP17 */ }`.
  - PKCS#12 read and write: PBES2 with PBKDF2 and AES-256 through RustCrypto `pkcs5`/`pbkdf2`, plus reading the legacy `pbeWithSHAAnd3-KeyTripleDES` (`des`, MIT/Apache) because Windows and older tools still export it. The RustCrypto `pkcs12` crate is only at `0.2.0-pre`.
  - PKCS#11 through `cryptoki` 0.10 (Apache-2.0; it loads the module at run time, so no C build).
  - Keychain through `security-framework` (already locked) and CNG through `windows-sys` NCrypt, both behind `cfg`. CNG is checked with the repo's existing `onionskin_check_windows` pattern.
  - Self-signed IDs: `x509-cert` builder, key through `ring` (ECDSA P-256/P-384), RSA 2048 per D1. Acrobat's default is RSA 2048, so ECDSA-only is a stated gap if D1 is "no rsa".
- **Tests:** round trip our .p12 through `openssl pkcs12 -info` and back. Import openssl-made .p12 files (AES and 3DES). Sign a digest with SoftHSM2 through the PKCS#11 path (`apt install softhsm2` in CI). A self-signed certificate parsed by `openssl x509 -text`. Headless dialog tests.
- **Risks:** secrets on disk (owner-only files, zeroize key buffers), and PIN prompts (a modal with no echo, like the password dialog). The Keychain and CNG paths stay unverified. Following the precedent of the Windows print row, #57 stays `partial` until they run on those hosts.

### WP7. M6 accessibility checker and report, title, language (L, about 1,600 lines) → `docs/evidence/m6-accessibility-checker.md`
- **Rows:** #65 Check, #66 Report, #47 Accessibility report pane, #71 language, #72 title, #15 Properties Advanced tab (M3).
- **Files:** `plugins/tools-accessibility/src/{checker.rs, rules/*.rs, report.rs, fix.rs}`, new `crates/app/src/shell/panes/accessibility.rs`, `chrome/properties_dialog/`, `core/src/metadata/view.rs` (DisplayDocTitle), `preferences.rs` (Accessibility category).
- **Design:**
  - `Rule { id, group, name, check: fn(&Ctx) -> Outcome::{Passed, Failed(Vec<Where>), Manual} , fix: Option<fn(&mut Transaction)> }` over Acrobat's 32 checks. The existing structural findings become the "Tagged content" and "Tagged annotations" rules.
  - Fixes: Title, Primary language, Tab order (sets `/Tabs /S` on every page), Tagged PDF (points to Autotag in WP15), Accessibility permission (points to Remove Security, or to Protect with the screen-reader permission).
  - Report: the pane plus `write_html(path)`.
- **Tests:**
  - Rules over the veraPDF PDF/UA-1 pass and fail files, whose names say which Matterhorn checkpoint they break.
  - Map Acrobat rules to Matterhorn checkpoints and assert each failing file fails the matching rule.
  - Guarantee 8 now uses the full checker.
  - Language and title read back with pikepdf (`/Lang`, `/ViewerPreferences`).
- **Risk:** Acrobat's rule wording and verdicts can only be matched by observing it. Where not observed, pin to Matterhorn and say so.

### WP8. M6 signing, certify, lock, edit signed/certified (L, about 2,000 lines) → `docs/evidence/m6-signing.md`
- **Rows:** #55 sign, #56 certify, #64 lock, #50 edit a signed or certified PDF.
- **Files:**
  - `crates/cos/src/writer.rs` and `document.rs`: a placeholder hex string written with a fixed width, and `section_for` returning placeholder offsets.
  - New `crates/core/src/sign.rs`, `core/src/security.rs` (the edit door gains the signature policy), `core/src/save.rs`, `crates/crypto/src/signature/build.rs`.
  - `plugins/tools-protect` (the Digitally Sign tool drags a rectangle; Certify (visible or invisible) commands), `tabs/signature.rs` (clicking an unsigned `/Sig` field with the Hand tool signs it), a new sign dialog in `chrome/`.
- **Design:**
  - `core::sign::prepare(tx, target: FieldRef | NewField{page, rect}, SignOptions { reason, location, contact, certify: Option<P>, lock: Option<Lock>, appearance }) -> Prepared`.
  - `save::sign_to_path(session, dest, identity) -> Result<SaveOutcome>`:
    1. Build the section with `/Contents <00…>` reserved (the chain size, plus 8 KB, plus the timestamp reserve) and `/ByteRange [0 ########## ########## ##########]`.
    2. Write it to a temporary file.
    3. Digest the two ranges and build CMS SignedData with signed attributes `contentType`, `messageDigest` and `signingCertificateV2` (no `signingTime` under PAdES).
    4. Call `identity.sign`, patch both placeholders, and publish atomically.
  - Signing is a save, not an undoable edit, and Acrobat always asks for Save As.
  - Certify writes `/Perms /DocMDP` plus `/Reference [<< /TransformMethod /DocMDP /TransformParams << /P n /V /1.2 >> >>]`.
  - Lock writes the field's `/Lock` and the FieldMDP reference.
  - Edit door: `Policy::from_signatures(doc)` refuses a change P forbids, with the reason shown on each disabled entry, as the `/P` enforcement does. Rewriting commands (Compress, Reduce, Protect, Remove Security, Apply Redactions) warn that signatures will be invalidated.
- **Tests:**
  - Our signatures validate in pdfsig and in pyhanko (strict).
  - Our WP1 validator accepts them.
  - Countersign a fixture signed by another tool and both still validate.
  - Certify P1, then try each edit: refused. P2 allows filling, P3 allows commenting.
  - qpdf `--check` on every output.
  - Sign an encrypted document (`/Contents` in the clear).
  - Headless dialog tests.
- **Risks:** reserve too small (Acrobat reserves generously; fail loudly and retry larger), the section's own trailer and xref must stay outside the ByteRange hole, and whether Acrobat accepts our signatures is only proved by opening them in Acrobat Reader (recommended once).

### WP9. M6 signature appearances (M, about 800 lines) → `docs/evidence/m6-signature-appearances.md`
- **Row:** #59.
- **Files:** new `crates/core/src/sign/appearance.rs`, `chrome/signature_appearance_dialog.rs`, reuse of `plugins/tools-fill-sign` image and PDF import, app-data `signature-appearances.json`.
- **Design:** `Appearance { graphic: None | Name | Imported(PdfPage) | Drawn, text: TextSet { name, date, dn, reason, location, labels, logo }, font: Standard(Helvetica) }` produces the `/AP /N` form XObject (no n0 or n2 layers). Several named appearances, chosen in the sign dialog.
- **Tests:** appearance-stream unit tests (text found by `pdftotext`, bbox), pdfsig still validating, a render check through `render`.
- **Risk:** fonts are the standard 14 until WP10. Non-Latin names are drawn as `?`, which goes under "Not claimed" until WP12 re-points it.

### WP10. M5 text-engine I: system fonts, embedding, fsType (L, about 2,500 lines) → `docs/evidence/m5-text-engine.md`
- **Row:** #30 Edit text. It also unlocks #31, #32, #36, #41, #59 fonts.
- **Files:** `crates/text-engine/src/{lib,system,matching,embed,subset,tounicode,fstype}.rs`, `crates/content/src/edit_text.rs`, `core/src/text_edit.rs`, `crates/content/src/font/*` (move off `ttf-parser` to `skrifa`/`read-fonts`, which drops the direct use behind `RUSTSEC-2026-0192`), `deny.toml` comment.
- **Design:**
  - `SystemFonts::scan()` through `fontdb` (MIT/Apache), and `match_font(base_font_name, family, weight, italic, needs: &str) -> Match`.
  - `embed(font, glyphs) -> Type0 { CIDFontType2 | CIDFontType0C, Identity-H, W, ToUnicode, FontFile2 | FontFile3 }`, with subsets from `subsetter` 0.2 (MIT/Apache, typst).
  - `fstype(os2) -> Allowed | PreviewOnly | Restricted | NoSubsetting | BitmapOnly`. Restricted is refused with the reason. NoSubsetting embeds the whole font.
  - Several fonts within one line: rewrite each run in its own font.
  - Text inside form XObjects: rewrite the form's stream, under the same rule as page streams.
  - Shaping: none for 1.0 (Latin, Cyrillic, Greek by cmap plus kerning). Complex scripts are the post-1.0 bidi row. If shaping is wanted, use `harfrust` rather than `rustybuzz`, whose advisory exception is argued only for gpui's use.
- **Tests:**
  - Edit lines in `corpus/organize/embedded-font.pdf` and the hayro corpus with characters outside WinAnsi.
  - `pdffonts` (poppler, if installed; else pikepdf) shows `Identity-H`, embedded and subset.
  - `pdftotext` extracts the new text, which checks ToUnicode.
  - A restricted font (build one with fontTools if installed, or patch `fsType` bytes) is refused.
  - Guarantee 8 still passes for Edit Text.
- **Risks:** font discovery differs per platform (fontconfig, CoreText and DirectWrite paths). fontdb covers all three, but only Linux runs here. CFF-to-CID conversion. Subsetting corner cases, which is why the output is checked with poppler.

### WP11. M6 timestamps, revocation, LTV (L, about 2,000 lines) → `docs/evidence/m6-timestamps-ltv.md`
- **Rows:** #61 timestamps, #62 LTV, and the revocation half of #60. Time Stamp Servers joins the Security Settings console.
- **Files:** `crates/crypto/src/{tsp,revocation}.rs` (`x509-tsp` 0.1, `cmpv2` 0.2, `x509-ocsp` 0.2, `x509-cert` crl), new `crates/core/src/dss.rs`, `core/src/sign.rs`, HTTP client `ureq` 3 (MIT/Apache, rustls already locked, honours the proxy environment), `chrome/timestamp_servers.rs`, `preferences.rs`.
- **Design:**
  - `tsp::request(url, digest) -> TimeStampToken`, checking the message imprint, nonce and TSA chain.
  - Signature timestamps go into the unsigned `id-aa-signatureTimeStampToken` attribute (reserve the space in WP8).
  - `core::sign::document_timestamp(...)` writes `/DocTimeStamp`.
  - `revocation::check(cert, issuer, sources: Embedded | DSS | Online) -> Good | Revoked(at) | Unknown`.
  - `dss::add_verification_info(tx, validations)` writes `/DSS` with `/VRI` keyed by the SHA-1 of each signature's `/Contents`. It is offered as "Add Verification Information" in the signature panel's menu.
  - Validation picks the secure time when a timestamp is present.
- **Tests:**
  - A local TSA: `openssl ts -reply` behind a std `TcpListener` test server serving `application/timestamp-reply`.
  - A local `openssl ocsp -index -port` responder, and CRLs from `openssl ca -gencrl` served over HTTP.
  - Revoked and good cases.
  - pyhanko's `--retroactive-revinfo` and pdfsig agree. A pyhanko LTV check (`ltvaudit`) on our DSS output.
- **Risk:** no live public TSA is reachable from here. The implementation is proved against RFC 3161 locally, and a live check is a user-gated item. Offline-first: nothing fetches unless the user asks or the preference allows it (the privacy position).

### WP12. M5 text-engine II: layout, fonts elsewhere, redaction properties (M, about 1,000 lines) → `docs/evidence/m5-text-layout.md`
- **Rows:** #31 alignment and spacing, #32 wrapping Add Text box, #41 redaction fill opacity and overlay font.
- **Files:** `crates/text-engine/src/layout.rs` (greedy line breaking with `unicode-linebreak`, already locked), `plugins/tools-edit/src/text_tool.rs`, `shell/line_editor.rs`, `plugins/redact`, `chrome/redact_dialog/`, and optionally re-pointing WP9 appearances and page-mark fonts.
- **Tests:** layout unit tests (widths from font metrics), rendered bounds, redaction with an overlay font verified by the redaction verifier and `pdftotext`, fill opacity through the `/ca` ExtGState read back with pikepdf.

### WP13. M6 alt text, Tags and Content pane editing, table editor (L, about 2,200 lines) → `docs/evidence/m6-tag-editing.md`
- **Rows:** #69 Tags/Content editing, #70 alt text, #73 table editor, and the edit halves of #44 and #45.
- **Files:** `core/src/structure/{maintain,edit}.rs` (new mutations), `crates/content` (make an artifact: wrap operators in `/Artifact BMC … EMC` and drop their MCIDs), `panes/{tags,content}.rs`, new `chrome/alt_text_dialog.rs`, `chrome/table_editor/`.
- **Design:**
  - `structure::{new_element, retag, move_element, delete_element, set_attr(Attr::Alt|ActualText|Lang|Title|TableScope|RowSpan|ColSpan|Summary|Headers)}`, each one transaction.
  - Decorative figure means make it an artifact, then delete the element.
  - Table editor: a grid view over a Table element's rows and cells.
- **Tests:** every mutation graded by the WP7 checker (nothing new broken). Read back with pikepdf. The veraPDF table and figure fixtures before and after. Headless pane tests.
- **Risk:** marked-content edits inside TJ runs need the same splitting `edit_text` does. Scope Content-pane editing to making artifacts and deleting; reordering content streams is low value.

### WP14. M5 forms remainder (L, about 1,800 lines) → `docs/evidence/m5-forms-remainder.md`
- **Rows:** #35 Button, #36 General/Appearance/Actions, #37 simplified field notation and calculation order, #38 tab order, #40 Auto-Complete Advanced, #34 comb and table detection, and #39 keystrokes as typed (not guarantee 7).
- **Files:** `core/src/forms/{properties,author,appearance,write}.rs`, `plugins/tools-form/src/{fill,detect,scripts,prepare}.rs`, `crates/scripting/src/{lib.rs,prelude.js}`, `chrome/field_dialog/`, `shell/field_editor.rs`, new `chrome/tab_order_dialog.rs` and `calculation_order_dialog.rs`.
- **Design:**
  - Actions (`/A`, `/AA`): Go to a page view, Open a web link (through the Trust Manager), Reset a form, Show/hide a field, Run a JavaScript (forms subset), Execute a menu item (a whitelist of registry commands, as a judgment). Submit is not offered.
  - Button icons are `/MK /I /RI /IX` with layout options.
  - Border width 1, 2 or 3 and style S, D, B, I or U.
  - The simplified field notation parser emits the JavaScript Acrobat writes: `AFSimple_Calculate` is not used; it writes an `event.value = …` expression with `AFMakeNumber` calls (observe one Acrobat sample to pin it).
  - Calculation order edits `/CO`. Tab order reads `/Tabs` R/C/S or the structure order, and the prepare dialog writes it.
- **Tests:** unit tests per action, run through the Hand tool in headless tests, pikepdf read-back, and scripting tests in `crates/scripting/tests/forms_api.rs`.
- **Risk:** the exact notation-to-JavaScript form is Acrobat's own output and needs one observed sample. Guarantee 7 still blocks #39 whatever this package does.

### WP15. M6 marked-content writer and autotag (L, about 2,500 lines) → `docs/evidence/m6-autotag.md`
- **Rows:** #67.
- **Files:** `crates/content/src/tag.rs` (new: wrap operator spans in BDC/EMC with an MCID, splitting TJ at glyph boundaries), `core/src/structure/create.rs` (build `StructTreeRoot`, `ParentTree`, `MarkInfo`, `StructParents` from nothing), `plugins/tools-accessibility/src/autotag/{layout,classify}.rs`.
- **Design:**
  - Layout: `content::text_lines`, then blocks by leading and indent, then classes. Headings are ranked by relative font size and weight into H1 to H6. Lists come from bullet or number glyphs followed by an indent (L, LI, Lbl, LBody). Images become Figure with empty alt, flagged for WP13. Tables come from aligned column gaps, marked "Needs manual check". Page marks from WP-M5 are already artifacts.
  - Refuse on an already-tagged document unless the user asks to replace its tags, as Acrobat does.
- **Tests:** autotag untagged corpus files, then the WP7 checker shows no "Tagged content" failures. pikepdf structure walk. Headings compared with a hand-labelled set of about 10 documents in `corpus/tagged/autotag/` (expected tags written by hand, which is not an Acrobat observation). Guarantee 8.
- **Risk:** heuristics quality. Acrobat's autotagger is itself imperfect, so the scoreboard row should state the rule set as the scope.

### WP16. M6 Reading Order tool and Order pane (M-L, about 1,300 lines) → `docs/evidence/m6-reading-order.md`
- **Rows:** #68, #46.
- **Files:** `plugins/tools-accessibility/src/reading_order.rs` (a tool: `PointerInput` drag gives a region, then the side panel's tag buttons), `panes/order.rs`, `core/src/structure/edit.rs` (reorder `/K`), reuse of `content/src/tag.rs`.
- **Tests:** headless gesture tests (drag a region, tag it as Heading 1, and the element and MCIDs exist), Order pane drag reorders the tree (checked with pikepdf), guarantee 8.

### WP17. M6 certificate encryption (L, about 1,300 lines) → `docs/evidence/m6-certificate-security.md`
- **Row:** #51.
- **Files:** `crates/crypto/src/pubsec.rs` (EnvelopedData via `cms` 0.2 with KeyTransRecipientInfo RSA PKCS#1 v1.5, content AES-256-CBC), `crates/cos/src/{decrypt,encrypt}.rs` (handler dispatch on `/Filter`), `core/src/protection.rs`, `chrome/protect_dialog.rs` (the "Encrypt with Certificate" path), the open path through `password_dialog.rs` (choose a digital ID).
- **Design:** `/Filter /Adobe.PubSec /SubFilter /adbe.pkcs7.s5 /V 5` with `/CF /DefaultCryptFilter << /CFM /AESV3 /Recipients [...] >>`. The file key is SHA-256 over the seed, each recipient blob, and `0xFFFFFFFF` when metadata is not encrypted (ISO 32000-2 7.6.5.3). Permissions are per recipient.
- **Tests:** pyhanko decrypts ours and we decrypt pyhanko's. qpdf reports it cannot open the document (no PubSec support), which is the expected result, not a failure. Round trip through incremental save of an allowed change.
- **Risk:** D1 (RSA decryption).

### WP18. M6 Read Out Loud (M, about 900 lines) → `docs/evidence/m6-read-out-loud.md`
- **Rows:** #43, #74.
- **Files:** `plugins/tools-accessibility/src/speech/{mod,ssip,avspeech,sapi}.rs`, View menu entries in `tabs/menu.rs`, `preferences.rs` (Reading category: voice, rate, pitch, volume, read form fields, reading order: infer / left-to-right / use the structure order).
- **Design:**
  - `trait Speech { speak(&str, Voice, rate); pause; resume; stop; events() }`.
  - Linux uses a hand-written SSIP client over `$XDG_RUNTIME_DIR/speech-dispatcher/speechd.sock` (about 300 lines). Avoid `tts` 0.26, which links `speech-dispatcher-sys`, a C library needed at build time.
  - macOS uses `objc2-avf-audio` (MIT, Zlib or Apache-2.0).
  - Windows uses SAPI through `windows` (already locked).
  - Text comes from WP4's reading order when tagged, otherwise content lines.
  - Acrobat keystrokes: Shift+Cmd+Y, V, B, C, E.
- **Tests:** a fake `Speech` records utterances (page-only, to end, pause and stop state machine). The SSIP client is tested against a fake Unix-socket server. An optional smoke run with speech-dispatcher, espeak-ng and a null audio device.
- **Risk:** actually hearing it on macOS/Windows is user-gated.

### WP19. M5 edit objects (L, about 2,000 lines) → `docs/evidence/m5-objects.md`
- **Row:** #33.
- **Files:** `plugins/tools-edit/src/{image_tool,images}.rs` plus a new `object_tool.rs`, `core/src/image_edit.rs`, `crates/content/src/{placements,shapes}.rs` (inline images; path objects with their graphics state isolated).
- **Design:** crop as `q x y w h re W n … Do Q`. Multi-select, then align left/centre/right/top/middle/bottom. Arrange by moving `q…Q` groups within the stream. A vector object is a path-painting sequence plus the state it needs, wrapped in `q cm … Q` to move it.
- **Tests:** geometry unit tests, render diff before and after (rendered through `render`; gs as an independent renderer), guarantee 8 for tagged pages, qpdf `--check`.

### WP20. M6 rulers, grid, guides, snap (M, about 900 lines) → `docs/evidence/m6-rulers-grid.md`
- **Row:** #42.
- **Files:** `crates/app/src/shell/canvas.rs` (overlays through the coordinate-mapping layer), `shell/canvas/` (new rulers module), `crates/plugin-api` (a snap-to-grid step before tools see `PointerInput`; the Measure snapping hook is the model), `preferences.rs` (Units & Guides: units, grid spacing, offset, subdivisions, colour, grid on top).
- **Tests:** mapping unit tests under rotation and crop boxes (the M2 correctness risk), a snapped field-tool drag in the headless gesture test, and preference persistence.

### WP21. M5 spelling languages and as-you-type (M, about 700 lines) → `docs/evidence/m5-spelling-languages.md`
- **Row:** #29.
- **Files:** `plugins/spelling/src/lib.rs` (`Checker::for_language`), `plugins/spelling/dictionary/*`, `chrome/spelling_dialog.rs` (language list, Change All), `shell/{inline_text,field_editor}.rs` (squiggles and suggestions), `preferences.rs` (Spelling category).
- **Dictionary licences** (data, not crates, so `deny.toml` does not cover them; record them in the package):
  - Bundle: SCOWL en_GB, en_CA, en_AU; fr (Grammalecte, MPL-2.0); es (RLA-ES, tri-licensed with MPL-2.0).
  - Do not bundle GPL-only ones: de (igerman98), it.
  - Also offer "Add dictionary…" for a user's own Hunspell pair.
- **Tests:** spellbook over each bundled dictionary with known words, as-you-type state tests, headless dialog tests.

### WP22. M2 Full Screen presentation (M, about 600 lines) → `docs/evidence/m2-full-screen.md`
- **Row:** #8.
- **Files:** the Full Screen code in `crates/app/src/shell/` (M2-VIEW-ZOOM), `shell/initial_view.rs` (honour `/PageMode /FullScreen` and `/NonFullScreenPageMode`, which also removes the Initial View row's caveat), `preferences.rs` (Full Screen category).
- **Tests:** headless state tests (advance, loop, Escape, timer with a fake clock).

### WP23. M3 JPEG 2000 export (S, about 250 lines) → note added to `m3-p14a-images.md`, or its own doc
- **Row:** #22.
- **Files:** `plugins/codecs-common` (the JPEG 2000 codec entry and its settings).
- **Tests:** Pillow opens the output and compares it with the PNG export within a PSNR bound. `pdfimages -list` for PDF round trips if used.
- **Risk:** `openjp2`'s `unsafe` port. Run the codec's tests under Miri if feasible, and fuzz the encoder inputs lightly. Needs your OK (D4).

### WP24. M2 Preferences closeout and final recount (S) → the 1.0 scoreboard reconciliation
- **Row:** #1, after every package that adds a category.
- **Work:** check the category list against Acrobat's, mark the out-of-scope categories (Adobe Online Services, Email Accounts, Internet, Multimedia, 3D, Tracker, Updater, Language (UI localization is post-1.0)) as such in the row, update PLAN.md's M5/M6 status lines, and recount.

## 3. What cannot be closed here, and what you would need to provide

| Row(s) | Blocker | What you would provide |
|---|---|---|
| #26 Print on Windows | Never run on Windows | A Windows machine (or VM) with a printer or Microsoft Print to PDF, and one session running the tests and a print. |
| #25 Duplex; #27 Booklet and #28 Poster native acceptance | Needs the macOS backend run (P16 is outstanding) and a duplex printer | A Mac, a duplex-capable printer, and Acrobat Pro reference captures of the Booklet and Poster panels and a printed tile label. |
| #39 Fill in a form (guarantee 7) | Expected values have to be recorded in Acrobat (`corpus/js-forms/README.md`) | A licensed Acrobat and a few hours replaying the scenario files to produce `*.expected.json`. |
| #3 App accessibility tree | Real VoiceOver acceptance; Windows adapter | A Mac session with VoiceOver (a person listening), and a Windows host for `accesskit_windows`. The Linux adapter can be done here. |
| #5 Register `.pdf` | Hosted release and packaged smoke tests on macOS/Windows | Hosted CI runners (or machines) and the signing/notarization credentials the release workflow uses. |
| #14 Window Cascade/Tile; #12 and #18 Copy With Formatting | The GPUI fork has no window placement and no rich-text clipboard | Either permission to create and host your own fork (e.g. `github.com/cristim/gpui`, added to `deny.toml`'s `allow-git`, as was done for hayro), or acceptance that these stay partial. Tile/Cascade cannot work on Wayland in any case. |
| #57 Manage digital IDs (Keychain, CNG parts) | Platform keystores | A Mac with a Keychain identity and a Windows host with a certificate-store identity. PKCS#12 and PKCS#11 are closable here. |
| #61 Timestamps (live) | Public timestamp servers are blocked by this proxy (403) | Either an allowlisted timestamp URL (DigiCert, Sectigo, freetsa) or your organization's TSA address. The implementation itself is proved locally. |
| #43 and #74 Read Out Loud (audible acceptance) | No audio here; the macOS and Windows speech APIs | One Mac run and one Windows run listening to the output. |
| #15 Properties tab list | No reference capture | An Acrobat Pro screenshot of File > Properties (kept private under `parity/reference/`). |
| #2 File > Export To | Partial by plan design | Nothing; leave it partial. |

**Decisions needed from you** (each blocks or shapes a package):

- **D1. RSA primitives** (blocks WP1, WP6, WP8, WP17). The options:
  - (a) Use `ring` (already in the lock and allowed by deny) for all signature verification and signing, and accept that it is not the RustCrypto family `crypto` uses today. RSA key generation and RSA decryption stay unsolved, so self-signed IDs would be ECDSA only and certificate encryption could not be opened with RSA IDs.
  - (b) Add RustCrypto `rsa` 0.9 with an advisory exception for RUSTSEC-2023-0071. That contradicts the current `deny.toml` rule.
  - (c) Wait for `rsa` 0.10 stable (currently `rc.18`), or pin the rc.

  My recommendation: (a) now for WP1 and WP8, and (c) for key generation and WP17, so WP17 goes last among the M6 security packages.
- **D2. Legacy formats:** whether to validate `adbe.x509.rsa_sha1` and SHA-1 signatures. Acrobat still shows them; validating them is safer for users than calling them unknown.
- **D3. Online checks:** whether revocation checking and timestamping may go to the network by default, or only when asked (the privacy position suggests only when asked).
- **D4. JPEG 2000:** accept `openjp2` (BSD-2-Clause, a transpiled pure-Rust port with heavy `unsafe`) or keep the row partial.
- **D5. Spelling:** which dictionary licences are acceptable to bundle.
- **D6. Acrobat cross-checks:** pdfsig and pyhanko are the independent checks here, but an interoperability claim ("Acrobat reads our signatures, certificate-encrypted files and DSS") needs one Acrobat Reader run on your Mac. It is recommended before 1.0, and black-box observation is lawful under legal rule 1.

## Appendix: the Measure rows treated as done
Scale ratio and units (#M6) and 2D snap settings (#M6) are still `partial` on the scoreboard. Per `m6-measure.md`, what is left is:
- a field for typing any scale,
- a choice of precision,
- snap sensitivity and hint colour settings,
- reading `/VP` scales from the document,
- re-measuring after a vertex is moved.

They are out of this plan as you asked. If you want the scoreboard to read `implemented`, the leftovers are about S each. The "Measuring (2D)" preferences category could sit with WP20 (Units & Guides).

### Critical Files for Implementation
- /home/claude/onionskin/crates/crypto/src/lib.rs (and new `signature/`, `trust/`, `identity/`, `tsp.rs`, `pubsec.rs` modules)
- /home/claude/onionskin/crates/core/src/signatures.rs (with `core/src/security.rs` and `core/src/save.rs` for signing and the DocMDP door)
- /home/claude/onionskin/crates/cos/src/writer.rs (reserved `/Contents` and `/ByteRange` placeholders with their offsets)
- /home/claude/onionskin/crates/core/src/structure/read.rs (with `crates/content/src/interpret.rs` for the marked-content attribution every accessibility row needs)
- /home/claude/onionskin/crates/text-engine/src/lib.rs (empty today; every M5 text row depends on it)
- /home/claude/onionskin/crates/app/tests/guarantees.rs (guarantee 4, currently `#[ignore]` at line 201)