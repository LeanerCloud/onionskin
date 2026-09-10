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

`--elide` is a normalization, so it gets its own set. A normalization tuned
until a diff comes out empty proves nothing, so the cases assert both halves:
that the rule absorbs a field rerouted through a sub-struct and the rewrap the
hop provokes, and that with the rule in force the inventory still reports a lost
statement, a changed call, a renamed leaf, a hop through a group nobody declared,
and a hop spelled inside a string literal. The group is synthetic and applied by
these cases, so they read the same before a restructuring and after one.

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


def inventory(paths, output, elide=()):
    flags = ["--elide", ",".join(elide)] if elide else []
    with open(output, "w", encoding="utf-8") as handle:
        subprocess.run(
            [sys.executable, str(INVENTORY), "emit"] + flags + [str(p) for p in paths],
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


def report_for(sources, mutated, elide=()):
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
        inventory(sorted(before_dir.iterdir()), work / "before.txt", elide)
        inventory(sorted(after_dir.iterdir()), work / "after.txt", elide)
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


GROUP = "probe_group"
OTHER_GROUP = "undeclared_group"
# A field that is read from several bodies, rerouted through a sub-struct the
# way a restructuring reroutes one. Synthetic, so these cases read the same
# before a restructuring and after it.
REGROUPED_FIELD = ".notices"

# Both lines sit in `ShellFrame::dismiss_notice` and each appears once across
# the files, so a case that rewrites one names exactly one body.
GUARD = "if index < self.%s.notices.len() {" % GROUP
REMOVE = "self.%s.notices.remove(index);" % GROUP

# `strip_visibility` and the elision both walk string literals to leave them
# alone, so the literal that tests it is a two-sided case: the same string with
# and without a hop spelled inside it. A rule that rewrote data would normalize
# the two into one and report nothing.
LITERAL_ANCHOR = '"no files could be chosen: {error}"'
LITERAL_PLAIN = LITERAL_ANCHOR.replace("chosen:", "chosen.:")
LITERAL_HOP = LITERAL_ANCHOR.replace("chosen:", "chosen.%s.:" % GROUP)

# A hop lengthens the chain it sits in, so rustfmt breaks the chain across
# lines. That has to be absorbed, or the rule reports the formatter.
INLINE_CHAIN = "self.notices.remove(index);"
WRAPPED_CHAIN = "self\n                .notices\n                .remove(index);"


def regroup(sources, group=GROUP):
    """Reroute every access to one field through `group`, as a restructuring does."""
    hop = ".%s%s" % (group, REGROUPED_FIELD)
    rewritten = {path: text.replace(REGROUPED_FIELD, hop) for path, text in sources.items()}
    moved = sum(text.count(REGROUPED_FIELD) for text in sources.values())
    if moved < 10:
        raise SystemExit("expected the regrouped field in many bodies, found %d" % moved)
    return rewritten, moved


def reported(report):
    return [line for line in report.splitlines() if line.startswith(REPORTED)]


def elision_case(name, left, right, expect_reported, elide=(GROUP,)):
    report = report_for(left, right, elide)
    changed = reported(report)
    print("case: %s" % name)
    for line in changed:
        print("  %s" % line.strip())
    ok = bool(changed) == expect_reported
    if expect_reported and not changed:
        print("  NOT REPORTED: the rule is normalizing away a real change")
    if not expect_reported and changed:
        print("  REPORTED: the rule failed to absorb the hop it exists for")
    if not expect_reported and not changed:
        print("  nothing reported, which is what this case asserts")
    print()
    return ok


def elision_cases(sources):
    """The `--elide` rule: it must absorb the hop and nothing else."""
    regrouped, moved = regroup(sources)

    # Without the rule the same regrouping is a wall of noise, which is the
    # reason the rule exists and the reason it has to be shown to still bite.
    unelided = reported(report_for(sources, regrouped))
    print("case: a field rerouted through a sub-struct, without --elide")
    print("  %d item lines differ, from one field moving in %d places" % (len(unelided), moved))
    print()
    ok = len(unelided) > 0

    ok &= elision_case(
        "the same regrouping, with --elide %s" % GROUP, sources, regrouped, False
    )
    ok &= elision_case(
        "a regrouped body that also lost a statement",
        sources,
        apply_once(regrouped, REMOVE, "", "statement to delete"),
        True,
    )
    ok &= elision_case(
        "a regrouped body whose call changed",
        sources,
        apply_once(regrouped, REMOVE, REMOVE.replace(".remove(", ".swap_remove("), "call"),
        True,
    )
    ok &= elision_case(
        "a leaf renamed on the way into the group",
        sources,
        apply_once(regrouped, GUARD, GUARD.replace(".notices.", ".warnings."), "leaf to rename"),
        True,
    )
    ok &= elision_case(
        "a hop through a group that was not declared",
        sources,
        regroup(sources, OTHER_GROUP)[0],
        True,
    )
    rewrapped = apply_once(sources, INLINE_CHAIN, WRAPPED_CHAIN, "chain to rewrap")
    unelided_rewrap = reported(report_for(sources, rewrapped))
    print("case: a chain the formatter rewrapped, without --elide")
    print("  %d item lines differ, so the tolerance is opt-in" % len(unelided_rewrap))
    print()
    ok &= len(unelided_rewrap) > 0
    ok &= elision_case("the same rewrap, with --elide %s" % GROUP, sources, rewrapped, False)

    ok &= elision_case(
        "a hop spelled inside a string literal",
        apply_once(sources, LITERAL_ANCHOR, LITERAL_PLAIN, "literal to change"),
        apply_once(sources, LITERAL_ANCHOR, LITERAL_HOP, "literal to change"),
        True,
    )
    return ok


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

    ok &= elision_cases(sources)

    print("MUTATION CHECK: every mutation was reported" if ok else "MUTATION CHECK FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
