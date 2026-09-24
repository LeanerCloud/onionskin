# M5 verification: links, and the Trust Manager

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`:**
  - Edit a PDF: Add or edit links; Auto-create links from URLs; Remove web
    links.
  - Protect: Trust Manager: allow or block links and attachment opening.
- **Also changed:** the page canvas context menu row, whose Create Link is
  now live. The row stays `partial`: Copy With Formatting, Edit Text and
  Redact Text are still disabled.
- **Headline:** 142 planned / 30 partial / 80 out-of-scope, 151
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **The Link tool** (tool rail). While it is chosen, every link on the page
  is outlined, the invisible ones included.
  - Drag a rectangle and **Create Link** opens.
  - Click a link and **Link Properties** opens on it.
- **Create Link from text.** Select text, then use the canvas context
  menu's **Create Link** to open Create Link over the selection. With
  nothing selected, the entry chooses the Link tool.
- **The dialog.** A link can:
  - go to a page, typed from 1 and opening on the page on screen;
  - open a web page (`example.com` is given `http://`);
  - open a chosen file;
  - keep an action the dialog does not write. A link that runs JavaScript
    can still have its look changed.

  The look has a Visible Rectangle switch with Line Thickness (Thin,
  Medium, Thick), Line Style (Solid, Dashed, Underline) and Colour, plus a
  Highlight Style (None, Invert, Outline, Inset). Link Properties adds
  **Delete Link**. A page number or address the dialog cannot use is said
  in the dialog, which stays open.
- **Following links.** A click with the Hand tool on a link follows it. A
  drag, even one that comes back to where it started, does not.
  - A page link goes to that page.
  - A file link opens a PDF, beside the document or at its path, in a new
    tab. Anything else is refused with a notice, and so is a missing file.
  - A JavaScript or other action says it is not run.
  - A web link goes through the Trust Manager.
- **Trust Manager** (Preferences > Trust Manager): Open web links is one of
  Ask (the default), Always allow or Never.
  - Under Ask, **Open Web Link** names the address and offers Open, Always
    Allow *site*, and Cancel.
  - Each always-allowed site is listed with Forget.
  - Never refuses every web link, trusted sites included, with a notice.
- **Edit > Create Links from URLs** gives every `http://`, `https://` and
  `www.` address in the text an invisible link, unless it already has one.
  Trailing punctuation and brackets are left out.
- **Edit > Remove Web Links** removes the web links and keeps the page and
  file ones.
- Every change is one undo step: Create Link, Edit Link, Delete Link,
  Create Links from URLs, Remove Web Links. On a protected document the
  tool, the dialog and both menu entries are disabled with its reason.

## How it works

- **core, `links/`.**
  - `read_links` reads every `/Link` on the session's current view, so a
    link made a moment ago is followed.
  - A `/Dest`, a `/GoTo` (a named destination included, through the same
    resolver the bookmarks use), a `/URI`, a `/Launch` or `/GoToR` file
    specification, and any other action by name.
  - Border width comes from `/BS` or `/Border`; also `/C`, `/H` and
    `/BS /S`.
  - `add_link` writes `/GoTo [page /Fit]`, `/URI` or `/Launch`. On a tagged
    document it attaches a `/Link` structure element with its
    `/StructParent`, so PDF/UA readers find it.
  - `set_link`, `remove_link` and `remove_web_links` round out the writes.
  - `Document::links`, `request_link` and `take_link_request` carry a tool's
    Create, Edit or Follow to the shell, as the snapshot tool's request
    does.
- **tools-basic.** The Hand tool tells a click from a drag by how far the
  pointer moved on screen. The pan keeps the grabbed page point under the
  pointer, so the page point alone cannot tell them apart.
- **tools-edit.**
  - `LinkTool` declares a new capability, `Link`, which is also how the
    context menu finds it.
  - The crop and link tools share `Marquee`.
  - `links` holds `create_link`, `edit_link`, `delete_link`, `find_link`,
    `remove_web_links`, and `create_links_from_urls`. The last finds
    addresses with `find_urls` and bounds each by the glyphs whose text it
    covers.
