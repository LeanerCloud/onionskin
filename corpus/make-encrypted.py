#!/usr/bin/env python3
"""Generate the encrypted fixtures in corpus/encrypted/.

One file per standard-security-handler revision, written by qpdf (through
pikepdf) rather than by anything in this repository, so the decryptor under
test is checked against an independent implementation of ISO 32000 7.6. Every
fixture carries the same known plaintext in two places the handler decrypts
differently:

  * a content stream, which is a *stream* and decrypted under /StmF;
  * a string in the document information dictionary, which is a *string* and
    decrypted under /StrF.

A handler that decrypts one and not the other, or keys them the same way when
the crypt filters differ, gets one of the two wrong.

Fixtures, by what they exercise:

  r2-rc4-40.pdf         /V 1 /R 2: RC4, 40-bit key, algorithm 2 and 4
  r3-rc4-128.pdf        /V 2 /R 3: RC4, 128-bit key, algorithm 5
  r4-rc4-128.pdf        /V 4 /R 4: crypt filters, /CFM /V2 (RC4)
  r4-aes-128.pdf        /V 4 /R 4: crypt filters, /CFM /AESV2
  r4-aes-128-plain-metadata.pdf
                        as above with /EncryptMetadata false: the XMP stream
                        must stay plaintext and must not be "decrypted"
  r4-aes-128-objstm.pdf as r4-aes-128 with object streams: strings inside an
                        object stream are not separately encrypted, because
                        the container already was
  r6-aes-256.pdf        /V 5 /R 6: AES-256, the hardened hash of algorithm 2.B
  r6-aes-256-user-password.pdf
                        /R 6 with a user password of "secret": an empty
                        password must NOT open it
  r6-aes-256-print-only.pdf
                        /R 6 whose permissions allow printing and nothing
                        else: every change is refused, and so is copying
                        pages out, unless the owner password opens it
  r6-aes-256-comments-only.pdf
                        /R 6 whose permissions allow comments and form
                        filling, and no other change

Named fixtures on the command line are the only ones written, so adding one
leaves the committed bytes of the rest alone.

Not byte-deterministic: AES needs a random IV per string and stream, and the
/R 6 file key is random. Re-running produces different bytes and equivalent
fixtures, so the tests assert decrypted content and never compare bytes. The
committed output is what the tests read.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pikepdf

OUT = Path(__file__).resolve().parent / "encrypted"

# The plaintext every fixture carries. Distinctive enough that finding it in
# the decrypted output cannot be an accident.
CONTENT_TEXT = "Onionskin decrypts this sentence"
INFO_TITLE = "Onionskin encrypted fixture"
XMP = (
    '<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>'
    '<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
    'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
    '<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/">'
    "<dc:title>Onionskin plaintext metadata</dc:title>"
    "</rdf:Description></rdf:RDF></x:xmpmeta>"
    '<?xpacket end="w"?>'
)


def base() -> pikepdf.Pdf:
    pdf = pikepdf.new()
    font = pdf.make_indirect(
        pikepdf.Dictionary(
            Type=pikepdf.Name.Font,
            Subtype=pikepdf.Name.Type1,
            BaseFont=pikepdf.Name.Helvetica,
        )
    )
    content = f"BT /F1 14 Tf 72 720 Td ({CONTENT_TEXT}) Tj ET".encode("ascii")
    page = pikepdf.Dictionary(
        Type=pikepdf.Name.Page,
        MediaBox=[0, 0, 612, 792],
        Resources=pikepdf.Dictionary(Font=pikepdf.Dictionary(F1=font)),
        Contents=pdf.make_stream(content),
    )
    pdf.pages.append(pikepdf.Page(page))
    pdf.docinfo[pikepdf.Name.Title] = INFO_TITLE
    metadata = pdf.make_stream(XMP.encode("utf-8"))
    metadata[pikepdf.Name.Type] = pikepdf.Name.Metadata
    metadata[pikepdf.Name.Subtype] = pikepdf.Name.XML
    pdf.Root[pikepdf.Name.Metadata] = metadata
    return pdf


FIXTURES = [
    # R 2 and 3 predate /EncryptMetadata: metadata is always encrypted there,
    # and qpdf refuses to be asked otherwise, so the flag is left at its
    # default rather than stated.
    ("r2-rc4-40.pdf", dict(R=2, metadata=False, aes=False), {}),
    ("r3-rc4-128.pdf", dict(R=3, metadata=False, aes=False), {}),
    # qpdf will only encrypt metadata under AES, so the RC4 crypt-filter
    # fixture carries /EncryptMetadata false as well.
    ("r4-rc4-128.pdf", dict(R=4, aes=False, metadata=False), {}),
    ("r4-aes-128.pdf", dict(R=4, aes=True), {}),
    ("r4-aes-128-plain-metadata.pdf", dict(R=4, aes=True, metadata=False), {}),
    (
        "r4-aes-128-objstm.pdf",
        dict(R=4, aes=True),
        dict(object_stream_mode=pikepdf.ObjectStreamMode.generate),
    ),
    ("r6-aes-256.pdf", dict(R=6), {}),
    ("r6-aes-256-user-password.pdf", dict(R=6, user="secret"), {}),
    (
        "r6-aes-256-print-only.pdf",
        dict(
            R=6,
            allow=pikepdf.Permissions(
                accessibility=True,
                extract=False,
                modify_annotation=False,
                modify_assembly=False,
                modify_form=False,
                modify_other=False,
                print_lowres=True,
                print_highres=True,
            ),
        ),
        {},
    ),
    (
        "r6-aes-256-comments-only.pdf",
        dict(
            R=6,
            allow=pikepdf.Permissions(
                accessibility=True,
                extract=False,
                modify_annotation=True,
                modify_assembly=False,
                modify_form=True,
                modify_other=False,
                print_lowres=True,
                print_highres=True,
            ),
        ),
        {},
    ),
]


def main() -> int:
    OUT.mkdir(exist_ok=True)
    wanted = set(sys.argv[1:])
    for name, spec, save_options in FIXTURES:
        if wanted and name not in wanted:
            continue
        user = spec.pop("user", "")
        encryption = pikepdf.Encryption(owner="owner-password", user=user, **spec)
        pdf = base()
        pdf.save(OUT / name, encryption=encryption, **save_options)
        pdf.close()

        # Checked through qpdf before it is committed: the fixture opens with
        # the password it was made with, and says so in its trailer.
        check = pikepdf.open(OUT / name, password=user)
        assert check.is_encrypted, name
        assert str(check.docinfo[pikepdf.Name.Title]) == INFO_TITLE, name
        check.close()
        print(f"wrote {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
