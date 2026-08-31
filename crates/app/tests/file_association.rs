//! `.pdf` is registered as an alternate handler on every platform, and never
//! as the default one.
//!
//! PLAN.md's legal line puts it plainly: "Onionskin registers `.pdf` as
//! openable, never as the default handler: it joins the 'Open with' menu
//! rather than taking files off Acrobat." The packaging manifests are where
//! that promise is kept, and a one-word edit in any of them breaks it
//! silently, on a platform whose installer nobody runs in CI. So the promise
//! is a test.
//!
//! What this cannot check is the installed result: `lsregister -dump` on
//! macOS, the Open With list on Windows, and `xdg-mime query default` on
//! Linux are inspection on a real installation, which is the confidence
//! level packaging/README.md already states for the Linux and Windows
//! scripts. What it does check is that the manifests say the right thing.

use std::path::{Path, PathBuf};

fn packaging(file: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging")
        .join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()))
}

/// Launch Services ranks handlers, and `Alternate` is the rank that joins the
/// Open With menu without claiming the type. `Owner` or `Default` would take
/// PDFs off whatever the user already uses.
#[test]
fn the_macos_bundle_claims_pdf_only_as_an_alternate_handler() {
    let plist = packaging("macos/Info.plist");

    assert!(
        plist.contains("<key>LSHandlerRank</key><string>Alternate</string>"),
        "Info.plist does not rank itself Alternate"
    );
    assert!(plist.contains("com.adobe.pdf"), "Info.plist claims no PDFs");
    for forbidden in ["<string>Owner</string>", "<string>Default</string>"] {
        assert!(
            !plist.contains(forbidden),
            "Info.plist contains {forbidden}, which claims the type"
        );
    }
    // Exporting the type declaration would claim ownership of PDF itself,
    // which is a stronger claim than any handler rank.
    assert!(
        !plist.contains("UTExportedTypeDeclarations"),
        "Info.plist exports a type declaration for a format it did not define"
    );
}

