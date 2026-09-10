#!/usr/bin/env python3
"""Mutate the acceptance procedure rather than the product, and check it bites.

A green suite does not prove a file split preserved behaviour, so the split is
gated on an item inventory instead. That makes the inventory the thing being
trusted, so the inventory is what gets mutation-tested.

The first case is the one the plan names: take a copy of the files the split
starts from, delete one match arm and one whole `impl` method, and confirm the
inventory reports exactly those two and nothing else. If it reports nothing, the
inventory is decorative.

A deletion is the case an inventory handles most easily, so more follow, each
aimed at a way an inventory can be blind:

  * a changed literal inside a body, which a name-and-signature listing misses;
  * a changed literal that contains the word `pub`, which a body hash misses if
    it strips visibility textually;
  * two match arms swapped, same text in a different order, which any listing
    that sorts or sets its contents misses;
  * a method moved between `impl ShellFrame` and `impl Render for ShellFrame`,
    which a listing keyed on the name alone misses while dispatch breaks;
  * a widened tuple-struct field, which a listing that only walks braced bodies
    never sees;
  * an added `pub use`, which a listing that skips every `use` never sees;
  * a one-tuple parameter flattened to a plain type on a body-less trait
    method, where the signature is the only thing that can catch it.

The files are passed on the command line, so this keeps working after the split:
each snippet is located across the set rather than in one file.

Usage:
    procedure_mutation.py FILE...
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
INVENTORY = HERE / "item_inventory.py"

# A whole `impl ShellFrame` method.
DELETED_METHOD = """
    fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.toggle_fullscreen();
        cx.on_next_frame(window, |frame, window, cx| {
            frame.window_bounds_changed(window, cx);
        });
    }
"""

# One arm of `run_canvas_context_command`'s dispatch, which lives inside a body
# and so is invisible to any inventory that stops at the signature.
DELETED_ARM = """            CanvasContextCommand::RotateClockwise => {
                self.run_view_action(ViewAction::RotateClockwise, cx)
            }
"""

RENDER_IMPL = "impl Render for ShellFrame {\n"

CASES = [
    (
        "a changed literal inside a body",
        "MenuCommand::Quit => {\n                cx.quit();",
        "MenuCommand::Quit => {\n                cx.notify();",
    ),
    (
        "a changed literal that contains the word pub",
        'const TOOL_ACTIVATION_FAILED: &str =\n    "This tool did not activate',
        'const TOOL_ACTIVATION_FAILED: &str =\n    "This pub tool did not activate',
    ),
    (
        "two match arms swapped, same text in a different order",
        """            MenuCommand::ThemeLight => {
                self.set_theme(ThemePreference::Light, cx);
                Ok(())
            }
            MenuCommand::ThemeDark => {
                self.set_theme(ThemePreference::Dark, cx);
                Ok(())
            }
""",
        """            MenuCommand::ThemeDark => {
                self.set_theme(ThemePreference::Dark, cx);
                Ok(())
            }
            MenuCommand::ThemeLight => {
                self.set_theme(ThemePreference::Light, cx);
                Ok(())
            }
""",
    ),
    (
        "a widened tuple-struct field",
        "struct ExportPhase(AtomicU8);",
        "struct ExportPhase(pub AtomicU8);",
    ),
    (
        "an added pub use re-export",
        "impl Render for ShellFrame {",
        "pub use std::fmt::Debug as Reexported;\n\nimpl Render for ShellFrame {",
    ),
    (
        "a one-tuple parameter flattened on a body-less trait method",
        "fn page_completed(&self, _page: PageIndex) {}",
        "fn page_completed(&self, _page: (PageIndex,)) {}",
    ),
]

EXPECTED_MISSING = "fn toggle_fullscreen"
EXPECTED_CHANGED = "fn run_canvas_context_command"


def inventory(paths, output):
    with open(output, "w", encoding="utf-8") as handle:
        subprocess.run(
            [sys.executable, str(INVENTORY), "emit"] + [str(p) for p in paths],
            stdout=handle,
            check=True,
        )


def compare(before, after):
    return subprocess.run(
        [sys.executable, str(INVENTORY), "compare", str(before), str(after)],
        capture_output=True,
        text=True,
    ).stdout


def apply_once(sources, old, new, what):
    """Replace one occurrence of `old`, in whichever file holds it."""
    hits = [path for path, text in sources.items() if text.count(old) == 1]
    total = sum(text.count(old) for text in sources.values())
    if len(hits) != 1 or total != 1:
        raise SystemExit("expected exactly one %s across the files, found %d" % (what, total))
    mutated = dict(sources)
    mutated[hits[0]] = sources[hits[0]].replace(old, new)
    return mutated


def flat_name(path):
    """A copy's file name, keeping the whole path so two `mod.rs` cannot collide."""
    return str(path).lstrip("/").replace("/", "__")


