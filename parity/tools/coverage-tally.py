#!/usr/bin/env python3
"""Tally docs/evidence/pro-surface-coverage.tsv and reconcile it with the board.

    coverage-tally.py <coverage.tsv> <ACROBAT-PARITY.md>

Reconciliation is by row count only. It catches a row added to or removed from
the scoreboard; it does not catch a row renamed or moved between sections,
because the verdict file abbreviates long board rows and so cannot be joined to
them on text.
"""
import re
import sys
from collections import defaultdict

PRO_SECTIONS = [
    "Toolset: Edit a PDF", "Toolset: Create a PDF", "Toolset: Combine files",
    "Toolset: Organize pages", "Toolset: Compress a PDF", "Toolset: Export a PDF",
    "Toolset: Prepare a form", "Toolset: Redact a PDF", "Toolset: Protect a PDF",
    "Toolset: Use a certificate (digital signatures)", "Toolset: Scan & OCR",
    "Toolset: Measure objects", "Toolset: Prepare for accessibility",
    "Toolset: Use print production",
    "Toolset: Use guided actions (Action Wizard)", "Toolset: Compare files",
]
IN_SCOPE = {"planned", "partial", "implemented"}
VERDICTS = ("Rb", "Ra", "D", "N")


def board_rows(path):
    section, n = None, 0
    for line in open(path):
        m = re.match(r"^## (.+)$", line)
        if m:
            section = m.group(1).strip()
            continue
        if section not in PRO_SECTIONS or not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) >= 4 and cells[1] in IN_SCOPE:
            n += 1
    return n


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    per_section = defaultdict(lambda: defaultdict(int))
    total = defaultdict(int)
    n = 0
    for line in open(sys.argv[1]):
        if line.startswith("#") or not line.strip():
            continue
        parts = line.rstrip("\n").split("\t")
        if len(parts) != 4 or not all(p.strip() for p in parts):
            raise SystemExit(f"malformed line, want 4 non-empty fields: {line!r}")
        sec, _row, verdict, _evidence = parts
        if verdict not in VERDICTS:
            raise SystemExit(f"bad verdict {verdict!r} in {line!r}")
        per_section[sec][verdict] += 1
        total[verdict] += 1
        n += 1
    if not n:
        raise SystemExit("no verdict lines found")

    head = "".join(f"{k:>4}" for k in VERDICTS)
    print(f"{'section':<30}{head}{'all':>6}")
    for sec in sorted(per_section):
        v = per_section[sec]
        print(f"{sec:<30}" + "".join(f"{v[k]:>4}" for k in VERDICTS)
              + f"{sum(v.values()):>6}")
    print(f"{'TOTAL':<30}" + "".join(f"{total[k]:>4}" for k in VERDICTS)
          + f"{n:>6}")
    for k in VERDICTS:
        print(f"  {k}: {total[k]} of {n} = {100 * total[k] / n:.1f}%")
    reach = total["Rb"] + total["Ra"]
    print(f"  Rb+Ra reachable in Reader: {reach} of {n} = {100 * reach / n:.1f}%")
    print(f"  Rb+Ra+D sourced at all:    {reach + total['D']} of {n} = "
          f"{100 * (reach + total['D']) / n:.1f}%")

    expected = board_rows(sys.argv[2])
    if expected != n:
        print(f"\nMISMATCH: scoreboard has {expected} in-scope Pro-toolset rows, "
              f"the verdict file has {n}")
        return 1
    print(f"\nrow count reconciles with {sys.argv[2]}: {expected} rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
