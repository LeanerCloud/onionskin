#!/usr/bin/env python3
"""Generate the built-in stamps: constants in, SVGs and a Rust catalog out.

Every stamp Onionskin ships is drawn here, from the table below, in
Onionskin's own style: a square frame with a solid colour bar for the
business stamps, a notched ribbon for the Sign Here set, and a two-line
frame for the dynamic stamps. None of it is traced from, or drawn to
resemble, any other product's artwork. PLAN.md's legal posture requires
that, and generating every stamp from this file is how it stays true: an
SVG edited by hand no longer matches what this script produces, and the
test that runs `--check` fails.

Two outputs, from one geometry:

- `assets/stamps/<id>.svg`, what the Stamps dialog previews and what a
  reviewer looks at;
- `plugins/tools-comment/src/stamp/catalog.rs`, the same drawing as PDF
  content, which is what lands in a document.

The font widths are the standard Helvetica and Helvetica-Bold advances, in
thousandths of an em, for the printable ASCII range. The Rust catalog carries
them too, so a dynamic stamp's second line is centred at run time with the
widths the static lines were centred with here.

Usage: `python3 tools/stamps.py` writes both; `--check` compares instead and
exits 1 on any difference.
"""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SVG_DIR = ROOT / "assets" / "stamps"
CATALOG = ROOT / "plugins" / "tools-comment" / "src" / "stamp" / "catalog.rs"

# Advances for characters 32 through 126, thousandths of an em.
HELVETICA = [
    278, 278, 355, 556, 556, 889, 667, 222, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    222, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
]
HELVETICA_BOLD = [
    278, 333, 474, 556, 556, 889, 722, 278, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    278, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
]
# What a character outside the table is measured as.
FALLBACK = 556
# Cap height of both faces, as a fraction of the size.
CAP = 0.718

GREEN = (0.13, 0.5, 0.2)
RED = (0.72, 0.12, 0.12)
BLUE = (0.12, 0.3, 0.62)
AMBER = (0.7, 0.42, 0.05)
SLATE = (0.3, 0.33, 0.38)

BUSINESS = "Standard Business"
SIGN = "Sign Here"
DYNAMIC = "Dynamic"

# id, label, category, /Name, colour. /Name is ISO 32000's own stamp name
# where the format has one, which is what another reader keys an icon on.
STAMPS = [
    ("business-approved", "Approved", BUSINESS, "Approved", GREEN),
    ("business-not-approved", "Not Approved", BUSINESS, "NotApproved", RED),
    ("business-draft", "Draft", BUSINESS, "Draft", BLUE),
    ("business-final", "Final", BUSINESS, "Final", GREEN),
    ("business-completed", "Completed", BUSINESS, "Completed", GREEN),
    ("business-confidential", "Confidential", BUSINESS, "Confidential", RED),
    ("business-for-public-release", "For Public Release", BUSINESS, "ForPublicRelease", GREEN),
    ("business-not-for-public-release", "Not For Public Release", BUSINESS, "NotForPublicRelease", RED),
    ("business-for-comment", "For Comment", BUSINESS, "ForComment", BLUE),
    ("business-void", "Void", BUSINESS, "Void", RED),
    ("business-preliminary-results", "Preliminary Results", BUSINESS, "PreliminaryResults", AMBER),
    ("business-information-only", "Information Only", BUSINESS, "InformationOnly", SLATE),
    ("sign-sign-here", "Sign Here", SIGN, "SignHere", RED),
    ("sign-initial-here", "Initial Here", SIGN, "InitialHere", BLUE),
    ("sign-witness", "Witness", SIGN, "Witness", AMBER),
    ("sign-accepted", "Accepted", SIGN, "Accepted", GREEN),
    ("sign-rejected", "Rejected", SIGN, "Rejected", RED),
    ("dynamic-approved", "Approved", DYNAMIC, "Approved", GREEN),
    ("dynamic-reviewed", "Reviewed", DYNAMIC, "Reviewed", BLUE),
    ("dynamic-received", "Received", DYNAMIC, "Received", SLATE),
    ("dynamic-revised", "Revised", DYNAMIC, "Revised", AMBER),
    ("dynamic-confidential", "Confidential", DYNAMIC, "Confidential", RED),
]

# What the SVG preview shows where a dynamic stamp's second line goes.
DYNAMIC_SAMPLE = "Your Name, 2026-01-01 09:00 UTC"


def width(text: str, table: list[int], size: float) -> float:
    total = 0
    for character in text:
        code = ord(character)
        total += table[code - 32] if 32 <= code <= 126 else FALLBACK
    return total * size / 1000.0


def num(value: float) -> str:
    """A number as both formats write it: two decimals at most, no noise."""
    text = f"{round(value, 2):.2f}".rstrip("0").rstrip(".")
    return "0" if text in ("-0", "") else text


