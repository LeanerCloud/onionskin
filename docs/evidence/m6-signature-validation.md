# M6 verification: signature validation (guarantee test 4)

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed. No file was signed or opened in Acrobat for this
entry. The fixtures are signed by pyHanko 0.37, and poppler's `pdfsig`
24.02 is the independent validator every verdict is checked against.

## Rows

- **To `implemented`:**
  - Signature properties and validation report;
  - Preserve signature validity across edits;
  - Signatures pane (was `partial`, M2).
- **To `partial`:** Validate an existing signature. Integrity, the
  signer's key and what changed after signing are checked; whether the
  signer is trusted landed next, in `m6-trust.md`.
- **Headline:** 86 planned / 44 partial / 80 out-of-scope, 193
  implemented. By milestone unchanged.
  `acrobat_parity_headline_matches_every_inventory_row` passes.

## What the user gets

- **The Signatures pane** leads each field with a verdict:
  - Valid, signer unknown;
  - Valid, changed after signing, signer unknown: later sections added
    only what the signature allows;
  - Invalid: the signed bytes changed, the key did not sign them, or a
    certification forbids what was changed;
  - Validity unknown, with the reason, for a format we cannot check;
  - Not signed, for an empty field.

  A certification signature is prefixed "Certified:", and a SHA-1
  signature is marked "(SHA-1, weak)" (decision D2). Under the verdict, a
  one-line summary names the signer and what changed. The pane's foot says
  identities are not checked against trusted certificates yet.
- **Signature Properties** opens from a signed row and lists: the signer,
  certificate subject, issuer, validity dates and serial; the signature
  and hash algorithms; the signing time, labelled as the signer's clock;
  whether a timestamp is attached (not checked, D3); the certification
  level; which revision the signature covers; the changes made after it
  and any a certification forbids; the format; and the reason and
  location, labelled as the file's own words.
- **View Signed Version** opens, in a new tab, the document as the
  signature signed it: the file cut at the end of its byte range, written
  under the temporary directory. It is offered only when later revisions
  exist.
- **Annotating a signed document** keeps its approval signatures valid:
  the save appends a section and never touches signed bytes.

## How it works

- **crypto** (`signature/`): CMS `SignedData` parsing (`cms`,
  `x509-cert`); the signer found by issuer and serial or by key
  identifier; the `messageDigest` attribute checked against the signed
  bytes; the signature checked over the signed attributes. RSA PKCS#1 v1.5
  and ECDSA P-256 and P-384 through `ring` (D1); RSA-PSS through our own
  EMSA-PSS decoder, because `ring` accepts only a salt as long as the
  hash and pyHanko uses the longest; `adbe.x509.rsa_sha1` and
  `adbe.pkcs7.sha1` too.
- **core** (`signatures/`):
  - `validate` checks each signed field's byte range (starts at 0, the
    gap is exactly `/Contents`) and finds the revision it covers.
  - `changes` compares the cross-reference entries of the signed revision
    with the current file and sorts each difference: annotations, form
    fields, signatures, validation data, metadata, appearances, other.
  - A certification's `/P` (DocMDP) decides which of those are allowed.
  - `Session::validate_signatures` caches by byte generation;
    `Session::signed_version` cuts the file.
- **app:** `panes/signatures.rs` (rows and verdicts),
  `chrome/signature_properties.rs` and `tabs/signature_properties.rs`
  (the dialog and View Signed Version).
- **corpus:** `corpus/make-signed.py` writes twelve fixtures with pyHanko
  and a test CA made each run: each algorithm, CAdES, certified P1 and
  P2, two signatures, a note added after signing, a tampered byte, and an
  empty field.

## Runs

- `cargo test -p onionskin-crypto --test signatures`: 4 pass. Every
  algorithm verifies, SHA-1 among them and marked weak; a changed byte
  breaks the digest and not the signature; a second signature and a later
  note leave each signature valid over its own range; a signature over
  other bytes is invalid.
- `cargo test -p onionskin-core --test signatures`: 4 pass. Every fixture
  is valid over the whole file; tampered is invalid and an empty field is
  not listed; a later signature and a later note cover earlier revisions,
  and the signed version is the earlier file byte for byte; a note saved
  by us keeps approval signatures valid and breaks P1 and P2
  certifications, with `pdfsig` agreeing on the cryptography.
- `cargo test -p onionskin-app --features shell-test-support --lib
  signature`: 24 pass, among them the pane's verdicts on real windows,
  Signature Properties' facts, View Signed Version opening the earlier
  file, and Close.
- **Guarantee test 4**, `cargo test -p onionskin-app --test
  signature_guarantee`: 2 pass. The Sticky Note tool places a note on
  each of six approval-signed fixtures, `DocumentFile::save` saves it, and
  then: the signed bytes are unchanged; every signature is still `Valid`
  with the note reported as a change after signing; `pdfsig` says
  "Signature is Valid." with no digest mismatch. On P1 and P2
  certifications the note makes our verdict invalid, while `pdfsig`
  still verifies the cryptography.
- `--test guarantees`: 43 pass, 1 ignored. Guarantee 4 is no longer
  ignored: it reads `signature_guarantee.rs`, checks both tests are
  plain live tests asserting each clause, and that CI reaches
  `crates/app`. CI's Linux test job installs `poppler-utils`, so `pdfsig`
  runs there; elsewhere that half is skipped and says so.

## Not claimed

- **Trust.** No certificate is checked against trusted identities, and no
  chain is built. Every valid verdict says the signer is unknown.
- **Revocation and timestamps.** No OCSP or CRL request is made, and an
  attached timestamp token is detected but not verified (D3: the network
  only when asked, which lands with the verification preferences).
- **The MCP half of guarantee 4.** The MCP server is post-1.0.
- **Acrobat interoperability.** No Acrobat-signed file has been validated
  yet, and Acrobat has not read our verdicts' fixtures. Both wait on
  files and a pass from a Mac with Acrobat.
