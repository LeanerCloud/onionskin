# ActualText checkpoint evidence

This note records the bounded ActualText integration evidence without claiming
native accessibility or tagged-PDF parity.

| Checkpoint | Source anchors | Evidence and limits |
|---|---|---|
| C1 | `7e17ce8`; `crates/content/tests/{regressions,edit_text,redact}.rs`; `crates/core/tests/text_edit.rs` | The committed tests cover occurrence ownership, semantic search/selection/edit guards, redaction scope handling, protected refusal, and ordinary-neighbor preservation. |
| C2 | `bdf05ad`; `crates/core/tests/{selection_spans,search}.rs` | `actual_text_nested_members_do_not_select_the_hole`, `actual_text_separated_geometry_keeps_member_quads`, `actual_text_unmapped_continuation_remains_selectable`, and `actual_text_worker_search_covers_every_member` pass with independent source geometry checks. |
| C3 | `79620d6`; `crates/app/src/shell/canvas.rs`; `crates/app/src/shell/fixtures.rs` | `actual_text_accessibility_uses_all_member_geometry` and `actual_text_accessibility_keeps_repeated_form_occurrences_separate` cover semantic labels, all member quads, and repeated Form separation in the headless shell projection. Native VoiceOver and tagged-PDF structure reading are not covered. |
| C4 | `13968e241f330b79114e2a9f4b8c02f98c766d58`; `plugins/redact/tests/guarantee.rs`; `plugins/tools-edit/tests/text.rs`; `/tmp/claude/onionskin-actualtext-c4-runtime.Sl9gRa` | Five redaction tests (`a_word_on_the_seeds_is_gone_after_redaction`, `a_word_however_it_is_drawn_is_gone_after_redaction`, `nested_actual_text_redaction_observes_open_ancestor_spelling`, `whole_outer_actual_text_redaction_preserves_inner_semantics`, `partial_repeated_form_redaction_keeps_second_occurrence`) and seven plugin text tests, including `actual_text_ordinary_replacements_preserve_members_and_history`, pass in the audited runtime. Native VoiceOver and tagged-PDF structure reading remain outside this checkpoint. |

The durable source anchors above are the regression contract. The raw C4 log is
execution evidence for the audited headless tests, not a claim of native
VoiceOver acceptance, tagged-PDF structure support, or complete Acrobat parity.
