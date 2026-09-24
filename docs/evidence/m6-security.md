# M6 verification: password security

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed. No file was opened in Acrobat for this entry;
qpdf 11 is the independent implementation it was checked against.

## Rows

- **To `implemented`:**
  - Protect using a password (open password);
  - Restrict editing and printing (permissions password);
  - Set permission details;
  - Remove security;
  - Document properties > Security tab;
  - Open an encrypted document (was `partial`, M3; now M6).
- **Still planned:** Encrypt with a certificate; the Security Settings pane.
- **Headline:** 89 planned / 44 partial / 80 out-of-scope, 190
  implemented. By milestone: M3 96, M6 46.
  `acrobat_parity_headline_matches_every_inventory_row` passes.

This supersedes M3's ruling A (`m3-p1b-encryption.md`): encrypted
documents are no longer read-only across the board.

## What the user gets

- **Opening.** A document that needs a password asks for it: the
  document's name, a masked field, Open and Cancel. A wrong password is
  said so, and the field is cleared. Either password opens it:
  - the user password, with the document's permissions;
  - the permissions password, with all of them.

  Several such documents opened at once are asked about one at a time.
- **Working on a protected document.** What the permissions allow can be
  done, and the rest is refused before it starts. Each kind of change
  needs its own permission, as in Acrobat:

  | Kind of change | Needs |
  | --- | --- |
  | Content: text, images, links, redaction marks, form design, metadata | changes |
  | Comments, including Fill & Sign's marks and signature pictures | commenting, or changes |
  | Filling in form fields, and Clear Form | form filling, commenting, or changes |
  | Inserting, deleting, moving and turning pages | document assembly, or changes |

  Copying content out into another file (extract, split, combine,
  compress, the comment summary, vector print to file) needs the copy
  permission, plus changes or assembly. A disabled entry says which
  permission it lacks. The notice on opening lists what is not allowed and
  says the permissions password lifts it. Allowed changes are saved
  encrypted with the document's own key.
- **File > Protect Using Password…** opens Acrobat's Password Security
  settings:
  - Compatibility: Acrobat X and later (256-bit AES) or Acrobat 7 and
    later (128-bit AES);
  - Require a password to open the document, and the password;
  - Restrict editing and printing, and the permissions password:
    - Printing Allowed: None, Low Resolution or High Resolution;
    - Changes Allowed: None; inserting, deleting and rotating pages;
      filling in form fields and signing; commenting, filling in form
      fields and signing; or any except extracting pages;
    - Enable copying of text, images and other content;
    - Enable text access for screen readers.

  The passwords are checked before anything is written: at least one is
  required, each is required when its box is ticked, and the two must
  differ. **Apply and Save** saves the whole document with that security,
  unsaved changes included. Work then goes on from the saved file, with
  full access.
- **File > Remove Security** saves the document with no security.
- **Both entries** are disabled on a document opened without its
  permissions password, and say so. Remove Security is also disabled on a
  document that has no security.
- **Properties > Security** lists:
  - the security method and the encryption level;
  - the password the document was opened with;
  - printing, changes, document assembly, copying, copying for
    accessibility, commenting and form filling;
  - whether security can be changed.
- **Revert** reopens a protected document with the password it opened
  with.

## How it works

- **crypto, write side** (`encrypt.rs`):
  - `/R` 6: algorithms 8, 9 and 10 make `/U` and `/UE`, `/O` and `/OE`,
    and `/Perms`;
  - `/R` 4 with AESV2: algorithms 3 and 5 make `/O` and `/U`;
  - `SecurityHandler::encrypt_string` and `encrypt_stream`;
  - `SecurityHandler::open_with` opens with either password, and says
    which;
  - `user_password_from_owner` recovers, for `/R` 2 to 4, the user
    password an owner password unlocks. The renderer (hayro) knows only
    user passwords at those revisions.
  - Randomness is passed in; `system_random` is the operating system's.
- **cos:**
  - opening with a password;
  - `access` and `encryption`;
  - `encrypt.rs` encrypts every string and stream a section writes. It
    leaves alone what decryption leaves alone: the `/Encrypt`
    dictionary, a signature's `/Contents`, cross-reference streams, and
    plaintext metadata.
  - `Document::rewrite` writes the whole document afresh, under new
    protection or none, and nulls references to objects it does not
    have.
- **core:**
  - `protection` decides by `EditKind`, from `/P` or, for an owner,
    everything. `EditSession::transact_as` is the one gate.
  - `Document::edit_content` and `edit_form_fields` join `edit_pages` and
    `edit_annotations`.
  - Every reader of the bytes opens them with the document's password:
    the renderer, search, previews, saves, exports, generations and
    recovery.
  - `security::save_with_security` writes and reopens.
  - `SecurityFacts` feeds the Security tab.
