---
source_id: app-features
audit_date: 2026-08-31
repo: onionskin
baseline: 7413186
scope: read-only independent audit of merged P7-P11 and P13 UI/shell features
auditor: /root/audit_app_features
---

# 2026-08-31 app shell, P8-P11, and P13 feature audit

Read-only audit completed against `main` at `7413186`. The audit did not edit files, run formatters, stage, commit, or launch the app. Evidence came from source, tests, `docs/plans/m2-viewer.md`, and `known-issues.md`; no `cargo test` run was performed.

All `file:line` references in this frozen report refer to baseline `7413186` and
may shift in later audit-ledger edits.

## Findings

| ID | Severity | File:line | Finding | Evidence / reproduction | Existing-ledger status | Recommended fix / test |
|---|---|---:|---|---|---|---|
| APP-001 | Medium | `crates/app/src/shell/chrome/tabs.rs:873`, `crates/app/src/shell/chrome/tabs.rs:891`, `crates/app/src/shell/chrome/tabs.rs:907`, `crates/app/src/shell/panes/attachments.rs:65`, `crates/app/src/shell/panes/attachments.rs:78`, `crates/app/src/shell/panes/attachments.rs:84` | Async export and attachment-save completions retain a cloned `Canvas` and do not re-check that the originating tab is still open/current before writing. | Start export or attachment save, let the path prompt remain pending, close or switch the tab, then resolve the prompt. The closure still calls `run_export(&canvas, ...)` or `attachment_bytes(index)` on the captured entity. | Not ledgered. Related UI-thread save/export items are ledgered, but this lifecycle/stale-entity issue is not. | Capture a tab/document identity and verify it still belongs to the tab set before writing, or cancel pending operations on tab close. Add a controllable prompt test proving close-before-prompt-resolution writes nothing and reports or silently cancels by design. |
| APP-002 | Medium | `crates/app/src/shell/chrome/tabs.rs:2357`, `crates/app/src/shell/chrome/tabs.rs:2360`, `crates/app/src/shell/chrome/tabs.rs:2376` | Derived numbered export files can overwrite without the base filename's confirmation, and fixed three-digit padding stops preserving lexical page order above 999 pages. | `write_export` applies the chosen base-path decision to all derived paths and formats page numbers with `{:03}`. | Confirmed: `known-issues.md:50`. The old "failed save dialog swallowed" subitem is stale because `start_export` handles `Ok(Err(error))`. | Confirm every derived overwrite and derive numbering width from the exported page count. Add multi-file tests covering an existing derived path and at least 1,000 pages. |
| APP-003 | Medium | `crates/core/src/layers.rs:53`, `crates/core/src/layers.rs:81` | Layers pane ignores `/OCProperties /D /Order`, listing flat `/OCGs` file order and losing nesting plus "omitted from Order" semantics. | Reader gathers `groups = references(... OCGs)` and iterates `groups.into_iter()`; no `/Order` lookup exists. | Confirmed: `known-issues.md:99`. | Read `/D /Order` tree, preserve nesting, hide omitted groups per spec, and add a fixture with nested order plus an omitted OCG. |
| APP-004 | Low | `crates/app/src/shell/panes/thumbnails.rs:296`, `crates/app/src/shell/panes/thumbnails.rs:333`, `crates/app/src/shell/canvas.rs:556`, `crates/app/src/shell/canvas.rs:595` | Thumbnail size-change residual remains for pending pages outside the new band. | Size change clears pane images/requested range, but does not clear canvas pending thumbnails or advance thumbnail epoch. Old pending answers can still be accepted by `CanvasModel::accept_thumbnail` and later inserted into the pane. | Confirmed: `known-issues.md:96`. | On size change, clear pending thumbnails or advance an epoch that invalidates all outstanding thumbnail responses. Add test where old-size response arrives after Reduce/Enlarge and must be dropped. |
| APP-005 | Medium | `crates/app/src/shell/mod.rs:287`, `crates/app/src/shell/mod.rs:299`, `crates/app/src/shell/mod.rs:310`, `crates/app/src/shell/mod.rs:317`, `crates/app/src/shell/canvas.rs:1270`, `crates/app/src/shell/canvas.rs:1281`, `crates/app/src/shell/canvas.rs:1313` | Snapshot path can overwrite an earlier pointer/render error and encodes PNG synchronously with no size cap. | `handle_change` records the pointer/action error, then always calls `copy_pending_snapshot`; `record_error` has one status slot. `take_snapshot_png` crops and encodes immediately on the shell path. | Confirmed: `known-issues.md:60`. | Keep first error or queue statuses. Add snapshot max pixel/byte guard and offload encode if large. Test combined pointer-error plus snapshot-error ordering. |
| APP-006 | Low | `crates/app/src/shell/chrome/tabs.rs:2177`, `crates/app/src/shell/chrome/tabs.rs:2189` | Second right-click while a canvas context menu is open is swallowed by the occluding dismiss layer, not used to reposition/open a new context menu. | When `canvas_context_menu.is_some()`, root adds full-screen `menu-dismiss-layer` with `.occlude().on_click(...)`; no right-click handler reaches the canvas behind it. | Confirmed: `known-issues.md:63`. | Let secondary-click on dismiss layer reopen at the new point, or dismiss and re-dispatch. Add GPUI mouse test for right-click while menu is open. |
| APP-007 | Low | `crates/app/src/keymap.rs:267`, `crates/app/src/keymap.rs:287` | Keymap whitelist still rejects real GPUI/platform keys such as `f19` through `f35`, `back`, and `forward`. | `unbindable` only accepts single chars or `NAMED_KEYS`; `NAMED_KEYS` stops at `f18`. | Confirmed: `known-issues.md:103`. | Expand list from GPUI's actual table or remove the brittle whitelist in favor of a canonical parser. Add tests for `f19`, `back`, and `forward`. |
| APP-008 | Low | `crates/app/src/preferences.rs:437`, `crates/app/src/preferences.rs:462` | Preferences carry-forward cap is not exactly 64 unknown settings. | Loop uses `existing.into_iter().take(file.len() + MAX_CARRIED_SETTINGS)` before filtering known keys via `entry(...).or_insert(...)`, so retained unknown count depends on ordering and known-key prefix. | Confirmed: `known-issues.md:108`. | Filter out known keys before `take(MAX_CARRIED_SETTINGS)`. Add test with many unknown keys interleaved with known keys. |
| APP-009 | Low | `crates/app/src/shell/panes/attachments.rs:78`, `crates/app/src/shell/panes/attachments.rs:80` | Attachment save dialog open errors are swallowed like cancel. | `let Ok(Ok(Some(path))) = chosen.await else { return; };` drops `Ok(Err(error))`. Export has an explicit prompt-error notice, so this path is inconsistent. | Not ledgered. Ledger only notes attachment writes on UI thread at `known-issues.md:87`. | Match export's prompt-error handling and add a prompt failure test. |
| APP-010 | Medium | `crates/app/src/shell/chrome/tabs.rs:2344`, `crates/app/src/shell/canvas.rs:415` | Export runs on the UI thread and buffers all exported pages before the first write. | `run_export` executes inside `frame.update`; `CanvasModel::export` returns `Vec<ExportedFile>` for whole-document `PageRange::whole`; `write_export` then writes every buffered file synchronously. | Confirmed: `known-issues.md:50`. | Move export to a cancellable background worker, stream per-page output, and add page-range/progress UI. Add an E2E/perf harness proving the UI keeps ticking during a large export. |

## Reviewed areas with no new findings

- Native command dispatch: the old synchronous listener ledger entry is stale. `global_bar.rs:807-811` defers `RunCommand`, and shell tests cover real find/close/view keystrokes.
- Menus and schema availability: disabled menu items carry reasons; export menu availability is schema-driven.
- Find UI basics: Ctrl+F opens the bar, Escape closes it, active canvas search starts while other canvases cancel. Remaining bidi and cancel-race items are already ledgered.
- Recents/preferences golden paths: opening records recents, preference changes persist, lowering recent count truncates immediately.
- Navigation panes: attachment filename sanitization, disabled Open, signature "no validity claim" labeling, and layer toggle invalidation are covered. Main remaining pane issues are listed above.
- Document/tab basics: close active/others/all refreshes navigation, find, observed view state, and native menus.

## Missing E2E coverage

- Export lifecycle: prompt pending, tab close/switch, then prompt resolution.
- Large export responsiveness and memory behavior.
- Attachment save prompt failure and tab-close-before-save.
- Context-menu second right-click behavior.
- Thumbnail old-size response after Reduce/Enlarge.
- Layer `/D /Order` fixture and UI representation.
- P12 accessibility acceptance: still only spike/direct-message evidence; `known-issues.md:135-137` remains valid.
