#!/usr/bin/env python3
"""Audit a directory of private captures before any of them is measured.

    capture-audit.py <dir>

Groups the captures by SHA-256 of the file bytes and reports every group with
more than one member. A group of size N means N filenames name states that are
byte-identical, which means at most one of those filenames can be true.

This exists because a click-and-capture run fails silently: a click that does not
land, or a modal that swallows every later click, produces a full directory of
plausible filenames over one frame. Run this before trusting any filename.
"""
import hashlib
import pathlib
import sys
from collections import defaultdict


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    d = pathlib.Path(sys.argv[1])
    groups = defaultdict(list)
    for p in sorted(d.glob("*.png")):
        groups[hashlib.sha256(p.read_bytes()).hexdigest()].append(p.name)
    dupes = sum(len(v) - 1 for v in groups.values() if len(v) > 1)
    total = sum(len(v) for v in groups.values())
    for h, names in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        print(f"{h[:16]}  x{len(names)}")
        for n in names:
            print(f"    {n}")
    print(f"\n{total} files, {len(groups)} distinct frames, "
          f"{dupes} filenames that cannot be what they say")
    return 1 if dupes else 0


if __name__ == "__main__":
    sys.exit(main())
