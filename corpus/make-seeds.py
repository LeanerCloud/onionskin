#!/usr/bin/env python3
"""Generate the tiny hand-rolled seed PDFs in corpus/seeds/.

The seeds are the input to make-malformed.sh. Two properties are deliberate and
load the rest of the corpus tooling:

  * Pure ASCII, no compressed streams. make-malformed.sh can then do its xref
    surgery with line-oriented tools without risking a hit inside binary data.
  * Every xref entry is exactly 20 bytes and ends with "space LF", never CRLF,
    so the whole file contains no carriage returns.

Output is byte-deterministic: same script, same bytes, every run. The generated
PDFs are committed, so this script only needs re-running when a seed changes.
"""

from __future__ import annotations

import sys
from pathlib import Path

SEEDS_DIR = Path(__file__).resolve().parent / "seeds"

HEADER = b"%PDF-1.7\n"


def stream_obj(dict_body: str, data: bytes) -> bytes:
    """Build a stream object body with a correct /Length."""
    head = f"<< {dict_body} /Length {len(data)} >>\n".encode("ascii")
    return head + b"stream\n" + data + b"\nendstream"


def build_pdf(objects: list[bytes], root: int = 1) -> bytes:
    """Assemble numbered objects into a PDF with a correct classic xref table."""
    out = bytearray(HEADER)
    offsets: list[int] = []
    for num, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{num} 0 obj\n".encode("ascii") + body + b"\nendobj\n"

    xref_offset = len(out)
    size = len(objects) + 1
    out += b"xref\n"
    out += f"0 {size}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode("ascii")
    out += b"trailer\n"
    out += f"<< /Size {size} /Root {root} 0 R >>\n".encode("ascii")
    out += b"startxref\n"
    out += f"{xref_offset}\n".encode("ascii")
    out += b"%%EOF\n"

    verify(bytes(out), offsets, xref_offset)
    return bytes(out)


def verify(data: bytes, offsets: list[int], xref_offset: int) -> None:
    """Fail loud if the assembled bytes do not match what the xref promises."""
    if b"\r" in data:
        raise AssertionError("seed contains a carriage return")
    if not data.isascii():
        raise AssertionError("seed is not pure ASCII")
    for num, offset in enumerate(offsets, start=1):
        expected = f"{num} 0 obj".encode("ascii")
        if not data.startswith(expected, offset):
            raise AssertionError(f"xref offset for object {num} does not point at it")
    if not data.startswith(b"xref\n", xref_offset):
        raise AssertionError("startxref does not point at the xref table")
    if not data.endswith(b"%%EOF\n"):
        raise AssertionError("seed does not end with %%EOF")


def page(parent: int, extra: str = "") -> bytes:
    return (
        f"<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 200 100]{extra} >>"
    ).encode("ascii")


def text_stream(text: str) -> bytes:
    return stream_obj(
        "",
        f"BT /F1 18 Tf 20 40 Td ({text}) Tj ET\n".encode("ascii"),
    )


HELVETICA = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"


def minimal_pdf() -> bytes:
    """Smallest thing that is still a valid one-page document: no content at all."""
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            page(2, " /Resources << >>"),
        ]
    )


def hello_pdf() -> bytes:
    """One page, one uncompressed content stream, one standard-14 font."""
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            page(2, " /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R"),
            text_stream("Hello Onionskin"),
            HELVETICA,
        ]
    )


def two_page_pdf() -> bytes:
    """Two pages, with crop and rotation coverage on the second page."""
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>",
            page(2, " /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R"),
            text_stream("Page one"),
            HELVETICA,
            page(
                2,
                " /CropBox [10 10 190 90] /Rotate 90"
                " /Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R",
            ),
            text_stream("Page two"),
        ]
    )


SEEDS = {
    "minimal.pdf": minimal_pdf,
    "hello.pdf": hello_pdf,
    "two-page.pdf": two_page_pdf,
}


def main() -> int:
    SEEDS_DIR.mkdir(parents=True, exist_ok=True)
    for name, builder in sorted(SEEDS.items()):
        data = builder()
        (SEEDS_DIR / name).write_bytes(data)
        print(f"{name}: {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
