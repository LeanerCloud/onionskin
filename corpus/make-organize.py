#!/usr/bin/env python3
"""Generate the page-organization fixtures in corpus/organize/.

The importer's one decisive test is a render comparison: a page inserted from
another document must draw exactly as it drew there. That needs a source page
whose look depends on objects **several references away** from the page
dictionary, so an importer that copies too little renders something else:

  embedded-font.pdf   two pages. Page 1 carries:
                        * text in an embedded, subset TrueType font (DejaVu
                          Sans, through reportlab), so the glyph outlines live
                          in a /FontFile2 stream three references below the
                          page: page -> /Resources -> /Font -> /FontDescriptor
                          -> /FontFile2;
                        * an image XObject;
                        * a form XObject whose own /Resources name the form
                          itself - a legal cycle, and the shape a naive
                          recursive copier never returns from;
                        * a /Text annotation with its own appearance stream.
                      Page 2 carries different text in the same font, so a
                      reorder or extract is visible in extracted text.

Deterministic: reportlab is run with invariant=1 and pikepdf writes with
deterministic IDs, so re-running produces the same bytes. The committed output
is what the tests read, so a machine without reportlab or DejaVu still runs
them.

DejaVu Sans is under the Bitstream Vera licence, which permits embedding.
"""

from __future__ import annotations

import io
import sys
from pathlib import Path

import pikepdf
from reportlab.lib.pagesizes import letter
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas

OUT = Path(__file__).resolve().parent / "organize"
FONT = Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")

PAGE_ONE = "Imported glyphs"
PAGE_TWO = "Second source page"


def base_document() -> bytes:
    pdfmetrics.registerFont(TTFont("DejaVu", str(FONT)))
    buffer = io.BytesIO()
    page = canvas.Canvas(buffer, pagesize=letter, invariant=1)
    page.setFont("DejaVu", 40)
    page.drawString(72, 650, PAGE_ONE)
    page.showPage()
    page.setFont("DejaVu", 40)
    page.drawString(72, 650, PAGE_TWO)
    page.showPage()
    page.save()
    return buffer.getvalue()


def add_image(pdf: pikepdf.Pdf, resources: pikepdf.Dictionary) -> None:
    # An 8x8 RGB checkerboard of two strong colours: blurred or missing, it
    # changes a great many pixels.
    pixels = bytearray()
    for y in range(8):
        for x in range(8):
            pixels += b"\xd0\x20\x20" if (x + y) % 2 else b"\x20\x40\xd0"
    image = pikepdf.Stream(pdf, bytes(pixels))
    image.Type = pikepdf.Name.XObject
    image.Subtype = pikepdf.Name.Image
    image.Width = 8
    image.Height = 8
    image.ColorSpace = pikepdf.Name.DeviceRGB
    image.BitsPerComponent = 8
    resources.XObject.Im1 = pdf.make_indirect(image)


def add_cyclic_form(pdf: pikepdf.Pdf, resources: pikepdf.Dictionary) -> None:
    form = pikepdf.Stream(pdf, b"0 0.6 0 rg 0 0 120 40 re f")
    form.Type = pikepdf.Name.XObject
    form.Subtype = pikepdf.Name.Form
    form.BBox = [0, 0, 120, 40]
    form = pdf.make_indirect(form)
    # The cycle: the form's resources name the form. Never drawn recursively
    # (its content does not invoke /Self), so every reader renders it once.
    form.Resources = pikepdf.Dictionary(XObject=pikepdf.Dictionary(Self=form))
    resources.XObject.Fm1 = form


def add_annotation(pdf: pikepdf.Pdf, page: pikepdf.Page) -> None:
    appearance = pikepdf.Stream(pdf, b"0.9 0.7 0 rg 0 0 24 24 re f")
    appearance.Type = pikepdf.Name.XObject
    appearance.Subtype = pikepdf.Name.Form
    appearance.BBox = [0, 0, 24, 24]
    note = pikepdf.Dictionary(
        Type=pikepdf.Name.Annot,
        Subtype=pikepdf.Name.Text,
        Rect=[400, 500, 424, 524],
        Contents=pikepdf.String("An imported note"),
        AP=pikepdf.Dictionary(N=pdf.make_indirect(appearance)),
        F=4,
    )
    page.obj.Annots = pdf.make_indirect(pikepdf.Array([pdf.make_indirect(note)]))


def embedded_font() -> bytes:
    pdf = pikepdf.open(io.BytesIO(base_document()))
    page = pdf.pages[0]
    resources = page.obj.Resources
    if "/XObject" not in resources:
        resources.XObject = pikepdf.Dictionary()
    add_image(pdf, resources)
    add_cyclic_form(pdf, resources)
    add_annotation(pdf, page)
    page.contents_add(
        pikepdf.Stream(pdf, b"q 96 0 0 96 72 420 cm /Im1 Do Q q 1 0 0 1 250 420 cm /Fm1 Do Q"),
        prepend=False,
    )
    out = io.BytesIO()
    pdf.save(out, deterministic_id=True, object_stream_mode=pikepdf.ObjectStreamMode.disable)
    return out.getvalue()


def main() -> int:
    if not FONT.is_file():
        print(f"{FONT} is missing; install fonts-dejavu-core", file=sys.stderr)
        return 1
    OUT.mkdir(exist_ok=True)
    (OUT / "embedded-font.pdf").write_bytes(embedded_font())
    print(f"wrote {OUT / 'embedded-font.pdf'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
