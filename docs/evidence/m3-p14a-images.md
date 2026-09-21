# M3 P14a verification: image import and export

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain. The windowed shell tests ran here; no macOS, Windows or hosted-CI
run is claimed.

Rows closed: 1 Convert (global bar entry point), 9 File > Create, 33 Create
from a single image file, 36 Create from the clipboard, 54 Export all images.
Row 53, Export to JPEG / JPEG 2000 / TIFF, ships `partial`: JPEG and TIFF are
in, JPEG 2000 is not (see below).

## Runs

- `cargo test -p onionskin-core --test images`: 9 passing.
- `cargo test -p onionskin-codecs-common`: 20 unit tests and 24 integration
  tests passing (11 export, 12 images, 1 round trip).
- `cargo test -p onionskin-plugin-api`: 15 passing.
- `cargo test -p onionskin-commands-core`: 13 unit tests and 20 integration
  tests passing.
- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support,codecs-common --lib` (the plan's run): 578 pass and
  5 fail. The 5 are the environmental set in `known-issues.md`: the snapshot
  rotation, the unmeasured page and the three export rollback tests. The five
  new Convert tests and the new JPEG quality test pass.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`: 600
  pass, 6 fail, and the 6 are the same environmental set plus the frame-open
  test.
- `cargo test -p onionskin-app --test kernel_emptiness`, with and without
  default features: passing. The count is now five codecs.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. The shell
  clippy, in both feature sets, is clean apart from the Linux-only dead code
  in `a11y/mod.rs` that CI lints on macOS. `cargo fmt --all -- --check`:
  clean.

## A page from an image: `core::images`

- **The page is the image's physical size.**
  - A 2550×3300 PNG with a 300 DPI `pHYs` becomes a 612×792 page, which is US
    Letter.
  - A 2480×3508 image at 300 DPI becomes A4.
  - Horizontal and vertical resolution are applied separately.
- **The page draws the image.**
  - The render comparison samples every source pixel away from the edges,
    against a 96×64 test card. The worst channel difference is at most 8 for
    a PNG and at most 24 for a JPEG.
  - A four-quadrant image renders the right way up.
- **The plan's mutation.** Fixing `page_size` to A4 fails 5 of the 12 codecs
  tests, among them the 300 DPI test, which uses Letter precisely so that A4
  cannot pass it. It also fails 4 of the 9 core tests. Ignoring the resolution
  fails the 300 DPI tests in both suites.
- **JPEG is carried, never decoded.**
  - The PDF's image stream is the file byte for byte, and the test asserts
    it.
  - Adobe CMYK (APP14) gets `/Decode [1 0 1 0 1 0 1 0]`.
  - A 12-bit or 2-component JPEG is refused with its reason.
- **Alpha.**
  - Transparency becomes an `/SMask`. The rendered clear half shows the
    page.
  - An alpha channel that is opaque everywhere is dropped rather than written.
- **Colour is kept.**
  - Gray stays gray.
  - CMYK TIFF stays CMYK. The samples read back out equal the samples that
    went in.
  - An embedded ICC profile, from PNG `iCCP`, JPEG APP2 chunks (joined in
    sequence order) or a TIFF tag, becomes the image's `/ICCBased` colour
    space with `/N` and a device `/Alternate`. That answers the review risk
    that asked whether the profile is dropped.
- **Several pages.** A multi-page TIFF becomes a page per directory, each at
  its own size, through `pages::Assembly`.

## `CodecPlugin`'s import half

Its three callers shape it:

- Create From File hands one file's bytes.
- Create From Multiple Files hands each input to `combine`.
- Create From Clipboard hands the pasteboard's encoded image.

All three have bytes and want a document, so the half is three defaulted
methods:

- `imports()`: whether the codec has an import half at all, which is what
  makes the Create entries live before there are bytes.
- `reads(bytes)`: recognises the format by its signature. The clipboard has
  no file name, and a file name can lie.
