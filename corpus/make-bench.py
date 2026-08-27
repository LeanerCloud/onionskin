#!/usr/bin/env python3
"""Generate corpus/bench/pages-1000.pdf, the performance-budget document.

The corpus holds no real 1000-page file, and the budgets in decision 11 are
about a document that size. Three properties make this one able to measure
them:

  * **A balanced page tree**, branching by ten, so reaching page 500 walks four
    nodes rather than one thousand. A flat tree would make the lazy-open bench
    measure the cross-reference and nothing else.
  * **Per-page varied text**, drawn from a fixed vocabulary by a seeded
    generator, so a renderer cannot cache its way out of page 999 after having
    drawn page 1.
  * **Byte-deterministic output**: no timestamps, no randomness, no
    compression. Two runs produce identical bytes, which is what lets a bench
    assert on numbers taken from it.

The output is gitignored: it is generated, not committed, like the rest of the
non-seed corpus.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

CORPUS = Path(__file__).resolve().parent
BENCH_DIR = CORPUS / "bench"
OUTPUT = BENCH_DIR / "pages-1000.pdf"

PAGES = 1000
"""Pages in the file. Decision 11 states both budgets against this number."""

BRANCH = 10
"""Page-tree branching factor, matching what Acrobat and Ghostscript write."""

LINES_PER_PAGE = 30
WORDS_PER_LINE = 8

MEDIA_BOX = "[0 0 612 792]"

VOCAB = [
    "alpha", "anchor", "amber", "arbor", "basin", "beacon", "bramble", "burrow",
    "cadence", "cinder", "clover", "current", "delta", "drift", "dusk", "dynamo",
    "ember", "estuary", "etching", "eddy", "fathom", "ferrule", "flint", "furrow",
    "gable", "granite", "grotto", "gyre", "harbor", "heather", "hollow", "hydra",
    "ingot", "iris", "isthmus", "ivory", "jetty", "juniper", "kelp", "kestrel",
    "lantern", "ledger", "lichen", "lumen", "marrow", "meridian", "mortar", "myriad",
    "nimbus", "nocturne", "obsidian", "orchard", "parapet", "plinth", "quarry", "quill",
    "rampart", "rivulet", "sable", "sextant", "talus", "thicket", "umbra", "vellum",
]


def _seeds_module():
    """Load make-seeds.py's PDF assembly rather than restating it here.

    The two generators write different documents but the same file format, and
    a second copy of the cross-reference arithmetic is a second thing to keep
    right. The filename has a hyphen, so this is the import that reaches it.
    """
    path = CORPUS / "make-seeds.py"
    spec = importlib.util.spec_from_file_location("onionskin_make_seeds", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    # Loading a sibling script would otherwise drop a __pycache__ directory in
    # the corpus tree, which is neither generated corpus nor committed source.
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = previous
    return module


def words_for(page: int) -> list[str]:
    """Deterministic per-page word list, keyed only by the page index.

    Seeded from the index rather than carried across pages, so a change to one
    page's shape cannot shift every later page's text.
    """
    state = (page * 2654435761 + 1) & 0xFFFFFFFF
    out = []
    for _ in range(LINES_PER_PAGE * WORDS_PER_LINE):
        state = (state * 1103515245 + 12345) & 0x7FFFFFFF
        out.append(VOCAB[state % len(VOCAB)])
    return out


def content_for(page: int) -> bytes:
    words = words_for(page)
    lines = [f"Onionskin bench page {page + 1} of {PAGES}"]
    for line in range(LINES_PER_PAGE):
        start = line * WORDS_PER_LINE
        lines.append(" ".join(words[start : start + WORDS_PER_LINE]))
    body = ["BT /F1 10 Tf 12 TL 1 0 0 1 36 750 Tm"]
    for line in lines:
        body.append(f"({line}) Tj T*")
    body.append("ET")
    return ("\n".join(body) + "\n").encode("ascii")


def chunks(items: list[int], size: int) -> list[list[int]]:
    return [items[at : at + size] for at in range(0, len(items), size)]


def build() -> bytes:
    seeds = _seeds_module()

    # Object numbers are assigned up front so every /Parent and /Kids entry can
    # be written in one pass.
    catalog = 1
    font = 2
    first_page = 3
    first_content = first_page + PAGES
    next_internal = first_content + PAGES

    leaves = list(range(first_page, first_page + PAGES))
    count_of = {number: 1 for number in leaves}
    parent_of: dict[int, int] = {}
    internal: list[tuple[int, list[int]]] = []

    level = leaves
    while len(level) > 1:
        parents = []
        for chunk in chunks(level, BRANCH):
            number = next_internal
            next_internal += 1
            internal.append((number, chunk))
            parents.append(number)
            count_of[number] = sum(count_of[child] for child in chunk)
            for child in chunk:
                parent_of[child] = number
        level = parents
    root = level[0]

    objects: list[bytes] = [b""] * (next_internal - 1)

    def place(number: int, body: bytes) -> None:
        objects[number - 1] = body

    place(catalog, f"<< /Type /Catalog /Pages {root} 0 R >>".encode("ascii"))
    place(font, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")

    for index, number in enumerate(leaves):
        place(
            number,
            (
                f"<< /Type /Page /Parent {parent_of[number]} 0 R "
                f"/MediaBox {MEDIA_BOX} "
                f"/Resources << /Font << /F1 {font} 0 R >> >> "
                f"/Contents {first_content + index} 0 R >>"
            ).encode("ascii"),
        )
        place(first_content + index, seeds.stream_obj("", content_for(index)))

    for number, kids in internal:
        parent = parent_of.get(number)
        parent_entry = f"/Parent {parent} 0 R " if parent is not None else ""
        kid_list = " ".join(f"{kid} 0 R" for kid in kids)
        place(
            number,
            (
                f"<< /Type /Pages {parent_entry}/Kids [{kid_list}] "
                f"/Count {count_of[number]} >>"
            ).encode("ascii"),
        )

    if any(body == b"" for body in objects):
        raise AssertionError("an object number was reserved and never filled")
    if count_of[root] != PAGES:
        raise AssertionError(f"the root counts {count_of[root]} pages, not {PAGES}")

    return seeds.build_pdf(objects, root=catalog)


def main() -> int:
    data = build()
    BENCH_DIR.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_bytes(data)
    print(f"{OUTPUT.relative_to(CORPUS)}: {len(data)} bytes, {PAGES} pages")
    return 0


if __name__ == "__main__":
    sys.exit(main())
