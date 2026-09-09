#!/usr/bin/env python3
"""Render an ax menu dump JSON as an indented text tree."""
import json
import sys

MODS = {0: "cmd", 1: "cmd+shift", 2: "cmd+opt", 3: "cmd+opt+shift",
        4: "cmd+ctrl", 8: "no-cmd", 9: "shift", 10: "opt", 12: "ctrl", 24: "fn"}


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
            mod = MODS.get(r.get("cmdmod"), r.get("cmdmod"))
            bits.append(f"key={mod}+{r['cmdchar']}")
        if r.get("submenu"):
            bits.append("submenu")
        tail = ("  [" + ", ".join(bits) + "]") if bits else ""
        print(f"{pad}{r['index']:>2}. {r['title']}{tail}")


if __name__ == "__main__":
    main()
