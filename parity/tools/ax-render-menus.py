#!/usr/bin/env python3
"""Render an ax menu dump JSON as an indented text tree."""
import json
import sys

# AXMenuItemCmdModifiers is a bitmask: bit 0 shift, bit 1 option, bit 2 control,
# bit 3 *suppresses* the implied command key. Decoded rather than tabled so an
# unseen combination renders as modifiers instead of as a bare number.
BITS = ((0, "shift"), (1, "opt"), (2, "ctrl"))


def modifiers(mask):
    if mask is None:
        return "?"
    names = [] if mask & 8 else ["cmd"]
    names += [name for bit, name in BITS if mask & (1 << bit)]
    return "+".join(names) if names else "no-modifier"


def main() -> None:
    rows = json.load(open(sys.argv[1]))
    for r in rows:
        pad = "  " * r["depth"]
        if r["kind"] == "separator":
            print(f"{pad}---")
            continue
        if r["kind"] == "menubar":
            print(f"\n=== {r['title']} ===")
            continue
        bits = []
        if r.get("enabled") is False:
            bits.append("DISABLED")
        if r.get("mark"):
            bits.append(f"mark={r['mark']!r}")
        if r.get("cmdchar"):
            bits.append(f"key={modifiers(r.get('cmdmod'))}+{r['cmdchar']}")
        if r.get("submenu"):
            bits.append("submenu")
        tail = ("  [" + ", ".join(bits) + "]") if bits else ""
        print(f"{pad}{r['index']:>2}. {r['title']}{tail}")


if __name__ == "__main__":
    main()
