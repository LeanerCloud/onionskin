# M3 P10 verification: stamps, attachments and the comment summary

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain. The windowed shell tests ran here; no macOS, Windows or hosted-CI
run is claimed.

Rows closed: 73 Attach a file as a comment, 76 Summarize comments, 79 Place a
stamp, 80 Standard business stamps, 81 Sign Here stamp category, 82 Dynamic
stamps, 83 Create a custom stamp, 84 Manage stamps, 85 Paste clipboard image
as stamp. Row 74 (Comment properties) is P20's, as the plan moved it.

## Runs

- `cargo test -p onionskin-core --test embedded --test stamp_art`: 5 and 3
  passing, plus 2 new unit tests in `core::embedded`.
- `cargo test -p onionskin-tools-comment`: 21 unit tests. Integration tests:
  7 in `stamps`, 5 in `summary`, and every earlier suite still passing.
- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support,tools-comment --lib` (the plan's run): 566 pass
  and 6 fail. The 6 are the environmental set in `known-issues.md`: the
  snapshot, the unmeasured page, the frame-open test and the three rollback
  tests. The stamps and summary window tests pass.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  610 pass and 5 fail, all from the same set.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean. The shell
  clippy is clean in both feature sets, apart from the Linux-only dead code
  in `a11y/mod.rs`. `cargo fmt --all -- --check` is clean.

## Attach File

- **`core::embedded` is the writer; `core::attachments` stays the reader.**
  - `embed_file` writes a Flate-compressed `/EmbeddedFile` stream with
    `/Params /Size /ModDate` and a MIME `/Subtype`, and a `/Filespec` whose
    `/F` and `/UF` name it.
  - `add_to_attachments` names a file spec in `/Names /EmbeddedFiles`. It
    reads the whole tree, including a `/Kids` tree, and writes it back as
    one sorted leaf. A key that is already taken becomes `name (2)`, so an
    existing entry is never overwritten. P13b calls it for the Attachments
    pane.
- **A name that is a path is refused**: empty, `.`/`..`, or containing `/`,
  `\` or NUL. The reader keeps its rule of stripping directories from the
  name it suggests; the writer never produces such a name. The test tries
  four such names and asserts no edit was recorded.
- **Round trip.** The payload is every byte value followed by compressible
  runs. It is attached, saved, reopened, and read back byte-identical.
- **Listed after a reopen.** The attachments reader now lists each page's
  `/FileAttachment` comments after the document's own attachments, each
  with the page it is on. That is also what the Attachments pane shows.
- **The tool.** `AttachFileTool` declares the new
  `ToolCapability::ChoosesFile`. The shell asks for a file when the tool is
  chosen and passes its path to `choose`. Each click then embeds the file as
  it is at that moment, with a MIME type from its extension. The paperclip
  appearance is drawn here, and a render test sees its ink.

## Stamps

- **Every built-in is generated.** `tools/stamps.py` holds the table of
  constants: 12 business, 5 Sign Here and 5 dynamic stamps, each with a
  label, a colour and an ISO 32000 `/Name` where the format has one. It
  writes both `assets/stamps/*.svg` and the Rust catalog from one geometry.
  - `the_committed_stamps_are_what_the_generator_makes` runs `--check`.
  - Hand-editing one SVG's colour made `--check` exit 1 and name the file.
  - `cargo fmt` reformatting the generated catalog failed the test too, so
    the catalog module is `#[rustfmt::skip]`.
- **The artwork is Onionskin's own**, and the reviewer should look at it in
  `assets/stamps/`:
  - business stamps are a square frame with a solid left colour bar;
  - Sign Here stamps are a notched ribbon in a solid colour with white
    lettering;
  - dynamic stamps are a thin frame with a title over a ruled second line.

  None of it is traced from other artwork. The fonts are the standard
  Helvetica faces, named and never embedded.
- **Dynamic stamps.**
  - The second line is the author and the time. The author comes from
    `ToolEnvironment::author`, which is `None` until P20's Commenting
    preference exists, and the line then carries the time alone. The OS
    account name is never read.
  - The time is UTC and the stamp says so.
  - The clock is injected. The test places the stamp at two instants and
    asserts that each appearance contains its own date and that the
    renders differ. The plan's mutation, freezing the clock, fails the test.
  - Text is set in WinAnsiEncoding: `José` is written as `Jos\351`, and the
    appearance's font dictionaries now name `/Encoding /WinAnsiEncoding`.
- **Custom stamps.** A custom stamp is a one-page PDF in the tool's data
  folder, stored as `<category>/<name>.pdf`.
  - From a PDF: the chosen page is taken through `pages::extract_pages`.
  - From an image: the page is the one P14a's importer makes.
  - Placing one imports that page as a Form XObject through the new
    `pages::import_page_as_form`, at its own size up to 200 pt wide. A
    render test finds the image's colour at the stamp's centre.
  - An encrypted source is refused at creation (`LibraryError::Source`) and
    at placement (`Error::Protected`).
- **Manage stamps** is Edit > Stamps….
  - It lists every choice the tool offers, under its category heading.
    Choosing one arms the tool and closes the dialog.
  - Create Custom Stamp… takes a PDF or an image. A name that is already
    taken is refused in the dialog.
  - Only custom stamps have a Delete. Deleting the last stamp in a category
    removes its folder.
  - A built-in id handed to Delete is refused, and all 22 built-ins stay
    listed. Built-ins are compiled in, so no folder operation can remove
    one.
- **Paste Clipboard Image as Stamp** is in the Edit menu and in the dialog.
  - It reads the clipboard through P14a's `tabs::clipboard_image`, the one
    pasteboard read in the app, and imports the image through the codecs.
  - The result is stored as `Pasted/Clipboard Image`, replaced by each
    paste, and chosen on the stamp tool.
  - An empty clipboard says so and chooses nothing.

## Summarize Comments

- **Two layouts**: the comments alone, or each page followed by its comments.
- **Built on P12's assembly.** The pages of comments are Letter pages set
  here, wrapped with the standard font widths, one section per source page,
  each section starting on a fresh page. They are written with `write_new`
  and appended through `Assembly`, and the source pages are its transitive
  copies. There is no second page copier.
- **What the tests read back.** Every comment's kind, author, date and
  contents are in the extracted text, a long comment runs onto a second
  page, and the interleaved layout's page 1 and page 3 are the source
  pages.
- **Encrypted sources are refused** in two places:
  - the command is `CommandEffect::ReadsOut`, so the Edit menu entry is
    disabled with the document's own reason, which is asserted on a real
    window;
  - `summarize` checks as well. Removing that check fails the
    encrypted-source test, because the comments-only layout never appends
    a source page and so never reaches the assembly's own check.
- **A document with no comments** says so, in the dialog, and nothing is
  written.
- **The registered command** writes `<name> - Comments.pdf` beside the
  document and never over an existing file. The menu entry opens the dialog
  instead, which asks where the summary goes and opens it in a tab.

## Plugin API

The API gains:

- `ToolChoice`;
- `ToolEnvironment`, which holds the author and the tool's data folder;
- `ToolPlugin::{configure, choices, choose, chosen}`, all defaulted;
- the `ChoosesFile` and `Stamp` capabilities;
- `PluginRegistry::configure_tools`.

The shell configures every tab's tools with `<config>/data`.

## Not done here, and said

- **The author name** waits for P20's Commenting preferences. Until then,
  stamps and attachments carry no author, which is the rule rather than a
  gap.
- **Local time.** Dynamic stamps are in UTC. The timezone database a local
  time needs is not carried.
- **Acrobat's other summary layouts** are not offered: connector lines, and
  sequence numbers on the page itself. The plan names the two built here.
- **Attach File asks for the file before the click**, not after. The file
  and the place it lands are the same either way.
