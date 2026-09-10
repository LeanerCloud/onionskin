#!/usr/bin/env python3
"""Measure a side pane from a private Acrobat capture, in logical points.

The capture stays local. Only the numbers this prints are committable.

    measure-pane.py bands <capture.png> [scale]
        Find the pane's right edge, then every horizontal band of the pane that
        contains ink. Prints top, height and pitch for each band.

    measure-pane.py columns <capture.png> <y_top_px> <y_bottom_px> <x_limit_px> [scale]
        Column profile of one band: the left edge and width of each run of ink.

`scale` is the capture's device pixel ratio and defaults to 2.

What this measures is ink extent, not control boxes. A control box is only
observable under hover, so a ledger line derived from this output says "ink" and
a reader knows not to treat it as a hit target.
"""
import sys

import numpy as np
from PIL import Image


def load(path):
    return np.asarray(Image.open(path).convert("RGB")).astype(int)


def pane_edge(a, probe_y):
    """First column where the pane background ends and stays ended.

    Assumes the pane starts at x=0 and that x=10 on the probe row is pane
    background. Neither is checked, because nothing in a single frame can check
    them: if either is false this returns a confident wrong number, so read the
    reported background colour before trusting the edge.
    """
    row = a[probe_y]
    bg = row[10]
    diff = np.abs(row - bg).sum(axis=1)
    for x in range(20, a.shape[1] - 25):
        if diff[x] > 30 and (diff[x:x + 20] > 20).all():
            return x, bg
    raise SystemExit("no pane edge found; is this capture a full window?")


def bands(path, scale):
    a = load(path)
    h, w, _ = a.shape
    print(f"capture {w}x{h} px = {w // scale}x{h // scale} pt")
    edge, bg = pane_edge(a, h // 2)
    print(f"pane background at (10,{h // 2}) {tuple(bg)}")
    print(f"pane right edge x={edge} px = {edge / scale:.1f} pt")
    pane = a[:, 5:edge - 5]
    ink = (np.abs(pane - bg).sum(axis=2) > 40).sum(axis=1)
    found, run = [], None
    for y in range(h):
        if ink[y] > 2:
            if run is None:
                run = y
        elif run is not None:
            if y - run >= 4:
                found.append((run, y - 1))
            run = None
    if run is not None:
        found.append((run, h - 1))
    print(f"\n{len(found)} ink bands:")
    prev = None
    for i, (t, b) in enumerate(found):
        pitch = "" if prev is None else f"  pitch={(t - prev) / scale:.1f} pt"
        prev = t
        print(f"  {i:2d}  y={t:4d}..{b:<4d} px  top={t / scale:.1f} pt  "
              f"height={(b - t + 1) / scale:.1f} pt{pitch}")


def columns(path, y0, y1, xlim, scale):
    if y0 < 6:
        raise SystemExit("y_top must be at least 6 px: the background is sampled "
                         "six rows above the band")
    a = load(path)
    band = a[y0:y1 + 1, :xlim]
    bg = a[y0 - 6, 10]
    ink = (np.abs(band - bg).sum(axis=2) > 40).sum(axis=0)
    runs, run = [], None
    for x in range(xlim):
        if ink[x] > 0:
            if run is None:
                run = x
        elif run is not None:
            runs.append((run, x - 1))
            run = None
    if run is not None:
        runs.append((run, xlim - 1))
    merged = []
    for s, e in runs:
        if merged and s - merged[-1][1] <= 6:
            merged[-1] = (merged[-1][0], e)
        else:
            merged.append((s, e))
    print(f"background {tuple(bg)}  runs {len(runs)}  merged {len(merged)}")
    for s, e in merged:
        print(f"  x={s:4d}..{e:<4d} px  left={s / scale:.1f} pt  "
              f"width={(e - s + 1) / scale:.1f} pt")


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    mode = sys.argv[1]
    if mode == "bands":
        bands(sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 2)
    elif mode == "columns":
        y0, y1, xlim = (int(v) for v in sys.argv[3:6])
        columns(sys.argv[2], y0, y1, xlim,
                int(sys.argv[6]) if len(sys.argv) > 6 else 2)
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