- **plugin-api:** `ToolCapability::edit_kind`, and `Session`'s comment
  refusal.
- **app:**
  - `password_dialog.rs` and `protect_dialog.rs`, with `tabs/security.rs`;
  - two File menu entries;
  - the refusals the quick actions and context menu show, per kind.
- **corpus:** `r6-aes-256-print-only.pdf` and
  `r6-aes-256-comments-only.pdf`, written by qpdf through
  `make-encrypted.py`. Tests that meant a refused document use the first.

## Runs

- `cargo test -p onionskin-crypto`: 11 pass. Each level opens with either
  password and not without, the dictionary each level writes, an empty
  owner password, the owner password giving the user password below
  `/R` 5, and AES padding and IVs.
- `cargo test -p onionskin-cos --test encrypted`: 13 pass. The one
  failure, `the_external_encrypted_files_open`, needs the fetched external
  corpus, which is not here.
  - Every fixture revision takes an appended edit, encrypted, which reads
    back and is not in the clear.
  - A section may not change the encryption.
  - A document opens with its user and owner passwords and not a guess.
  - Rewriting protects under both levels and removes protection, with the
    content and an unsaved edit carried through.
  - **qpdf** checks both levels: `--check` with the user password,
    `--show-encryption` with the owner password (the `P` value, printing
    and modify permissions, AESv3 or AESv2), and failure with a wrong
    password.
- `cargo test -p onionskin-core --test protection --test security`: 12
  pass.
  - The permission table over the qpdf fixtures, and the owner lifting it.
  - A refused transaction whose body never runs, and a comment
    transaction that does.
  - A password-protected document edited, saved encrypted (the new title
    is not in the file's bytes), reopened, read and rendered.
  - A plain document protected at both levels, reopened with each
    password, and unprotected again.
  - A user who cannot change security.
- On a real window (`tests::security`, 3 tests):
  - the password prompt, focused and masked, through a wrong password to
    the right one, with the document in the recent list, and Cancel;
  - Protect Using Password's checks, its settings through the
    accessibility tree, Apply and Save, and Remove Security, checked by
    reopening the file;
  - both entries disabled with their reason on the print-only fixture.
- `tests::properties`: the Security tab's level, printing, commenting
  and change-security rows on the print-only fixture.
- **Whole-suite runs.**
  - App: 967 passed. The 5 failures are the known environmental ones: 2
    timing-sensitive canvas tests, which pass alone, and the 3 export
    rollback tests that fail when run as root.
  - App integration tests, guarantees included: 42 pass, 2 ignored.
  - `--no-default-features --test kernel_emptiness`: 4 pass.
  - Every other crate: every suite passes, apart from the tests that
    need the fetched external corpus.

## Coverage

`cargo tarpaulin` with optimisation off:

| Files | Lines covered |
| --- | --- |
| `crypto` (all of it, read side included) | 350 of 421 (83.1%) |
| `crypto/src/encrypt.rs` | 103 of 104 |
| `cos/src/encrypt.rs`, `rewrite.rs` | 124 of 130 (95.4%) |
| `core/src/protection.rs`, `security.rs` | 108 of 118 (91.5%) |

## Mutations

Each was caught, then reverted.

- `refusals()` borrowing the document twice in one expression panicked
  on the first real-window run, before it was committed.
- The `/R` 4 owner password given to hayro as it is: every page of an
  AES-128 document opened by its owner failed to render
  (`a_plain_document_is_protected_and_unprotected`).

## Clippy and format

The following report only the existing `a11y::Shared::record` warning:

- `cargo clippy --workspace --all-targets --features
  onionskin-app/shell-test-support`;
- the app built with `--no-default-features` and `shell`, and with
  `shell,tools-edit`.

`cargo fmt --all --check` is clean.

## Not claimed

- **Certificate encryption** (public-key security handler) and security
  policies.
- **RC4 output.** Documents are only ever encrypted with AES. RC4
  documents are read and edited, and their sections are encrypted with
  RC4 because that is their key.
- **SASLprep.** `/R` 6 passwords are used as typed, in UTF-8, without
  normalisation.
- **Acrobat.** No file written here has been opened in Acrobat. qpdf is
  the independent check.
- **Per-row edit kinds are the ones listed.** Bookmarks, page labels and
  metadata count as content changes, which is stricter than Acrobat in
  places.