def rust_num(value: float) -> str:
    """The same number as a Rust `f64` literal, which needs its point."""
    text = num(value)
    return text if "." in text else f"{text}.0"


def tint(color: tuple[float, float, float]) -> tuple[float, float, float]:
    return tuple(0.88 + 0.12 * channel for channel in color)


def pdf_color(color, operator: str) -> str:
    return " ".join(num(channel) for channel in color) + f" {operator}"


def svg_color(color) -> str:
    return "#" + "".join(f"{round(channel * 255):02x}" for channel in color)


def pdf_string(text: str) -> str:
    return "(" + text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)") + ")"


def svg_text(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


@dataclass
class Drawing:
    width: float
    height: float
    pdf: list[str]
    svg: list[str]
    # (size, baseline) of a dynamic stamp's second line.
    dynamic: tuple[float, float] | None = None


def business(label: str, color) -> Drawing:
    size, pad, bar, height = 16.0, 10.0, 7.0, 30.0
    text = label.upper()
    w = bar + pad + width(text, HELVETICA_BOLD, size) + pad
    baseline = (height - size * CAP) / 2
    pdf = [
        f"{pdf_color(tint(color), 'rg')} 0 0 {num(w)} {num(height)} re f",
        f"{pdf_color(color, 'rg')} 0 0 {num(bar)} {num(height)} re f",
        f"{pdf_color(color, 'RG')} 2 w 1 1 {num(w - 2)} {num(height - 2)} re S",
        f"BT /HeBo {num(size)} Tf {pdf_color(color, 'rg')} {num(bar + pad)} {num(baseline)} Td {pdf_string(text)} Tj ET",
    ]
    svg = [
        f'<rect x="0" y="0" width="{num(w)}" height="{num(height)}" fill="{svg_color(tint(color))}"/>',
        f'<rect x="0" y="0" width="{num(bar)}" height="{num(height)}" fill="{svg_color(color)}"/>',
        f'<rect x="1" y="1" width="{num(w - 2)}" height="{num(height - 2)}" fill="none" stroke="{svg_color(color)}" stroke-width="2"/>',
        f'<text x="{num(bar + pad)}" y="{num(height - baseline)}" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="{num(size)}" fill="{svg_color(color)}">{svg_text(text)}</text>',
    ]
    return Drawing(w, height, pdf, svg)


def ribbon(label: str, color) -> Drawing:
    size, pad, notch, height = 15.0, 10.0, 10.0, 30.0
    text = label.upper()
    w = notch + pad + width(text, HELVETICA_BOLD, size) + pad
    baseline = (height - size * CAP) / 2
    outline = [(0, 0), (w, 0), (w, height), (0, height), (notch, height / 2)]
    pdf_path = " ".join(
        f"{num(x)} {num(y)} {'m' if index == 0 else 'l'}" for index, (x, y) in enumerate(outline)
    )
    svg_points = " ".join(f"{num(x)},{num(height - y)}" for x, y in outline)
    pdf = [
        f"{pdf_color(color, 'rg')} {pdf_path} h f",
        f"BT /HeBo {num(size)} Tf 1 1 1 rg {num(notch + pad)} {num(baseline)} Td {pdf_string(text)} Tj ET",
    ]
    svg = [
        f'<polygon points="{svg_points}" fill="{svg_color(color)}"/>',
        f'<text x="{num(notch + pad)}" y="{num(height - baseline)}" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="{num(size)}" fill="#ffffff">{svg_text(text)}</text>',
    ]
    return Drawing(w, height, pdf, svg)


def dynamic(label: str, color) -> Drawing:
    title_size, line_size, pad, height = 15.0, 8.5, 10.0, 44.0
    text = label.upper()
    w = max(width(text, HELVETICA_BOLD, title_size), 150.0) + 2 * pad
    title_x = (w - width(text, HELVETICA_BOLD, title_size)) / 2
    title_baseline = height - 8 - title_size * CAP
    line_baseline = 9.0
    sample_x = (w - width(DYNAMIC_SAMPLE, HELVETICA, line_size)) / 2
    pdf = [
        f"{pdf_color(tint(color), 'rg')} 0 0 {num(w)} {num(height)} re f",
        f"{pdf_color(color, 'RG')} 1.5 w 0.75 0.75 {num(w - 1.5)} {num(height - 1.5)} re S",
        f"{pdf_color(color, 'RG')} 0.75 w {num(pad)} {num(line_baseline + line_size + 3)} m {num(w - pad)} {num(line_baseline + line_size + 3)} l S",
        f"BT /HeBo {num(title_size)} Tf {pdf_color(color, 'rg')} {num(title_x)} {num(title_baseline)} Td {pdf_string(text)} Tj ET",
    ]
    rule_y = height - (line_baseline + line_size + 3)
    svg = [
        f'<rect x="0" y="0" width="{num(w)}" height="{num(height)}" fill="{svg_color(tint(color))}"/>',
        f'<rect x="0.75" y="0.75" width="{num(w - 1.5)}" height="{num(height - 1.5)}" fill="none" stroke="{svg_color(color)}" stroke-width="1.5"/>',
        f'<line x1="{num(pad)}" y1="{num(rule_y)}" x2="{num(w - pad)}" y2="{num(rule_y)}" stroke="{svg_color(color)}" stroke-width="0.75"/>',
        f'<text x="{num(title_x)}" y="{num(height - title_baseline)}" font-family="Helvetica, Arial, sans-serif" font-weight="bold" font-size="{num(title_size)}" fill="{svg_color(color)}">{svg_text(text)}</text>',
        f'<text x="{num(sample_x)}" y="{num(height - line_baseline)}" font-family="Helvetica, Arial, sans-serif" font-size="{num(line_size)}" fill="{svg_color(color)}">{svg_text(DYNAMIC_SAMPLE)}</text>',
    ]
    return Drawing(w, height, pdf, svg, dynamic=(line_size, line_baseline))


def draw(label: str, category: str, color) -> Drawing:
    return {BUSINESS: business, SIGN: ribbon, DYNAMIC: dynamic}[category](label, color)


def svg_document(drawing: Drawing) -> str:
    body = "\n".join(f"  {element}" for element in drawing.svg)
    return (
        "<!-- Generated by tools/stamps.py. Do not edit: change the script. -->\n"
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{num(drawing.width)}" '
        f'height="{num(drawing.height)}" viewBox="0 0 {num(drawing.width)} {num(drawing.height)}">\n'
        f"{body}\n</svg>\n"
    )


def rust_table(name: str, values: list[int]) -> str:
    rows = []
    for start in range(0, len(values), 16):
        rows.append("    " + ", ".join(str(value) for value in values[start : start + 16]) + ",")
    return f"pub(crate) const {name}: [u16; {len(values)}] = [\n" + "\n".join(rows) + "\n];\n"


def rust_catalog() -> str:
    entries = []
    for stamp_id, label, category, name, color in STAMPS:
        drawing = draw(label, category, color)
        content = "\\n".join(drawing.pdf)
        dynamic_line = (
            f"Some(({rust_num(drawing.dynamic[0])}, {rust_num(drawing.dynamic[1])}))"
            if drawing.dynamic
            else "None"
        )
        entries.append(
            "    Builtin {\n"
            f'        id: "{stamp_id}",\n'
            f'        label: "{label}",\n'
            f'        category: "{category}",\n'
            f'        name: "{name}",\n'
            f"        size: ({rust_num(drawing.width)}, {rust_num(drawing.height)}),\n"
            f"        color: ({rust_num(color[0])}, {rust_num(color[1])}, {rust_num(color[2])}),\n"
            f'        content: "{content}",\n'
            f"        dynamic_line: {dynamic_line},\n"
            "    },"
        )
    return (
        "// @generated by tools/stamps.py. Do not edit: change the script and rerun it.\n"
        "// The test `the_committed_stamps_are_what_the_generator_makes` runs its\n"
        "// `--check`, so an edit made here instead fails the build.\n\n"
        "use super::Builtin;\n\n"
        "/// Helvetica advances for characters 32 through 126, thousandths of an em.\n"
        + rust_table("HELVETICA", HELVETICA)
        + "\n/// Helvetica-Bold advances for characters 32 through 126.\n"
        + rust_table("HELVETICA_BOLD", HELVETICA_BOLD)
        + "\n/// What a character outside those tables is measured as.\n"
        f"pub(crate) const FALLBACK: u16 = {FALLBACK};\n\n"
        "/// Every built-in stamp, in the order the Stamps dialog lists them.\n"
        "pub(crate) const BUILTINS: &[Builtin] = &[\n"
        + "\n".join(entries)
        + "\n];\n"
    )


def outputs() -> dict[Path, str]:
    files = {CATALOG: rust_catalog()}
    for stamp_id, label, category, _name, color in STAMPS:
        files[SVG_DIR / f"{stamp_id}.svg"] = svg_document(draw(label, category, color))
    return files


def main(argv: list[str]) -> int:
    files = outputs()
    if "--check" in argv:
        stale = [
            path
            for path, text in files.items()
            if not path.exists() or path.read_text(encoding="utf-8") != text
        ]
        extra = sorted(set(SVG_DIR.glob("*.svg")) - set(files)) if SVG_DIR.exists() else []
        for path in stale:
            print(f"differs from a fresh generation: {path.relative_to(ROOT)}")
        for path in extra:
            print(f"not generated by this script: {path.relative_to(ROOT)}")
        return 1 if stale or extra else 0
    SVG_DIR.mkdir(parents=True, exist_ok=True)
    CATALOG.parent.mkdir(parents=True, exist_ok=True)
    for path, text in files.items():
        path.write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
