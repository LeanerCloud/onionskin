//! Redaction code sets: the two built in, and the user's own kept as files.

use onionskin_redact::codes::{built_in, CodeError, CodeLibrary};

#[test]
fn the_built_in_sets_are_foia_and_the_privacy_act() {
    let sets = built_in();
    assert_eq!(sets.len(), 2);
    assert_eq!(sets[0].name, "U.S. FOIA");
    assert_eq!(sets[0].codes.first().map(String::as_str), Some("(b)(1)"));
    assert!(sets[0].codes.contains(&"(b)(7)(C)".to_owned()));
    assert_eq!(sets[1].name, "U.S. Privacy Act");
    assert!(sets.iter().all(|set| set.built_in));
}

#[test]
fn a_users_sets_are_added_renamed_imported_exported_and_removed() {
    let dir = tempfile::tempdir().expect("dir");
    let library = CodeLibrary::in_data_dir(dir.path());
    assert_eq!(library.sets().len(), 2, "only the built-in sets at first");

    let saved = library
        .save(
            "Legal",
            &[
                "  Privileged ".to_owned(),
                String::new(),
                "Work product".to_owned(),
            ],
        )
        .expect("saves");
    assert_eq!(saved.codes, ["Privileged", "Work product"]);
    assert!(!saved.built_in);
    library.save("Alpha", &["A".to_owned()]).expect("saves");
    let names: Vec<String> = library.sets().into_iter().map(|set| set.name).collect();
    assert_eq!(names, ["U.S. FOIA", "U.S. Privacy Act", "Alpha", "Legal"]);

    library.rename("Legal", "Counsel").expect("renames");
    assert!(matches!(
        library.rename("Legal", "Other"),
        Err(CodeError::Missing(_))
    ));
    assert!(matches!(
        library.rename("Alpha", "Counsel"),
        Err(CodeError::Exists(_))
    ));

    let exported = dir.path().join("Counsel export.txt");
    let counsel = library
        .sets()
        .into_iter()
        .find(|set| set.name == "Counsel")
        .expect("renamed");
    CodeLibrary::export(&counsel, &exported).expect("exports");
    let imported = library.import(&exported).expect("imports");
    assert_eq!(imported.name, "Counsel export");
    assert_eq!(imported.codes, counsel.codes);
    assert!(matches!(
        library.import(&exported),
        Err(CodeError::Exists(_))
    ));

    library.remove("Alpha").expect("removes");
    assert!(matches!(
        library.remove("Alpha"),
        Err(CodeError::Missing(_))
    ));
    assert_eq!(library.sets().len(), 4);
}

#[test]
fn names_that_cannot_be_used_are_refused() {
    let dir = tempfile::tempdir().expect("dir");
    let library = CodeLibrary::in_data_dir(dir.path());
    for name in ["", "  ", "U.S. FOIA", "a/b", ".hidden"] {
        let error = library.save(name, &["x".to_owned()]).unwrap_err();
        assert!(matches!(error, CodeError::BadName(_)), "{name:?}");
        assert!(error.to_string().contains("cannot name a code set"));
    }
    assert!(matches!(
        library.remove("U.S. FOIA"),
        Err(CodeError::BadName(_))
    ));
    assert!(CodeError::Exists("x".into())
        .to_string()
        .contains("already"));
    assert!(CodeError::Missing("x".into())
        .to_string()
        .contains("no code set"));
    let missing_file = library.import(&dir.path().join("none.txt")).unwrap_err();
    assert!(matches!(missing_file, CodeError::Io(_)), "{missing_file}");
}
