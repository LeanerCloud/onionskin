# M3 P7 verification: the plugin edit contract

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P2 and P3.

Linux x86-64, stable toolchain. **No macOS, Windows or hosted-CI run is claimed
here.**

## Runs

- `cargo test -p onionskin-app --test contract`: 8 tests with every plugin
  compiled in, 6 with `--no-default-features`.
- `cargo test -p onionskin-plugin-api`, `-p onionskin-core`,
  `-p onionskin-app`, `-p onionskin-app --no-default-features`, and
  `--no-default-features --features tools-comment`: all passing.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.

## The review risk, answered by moving the save path

P7 asks the sharper version of "did `ToolCtx` grow a field": **what does
`&mut Document` already let a tool reach?** After P3 it reached `save`,
`save_as`, `revert_to`, `autosave` and `set_recovery`. A tool that can call
`revert_to` is a tool that can truncate the user's file in the middle of a
gesture, and no amount of care in any individual tool makes that safe, because
the question is what the type system permits.

So the file operations moved to `core::DocumentFile`, which owns the document
and is the only thing that can write, truncate or recover it. Tools are handed
the `&mut Document` inside it, and there is no path from a `Document` back to
the file that owns it. `DocumentFile` dereferences to `Document`, so the
narrowing runs one way only, which is the direction that matters.

`the_save_path_is_not_on_what_a_tool_is_handed` asserts it by reading the
source. A test that calls `document.save()` and expects a compile error would
be the better proof, and it needs a compile-fail harness this workspace does
not carry; the source check is the honest second-best and says so.

## One deviation from the plan's shape

P7 specifies `impl Document { pub fn edits(&mut self) -> &mut EditSession }`.
What exists is `edit_mut(&mut self) -> (&mut EditSession, &cos::Document)`,
because every `EditSession` method that captures a `before` needs the base
document to capture it against; handing out the session alone would give a
caller something it cannot use. The plan's real constraint, that no
`&mut EditSession` is stored beside a `&mut Document`, holds: both come out of
one call and neither is kept.

## The contract, and why it is split across two crates

The rules live in `plugin-api::contract`; the exhaustive run over the real
`build_registry()` lives in the app's tests, because `build_registry` is the
app's and `plugin-api` cannot depend on it. A plugin crate can therefore check
itself without the whole app, and there is one statement of the contract rather
than one per test that goes looking for it.

The behavioural half drives each tool's **real gesture lifecycle**, not a
synthetic `DocumentEdit`. A synthetic edit proves the edit graph works, which is
P2's job.

**Undoability is asserted on the overlay, not on saved bytes.** Bytes pass
through a serializer that can normalize a difference away; this is a statement
about the edit graph.

## The clause that was vacuous, and what was done about it

**No tool M2 shipped edits anything**, so `check_edits` found nothing to check
against the real registry and every behavioural clause passed by having no
subject. Reporting that as a green suite would have been the exact failure this
package exists to prevent.

`the_contract_checks_a_tool_that_really_edits` registers a synthetic tool that
authors an annotation on commit, and it is the test the required mutation has
to break.

| Mutation | Tests failed |
| --- | --- |
| Q. `EditSession::undo` is a no-op | `the_contract_checks_a_tool_that_really_edits` |

The plan says this mutation must fail the property test for every tool, and that
failing for only some means the test is not exhaustive. It fails for exactly one
here because exactly one tool edits. That is the honest reading today; when P8
lands the comment tools, this mutation must fail for each of them, and if it does
not, the plan's conclusion applies.

## Not done, and not claimed

- **`Requirement` has not moved** from `crates/app/src/shell/context_menu.rs`
  into `plugin-api`, and has not gained its `Command(&'static str)` variant.
  That file is shell code and compiles only under
  `--features shell,shell-test-support`, which builds GPUI; that build outlasts
  this environment's per-command limit, so the change could not be compiled,
  let alone tested. Changing shell code blind would be worse than leaving it.
- Consequently the assertion that no `Requirement::Milestone` arm names M3 does
  not exist yet. None of the four M3 plugins those arms name (rich-text export,
  Add Bookmark, Print, Page Commands) has shipped, so every one of those arms is
  still correct as written.
- `check_edits` does not yet assert determinism. The variant exists and nothing
  produces it; running one gesture twice needs a document reset between runs
  that the current shape does not have.
