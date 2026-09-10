#!/usr/bin/env python3
"""Tally docs/evidence/pro-surface-coverage.tsv and reconcile it with the board.

    coverage-tally.py <coverage.tsv> <ACROBAT-PARITY.md>

Fails when the verdict file's row count does not equal the number of in-scope
rows in the Pro-gated toolset sections of the scoreboard, so a row added to the
board cannot silently drop out of the coverage number.
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


def board_counts(path):
    section, counts = None, defaultdict(int)
    for line in open(path):
        m = re.match(r"^## (.+)$", line)
        if m:
            section = m.group(1).strip()
            continue
        if section not in PRO_SECTIONS or not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) >= 4 and cells[1] in IN_SCOPE:
            counts[section] += 1
    return counts


def main():
    verdicts = defaultdict(lambda: defaultdict(int))
    total = defaultdict(int)
    n = 0
    for line in open(sys.argv[1]):
        if line.startswith("#") or not line.strip():
            continue
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 3:
            raise SystemExit(f"malformed line: {line!r}")
        sec, _row, verdict = parts[0], parts[1], parts[2]
        if verdict not in {"R", "D", "N"}:
            raise SystemExit(f"bad verdict {verdict!r} in {line!r}")
        verdicts[sec][verdict] += 1
        total[verdict] += 1
        n += 1

    print(f"{'section':<30}{'R':>4}{'D':>4}{'N':>4}{'all':>6}")
    for sec in sorted(verdicts):
        v = verdicts[sec]
        print(f"{sec:<30}{v['R']:>4}{v['D']:>4}{v['N']:>4}"
              f"{v['R'] + v['D'] + v['N']:>6}")
    print(f"{'TOTAL':<30}{total['R']:>4}{total['D']:>4}{total['N']:>4}{n:>6}")
    for k in ("R", "D", "N"):
        print(f"  {k}: {total[k]} of {n} = {100 * total[k] / n:.1f}%")

    board = board_counts(sys.argv[2])
    expected = sum(board.values())
    if expected != n:
        print(f"\nMISMATCH: scoreboard has {expected} in-scope Pro-toolset rows, "
              f"the verdict file has {n}")
        return 1
    print(f"\nreconciles with {sys.argv[2]}: {expected} in-scope Pro-toolset rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
