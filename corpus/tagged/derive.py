import json, pikepdf
from pathlib import Path
import importlib.util
spec = importlib.util.spec_from_file_location("probe", "/tmp/probe.py")

FIXTURES = [
 "Isartor test files/doc/Isartor test suite manual.pdf",
 "PDF_UA-1/7.2 Text/7.2-t27-pass-a.pdf",
 "PDF_UA-1/7.2 Text/7.2-t15-pass-a.pdf",
 "PDF_UA-2/8.2 Logical structure/8.2.5 Additional requirements for specific structure types/8.2.5.26 Table (Table, TR, TH, TD, THead, TBody, TFoot)/8.2.5.26-t01-pass-a.pdf",
]

def walk(root, seen, out, depth=0):
    if depth > 64: return
    k = root.get("/K")
    if k is None: return
    items = k if isinstance(k, pikepdf.Array) else [k]
    for it in items:
        if isinstance(it, pikepdf.Dictionary):
            t = it.get("/Type")
            if t is not None and str(t) in ("/MCR", "/OBJR"): continue
            key = it.objgen
            if key in seen: continue
            seen.add(key)
            out.append(it)
            walk(it, seen, out, depth+1)

base = Path("corpus/external/verapdf")
for rel in FIXTURES:
    pdf = pikepdf.open(base / rel)
    root = pdf.Root["/StructTreeRoot"]
    seen, out = set(), []
    walk(root, seen, out)
    pages = {p.objgen: i for i, p in enumerate(pdf.pages)}
    per_page = {}
    for el in out:
        pg = el.get("/Pg")
        if pg is None: continue
        i = pages.get(pg.objgen)
        if i is None: continue
        per_page[i] = per_page.get(i, 0) + 1
    pt = root.get("/ParentTree")
    nums = 0
    if pt is not None:
        def count_nums(n, d=0):
            global nums
            if d > 32: return
            if "/Nums" in n: nums += len(n["/Nums"]) // 2
            if "/Kids" in n:
                for k in n["/Kids"]: count_nums(k, d+1)
        count_nums(pt)
    print(json.dumps({"fixture": rel, "elements": len(out), "pages": len(pdf.pages),
                      "per_page": {str(k): v for k, v in sorted(per_page.items())},
                      "parent_tree_keys": nums,
                      "id_tree": root.get("/IDTree") is not None}))
