#!/usr/bin/env python3
"""Mutate the acceptance procedure rather than the product, and check it bites.

A green suite does not prove a file split preserved behaviour, so the split is
gated on an item inventory instead. That makes the inventory the thing being
trusted, so the inventory is what gets mutation-tested: take a copy of the file
the split starts from, delete one match arm and one whole `impl` method from
it, run the inventory over the copy, and confirm it reports exactly those two
and nothing else. If it reports nothing, the inventory is decorative.

The match arm chosen is one a wildcard arm would absorb silently, so the
compiler would not have caught its loss. That is the failure mode the inventory
exists to catch, and picking an arm from an already-exhaustive match would test
the compiler instead.

Usage:
    procedure_mutation.py FILE
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent

# A whole `impl ShellFrame` method.
DELETED_METHOD = """
    fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.toggle_fullscreen();
        cx.on_next_frame(window, |frame, window, cx| {
            frame.window_bounds_changed(window, cx);
        });
    }
"""

# One arm of `run_canvas_context_command`'s dispatch, chosen because the
# wildcard arm below it would absorb its loss without a compile error.
DELETED_ARM = """            CanvasContextCommand::RotateClockwise => {
                self.run_view_action(ViewAction::RotateClockwise, cx)
            }
"""

EXPECTED_MISSING = "fn toggle_fullscreen"
EXPECTED_CHANGED = "fn run_canvas_context_command"


def inventory(args, output):
    with open(output, "w", encoding="utf-8") as handle:
        subprocess.run(
            [sys.executable, str(HERE / "item_inventory.py"), "emit"] + args,
            stdout=handle,
            check=True,
        )


def delete_once(text, snippet, what):
    count = text.count(snippet)
    if count != 1:
        raise SystemExit("expected exactly one %s to delete, found %d" % (what, count))
    return text.replace(snippet, "\n" if snippet.startswith("\n") else "")


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    source = Path(argv[1])
    original = source.read_text(encoding="utf-8")
    mutated = delete_once(original, DELETED_METHOD, "method")
    mutated = delete_once(mutated, DELETED_ARM, "match arm")

    with tempfile.TemporaryDirectory() as work_str:
        work = Path(work_str)
        copy = work / source.name
        copy.write_text(mutated, encoding="utf-8")
        inventory([str(source)], work / "before.txt")
        inventory([str(copy)], work / "after.txt")
        report = subprocess.run(
            [
                sys.executable,
                str(HERE / "item_inventory.py"),
                "compare",
                str(work / "before.txt"),
                str(work / "after.txt"),
            ],
            capture_output=True,
            text=True,
        ).stdout

    print("deliberately deleted:")
    print("  method:    ShellFrame::toggle_fullscreen")
    print("  match arm: CanvasContextCommand::RotateClockwise "
          "in ShellFrame::run_canvas_context_command")
    print()
    print(report, end="")

    missing = [line for line in report.splitlines() if line.startswith("  -")]
    added = [line for line in report.splitlines() if line.startswith("  +")]
    ok = (
        len(missing) == 2
        and len(added) == 1
        and any(EXPECTED_MISSING in line for line in missing)
        and any(EXPECTED_CHANGED in line for line in missing)
        and any(EXPECTED_CHANGED in line for line in added)
    )
    print()
    if ok:
        print("MUTATION CHECK: the inventory reported exactly the two deletions")
        return 0
    print("MUTATION CHECK FAILED: the inventory did not report exactly the two deletions")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
