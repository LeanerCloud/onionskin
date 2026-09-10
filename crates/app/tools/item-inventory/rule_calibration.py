#!/usr/bin/env python3
"""Break the `--elide` rule and confirm `procedure_mutation.py` catches it.

`procedure_mutation.py` mutates the product and checks the inventory reports it.
That leaves one thing untested: whether a case would still pass against a rule
that was broken. A case that passes either way is decoration, and `--elide` is
the one normalization in the inventory that can be tuned to its own result, so
it is the one worth calibrating.

Each break below is a way the rule can be wrong that a reviewer actually
proposed. The suite has to exit nonzero for every one of them and zero for the
rule as it stands. Nothing in the repository is modified: each break is applied
to a scratch copy of the tool under a temporary directory.

Usage:
    rule_calibration.py FILE...
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent

# The rule as it stands, and the `"`-only walk it replaced. The naive walk is
# what a hand-rolled scanner looks like: it does not know that a raw string
# closes on `"#`, or that `'"'` is one char, so it reads data as code.
CURRENT_SPLIT = """    cls = classify(text)
    spans, start = [], 0
    for i in range(1, len(text) + 1):
        if i == len(text) or (cls[i] == CODE) != (cls[start] == CODE):
            spans.append((cls[start] == CODE, text[start:i]))
            start = i
    return spans
"""

NAIVE_SPLIT = '''    spans, start, i, n = [], 0, 0, len(text)
    in_string = False
    while i < n:
        if in_string:
            if text[i] == "\\\\":
                i += 2
                continue
            if text[i] == '"':
                spans.append((False, text[start : i + 1]))
                start, in_string = i + 1, False
            i += 1
            continue
        if text[i] == '"':
            spans.append((True, text[start:i]))
            start, in_string = i, True
        i += 1
    spans.append((not in_string, text[start:]))
    return spans
'''

FIXED_POINT = """            previous = None
            while previous != span:
                previous = span
                for match in pattern.finditer(span):
                    hops[match.group(1)] += 1
                span = pattern.sub(".", span)
"""

ONE_PASS = """            for match in pattern.finditer(span):
                hops[match.group(1)] += 1
            span = pattern.sub(".", span)
"""

COLLAPSE = '            span = DOT.sub(".", span)\n'

TRAILING_HOP = COLLAPSE + """            span = re.compile(
                r"\\.(?:%s)(?![A-Za-z0-9_.])" % "|".join(sorted(names))
            ).sub("", span)
"""

LEAF_TOO = COLLAPSE + """            span = re.compile(
                r"\\.(?:%s)\\.\\w+" % "|".join(sorted(names))
            ).sub("", span)
"""

BREAKS = [
    ("a `\"`-only walk that reads a raw string as code", CURRENT_SPLIT, NAIVE_SPLIT),
    ("one pass rather than a fixed point", FIXED_POINT, ONE_PASS),
    ("stripping the hop at the end of a chain too", COLLAPSE, TRAILING_HOP),
    ("stripping the leaf along with the hop", COLLAPSE, LEAF_TOO),
]


def run(tool, paths):
    proc = subprocess.run(
        [sys.executable, str(tool / "procedure_mutation.py")] + [str(p) for p in paths],
        capture_output=True,
        text=True,
    )
    said = [
        line.strip()
        for line in proc.stdout.splitlines()
        if "NOT REPORTED" in line or line.strip().startswith("REPORTED:")
    ]
    return proc.returncode, said


def broken(work, old, new):
    tool = work / "tool"
    if tool.exists():
        shutil.rmtree(tool)
    shutil.copytree(HERE, tool)
    target = tool / "item_inventory.py"
    text = target.read_text(encoding="utf-8")
    if text.count(old) != 1:
        raise SystemExit("the break has no single anchor left; the rule moved under it")
    target.write_text(text.replace(old, new), encoding="utf-8")
    return tool


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    paths = argv[1:]
    code, _ = run(HERE, paths)
    ok = code == 0
    print("the rule as it stands: exit %d%s" % (code, "" if ok else "  UNEXPECTED"))
    with tempfile.TemporaryDirectory() as work_str:
        work = Path(work_str)
        for name, old, new in BREAKS:
            code, said = run(broken(work, old, new), paths)
            caught = code != 0
            ok &= caught
            print("\nbreak: %s" % name)
            print("  exit %d, %s" % (code, "caught" if caught else "NOT CAUGHT"))
            for line in said:
                print("  %s" % line)
    print(
        "\nCALIBRATION: every break is caught"
        if ok
        else "\nCALIBRATION FAILED: a break the suite does not catch"
    )
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
