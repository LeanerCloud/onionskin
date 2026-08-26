//! PLAN.md's guarantee tests, the spec as executable checks. Guarantee 5
//! runs today (see `kernel_emptiness.rs`); the rest are named here so the
//! suite has its final shape from day one and each one lands by deleting
//! an `#[ignore]`.

/// Guarantee 1, round-trip: for every corpus file, open then save with no
/// edit produces byte-identical output. A no-op save appends nothing.
#[test]
#[ignore = "lands with cos parse/save in M1"]
fn a_save_with_no_edit_is_byte_identical_to_the_original() {
    unimplemented!("needs cos open/save and the corpus fetch")
}

/// Guarantee 2, onionskin: open, edit, save produces the original bytes
/// followed by exactly one incremental section, and truncating that
/// section yields the byte-exact original back.
#[test]
#[ignore = "lands with the incremental writer in M1"]
fn an_edit_appends_one_incremental_section_that_truncates_away() {
    unimplemented!("needs cos incremental save and core's edit graph")
}

/// Guarantee 3, redaction: after redacting text T, the verifier extracts
/// all text and images from the output and finds no trace of T, and a raw
/// byte scan finds no trace of the original object bytes.
#[test]
#[ignore = "lands with the redact plugin and its verifier in M5"]
fn redacted_content_survives_neither_extraction_nor_a_byte_scan() {
    unimplemented!("needs the redact plugin's verifier")
}

/// Guarantee 4, signature preservation: annotating a signed corpus file
/// leaves its signature valid, and the UI and MCP report it as valid.
#[test]
#[ignore = "lands with crypto signature verification in M6"]
fn annotating_a_signed_document_keeps_its_signature_valid() {
    unimplemented!("needs crypto verification and a signed corpus file")
}

/// Guarantee 6, repair: every file in the malformed corpus set opens;
/// saving appends an incremental section carrying the repaired
/// structures; the corrupt original bytes stay byte-intact beneath.
#[test]
#[ignore = "lands with the cos scan-and-rebuild path in M1"]
fn every_malformed_file_opens_and_repairs_into_a_new_section() {
    unimplemented!("needs the cos repair path and the malformed corpus")
}

/// Guarantee 7, forms compute: every file in the JS-forms corpus set
/// fills the way Acrobat fills it - computed fields recalculate, formats
/// apply, validation fires - against Acrobat-produced expected values.
#[test]
#[ignore = "lands with scripting and tools-form in M5"]
fn js_form_fields_compute_the_way_acrobat_computes_them() {
    unimplemented!("needs scripting live and the JS-forms corpus")
}

/// Guarantee 8, tag integrity: editing a tagged corpus document leaves
/// its structure tree valid and consistent with the edited content,
/// checked by the accessibility plugin's own checker.
#[test]
#[ignore = "lands with tools-accessibility's checker in M5"]
fn editing_a_tagged_document_leaves_its_structure_tree_valid() {
    unimplemented!("needs core's structure tree and the accessibility checker")
}

/// Guarantee 9, performance: decision 11's budgets - time to first page
/// under 200 ms on the 1000-page corpus file, memory proportional to
/// viewed pages, 60 fps scroll - run as benches, and a regression past
/// budget fails the build like any other test.
#[test]
#[ignore = "lands with the M2 viewer, when there is something to measure"]
fn open_and_scroll_stay_within_the_performance_budgets() {
    unimplemented!("needs the viewer and the 1000-page corpus file")
}
