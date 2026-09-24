//! Guarantee test 7: forms compute. Every file in the JS-forms corpus set
//! fills the way Acrobat fills it - computed fields recalculate, formats
//! apply, validation fires - against Acrobat-produced expected values.
//!
//! Each `corpus/js-forms/pdfs/<stem>.pdf` has its `<stem>.scenario.json`
//! replayed through the plugin and compared with `<stem>.expected.json`,
//! recorded in Acrobat. The set is not built yet: the values can only come
//! from a licensed Acrobat (`corpus/js-forms/README.md`), so this waits on
//! them, and the harness is proved on a form of our own in `replay.rs`.

use std::path::{Path, PathBuf};

use onionskin_core::Document;
use onionskin_tools_form::replay::{parse_expectation, parse_scenario, replay};

fn set() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/js-forms/pdfs")
}

/// Every PDF in the set with its scenario and expectation.
fn cases() -> Vec<(PathBuf, PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(set()) else {
        return Vec::new();
    };
    let mut cases: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pdf"))
        .map(|pdf| {
            let scenario = pdf.with_extension("scenario.json");
            let expected = pdf.with_extension("expected.json");
            (pdf, scenario, expected)
        })
        .collect();
    cases.sort();
    cases
}

#[test]
#[ignore = "waits on the JS-forms corpus's values recorded in Acrobat (corpus/js-forms/README.md)"]
fn js_form_fields_compute_the_way_acrobat_computes_them() {
    let cases = cases();
    assert!(
        !cases.is_empty(),
        "the JS-forms set has files in {:?}",
        set()
    );
    let mut failures = Vec::new();
    for (pdf, scenario, expected) in cases {
        let read = |path: &Path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        };
        let steps = parse_scenario(&read(&scenario)).expect("the scenario is in format");
        let expectation =
            parse_expectation(&read(&expected)).expect("the expectation is in format");
        let mut doc = Document::open_path(&pdf).expect("the form opens");
        failures.extend(
            replay(&mut doc, &steps, &expectation)
                .into_iter()
                .map(|difference| format!("{}: {difference}", pdf.display())),
        );
    }
    assert!(
        failures.is_empty(),
        "every form must fill the way Acrobat filled it:\n{}",
        failures.join("\n")
    );
}
