"""Reading order of the tagged fixtures, from pikepdf: the oracle for
`reading_order` in `crates/core/tests/structure.rs`.

For each fixture, the object numbers of its structure elements in depth-first
`/K` order (a seen-set stops cycles, `/MCR` and `/OBJR` are not elements), and
the language each element has once `/Lang` is inherited from its nearest
ancestor that states one, starting from the catalog's (an empty `/Lang`
states "unknown" and ends the inheritance). Prints Rust constants to paste into the test.

    python3 corpus/tagged/reading_order.py
"""
import pikepdf
from pathlib import Path

FIXTURES = [
    "Isartor test files/doc/Isartor test suite manual.pdf",
    "PDF_UA-1/7.2 Text/7.2-t27-pass-a.pdf",
    "PDF_UA-1/7.2 Text/7.2-t15-pass-a.pdf",
    "PDF_UA-2/8.2 Logical structure/8.2.5 Additional requirements for specific structure types/8.2.5.26 Table (Table, TR, TH, TD, THead, TBody, TFoot)/8.2.5.26-t01-pass-a.pdf",
]


def text(value):
    return None if value is None else str(value)


def walk(node, lang, seen, order, languages, depth=0):
    if depth > 64:
        return
    kids = node.get("/K")
    if kids is None:
        return
    items = kids if isinstance(kids, pikepdf.Array) else [kids]
    for item in items:
        if not isinstance(item, pikepdf.Dictionary):
            continue
        kind = item.get("/Type")
        if kind is not None and str(kind) in ("/MCR", "/OBJR"):
            continue
        if item.objgen in seen:
            continue
        seen.add(item.objgen)
        # An element that states /Lang owns the answer, and an empty string says
        # the language is unknown: it does not fall back to an ancestor's.
        effective = (text(item.get("/Lang")) or None) if "/Lang" in item else lang
        order.append(item.objgen[0])
        if effective is not None:
            languages[item.objgen[0]] = effective
        walk(item, effective, seen, order, languages, depth + 1)


base = Path("corpus/external/verapdf")
print("type ReadingExpectation = (&'static str, &'static [u32], &'static [(u32, &'static str)]);\n")
print("const READING_ORDER: &[ReadingExpectation] = &[")
for rel in FIXTURES:
    pdf = pikepdf.open(base / rel)
    order, languages = [], {}
    catalog_lang = (text(pdf.Root.get("/Lang")) or None)
    walk(pdf.Root["/StructTreeRoot"], catalog_lang, set(), order, languages)
    numbers = ", ".join(str(n) for n in order)
    langs = ", ".join(f'({n}, "{v}")' for n, v in sorted(languages.items()))
    print(f'    ("{rel}",\n     &[{numbers}],\n     &[{langs}]),')
print("];")