- **app.**
  - `chrome/link_dialog/` (the form and its rows) and `tabs/link_editor.rs`
    (Create Link, Link Properties, the Edit menu's commands) are built only
    with `tools-edit`.
  - `chrome/web_link_dialog.rs` and `tabs/follow_link.rs` are in every
    build. A link request is taken when the canvas notifies and run at the
    next render, which has the window a dialog needs.
  - `preferences.rs` gains `web_links` and `trusted_sites`, saved and read
    like every other setting.

## Runs

- `cargo test -p onionskin-core --test links`: 6 pass:
  - a page link written and read back, including `link_at`;
  - target and look changed, an unwritten action kept, the link removed;
  - Remove Web Links keeping a page link;
  - a tagged page getting a `/Link` element with the invariant clean;
  - links as other producers write them (`/Dest`, named destination, file
    specification dictionary, JavaScript, `/Border` and `/BS` variants),
    with a note and a stream not taken for links;
  - missing pages and non-links refused.
- `cargo test -p onionskin-tools-basic`: 17 pass. The new
  `a_hand_click_asks_to_follow_a_link_and_a_drag_does_not` covers a click, a
  drag back to the start, and Escape.
- `cargo test -p onionskin-tools-edit`: every test passes, 5 of them in
  `tests/links.rs`:
  - create, edit, delete and three undos;
  - refusals named by edit;
  - Create Links from URLs on real text: rectangles over the address, run
    twice making none, then removed;
  - the Link tool's Create and Edit requests, a click on nothing, and
    outlines;
  - registration.

  The `find_urls` unit test covers punctuation, `www.` and hosts without a
  dot.
- `cargo test -p onionskin-app --features shell-test-support --lib`, on a
  real window (`tests::links`, 5 tests):
  - a drawn link typed to page 2, then followed there;
  - a clicked link changed to a web page. Following it asks; Cancel opens
    nothing. Always Allow opens it (`cx.opened_url()`) and trusts the site,
    after which it opens without asking. The site is forgotten from the
    Trust Manager. Never blocks it. Link Properties deletes the link;
  - a text file link refused, a missing PDF named, and a bad page number
    said in the dialog;
  - the Edit menu's two commands and their notices;
  - Create Link from selected text.

  Unit tests cover the dialog form, the Trust Manager decision, host
  parsing, the preference rows, the preferences round trip, and Create Link
  being live with the Link tool.
- **Whole-suite runs.**
  - Workspace without the app: 1128 passed, 0 failed.
  - App: 874 passed. The 6 failures are the known environmental ones: 3
    timing-sensitive canvas tests, and the 3 export rollback tests that fail
    when run as root.
  - App integration tests, guarantees included: all pass.
  - `cargo test -p onionskin-app --no-default-features --test
    kernel_emptiness`: 4 pass.

## Coverage

`cargo tarpaulin -p onionskin-tools-edit -p onionskin-tools-basic` with
optimisation off, over the link code: 354 of 415 lines (85.3%).

| File | Lines covered |
| --- | --- |
| `tools-edit/links.rs` | 84 of 86 |
| `tools-edit/link_tool.rs` | 44 of 46 |
| `tools-edit/marquee.rs` | 25 of 26 |
| `tools-basic/hand.rs` | 35 of 40 |
| `core/links/write.rs` | 83 of 96 |
| `core/links/read.rs` | 69 of 94 |

The lines left in `core/links` are the other-producer reading paths and
the label functions. `crates/core/tests/links.rs` and the app tests cover
those; they were not run under tarpaulin, because the full core suite
segfaults under ptrace in this container.

## Mutations

Each was caught, then reverted.

- Remove Web Links removing every link:
  `remove_web_links_leaves_the_links_to_pages` fails.
- The Hand tool following after a drag:
  `a_hand_click_asks_to_follow_a_link_and_a_drag_does_not` fails.
- A trusted site opening under Never:
  `the_trust_manager_decides_by_policy_then_site` fails.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support` and the plugin-less `--no-default-features
--features shell` build report only the existing `a11y::Shared::record`
warning. `cargo fmt --all --check` is clean.

## Not claimed

- **"Go to a page view."** A page link goes to the page fitted in the
  window. Acrobat's "set the current view as the destination" (a position
  and zoom) is not offered, and a destination's position in a file is not
  followed beyond its page.
- **Custom actions.** Acrobat's Actions tab (JavaScript, show/hide, several
  actions on one link) is not written. Such actions are kept, and are
  never run.
- **Links across windows.** The Link tool's outlines refresh when the tool
  is used or chosen, so a link made in another window appears after the
  next click.
- **Other files.** A file link opens only PDFs.