def report_for(sources, mutated):
    with tempfile.TemporaryDirectory() as work_str:
        work = Path(work_str)
        before_dir, after_dir = work / "before", work / "after"
        before_dir.mkdir()
        after_dir.mkdir()
        for path, text in sources.items():
            (before_dir / flat_name(path)).write_text(text, encoding="utf-8")
        for path, text in mutated.items():
            (after_dir / flat_name(path)).write_text(text, encoding="utf-8")
        if len(list(before_dir.iterdir())) != len(sources):
            raise SystemExit("two inputs collided on one copy name")
        inventory(sorted(before_dir.iterdir()), work / "before.txt")
        inventory(sorted(after_dir.iterdir()), work / "after.txt")
        return compare(work / "before.txt", work / "after.txt")


def deletion_case(sources):
    mutated = apply_once(sources, DELETED_METHOD, "\n", "method to delete")
    mutated = apply_once(mutated, DELETED_ARM, "", "match arm to delete")
    report = report_for(sources, mutated)

    print("case 1: one whole method and one match arm deleted")
    print("  method:    ShellFrame::toggle_fullscreen")
    print(
        "  match arm: CanvasContextCommand::RotateClockwise "
        "in ShellFrame::run_canvas_context_command"
    )
    print()
    print(report, end="")

    missing = [line for line in report.splitlines() if line.startswith("  -")]
    added = [line for line in report.splitlines() if line.startswith("  +")]
    return (
        len(missing) == 2
        and len(added) == 1
        and any(EXPECTED_MISSING in line for line in missing)
        and any(EXPECTED_CHANGED in line for line in missing)
        and any(EXPECTED_CHANGED in line for line in added)
    )


REPORTED = ("  -", "  +", "  VIS", "  DUP")


def reported_case(name, sources, old, new):
    report = report_for(sources, apply_once(sources, old, new, name))
    changed = [line for line in report.splitlines() if line.startswith(REPORTED)]
    print("case: %s" % name)
    for line in changed:
        print("  %s" % line.strip())
    if not changed:
        print("  NOT REPORTED: the inventory is blind to this")
    print()
    return bool(changed)


def move_method(sources):
    without = apply_once(sources, DELETED_METHOD, "\n", "method to move")
    return apply_once(
        without, RENDER_IMPL, RENDER_IMPL + DELETED_METHOD.lstrip("\n"), "impl to move it into"
    )


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    sources = {path: Path(path).read_text(encoding="utf-8") for path in argv[1:]}

    ok = deletion_case(sources)
    print()
    print("CASE 1: exactly the two deletions" if ok else "CASE 1 FAILED")
    print()

    for name, old, new in CASES:
        ok &= reported_case(name, sources, old, new)

    report = report_for(sources, move_method(sources))
    changed = [line for line in report.splitlines() if line.startswith(REPORTED)]
    print("case: a method moved to impl Render for ShellFrame")
    for line in changed:
        print("  %s" % line.strip())
    if not changed:
        print("  NOT REPORTED: the inventory is blind to this")
    ok &= bool(changed)
    print()

    print("MUTATION CHECK: every mutation was reported" if ok else "MUTATION CHECK FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