- `import(bytes)`: returns a PDF.

`PluginRegistry::importer(bytes)` finds the codec that reads the bytes. Every
export-only codec imports nothing, and a test asserts it. `combine` takes an
`Importer`, so a list, and Add Folder, can hold scans next to PDFs.
`PdfOnly` keeps the old behaviour for callers with no codecs.

## Export

| Format | Asserted |
| --- | --- |
| JPEG | Decodes through `image`'s JPEG reader, which is not the encoder, at the page's pixel size for the DPI. Quality 20 is under two thirds the size of quality 95. The request's quality overrides the codec's. The JFIF density is the export DPI. |
| TIFF | Decodes through `image`'s TIFF reader. `XResolution` is the export DPI as a rational. Deflate-compressed. |
| All images | On a known three-image document, three files come out. The JPEG is byte-identical to the one embedded. The PNG decodes to the exact samples. The CMYK image is a CMYK TIFF with the exact samples. A JBIG2 image is listed as skipped, with its page, its object and the reason. |

The export dialog shows the DPI field for PNG, JPEG and TIFF, and a Quality
field (1 to 100, default 90) for JPEG only. Out-of-range, fractional and
non-numeric qualities are refused in the dialog.

**Inline images (`BI`/`ID`/`EI`) are out of scope and stated.** They live
inside content streams rather than as objects. So is merging an `/SMask` into
its image; the mask is not written out.

## JPEG 2000: `partial`, with the reason

The format's essential patents have expired, so the format is not the
obstacle. The encoder is. No pure-Rust JPEG 2000 encoder exists at usable
quality, and the working ones, OpenJPEG and Kakadu, are C libraries. The
plan's rule is to ship `partial` with the reason rather than add a C
dependency, and `codecs-common`'s crate doc and `kernel_emptiness.rs` say
so. Row 53's Notes will carry it at the M3 scoreboard update.

## The surface

- **Convert.** A button in the global bar opens its own panel, which is the
  menu panel showing a Convert section. It holds Create PDF From File, From
  Clipboard and From Multiple Files, the five page exports, and Export All
  Images. Each entry is the File menu's own, with the same availability.
  Everything that closes the main menu closes it, and the main menu button
  switches panels rather than stacking a second one.
- **File > Create** gains Create PDF From File… and Create PDF From
  Clipboard, beside Create PDF From Multiple Files…. The entries are live when
  some codec imports, and otherwise disabled, saying so.
- **The clipboard read** is `tabs::clipboard_image`, in
  `shell/chrome/tabs/mod.rs`. It is the one pasteboard read, through GPUI, and
  P10's paste-as-stamp is meant to call it. No second pasteboard crate was
  added.
- **The new document is a file from the start.** The user is asked where to
  save it, it is written through a sibling temporary file and renamed into
  place, and it opens in a tab. A session opened from bytes has nowhere to
  save to, so an untitled buffer would have been a document that could not
  be kept.
- **Export All Images** asks for a folder and never overwrites. A name
  already taken is reported and skipped. The notice counts what was written
  and names what was not, with each reason.

The window tests drive the clipboard with a real PNG and simulate the save
prompt. They then assert the new tab, its file and the page size. The test
platform does not implement GPUI's open-file picker, so the file and folder
tests start from the path a picker would return.

## Not done here, and said

- **EXIF resolution is not read.** In practice it is 72 whatever the image,
  which is the default anyway. JFIF density is read.
- **GIF, BMP and WebP are not imported.** The clipboard notice says that PNG,
  JPEG and TIFF can be.
- **The CMYK JPEG and known-image-count corpus fixtures** are generated in the
  tests rather than fetched from `external/`. The encoder that generates them
  cannot write CMYK JPEG, so the Adobe inversion is asserted at the header
  and `/Decode` level, not rendered.
- **Split, extract and the image dialogs' previews** are unchanged.