/// On Windows the difference is one registry key: `OpenWithProgids` offers
/// the app, the extension's default value takes the association.
#[test]
fn the_windows_installer_offers_itself_without_taking_the_association() {
    let installer = packaging("windows/installer.nsi");

    assert!(
        installer.contains(r#"WriteRegStr HKCR ".${ext}\OpenWithProgids""#),
        "the installer does not register under OpenWithProgids"
    );
    assert!(
        installer.contains(r#"!insertmacro AssociateExt "pdf""#),
        "the installer associates no PDF extension"
    );
    for line in installer.lines() {
        assert!(
            !writes_default_handler(line),
            "the installer writes the extension's default handler: {line}"
        );
    }
    // The matcher has to catch the per-user form as well as the per-machine
    // one, which is what an installer reaches for when it cannot write HKLM.
    for taken in [
        r#"WriteRegStr HKCR ".pdf" "" "Onionskin.pdf""#,
        r#"WriteRegStr HKCU "Software\Classes\.pdf" "" "Onionskin.pdf""#,
        r#"WriteRegStr HKCR  ".${ext}"  ""  "Onionskin.${ext}""#,
        r#"WriteRegExpandStr HKLM "Software\Classes\.pdf" "" "Onionskin.pdf""#,
    ] {
        assert!(
            writes_default_handler(taken),
            "this would take the association and the check missed it: {taken}"
        );
    }
    // And it must not catch the two writes the installer needs.
    for allowed in [
        r#"WriteRegStr HKCR ".${ext}\OpenWithProgids" "Onionskin.${ext}" "" "#,
        r#"WriteRegStr HKCR "Onionskin.${ext}\shell\open\command" "" '"$INSTDIR\onionskin.exe" "%1"'"#,
    ] {
        assert!(
            !writes_default_handler(allowed),
            "the check refuses a write the installer needs: {allowed}"
        );
    }
    // The association is removed again, so uninstalling leaves no entry in
    // anyone's Open With menu.
    assert!(
        installer.contains(r#"!insertmacro UnassociateExt "pdf""#),
        "the uninstaller leaves the association behind"
    );
}

/// Whether a line writes the default handler for the extension: a registry
/// write whose key names the extension itself, rather than a subkey of it,
/// and whose value name is empty.
///
/// Both halves matter. `".pdf\OpenWithProgids"` names a subkey and is the
/// write that offers the app; `"Software\Classes\.pdf"` with an empty value
/// name is the same key as `HKCR ".pdf"` and takes the association.
fn writes_default_handler(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    let Some(command) = words.first() else {
        return false;
    };
    if !command.starts_with("WriteReg") {
        return false;
    }
    let names_the_extension = words.iter().any(|word| {
        let key = word.trim_matches('"');
        let last = key.rsplit('\\').next().unwrap_or(key);
        last == ".pdf" || last == ".${ext}"
    });
    names_the_extension && words.contains(&"\"\"")
}

/// On Linux a desktop entry advertising the MIME type appears in the "Open
/// With" list; the default lives in the user's mimeapps.list and is theirs to
/// set. The packaging script must not set it for them.
#[test]
fn the_linux_desktop_entry_advertises_pdf_without_claiming_the_default() {
    let desktop = packaging("linux/onionskin.desktop");
    let script = packaging("linux/package.sh");

    assert!(
        desktop
            .lines()
            .any(|line| line.trim() == "MimeType=application/pdf;"),
        "the desktop entry advertises no PDF support"
    );
    assert!(
        desktop
            .lines()
            .any(|line| line.starts_with("Exec=") && (line.contains("%f") || line.contains("%F"))),
        "the desktop entry takes no file argument, so Open With cannot use it"
    );
    assert!(
        !script.contains("xdg-mime default") && !script.contains("xdg-settings set"),
        "the packaging script sets a default handler"
    );
}

/// The other half of the promise: nothing anywhere in the workspace asks
/// the OS to make Onionskin the default at runtime, which is how an
/// application takes a file type after installation rather than during it.
///
/// Over every crate and plugin, not just the app: any of them could link a
/// platform call, and the whole point is that none does.
///
/// A guardrail rather than a test of behaviour: it proves an absence, so it
/// says nothing about what the app does and everything about what it must
/// never start doing.
#[test]
fn nothing_in_the_workspace_asks_to_become_the_default_handler_at_runtime() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // This file names every call it forbids, so it excludes itself by path
    // rather than by name.
    let self_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/file_association.rs");
    let mut offenders = Vec::new();
    let mut examined = Vec::new();
    let mut pending = vec![root.join("crates"), root.join("plugins")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("the workspace source is readable") {
            let path = entry.expect("a directory entry reads").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                pending.push(path);
                continue;
            }
            let is_source = path
                .extension()
                .is_some_and(|extension| extension == "rs" || extension == "sh");
            if !is_source || same_file(&path, &self_path) {
                continue;
            }
            examined.push(path.clone());
            let text = std::fs::read_to_string(&path).expect("a source file reads");
            for forbidden in [
                "LSSetDefaultRoleHandler",
                "LSSetDefaultHandlerForURLScheme",
                "xdg-mime",
                "xdg-settings",
                "SetUserFTA",
            ] {
                if text.contains(forbidden) {
                    offenders.push(format!("{} names {forbidden}", path.display()));
                }
            }
        }
    }

    // Named files rather than a count, so a walk that stops early fails
    // here instead of passing on whatever it happened to reach: one is the
    // app's own entry point, one is a plugin, one is deep in a crate.
    for expected in [
        root.join("crates/app/src/main.rs"),
        root.join("crates/app/src/shell/home.rs"),
        root.join("plugins/commands-core/src/lib.rs"),
    ] {
        assert!(
            examined.iter().any(|path| same_file(path, &expected)),
            "the walk never reached {}",
            expected.display()
        );
    }
    assert!(offenders.is_empty(), "{offenders:?}");
}

/// Paths built from `CARGO_MANIFEST_DIR` carry `..` segments, so they are
/// compared by what they resolve to.
fn same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
