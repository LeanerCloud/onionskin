# M6 verification: trusted certificates and the Security Settings pane

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed. OpenSSL 3.0's `openssl verify` is the reference
the trust path builder was checked against; no file was opened in Acrobat.

## Rows

- **To `implemented`:** Security Settings pane.
- **To `partial`:**
  - Trusted identities / manage trusted certificates. The platform trust
    stores are not consulted yet.
  - Signature verification preferences. Revocation checking and a
    timestamp's secure time wait on WP11.
- **Still `partial`, note updated:** Validate an existing signature. The
  identity is judged now; revocation and timestamps are what is left.
- **Headline:** 83 planned / 46 partial / 80 out-of-scope, 194
  implemented. By milestone unchanged.
  `acrobat_parity_headline_matches_every_inventory_row` passes.

## What the user gets

- **Preferences > Signatures:**
  - Verify signatures when the document is opened: On (the default) or
    Off. Off, the Signatures pane lists signed fields as "Signed, not
    checked" with a **Validate All** button.
  - Verify signatures using: the current time (the default), or the time
    at which the signature was created. The second is the signer's own
    clock, and the label says so wherever it is shown.
  - The trusted certificates: a count with **Add Certificate...**, which
    reads a `.pem`, `.crt`, `.cer` or `.der` file (every certificate in a
    PEM bundle), then each certificate by name, issuer and expiry, trusted
    for **Signed documents** and **Certified documents**, and **Remove**.
    Trusting for certified documents trusts for signed ones too; untrusting
    signed ones untrusts certified ones, as Acrobat's checkboxes do.
    Nothing is trusted until the user adds it.
- **The Signatures pane** now ends each valid verdict with the identity:
  "Valid" when the signer is trusted, "Valid, signer unknown" when no
  trusted certificate is reached, "Valid, signer's identity invalid" when
  one is reached but a check on the way fails. The detail line says which
  and why.
- **Signature Properties** adds the identity sentence, the certificates a
  trusted signer chains through ("Ada Signer > Onionskin Test CA"), and
  **Add to Trusted Certificates**, which trusts the signer's own
  certificate.
- **The Security Settings pane** appears in the navigation strip only on
  an encrypted document. It lists what the security allows, the Security
  tab's own rows (method, encryption level, the password it was opened
  with, and each permission), and **Permission Details** opens Document Properties on its Security
  tab.

## How it works

- **crypto** (`signature/trust.rs`): a path is built from the signer by
  issuer name and verified signature, through the certificates the
  signature carries and the trusted ones, until it reaches a trusted
  certificate. That path is then held to: validity at the chosen time;
  each issuer a CA (`basicConstraints`) allowed `keyCertSign`; the signer
  allowed `digitalSignature` or `nonRepudiation`. No path is Unknown; a
  failed check is Invalid. Trust is by purpose, approval or certification.
  `Certificate::from_file_bytes` reads PEM (several) or DER, and
  `to_pem` writes it.
- **core** (`signatures/identity.rs`): `identity(validation, anchors,
  time, now)`, kept out of the validation cache because trust changes
  when the user's list does, not when the file does.
- **app:** `trusted_certificates.rs` keeps the list as PEM in
  `trusted-certificates.json` beside the preferences, written owner-only;
  two settings in `preferences.json`; the pane is handed the trust it
  judges by and reads again when it changes; `panes/security.rs` for the
  Security Settings pane.
- **corpus:** `corpus/make-chains.py` writes ten certificates with fixed
  dates: a good chain, an expired and a not-yet-valid signer, a signer
  under a non-CA, a signer whose key may only sign certificates, and an
  unrelated root.

## Runs

- `cargo test -p onionskin-crypto --test trust`: 3 pass. Each of six
  chains gets the verdict `openssl verify -attime 2030-01-01 -purpose
  smimesign` gives it, and the test runs OpenSSL itself when it is
  installed; trust by purpose; a trusted intermediate is an anchor; a
  missing intermediate is unknown; PEM, several PEM, and DER files read.
- `cargo test -p onionskin-core --test signatures`: 5 pass, the new one
  trusting pyHanko's test CA: unknown, then valid through the CA, valid at
  the creation time, invalid twenty years on, and a certification
  unknown until the CA is trusted for certified documents.
- `cargo test -p onionskin-app --features shell-test-support --lib`: 976
  pass. The 6 failures are the known environmental ones (three canvas
  timing tests, three rollback tests that fail as root). New on real
  windows: trusting the test CA turns the pane's verdict to Valid, is
  kept as PEM, is listed in Preferences > Signatures, and toggling or
  removing it turns it back; Add to Trusted Certificates trusts the
  signer; a file with no certificate and a missing file are said so;
  Validate All; the Security Settings pane is absent on a plain document
  and present on an encrypted one, and Permission Details opens the
  Security tab.
- **Coverage** (tarpaulin, llvm engine) over the signature code in
  `crypto` and `core`: 82.77%, 658 of 795 lines; `trust.rs` 76 of 80,
  `identity.rs` 16 of 19.

## Not claimed

- The Keychain and Windows certificate stores are not read, and there
  is no "trust the system's roots" option yet.
- No revocation check (OCSP, CRL) and no timestamp verification: WP11.
- Certificate policies, name constraints and extended key usage are not
  checked. OpenSSL's `smimesign` purpose checks EKU when present; none of
  the fixtures carries one.
- A trust change is seen by the window it was made in. Another window
  open at the time keeps the list it started with until it is reopened.
