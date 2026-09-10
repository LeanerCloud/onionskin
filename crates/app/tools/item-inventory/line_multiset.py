#!/usr/bin/env python3
"""Cross-check the split by comparing source lines as a multiset.

Independent of item_inventory.py: it does not parse Rust at all. It strips
indentation, drops blank lines, comment-only lines, `use`/`mod` lines and the
visibility qualifiers, and compares what is left as a multiset. Anything that
survives in only one side is a line the split added or removed.
"""

import re
import sys
from collections import Counter
from pathlib import Path

VIS = re.compile(r"\bpub(\([^)]*\))?\s+")


def lines(paths):
    counter = Counter()
    for path in paths:
        for raw in Path(path).read_text(encoding="utf-8").splitlines():
            line = raw.strip()
            if not line:
                continue
            if line.startswith("//"):
                continue
            if re.match(r"^(pub(\([^)]*\))?\s+)?(use|mod)\s", line):
                continue
            counter[VIS.sub("", line)] += 1
    return counter


def main(argv):
    split = argv.index("--")
    before = lines(argv[1:split])
    after = lines(argv[split + 1 :])
    only_before = before - after
    only_after = after - before
    print("# before: %s" % " ".join(argv[1:split]))
    print("# after:  %s" % " ".join(argv[split + 1 :]))
    print("lines before: %d" % sum(before.values()))
    print("lines after:  %d" % sum(after.values()))
    print("only in before: %d" % sum(only_before.values()))
    for line, count in sorted(only_before.items()):
        print("  -%s  %s" % (("x%d" % count) if count > 1 else "  ", line))
    print("only in after: %d" % sum(only_after.values()))
    for line, count in sorted(only_after.items()):
        print("  +%s  %s" % (("x%d" % count) if count > 1 else "  ", line))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
